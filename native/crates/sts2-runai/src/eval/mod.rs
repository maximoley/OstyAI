//! Run-level evaluation: the tier-1 heuristic value function and its
//! Texel-style tunable weights (full-run-AI plan §5/M1, §2.5; bead
//! `sts2-rs-40ikw.5`).
//!
//! Every evaluator here consumes [`crate::view::VisibleRunView`] only —
//! see the §2.5 hard rule in `view.rs`.

pub mod card_stats;
pub mod heuristic_v;
pub mod map_features;
pub mod ranker_features;
pub mod run_value;
pub mod run_value_net;
pub mod surrogate;

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

pub use heuristic_v::HeuristicV;
pub use surrogate::{CombatOutcome, SurrogateError, SurrogateNet};

/// Embedded default weights file. The committed copy at
/// `sts2-rs/data/runai/heuristic_v.toml` is the source of truth;
/// [`Weights::load`] overrides it from disk for tuning sweeps.
pub const EMBEDDED_WEIGHTS_TOML: &str = include_str!("../../../../data/runai/heuristic_v.toml");

/// Boss-identity weights (plan §5/M1 "pinned-boss identity features").
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BossWeights {
    /// Extra V per AoE deck card when the pinned boss is summon-heavy.
    pub aoe_bonus: f64,
    /// `PinnedActBoss::display_name`s counted as summon-heavy.
    pub summon_boss_names: Vec<String>,
    /// Flat per-boss calibration offsets (rank-inert), keyed by
    /// display name.
    #[serde(default)]
    pub offsets: BTreeMap<String, f64>,
}

/// Potion-economy weights (hold value + boss insurance, plan §2).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PotionWeights {
    pub held: f64,
    pub boss_insurance: f64,
}

/// Ascension scalar (calibration only — constant offsets are rank-inert).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct AscensionWeights {
    pub per_level: f64,
}

/// Shop-surface policy knobs (plan §2 shop knapsack).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ShopWeights {
    /// Gold floor the knapsack never spends below.
    pub gold_reserve: i32,
    /// Minimum ΔV for any purchase.
    pub min_dv: f64,
    /// Minimum ΔV-per-gold ratio for any purchase.
    pub min_dv_per_gold: f64,
}

/// Tier-2/3 escalation knobs for `policy/tiered.rs` (plan §5/M2; bead
/// `sts2-rs-40ikw.6`). Margins are in V-score units: when the tier-1
/// top-2 margin on a card-shaped surface is *below* the surface's
/// threshold, the decision escalates to `rollout::fight_delta` probes.
/// E7 sweeps these later — the defaults are starting points.
fn default_meta_probe_weight() -> f64 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TierWeights {
    /// Card-reward pick/skip escalation margin.
    pub card_reward_margin: f64,
    /// Boss-relic pick escalation margin (escalations here run at the
    /// tier-3 budget — plan §2: tier-3 is reserved for boss relics).
    pub boss_relic_margin: f64,
    /// Deck-mutation surfaces (remove / upgrade / transform targets).
    pub deck_mutation_margin: f64,
    /// Rest-site smith target.
    pub smith_margin: f64,
    /// Map-pathing edge-commit escalation margin (M2.5 `path_delta`).
    /// `0.0` ⇒ never escalate pathing (the default — calibrated up by
    /// E12 before use).
    #[serde(default)]
    pub path_margin: f64,
    /// Rest-site heal-vs-smith *option* escalation margin (M2.5
    /// `rest_delta`; distinct from `smith_margin`, the smith-target card
    /// surface). `0.0` ⇒ never escalate (default; E13 calibrates).
    #[serde(default)]
    pub rest_margin: f64,
    /// Weight on the meta-move probe score when blended with the tier-1
    /// projected score on Path/Rest escalations (`combined = tier1 +
    /// meta_probe_weight · probe.score()`). Inert while the margins are
    /// `0.0`; E12/E13 calibrate.
    #[serde(default = "default_meta_probe_weight")]
    pub meta_probe_weight: f64,
    /// How many tier-1-ranked candidates a tier-2/3 escalation probes.
    pub top_candidates: usize,
    /// Tier-2 probe budget (plan §5/M2 defaults: 100 sims, K=16, T=5).
    pub tier2_sims: u32,
    pub tier2_k: u32,
    pub tier2_t_turns: u32,
    /// Tier-3 probe budget (boss-relic picks; "full eval-strong budget
    /// probes" scaled to stay affordable at ~2–5 escalations/run).
    pub tier3_sims: u32,
    pub tier3_k: u32,
    pub tier3_t_turns: u32,
}

