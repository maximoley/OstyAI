//! Champion-run boss/elite/common fixture harvest (bead `sts2-rs-cws0i`).
//!
//! `run_arena sweep --harvest-fixtures <dir>` needs to snapshot the live
//! `RunManager` at every combat ENTRY (before any action) whose room tier
//! is in the configured set. The snapshot → `ScenarioFile` TOML
//! serialization needs `sts2-cli`'s `ScenarioFile`/`resolve_potions`
//! types, which this crate cannot depend on (`sts2-cli` depends on
//! `sts2-runai`'s sibling crates, not the other way — see the workspace
//! layout in `CLAUDE.md`). So the write-out itself is a caller-supplied
//! hook: `run_arena` registers one process-wide callback via
//! [`set_harvest_hook`] before driving any cells, and [`HarvestPolicy`]
//! (a [`RunPolicy`] decorator in the same shape as
//! [`crate::trajectory::RecordingPolicy`] / [`crate::policy::AuditingPolicy`])
//! fires it at combat entry.
//!
//! The hook is a single `OnceLock`, not per-`CellSpec` state, because
//! [`crate::arena::CellSpec`] is `Copy` (cheaply threaded through rayon)
//! and a boxed closure isn't. That also fixes the concurrency model: a
//! sweep's cells run as rayon threads *within one process* when
//! harvesting (see `run_arena`'s `sweep()`, which forces the in-process
//! path whenever `--harvest-fixtures` is set) — `unsafe_code = "deny"`
//! workspace-wide rules out an `flock`-style cross-process lock for the
//! default process-per-seed path, so the harvest writer is guarded by a
//! plain in-process `Mutex` instead.

use std::sync::{Arc, OnceLock};

use sts2_core::full_run::RunPolicy;
use sts2_core::potions::CharacterId;
use sts2_core::run_manager::{Phase, RunAction, RunManager};
use sts2_core::runstate::RoomType;

use crate::policy::{CombatSearchRecord, DecisionTelemetry, MetaEvalRecord, VScoreSource};

/// Room tier the harvest can filter on. Mirrors the three combat-shaped
/// [`RoomType`] variants; `RoomType::Unknown` and non-combat room types
/// never reach [`HarvestPolicy::choose`]'s combat-entry branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarvestTier {
    Common,
    Elite,
    Boss,
}

impl HarvestTier {
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            HarvestTier::Common => "common",
            HarvestTier::Elite => "elite",
            HarvestTier::Boss => "boss",
        }
    }

    fn from_room_type(rt: RoomType) -> Option<Self> {
        match rt {
            RoomType::Monster => Some(HarvestTier::Common),
            RoomType::Elite => Some(HarvestTier::Elite),
            RoomType::Boss => Some(HarvestTier::Boss),
            _ => None,
        }
    }
}

/// Which tiers `--harvest-tiers` selected. Default (CLI unset) is
/// boss-only — see the Task 10 brief (boss-pool coverage is the gap
/// this harvest exists to close).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HarvestTierSet {
    pub common: bool,
    pub elite: bool,
    pub boss: bool,
}

impl HarvestTierSet {
    #[must_use]
    pub fn boss_only() -> Self {
        Self {
            common: false,
            elite: false,
            boss: true,
        }
    }

    #[must_use]
    pub fn all() -> Self {
        Self {
            common: true,
            elite: true,
            boss: true,
        }
    }

    /// Parse the `--harvest-tiers` token: `all`, or a comma-separated
    /// subset of `common|elite|boss` (e.g. `boss,elite`).
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("all") {
            return Ok(Self::all());
        }
        let mut out = Self::default();
        for tok in s.split(',') {
            match tok.trim().to_ascii_lowercase().as_str() {
                "common" => out.common = true,
                "elite" => out.elite = true,
                "boss" => out.boss = true,
                "" => {}
                other => {
                    return Err(format!(
                        "unknown --harvest-tiers token '{other}' (expected: \
                         all, or a comma list of common|elite|boss)"
                    ))
                }
            }
        }
        if !(out.common || out.elite || out.boss) {
            return Err("--harvest-tiers resolved to an empty set".to_string());
        }
        Ok(out)
    }

    fn contains(self, tier: HarvestTier) -> bool {
        match tier {
            HarvestTier::Common => self.common,
            HarvestTier::Elite => self.elite,
            HarvestTier::Boss => self.boss,
        }
    }
}

/// Everything the harvest hook needs about the just-entered combat,
/// beyond the `RunManager` itself.
#[derive(Clone, Copy, Debug)]
pub struct HarvestEntry {
    pub tier: HarvestTier,
    pub floor: i32,
    pub seed: u64,
    pub character: CharacterId,
    pub ascension: u8,
}

/// A caller-supplied combat-entry callback. `Send + Sync` so the same
/// registration is usable from every rayon worker thread.
pub type HarvestHook = Arc<dyn Fn(&RunManager, HarvestEntry) + Send + Sync>;

