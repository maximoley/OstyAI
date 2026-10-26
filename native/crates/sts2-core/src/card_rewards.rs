//! Card-reward rarity rolling — bead `sts2-rs-jyfvu` (P9.1.e thin slice).
//!
//! Ports the rarity-side of `sts2_env/run/rewards.py` plus the
//! pity-counter machinery in `sts2_env/run/odds.py::CardRarityOdds`.
//! The concrete card-pool sampler (`_pick_reward_card`,
//! `eligible_character_cards`, `create_card`) lives in `sts2-content`
//! and is deferred to a follow-up bead — it can't be reached from
//! `sts2-core` without inverting the workspace dependency direction.
//!
//! What this module lands today:
//!
//! - [`RewardRarity`] — Common / Uncommon / Rare (the subset of
//!   Python's `CardRarity` that reward rolls actually produce).
//! - [`CardRarityOdds`] — the rolling pity-counter struct with all of
//!   Python's per-context base tables (regular / elite / boss / shop /
//!   uniform) and the A7+ scarcity branch.
//! - `roll_for_upgrade` — the act-scaled upgrade roll used to pair an
//!   `upgraded` flag with each rolled rarity.
//! - [`RunState::generate_card_reward_rarities`] /
//!   [`RunState::generate_combat_card_rewards`] — the two top-level
//!   APIs that walk the `modify_card_reward_creation_options` channel
//!   first, then roll `opts.num_cards` rarities (and optional upgrade
//!   flags) through [`Self::card_rarity_odds`].
//!
//! Python pin:
//! `sts2-rl-agent@01c094d:sts2_env/run/odds.py::CardRarityOdds` +
//! `sts2_env/run/rewards.py::{generate_card_reward,
//! generate_combat_card_rewards, roll_for_upgrade}`.

use rand_xoshiro::rand_core::RngCore;
use rand_xoshiro::Xoshiro256PlusPlus;

/// The three rarities a card-reward roll can produce. Mirrors the
/// Common / Uncommon / Rare subset of Python's
/// `sts2_env/core/enums.py::CardRarity` (Basic / Status / Curse /
/// Ancient / Event / Quest are unreachable from a reward roll).
///
/// Lives in `sts2-core` rather than re-using
/// `sts2-content::cards::types::CardRarity` because the reward-roll
/// machinery sits in `RunState` and the workspace dep direction is
/// `content → core`. Downstream sites that pair a rarity with a
/// concrete card (lives in `sts2-content`) can convert via
/// `From<RewardRarity> for CardRarity`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RewardRarity {
    /// `CardRarity.COMMON` — base-tier reward card.
    Common,
    /// `CardRarity.UNCOMMON` — mid-tier reward card.
    Uncommon,
    /// `CardRarity.RARE` — top-tier reward card.
    Rare,
}

/// Card-reward roll context. Mirrors the Python `context: str` argument
/// (`"regular" | "elite" | "boss" | "shop" | "uniform"`) without the
/// stringy-typing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RarityContext {
    /// Normal-encounter card reward.
    Regular,
    /// Elite-encounter card reward.
    Elite,
    /// Boss-encounter card reward (always RARE, ignores pity).
    Boss,
    /// Merchant card-stock roll.
    Shop,
    /// Uniform 33/33/33 roll (used by a handful of relic surfaces).
    Uniform,
}

impl RarityContext {
    /// Parse the Python `context: str` value into a Rust variant.
    /// Unknown strings fall back to [`Self::Regular`], matching the
    /// `_get_odds` switch default.
    #[must_use]
    pub fn from_python_str(s: &str) -> Self {
        match s {
            "elite" => Self::Elite,
            "boss" => Self::Boss,
            "shop" => Self::Shop,
            "uniform" => Self::Uniform,
            _ => Self::Regular,
        }
    }
}

// ── Python pin: `CardRarityOdds` constants ───────────────────────────

/// Initial rare-threshold offset. Mirrors
/// `CardRarityOdds.BASE_RARITY_OFFSET = -0.05`.
pub const BASE_RARITY_OFFSET: f64 = -0.05_f32 as f64;
/// Cap on the rare-threshold offset. Mirrors
/// `CardRarityOdds.MAX_RARITY_OFFSET = 0.40`.
pub const MAX_RARITY_OFFSET: f64 = 0.40_f32 as f64;
/// Ascension level at which the scarcity branch activates. Mirrors
/// `CardRarityOdds.SCARCITY_ASCENSION_LEVEL = 7`. This branch is LIVE:
/// `RunManager::with_ascension` binds the run's `CardRarityOdds` to the
/// real ascension level (`run_manager.rs`), so the `*_SCARCITY` tables
/// and the halved upgrade scaling apply at A7+.
pub const SCARCITY_ASCENSION_LEVEL: u8 = 7;