/// Optionality-aware pathing weights — the *value of flexibility* in the
/// per-floor-replanned act-DAG plan (user request 2026-06-23; pathing
/// surface). The map is replanned every floor and only the best path's
/// FIRST edge commits, so the realized value of committing an edge is not
/// the single best path through it (the `tau == 0` hard-max) but the
/// *expected best path revealed after future floors* — which, under
/// Gumbel-distributed projection residuals, is exactly the log-sum-exp
/// soft-max at temperature `tau`. Two extra terms reward keeping options
/// open explicitly:
///
/// - `tau` — soft-max temperature over the path scores through a column.
///   `0.0` reduces to the historical hard-max (exact backward compat);
///   larger values reward a column with *many* good paths over one with a
///   single good path and the rest bad.
/// - `branch_bonus` — flat weight on `ln(#paths through the column)`: raw
///   route count, a cheap freedom proxy independent of value.
/// - `elite_skip_bonus` — the "skip an elite if you want" option, added
///   when a column keeps BOTH an elite-free and an elite-bearing path to
///   the boss reachable. **Gated on death risk** (`elite_skip_hp_threshold`):
///   elites are a reward *farm* you normally want, so this is ~0 when
///   healthy and ramps up only as HP falls — the agent keeps the escape
///   route open precisely when it might need to bail, and farms elites
///   otherwise.
/// - `elite_skip_hp_threshold` — HP fraction at/above which the elite-skip
///   option is worth nothing (you'll take the elite, so flexibility around
///   it is moot). The skip bonus scales linearly from `0` at this fraction
///   to full `elite_skip_bonus` at 0 HP: `danger = clamp((thr − hp/maxhp) /
///   thr, 0, 1)`. `0.0` disables the gate (skip bonus never fires).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct OptionalityWeights {
    pub tau: f64,
    pub branch_bonus: f64,
    pub elite_skip_bonus: f64,
    pub elite_skip_hp_threshold: f64,
}

impl Default for OptionalityWeights {
    fn default() -> Self {
        // Calibrated by `examples/pathing_optionality_ab.rs` (see
        // `docs/experiments/2026-06-23-pathing-optionality.md`). `tau == 0`
        // + `elite_skip_bonus == 0` here would restore the pre-feature
        // hard-max behavior exactly.
        Self {
            tau: 0.10,
            branch_bonus: 0.0,
            // Stronger than the ungated 0.03 because the death-risk gate
            // keeps it dormant while healthy — it only bites when wounded.
            elite_skip_bonus: 0.10,
            elite_skip_hp_threshold: 0.5,
        }
    }
}

impl OptionalityWeights {
    /// Death-risk weight in `[0, 1]` for the elite-skip option at the given
    /// HP fraction: `0` when `hp_frac >= elite_skip_hp_threshold` (healthy —
    /// farm the elite), ramping linearly to `1` at 0 HP. `0` whenever the
    /// threshold is non-positive (gate disabled).
    #[must_use]
    pub fn elite_skip_danger(&self, hp_frac: f64) -> f64 {
        if self.elite_skip_hp_threshold <= 0.0 {
            return 0.0;
        }
        ((self.elite_skip_hp_threshold - hp_frac) / self.elite_skip_hp_threshold).clamp(0.0, 1.0)
    }