static HOOK: OnceLock<(HarvestHook, HarvestTierSet)> = OnceLock::new();

/// Register the process-wide harvest hook + tier filter. Call once,
/// before driving any cells (`run_arena sweep --harvest-fixtures` calls
/// this before `run_panel`/`run_cell`). A second call is a no-op
/// (`OnceLock`: first registration wins) — callers only register once
/// per process.
pub fn set_harvest_hook(hook: HarvestHook, tiers: HarvestTierSet) {
    let _ = HOOK.set((hook, tiers));
}

fn room_tier(rm: &RunManager) -> Option<HarvestTier> {
    rm.run_state
        .current_room
        .as_ref()
        .and_then(|room| HarvestTier::from_room_type(room.room_type))
}

/// [`RunPolicy`] decorator: on combat ENTRY (the first `choose()` call
/// after the phase becomes [`Phase::Combat`], i.e. before any action is
/// taken this fight — the same point `corpus_gen`'s god-mode sampler
/// snapshots, see `drive_god_run_to_target`), if a hook is registered
/// and the room tier is in its configured set, invokes the hook with the
/// pre-action `RunManager`.
///
/// Always wraps (`HarvestPolicy::wrap` is unconditional in `run_cell`) so
/// the wrapped type is static regardless of whether `--harvest-fixtures`
/// was passed; `choose` gates the one dynamic check per decision on
/// `HOOK.get()`, so an unset hook costs a single `OnceLock::get()` —
/// "dead cheap" per the Task 10 brief.
#[derive(Debug)]
pub struct HarvestPolicy<P> {
    inner: P,
    seed: u64,
    character: CharacterId,
    ascension: u8,
    in_combat: bool,
}

impl<P: RunPolicy> HarvestPolicy<P> {
    pub fn wrap(inner: P, seed: u64, character: CharacterId, ascension: u8) -> Self {
        Self {
            inner,
            seed,
            character,
            ascension,
            in_combat: false,
        }
    }

    /// Borrow the wrapped policy (mirrors `RecordingPolicy::inner` /
    /// `AuditingPolicy::inner` so `run_cell`'s post-run telemetry reads
    /// stay a one-line `.inner()` swap regardless of harvest wrapping).
    pub fn inner(&self) -> &P {
        &self.inner
    }
}

impl<P: RunPolicy> RunPolicy for HarvestPolicy<P> {
    fn prepare(&mut self, rm: &mut RunManager) {
        self.inner.prepare(rm);
    }

    fn choose(&mut self, rm: &RunManager, actions: &[RunAction]) -> Option<usize> {
        if rm.phase() == Phase::Combat {
            if !self.in_combat {
                self.in_combat = true;
                if let Some((hook, tiers)) = HOOK.get() {
                    if let Some(tier) = room_tier(rm) {
                        if tiers.contains(tier) {
                            let entry = HarvestEntry {
                                tier,
                                floor: rm.run_state.total_floor,
                                seed: self.seed,
                                character: self.character,
                                ascension: self.ascension,
                            };
                            hook(rm, entry);
                        }
                    }
                }
            }
        } else {
            self.in_combat = false;
        }
        self.inner.choose(rm, actions)
    }
}

impl<P: DecisionTelemetry> DecisionTelemetry for HarvestPolicy<P> {
    fn combat_search(&self) -> Option<&CombatSearchRecord> {
        self.inner.combat_search()
    }

    fn take_meta_eval(&mut self) -> Option<MetaEvalRecord> {
        self.inner.take_meta_eval()
    }
}

impl<P: VScoreSource> VScoreSource for HarvestPolicy<P> {
    fn take_v_scores(&mut self) -> Option<(Vec<f64>, f64)> {
        self.inner.take_v_scores()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_set_parses_all() {
        let t = HarvestTierSet::parse("all").unwrap();
        assert_eq!(t, HarvestTierSet::all());
    }

    #[test]
    fn tier_set_parses_comma_list() {
        let t = HarvestTierSet::parse("boss,elite").unwrap();
        assert!(t.boss && t.elite && !t.common);
    }

    #[test]
    fn tier_set_default_boss_only() {
        let t = HarvestTierSet::boss_only();
        assert!(t.boss && !t.elite && !t.common);
    }

    #[test]
    fn tier_set_rejects_unknown_token() {
        assert!(HarvestTierSet::parse("bosses").is_err());
    }

    #[test]
    fn tier_set_rejects_empty() {
        assert!(HarvestTierSet::parse("").is_err());
    }

    #[test]
    fn tier_set_contains_matches_variant() {
        let t = HarvestTierSet::parse("elite").unwrap();
        assert!(t.contains(HarvestTier::Elite));
        assert!(!t.contains(HarvestTier::Boss));
        assert!(!t.contains(HarvestTier::Common));
    }
}