const REGULAR_COMMON_ODDS: f64 = 0.60;
const REGULAR_COMMON_ODDS_SCARCITY: f64 = 0.615;
const REGULAR_UNCOMMON_ODDS: f64 = 0.37;
const REGULAR_RARE_ODDS: f64 = 0.03;
const REGULAR_RARE_ODDS_SCARCITY: f64 = 0.0149;

const ELITE_COMMON_ODDS: f64 = 0.50;
const ELITE_COMMON_ODDS_SCARCITY: f64 = 0.549;
const ELITE_UNCOMMON_ODDS: f64 = 0.40;
const ELITE_RARE_ODDS: f64 = 0.10;
const ELITE_RARE_ODDS_SCARCITY: f64 = 0.05;

const BOSS_COMMON_ODDS: f64 = 0.00;
const BOSS_UNCOMMON_ODDS: f64 = 0.00;
const BOSS_RARE_ODDS: f64 = 1.00;

const SHOP_COMMON_ODDS: f64 = 0.54;
const SHOP_COMMON_ODDS_SCARCITY: f64 = 0.585;
const SHOP_UNCOMMON_ODDS: f64 = 0.37;
const SHOP_RARE_ODDS: f64 = 0.09;
const SHOP_RARE_ODDS_SCARCITY: f64 = 0.045;

const UNIFORM_COMMON_ODDS: f64 = 0.33;
const UNIFORM_UNCOMMON_ODDS: f64 = 0.33;
const UNIFORM_RARE_ODDS: f64 = 0.33;

const RARITY_GROWTH: f64 = 0.01;
const RARITY_GROWTH_SCARCITY: f64 = 0.005;

// ── Python pin: `roll_for_upgrade` constants ─────────────────────────

const COMBAT_CARD_UPGRADE_BASE_CHANCE: f64 = 0.0;
const CARD_UPGRADE_ODD_SCALING: f64 = 0.25;
const SCARCITY_CARD_UPGRADE_ODD_SCALING: f64 = 0.125;

/// One per-context odds table: `(common, uncommon, rare)`. Mirrors
/// `CardRarityOdds.regular_odds / elite_odds / boss_odds / shop_odds /
/// uniform_odds`.
///
/// Custom `PartialEq` / `Eq` on bit patterns: the odds are built from
/// named constants only (no NaN reachable), so bitwise comparison is
/// total and lets the containing `RunState` keep its derived `Eq`.
#[derive(Clone, Copy, Debug)]
struct RarityOdds {
    common: f64,
    uncommon: f64,
    rare: f64,
}

impl PartialEq for RarityOdds {
    fn eq(&self, other: &Self) -> bool {
        self.common.to_bits() == other.common.to_bits()
            && self.uncommon.to_bits() == other.uncommon.to_bits()
            && self.rare.to_bits() == other.rare.to_bits()
    }
}

impl Eq for RarityOdds {}

/// Pity-counter card-rarity roller. One per [`crate::RunState`]. Mirrors
/// `sts2-rl-agent@01c094d:sts2_env/run/odds.py::CardRarityOdds`.
///
/// `current_value` is the pity offset added to the rare threshold on
/// non-boss rolls. Rare rolls reset it to [`BASE_RARITY_OFFSET`];
/// non-rare rolls increment by `rarity_growth` capped at
/// [`MAX_RARITY_OFFSET`]. Boss rolls bypass the offset entirely.
#[derive(Clone, Copy, Debug)]
pub struct CardRarityOdds {
    /// Ascension level (gates the scarcity branch). Identity at the
    /// current pin; honored here for parity once ascension scaling
    /// lands.
    pub ascension_level: u8,
    /// Rolling pity offset added to the rare threshold on non-boss
    /// rolls. Starts at [`BASE_RARITY_OFFSET`].
    pub current_value: f64,
    regular: RarityOdds,
    elite: RarityOdds,
    boss: RarityOdds,
    shop: RarityOdds,
    uniform: RarityOdds,
    rarity_growth: f64,
}

impl Default for CardRarityOdds {
    fn default() -> Self {
        Self::new(0)
    }
}

impl PartialEq for CardRarityOdds {
    fn eq(&self, other: &Self) -> bool {
        self.ascension_level == other.ascension_level
            && self.current_value.to_bits() == other.current_value.to_bits()
            && self.regular == other.regular
            && self.elite == other.elite
            && self.boss == other.boss
            && self.shop == other.shop
            && self.uniform == other.uniform
            && self.rarity_growth.to_bits() == other.rarity_growth.to_bits()
    }
}

impl Eq for CardRarityOdds {}