    /// The pre-feature hard-max aggregator (no flexibility valuation):
    /// `tau == 0` ⇒ soft-max collapses to `max`, both bonuses off. Used as
    /// the A/B baseline and anywhere the legacy behavior must be pinned.
    #[must_use]
    pub fn hard_max() -> Self {
        Self {
            tau: 0.0,
            branch_bonus: 0.0,
            elite_skip_bonus: 0.0,
            elite_skip_hp_threshold: 0.0,
        }
    }
}

/// Pathing projection tables (plan §2 pathing; `projection.rs`).
/// Heuristic initial values, refit from M2 campaign statistics.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ProjectionTables {
    /// Expected HP loss per encounter class, indexed by act (0..=2).
    pub monster_hp_loss: [f64; 3],
    pub elite_hp_loss: [f64; 3],
    /// Event-room HP-loss fallback (per-event priors refine in M2).
    pub event_hp_loss: f64,
    pub unknown_hp_loss: f64,
    pub monster_gold: f64,
    pub elite_gold: f64,
    pub monster_reward_dv: f64,
    pub elite_reward_dv: f64,
    pub treasure_dv: f64,
    pub shop_dv: f64,
    pub event_dv: f64,
    pub unknown_dv: f64,
    /// Matches the engine's `floor(max_hp * 0.3)` rest heal.
    pub rest_heal_frac: f64,
    /// Flat ΔV stand-in for projecting a future smith.
    pub smith_dv: f64,
    /// Penalty scale when a projected path drives HP to <= 0.
    pub death_penalty: f64,
}

/// All HeuristicV weights + projection tables. Loaded from TOML
/// (embedded default or [`Weights::load`] file override); every field is
/// required — copy `data/runai/heuristic_v.toml` and edit, rather than
/// writing partial files, so a tuning sweep can never silently mix
/// defaults into an override.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weights {
    // HP.
    pub hp_base: f64,
    pub hp_act: f64,
    pub low_hp: f64,
    // Gold.
    pub gold_sqrt: f64,
    // Deck.
    pub frontload_per_card: f64,
    pub block_per_card: f64,
    pub aoe_count: f64,
    pub scaling_count: f64,
    pub upgrade_count: f64,
    pub draw_total: f64,
    pub energy_total: f64,
    pub curse_count: f64,
    pub deck_size_target: f64,
    pub deck_size_penalty: f64,
    pub heavy_cost_penalty: f64,
    // Relics.
    pub relic_default: f64,
    #[serde(default)]
    pub relic_values: BTreeMap<String, f64>,
    // Sections.
    pub boss: BossWeights,
    pub potions: PotionWeights,
    pub ascension: AscensionWeights,
    pub shop: ShopWeights,
    pub projection: ProjectionTables,
    /// Tier-2/3 escalation thresholds + probe budgets (M2,
    /// bead `sts2-rs-40ikw.6`).
    pub tiers: TierWeights,
    /// Optionality-aware pathing weights (value of flexibility in the
    /// per-floor-replanned act DAG). `#[serde(default)]` so a weights file
    /// predating the section still parses (fills from [`OptionalityWeights::default`]).
    #[serde(default)]
    pub optionality: OptionalityWeights,
}

impl Weights {
    /// The embedded default weights (parsed once, cached).
    pub fn embedded() -> &'static Weights {
        static CACHE: OnceLock<Weights> = OnceLock::new();
        CACHE.get_or_init(|| {
            toml::from_str(EMBEDDED_WEIGHTS_TOML)
                .expect("embedded data/runai/heuristic_v.toml parses as Weights")
        })
    }

    /// Load a full weights file from disk (tuning override).
    pub fn load(path: &std::path::Path) -> Result<Weights, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
    }
}

impl Default for Weights {
    fn default() -> Self {
        Self::embedded().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_weights_parse() {
        let w = Weights::embedded();
        assert!(w.hp_base > 0.0);
        assert!(w.shop.gold_reserve >= 0);
        assert_eq!(w.projection.monster_hp_loss.len(), 3);
        // Monotone act ramp on the projection tables.
        assert!(w.projection.elite_hp_loss[0] > w.projection.monster_hp_loss[0]);
    }
}