impl CardRarityOdds {
    /// Construct a fresh roller. Mirrors
    /// `CardRarityOdds.__init__(ascension_level)`.
    #[must_use]
    pub fn new(ascension_level: u8) -> Self {
        let scarcity = ascension_level >= SCARCITY_ASCENSION_LEVEL;
        Self {
            ascension_level,
            current_value: BASE_RARITY_OFFSET,
            regular: RarityOdds {
                common: if scarcity {
                    REGULAR_COMMON_ODDS_SCARCITY
                } else {
                    REGULAR_COMMON_ODDS
                },
                uncommon: REGULAR_UNCOMMON_ODDS,
                rare: if scarcity {
                    REGULAR_RARE_ODDS_SCARCITY
                } else {
                    REGULAR_RARE_ODDS
                },
            },
            elite: RarityOdds {
                common: if scarcity {
                    ELITE_COMMON_ODDS_SCARCITY
                } else {
                    ELITE_COMMON_ODDS
                },
                uncommon: ELITE_UNCOMMON_ODDS,
                rare: if scarcity {
                    ELITE_RARE_ODDS_SCARCITY
                } else {
                    ELITE_RARE_ODDS
                },
            },
            boss: RarityOdds {
                common: BOSS_COMMON_ODDS,
                uncommon: BOSS_UNCOMMON_ODDS,
                rare: BOSS_RARE_ODDS,
            },
            shop: RarityOdds {
                common: if scarcity {
                    SHOP_COMMON_ODDS_SCARCITY
                } else {
                    SHOP_COMMON_ODDS
                },
                uncommon: SHOP_UNCOMMON_ODDS,
                rare: if scarcity {
                    SHOP_RARE_ODDS_SCARCITY
                } else {
                    SHOP_RARE_ODDS
                },
            },
            uniform: RarityOdds {
                common: UNIFORM_COMMON_ODDS,
                uncommon: UNIFORM_UNCOMMON_ODDS,
                rare: UNIFORM_RARE_ODDS,
            },
            rarity_growth: if scarcity {
                RARITY_GROWTH_SCARCITY
            } else {
                RARITY_GROWTH
            },
        }
    }

    fn odds_for(&self, context: RarityContext) -> RarityOdds {
        match context {
            RarityContext::Regular => self.regular,
            RarityContext::Elite => self.elite,
            RarityContext::Boss => self.boss,
            RarityContext::Shop => self.shop,
            RarityContext::Uniform => self.uniform,
        }
    }

    /// Roll a rarity and update pity state. Mirrors `CardRarityOdds.roll`.
    ///
    /// Boss rolls bypass the pity offset. Rare rolls reset
    /// [`Self::current_value`] to [`BASE_RARITY_OFFSET`]; non-rare
    /// rolls add `rarity_growth` capped at [`MAX_RARITY_OFFSET`].
    pub fn roll(&mut self, rng: &mut Xoshiro256PlusPlus, context: RarityContext) -> RewardRarity {
        let odds = self.odds_for(context);
        let offset = if context == RarityContext::Boss {
            0.0
        } else {
            self.current_value
        };
        let roll_val = f64::from(next_unit_f64(rng) as f32);
        let rare_threshold = f64::from(odds.rare as f32 + offset as f32);

        if roll_val < rare_threshold {
            self.current_value = BASE_RARITY_OFFSET;
            RewardRarity::Rare
        } else if roll_val < f64::from(odds.uncommon as f32 + rare_threshold as f32) {
            self.current_value = f64::from(
                (self.current_value as f32 + self.rarity_growth as f32)
                    .min(MAX_RARITY_OFFSET as f32),
            );
            RewardRarity::Uncommon
        } else {
            self.current_value = f64::from(
                (self.current_value as f32 + self.rarity_growth as f32)
                    .min(MAX_RARITY_OFFSET as f32),
            );
            RewardRarity::Common
        }
    }

    /// Pity-tracking rarity roll using a [`crate::dotnet_rng::Rng`]
    /// (the per-stream RNG Python's reward-flow uses). Mirrors
    /// `CardRarityOdds.roll` (`run/odds.py`) with parity-grade
    /// `next_float()` for the unit draw — sequence advances 1 step
    /// per call.
    ///
    /// Behavior matches [`Self::roll`] except for the RNG type; the
    /// callsite migration off the legacy Xoshiro shim
    /// (`RunState::rewards_rng`) is tracked under bead
    /// `sts2-rs-7e0ek`.
    pub fn roll_with_dotnet(
        &mut self,
        rng: &mut crate::dotnet_rng::Rng,
        context: RarityContext,
    ) -> RewardRarity {
        let odds = self.odds_for(context);
        let offset = if context == RarityContext::Boss {
            0.0
        } else {
            self.current_value
        };
        let roll_val = rng.next_float(1.0);
        let rare_threshold = f64::from(odds.rare as f32 + offset as f32);

        let result = if roll_val < rare_threshold {
            self.current_value = BASE_RARITY_OFFSET;
            RewardRarity::Rare
        } else if roll_val < f64::from(odds.uncommon as f32 + rare_threshold as f32) {
            self.current_value = f64::from(
                (self.current_value as f32 + self.rarity_growth as f32)
                    .min(MAX_RARITY_OFFSET as f32),
            );
            RewardRarity::Uncommon
        } else {
            self.current_value = f64::from(
                (self.current_value as f32 + self.rarity_growth as f32)
                    .min(MAX_RARITY_OFFSET as f32),
            );
            RewardRarity::Common
        };
        trace_rarity("roll", context, offset, result, self.current_value);
        result
    }

    /// Roll a rarity for merchant cards without updating pity state.
    /// Mirrors `CardRarityOdds.roll_without_changing_odds`.
    ///
    /// Still honors the current pity offset on the rare threshold
    /// (matches C# `RollWithoutChangingFutureOdds`).
    pub fn roll_without_changing_odds(
        &self,
        rng: &mut Xoshiro256PlusPlus,
        context: RarityContext,
    ) -> RewardRarity {
        let odds = self.odds_for(context);
        let roll_val = f64::from(next_unit_f64(rng) as f32);
        let rare_threshold = f64::from(odds.rare as f32 + self.current_value as f32);
        if roll_val < rare_threshold {
            RewardRarity::Rare
        } else if roll_val < f64::from(odds.uncommon as f32 + rare_threshold as f32) {
            RewardRarity::Uncommon
        } else {
            RewardRarity::Common
        }
    }

    /// Same as [`Self::roll_without_changing_odds`] but reads the unit
    /// draw from a [`crate::dotnet_rng::Rng`] (the per-stream RNG Python's
    /// shop generator uses). Mirrors `rng.next_float()` directly so the
    /// sequence advances 1 step per call, matching Python.
    ///
    /// Bead `sts2-rs-64mq2` (P9.2.c.2 shop inventory roller).
    pub fn roll_without_changing_odds_with_dotnet(
        &self,
        rng: &mut crate::dotnet_rng::Rng,
        context: RarityContext,
    ) -> RewardRarity {
        let odds = self.odds_for(context);
        let roll_val = rng.next_float(1.0);
        let rare_threshold = f64::from(odds.rare as f32 + self.current_value as f32);
        let result = if roll_val < rare_threshold {
            RewardRarity::Rare
        } else if roll_val < f64::from(odds.uncommon as f32 + rare_threshold as f32) {
            RewardRarity::Uncommon
        } else {
            RewardRarity::Common
        };
        trace_rarity(
            "shop",
            context,
            self.current_value,
            result,
            self.current_value,
        );
        result
    }

    /// Roll a rarity using base odds — no pity offset, no state change —
    /// reading the unit draw from a [`crate::dotnet_rng::Rng`]. Mirrors C#
    /// `CardRarityOdds.RollWithBaseOdds` (v0.103.2 `CardRarityOdds.cs:88-100`),
    /// the branch `CardFactory.RollForRarity` takes for any non-`Encounter`
    /// `CardCreationSource` (LeadPaperweight / LostCoffer / HeftyTablet —
    /// all `CardCreationSource.Other`).
    ///
    /// Beta uses cumulative rare + uncommon thresholds for base rolls too.
    /// This restores the missing uncommon band for non-encounter rewards
    /// (BETA-348). Every threshold and pity update rounds as a source float.
    pub fn roll_with_base_odds_with_dotnet(
        &self,
        rng: &mut crate::dotnet_rng::Rng,
        context: RarityContext,
    ) -> RewardRarity {
        let odds = self.odds_for(context);
        let roll_val = rng.next_float(1.0);
        let result = if roll_val < f64::from(odds.rare as f32) {
            RewardRarity::Rare
        } else if roll_val < f64::from(odds.uncommon as f32 + odds.rare as f32) {
            RewardRarity::Uncommon
        } else {
            RewardRarity::Common
        };
        trace_rarity(
            "base",
            context,
            self.current_value,
            result,
            self.current_value,
        );
        result
    }

    /// Roll a rarity using base odds — no pity offset, no state change.
    /// Mirrors `CardRarityOdds.roll_with_base_odds`.
    pub fn roll_with_base_odds(
        &self,
        rng: &mut Xoshiro256PlusPlus,
        context: RarityContext,
    ) -> RewardRarity {
        let odds = self.odds_for(context);
        let roll_val = f64::from(next_unit_f64(rng) as f32);
        if roll_val < f64::from(odds.rare as f32) {
            RewardRarity::Rare
        } else if roll_val < f64::from(odds.uncommon as f32 + odds.rare as f32) {
            RewardRarity::Uncommon
        } else {
            RewardRarity::Common
        }
    }
}

/// Roll whether a reward card should be upgraded. Mirrors
/// `sts2_env/run/rewards.py::roll_for_upgrade`.
///
/// Formula: `odds = base_chance + (act_index * scaling)` for non-rare
/// rarities (RARE never auto-upgrades). Scaling is
/// [`CARD_UPGRADE_ODD_SCALING`] (0.25) at base ascension, or
/// [`SCARCITY_CARD_UPGRADE_ODD_SCALING`] (0.125) at
/// [`SCARCITY_ASCENSION_LEVEL`]+.
#[must_use]
pub fn roll_for_upgrade(
    rng: &mut Xoshiro256PlusPlus,
    rarity: RewardRarity,
    act_index: u8,
    ascension_level: u8,
    base_chance: f64,
) -> bool {
    let roll = next_unit_f64(rng);
    let mut odds = base_chance;
    if rarity != RewardRarity::Rare {
        let scaling = if ascension_level >= SCARCITY_ASCENSION_LEVEL {
            SCARCITY_CARD_UPGRADE_ODD_SCALING
        } else {
            CARD_UPGRADE_ODD_SCALING
        };
        odds += f64::from(act_index) * scaling;
    }
    roll <= odds
}

/// [`roll_for_upgrade`] variant that reads from a
/// [`crate::dotnet_rng::Rng`]. Mirrors Python `rewards.roll_for_upgrade`
/// (`rng.next_float()` directly) so the per-call sequence advances by
/// one step on the dotnet stream — `RunState.rng.rewards` at Python's
/// callsite. Bead `sts2-rs-7e0ek`.
#[must_use]
pub fn roll_for_upgrade_with_dotnet(
    rng: &mut crate::dotnet_rng::Rng,
    rarity: RewardRarity,
    act_index: u8,
    ascension_level: u8,
    base_chance: f64,
) -> bool {
    let roll = rng.next_float(1.0);
    let mut odds = base_chance;
    if rarity != RewardRarity::Rare {
        let scaling = if ascension_level >= SCARCITY_ASCENSION_LEVEL {
            SCARCITY_CARD_UPGRADE_ODD_SCALING
        } else {
            CARD_UPGRADE_ODD_SCALING
        };
        odds += f64::from(act_index) * scaling;
    }
    roll <= odds
}

/// Combat-context base chance for [`roll_for_upgrade`]. Mirrors
/// `COMBAT_CARD_UPGRADE_BASE_CHANCE = 0.0`.
#[must_use]
pub const fn combat_card_upgrade_base_chance() -> f64 {
    COMBAT_CARD_UPGRADE_BASE_CHANCE
}

// ── Python pin: `PotionRewardOdds` ───────────────────────────────────

/// Initial drop probability. Mirrors Python `PotionRewardOdds.INITIAL_VALUE = 0.40`.
pub const POTION_REWARD_INITIAL_VALUE: f64 = 0.40_f32 as f64;
/// Elite-encounter bonus (full delta; halved at the comparison site
/// per Python's `elite_bonus * 0.50`). Mirrors
/// `PotionRewardOdds.ELITE_BONUS = 0.25`.
pub const POTION_REWARD_ELITE_BONUS: f64 = 0.25;
/// Per-combat odds swing. Mirrors `PotionRewardOdds.SWING = 0.10`.
pub const POTION_REWARD_SWING: f64 = 0.10_f32 as f64;

/// Source beta potion-drop odds. Normal rolls compare against the current
/// source float value plus half the elite bonus, then update by ±0.1f.
/// Forced drops preserve both the odds and the RNG stream.
///
/// Custom `PartialEq` on bit patterns: `current_value` is built from named
/// constants only (no NaN reachable), so bitwise comparison is total and
/// lets the containing `RunState` keep its derived `Eq`.
///
/// Bead `sts2-rs-vk9av`.
#[derive(Clone, Copy, Debug)]
pub struct PotionRewardOdds {
    /// Live drop probability (excluding elite bonus). Starts at
    /// [`POTION_REWARD_INITIAL_VALUE`].
    pub current_value: f64,
}

impl Default for PotionRewardOdds {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for PotionRewardOdds {
    fn eq(&self, other: &Self) -> bool {
        self.current_value.to_bits() == other.current_value.to_bits()
    }
}

impl Eq for PotionRewardOdds {}

impl std::hash::Hash for PotionRewardOdds {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.current_value.to_bits().hash(state);
    }
}

impl PotionRewardOdds {
    /// Construct a fresh roller starting at
    /// [`POTION_REWARD_INITIAL_VALUE`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current_value: POTION_REWARD_INITIAL_VALUE,
        }
    }

    /// Source beta uses float arithmetic for the elite threshold and each
    /// adaptive update. Forced drops bypass both the draw and the update.
    pub fn roll(&mut self, rng: &mut crate::dotnet_rng::Rng, is_elite: bool, force: bool) -> bool {
        // v0.111.0 Core.Odds/PotionRewardOdds.cs: forced rewards bypass
        // both the RNG draw and the adaptive odds update.
        if force {
            return true;
        }
        let elite_bonus = if is_elite {
            POTION_REWARD_ELITE_BONUS
        } else {
            0.0
        };
        let threshold = self.current_value as f32 + elite_bonus as f32 * 0.5;
        let roll_val = rng.next_float(1.0) as f32;
        if roll_val < threshold {
            self.current_value = f64::from(self.current_value as f32 - POTION_REWARD_SWING as f32);
            true
        } else {
            self.current_value = f64::from(self.current_value as f32 + POTION_REWARD_SWING as f32);
            false
        }
    }
}

/// PARITY132 pity tracer. Gated on `STS2_TRACE_RNG` so the un-traced hot
/// path is untouched. Emits one line per rarity roll mirroring the C#
/// `RARITYTRACE` shape (`RunSimulator.cs::RngTracePatches`) so the per-roll
/// pity offset + rolled rarity can be diffed read-for-read. The C#
/// `CardRarityOddsType` maps to our [`RarityContext`] (Regular ↔ Encounter
/// regular, etc.); the diff tooling normalizes the labels.
fn trace_rarity(
    method: &str,
    context: RarityContext,
    offset: f64,
    result: RewardRarity,
    after: f64,
) {
    if std::env::var_os("STS2_TRACE_RNG").is_none() {
        return;
    }
    eprintln!(
        "RARITYTRACE method={method} type={context:?} offset={offset} r={result:?} after={after}"
    );
}

/// Convert a `u64` draw into a uniform `[0, 1)` `f64`. Matches the
/// Python `Rng.next_float()` contract (open upper bound) by using the
/// top 53 bits of the draw as the mantissa.
fn next_unit_f64(rng: &mut Xoshiro256PlusPlus) -> f64 {
    // Top 53 bits → `[0, 1)`. Cf. `rand::distributions::Standard` for
    // f64. This is the standard IEEE-754 mantissa fill.
    let bits = rng.next_u64() >> 11;
    (bits as f64) * (1.0 / ((1_u64 << 53) as f64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_xoshiro::rand_core::SeedableRng;

    fn rng(seed: u64) -> Xoshiro256PlusPlus {
        Xoshiro256PlusPlus::seed_from_u64(seed)
    }

    #[test]
    fn default_starts_at_base_offset() {
        let odds = CardRarityOdds::default();
        assert_eq!(odds.ascension_level, 0);
        assert!((odds.current_value - BASE_RARITY_OFFSET).abs() < 1e-12);
    }

    #[test]
    fn scarcity_branch_swaps_growth_and_rare_odds() {
        let base = CardRarityOdds::new(0);
        let scarce = CardRarityOdds::new(SCARCITY_ASCENSION_LEVEL);
        assert!((base.rarity_growth - RARITY_GROWTH).abs() < 1e-12);
        assert!((scarce.rarity_growth - RARITY_GROWTH_SCARCITY).abs() < 1e-12);
        assert!((base.regular.rare - REGULAR_RARE_ODDS).abs() < 1e-12);
        assert!((scarce.regular.rare - REGULAR_RARE_ODDS_SCARCITY).abs() < 1e-12);
    }

    #[test]
    fn boss_context_always_rare_and_ignores_pity() {
        let mut odds = CardRarityOdds::new(0);
        // Crank pity to a wild value — boss should still ignore it.
        odds.current_value = 0.99;
        let mut r = rng(42);
        for _ in 0..16 {
            assert_eq!(odds.roll(&mut r, RarityContext::Boss), RewardRarity::Rare);
        }
        // Boss rolls don't reset pity (current_value stays put if Rare
        // came from the boss branch, which it does — but the rare-branch
        // resets to BASE_RARITY_OFFSET unconditionally per Python).
        // Verify the Python reference: rare always resets, regardless
        // of context.
        assert!((odds.current_value - BASE_RARITY_OFFSET).abs() < 1e-12);
    }

    #[test]
    fn non_rare_roll_grows_pity_capped_at_max() {
        let mut odds = CardRarityOdds::new(0);
        // Seed an RNG that always rolls high — guarantees Common.
        let mut r = rng(1);
        for _ in 0..200 {
            let _ = odds.roll(&mut r, RarityContext::Regular);
        }
        // After many non-rare rolls, pity is capped at MAX.
        assert!(odds.current_value <= MAX_RARITY_OFFSET + 1e-12);
        assert!(odds.current_value > BASE_RARITY_OFFSET); // grew at least once
    }

    #[test]
    fn rare_roll_resets_pity_to_base() {
        let mut odds = CardRarityOdds::new(0);
        odds.current_value = 0.30;
        // Force rare by setting current_value high enough that even a
        // mid-range roll triggers rare. With regular rare base 0.03 +
        // offset 0.30 = 0.33, any roll < 0.33 hits rare.
        // We can't directly inject a roll, so iterate until rare fires.
        let mut r = rng(7);
        let mut saw_rare = false;
        for _ in 0..500 {
            if odds.roll(&mut r, RarityContext::Regular) == RewardRarity::Rare {
                saw_rare = true;
                assert!((odds.current_value - BASE_RARITY_OFFSET).abs() < 1e-12);
                break;
            }
        }
        assert!(saw_rare, "should have rolled rare within 500 tries");
    }

    #[test]
    fn roll_with_base_odds_does_not_mutate() {
        let odds = CardRarityOdds::new(0);
        let saved = odds.current_value;
        let mut r = rng(11);
        for _ in 0..50 {
            let _ = odds.roll_with_base_odds(&mut r, RarityContext::Regular);
        }
        // current_value is unchanged because the method takes &self.
        assert!((odds.current_value - saved).abs() < 1e-12);
    }

    #[test]
    fn roll_without_changing_odds_does_not_mutate() {
        let mut odds = CardRarityOdds::new(0);
        odds.current_value = 0.20;
        let saved = odds.current_value;
        let mut r = rng(13);
        for _ in 0..50 {
            let _ = odds.roll_without_changing_odds(&mut r, RarityContext::Shop);
        }
        assert!((odds.current_value - saved).abs() < 1e-12);
    }

    #[test]
    fn roll_for_upgrade_rare_never_upgrades_at_base_chance_zero() {
        let mut r = rng(17);
        for _ in 0..200 {
            assert!(!roll_for_upgrade(
                &mut r,
                RewardRarity::Rare,
                3,
                0,
                COMBAT_CARD_UPGRADE_BASE_CHANCE
            ));
        }
    }

    #[test]
    fn roll_for_upgrade_act_scaling_increases_odds() {
        // At act 0, scaling adds 0 → 0% upgrade chance.
        let mut r = rng(19);
        let mut act0_hits = 0;
        for _ in 0..1000 {
            if roll_for_upgrade(&mut r, RewardRarity::Common, 0, 0, 0.0) {
                act0_hits += 1;
            }
        }
        // At act 2, scaling adds 2 * 0.25 = 0.5 → ~50%.
        let mut r2 = rng(19);
        let mut act2_hits = 0;
        for _ in 0..1000 {
            if roll_for_upgrade(&mut r2, RewardRarity::Common, 2, 0, 0.0) {
                act2_hits += 1;
            }
        }
        assert_eq!(act0_hits, 0, "act 0 + base 0.0 → zero hits");
        assert!(
            (400..=600).contains(&act2_hits),
            "act 2 should be ~50% hits, got {act2_hits}"
        );
    }

    #[test]
    fn rarity_context_from_python_str_known_values() {
        assert_eq!(
            RarityContext::from_python_str("regular"),
            RarityContext::Regular
        );
        assert_eq!(
            RarityContext::from_python_str("elite"),
            RarityContext::Elite
        );
        assert_eq!(RarityContext::from_python_str("boss"), RarityContext::Boss);
        assert_eq!(RarityContext::from_python_str("shop"), RarityContext::Shop);
        assert_eq!(
            RarityContext::from_python_str("uniform"),
            RarityContext::Uniform
        );
        // Unknown → Regular fallback.
        assert_eq!(
            RarityContext::from_python_str("xyzzy"),
            RarityContext::Regular
        );
    }

    // ── 7e0ek: dotnet-RNG migration variants ──────────────────────

    #[test]
    fn roll_with_dotnet_boss_always_rare() {
        // Boss context has rare=1.0 / uncommon=0 / common=0; any
        // unit draw < 1.0 lands Rare. Same contract as `roll`.
        let mut odds = CardRarityOdds::new(0);
        let mut r = crate::dotnet_rng::Rng::with_name(7, "rewards");
        for _ in 0..100 {
            assert_eq!(
                odds.roll_with_dotnet(&mut r, RarityContext::Boss),
                RewardRarity::Rare
            );
        }
    }

    #[test]
    fn roll_with_dotnet_mutates_current_value_like_roll() {
        // Pity-state mutation parity: a single dotnet rare-roll
        // resets current_value to BASE_RARITY_OFFSET; a non-rare
        // roll bumps it by rarity_growth.
        let mut odds = CardRarityOdds::new(0);
        odds.current_value = 0.30;
        let mut r = crate::dotnet_rng::Rng::with_name(11, "rewards");
        // 100 draws to exercise both branches.
        for _ in 0..100 {
            let prev = odds.current_value;
            let got = odds.roll_with_dotnet(&mut r, RarityContext::Regular);
            match got {
                RewardRarity::Rare => assert!(
                    (odds.current_value - BASE_RARITY_OFFSET).abs() < 1e-12,
                    "rare → reset to base"
                ),
                _ => assert!(
                    odds.current_value >= prev
                        || (odds.current_value - MAX_RARITY_OFFSET).abs() < 1e-12,
                    "non-rare grows or saturates at MAX"
                ),
            }
        }
    }

    #[test]
    fn roll_for_upgrade_with_dotnet_rare_never_upgrades_at_base_chance_zero() {
        let mut r = crate::dotnet_rng::Rng::with_name(17, "rewards");
        for _ in 0..200 {
            assert!(!roll_for_upgrade_with_dotnet(
                &mut r,
                RewardRarity::Rare,
                3,
                0,
                COMBAT_CARD_UPGRADE_BASE_CHANCE
            ));
        }
    }

    #[test]
    fn roll_for_upgrade_with_dotnet_act_scaling_increases_odds() {
        let mut r0 = crate::dotnet_rng::Rng::with_name(19, "rewards");
        let mut act0_hits = 0;
        for _ in 0..1000 {
            if roll_for_upgrade_with_dotnet(&mut r0, RewardRarity::Common, 0, 0, 0.0) {
                act0_hits += 1;
            }
        }
        let mut r2 = crate::dotnet_rng::Rng::with_name(19, "rewards");
        let mut act2_hits = 0;
        for _ in 0..1000 {
            if roll_for_upgrade_with_dotnet(&mut r2, RewardRarity::Common, 2, 0, 0.0) {
                act2_hits += 1;
            }
        }
        assert_eq!(act0_hits, 0, "act 0 + base 0.0 → zero hits");
        assert!(
            (350..=650).contains(&act2_hits),
            "act 2 ~50% hits (dotnet RNG band), got {act2_hits}"
        );
    }

    // ── vk9av: PotionRewardOdds ───────────────────────────────────

    #[test]
    fn potion_reward_odds_starts_at_initial_value() {
        let o = PotionRewardOdds::new();
        assert!((o.current_value - POTION_REWARD_INITIAL_VALUE).abs() < 1e-12);
    }

    #[test]
    fn potion_reward_odds_force_preserves_odds_and_rng() {
        let mut o = PotionRewardOdds::new();
        let mut r = crate::dotnet_rng::Rng::with_name(7, "rewards");
        for _ in 0..50 {
            let prev = o.current_value;
            let dropped = o.roll(&mut r, false, /* force */ true);
            assert!(dropped, "force=true must always return true");
            assert_eq!(r.counter(), 0, "forced rewards do not draw RNG");
            assert!(
                (o.current_value - prev).abs() < 1e-12,
                "force-drop preserves normal odds"
            );
        }
    }

    #[test]
    fn potion_reward_odds_elite_bonus_increases_drop_rate() {
        // Compare the same seed sequence: non-elite vs elite should
        // yield ≥ non-elite hits (elite adds +12.5% to the return-check
        // threshold).
        let trials = 2000;
        let mut o_normal = PotionRewardOdds::new();
        let mut r_normal = crate::dotnet_rng::Rng::with_name(11, "rewards");
        let mut o_elite = PotionRewardOdds::new();
        let mut r_elite = crate::dotnet_rng::Rng::with_name(11, "rewards");
        let mut normal_hits = 0;
        let mut elite_hits = 0;
        for _ in 0..trials {
            if o_normal.roll(&mut r_normal, false, false) {
                normal_hits += 1;
            }
            if o_elite.roll(&mut r_elite, true, false) {
                elite_hits += 1;
            }
        }
        assert!(
            elite_hits >= normal_hits,
            "elite (>= via +12.5% bonus): got elite={elite_hits} vs normal={normal_hits}"
        );
        // Sanity band: both should be roughly in 30-65% (the pity
        // dynamics oscillate around 45-50%).
        let band = 600..=1300;
        assert!(
            band.contains(&elite_hits),
            "elite hits {elite_hits} in {band:?}"
        );
        assert!(
            band.contains(&normal_hits),
            "normal hits {normal_hits} in {band:?}"
        );
    }

    #[test]
    fn potion_reward_odds_oscillates_around_target() {
        // After many combats the pity counter should hover near the
        // 40-50% band rather than drifting unbounded.
        let mut o = PotionRewardOdds::new();
        let mut r = crate::dotnet_rng::Rng::with_name(13, "rewards");
        for _ in 0..200 {
            let _ = o.roll(&mut r, false, false);
        }
        assert!(
            (0.10..=0.80).contains(&o.current_value),
            "current_value stays in a sane band after 200 combats, got {}",
            o.current_value
        );
    }
}
