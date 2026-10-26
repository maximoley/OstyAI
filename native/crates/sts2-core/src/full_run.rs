//! Seeded full-run driver — the engine-level analog of the C# game's
//! `AutoSlay` harness.
//!
//! Bead `sts2-rs` (AutoSlay-inspired full-run fuzzer, B3/B4): the C#
//! `MegaCrit.Sts2.Core.AutoSlay` harness plays a whole seeded run with
//! uniform-random-but-legal actions, guarded by a watchdog, to surface
//! crashes/hangs. This is the headless engine-level version: it drives a
//! callback-wired [`RunManager`] through [`RunManager::get_available_actions`]
//! / [`RunManager::take_action`] picking actions from a pluggable
//! [`RunPolicy`], with a deterministic livelock detector (run-hash stall)
//! and a hard step cap in place of the C#'s real-time watchdog.
//!
//! The driver is **content-agnostic**: it drives whatever `RunManager`
//! it's handed. The caller wires the content layer (e.g.
//! `sts2_content::run_callbacks::new_run_manager_for_character_from_string_seed`)
//! before calling [`drive_full_run`]. This keeps the `content → core`
//! dependency direction intact, exactly like the PyO3 `random_playout`
//! convenience that this generalizes.
//!
//! ## Outcomes vs P9 gating
//!
//! Run-state porting (P9) is incomplete, so many [`RunAction`] variants
//! still route to [`ActionResult::NotImplemented`]. That is **not** a
//! crash — it's "this phase isn't ported yet", surfaced as
//! [`RunOutcome::NotImplemented`] (a clean stop). A genuine anomaly —
//! the engine *rejecting* an action that `get_available_actions` just
//! surfaced — is [`RunOutcome::Rejected`]. Crashes (panics) are caught
//! by the *caller's* sweep harness (see the `fuzz` CLI subcommand), not
//! here: the core driver stays panic-transparent so unit tests get clean
//! backtraces.

use rand::Rng;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;

use crate::hashing::run_hash;
use crate::run_manager::{ActionResult, Phase, RunAction, RunManager};
use crate::CombatState;

/// HP the player is pinned to at combat start in `--invincible` coverage
/// mode. The C# AutoSlay uses `PlatingPower`/`RegenPower` 999, but
/// `Plating` is an un-hooked marker stub at this pin (no damage-reduction
/// listener), so we inflate HP directly instead — engine-independent and
/// large enough to survive any Act 1–3 combat under random play.
pub const INVINCIBLE_HP: i32 = 100_000;

/// Default hard cap on total actions per run (matches the PyO3
/// `random_playout` default).
pub const DEFAULT_MAX_STEPS: usize = 100_000;

/// Default livelock window: this many consecutive actions with no change
/// to [`run_hash`] is treated as a stall. Generous enough that any real
/// progress resets the counter.
pub const DEFAULT_LIVELOCK_STALL_LIMIT: usize = 256;

/// Pluggable decision policy at the *run* level. The combat-side `Agent`
/// trait (in `sts2-cli`) is `Action`/`CombatState`-scoped and cannot be
/// reused here; `RunPolicy` is its run-level sibling, deliberately shaped
/// the same way so a future bridge is trivial.
pub trait RunPolicy {
    /// Pick an index into `actions` (guaranteed non-empty). Returning
    /// `None` halts the run cleanly ([`RunOutcome::PolicyHalt`]) — used
    /// by a replay policy that has exhausted its recorded trace.
    fn choose(&mut self, rm: &RunManager, actions: &[RunAction]) -> Option<usize>;

    /// Mutating pre-decision hook, called by [`drive_full_run`]
    /// immediately before [`RunManager::get_available_actions`] on
    /// every loop iteration. Default no-op — every existing policy is
    /// unaffected. Added for the PPO run-policy (2026-07-07
    /// ppo-rust-inference plan, Task 8): `observe()`'s event surface
    /// needs `&mut RunManager` to materialize the pending event
    /// (`materialize_event`) before `choose` can build an observation
    /// over `&RunManager`, and `choose`'s signature is `&RunManager`
    /// by design (every other policy is pure over the run state).
    /// Materialization is idempotent and RNG-free, so calling this on
    /// every iteration for policies that don't need it costs nothing
    /// observable.
    fn prepare(&mut self, _rm: &mut RunManager) {}
}

/// Uniform-random policy over the legal-action set (the AutoSlay default).
#[derive(Debug)]
pub struct RandomPolicy {
    rng: Xoshiro256PlusPlus,
}

impl RandomPolicy {
    /// Seed the policy RNG directly.
    #[must_use]
    pub fn from_seed(policy_seed: u64) -> Self {
        Self {
            rng: Xoshiro256PlusPlus::seed_from_u64(policy_seed),
        }
    }

    /// Derive the policy seed from the run's integer seed the same way
    /// the PyO3 `random_playout` does (so behavior matches that path).
    #[must_use]
    pub fn from_run_seed(run_seed: impl crate::dotnet_rng::RngSeed) -> Self {
        let seed_u64 = run_seed.rng_seed();
        Self::from_seed(seed_u64 ^ 0xA5A5_A5A5_DEAD_BEEF)
    }
}

impl RunPolicy for RandomPolicy {
    fn choose(&mut self, _rm: &RunManager, actions: &[RunAction]) -> Option<usize> {
        // `actions` is non-empty (the driver guarantees it); pick uniformly.
        if actions.is_empty() {
            None
        } else {
            Some(self.rng.gen_range(0..actions.len()))
        }
    }
}

/// How a full run terminated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    /// Reached [`Phase::RunOver`] with `player_won == true`.
    Won,
    /// Reached [`Phase::RunOver`] with `player_won == false`.
    Lost,
    /// Hit the `max_steps` cap without finishing.
    MaxSteps,
    /// `run_hash` was unchanged for `livelock_stall_limit` consecutive
    /// actions — the run made no observable progress.
    Livelock { phase: Phase, step: usize },
    /// [`RunManager::get_available_actions`] returned empty before the
    /// run was over — the state machine wedged.
    EmptyActions { phase: Phase, step: usize },
    /// The chosen action routed to [`ActionResult::NotImplemented`] — an
    /// unported phase (expected during P9), a clean stop, not a crash.
    NotImplemented {
        phase: Phase,
        action: RunAction,
        step: usize,
    },
    /// The engine *rejected* an action that `get_available_actions` just
    /// surfaced (`InvalidForPhase` / a shop failure / any other
    /// non-`Advanced`, non-`RunOver` result). An anomaly worth recording.
    Rejected {
        phase: Phase,
        action: RunAction,
        result: String,
        step: usize,
    },
    /// The policy returned `None` (e.g. a replay trace ran out).
    PolicyHalt { phase: Phase, step: usize },
}

impl RunOutcome {
    /// `true` for outcomes that indicate an engine bug worth a failing
    /// fixture (livelock, wedged state machine, surfaced-then-rejected
    /// action). `NotImplemented` is **not** anomalous — it's P9 gating.
    #[must_use]
    pub fn is_anomaly(&self) -> bool {
        matches!(
            self,
            RunOutcome::Livelock { .. }
                | RunOutcome::EmptyActions { .. }
                | RunOutcome::Rejected { .. }
        )
    }
}

/// Knobs for [`drive_full_run`].
#[derive(Clone, Copy, Debug)]
pub struct FullRunConfig {
    /// Hard cap on total actions (see [`DEFAULT_MAX_STEPS`]).
    pub max_steps: usize,
    /// Livelock window (see [`DEFAULT_LIVELOCK_STALL_LIMIT`]). `0`
    /// disables livelock detection.
    pub livelock_stall_limit: usize,
    /// Coverage mode: buff the player to near-invincibility at combat
    /// start so random play penetrates deeper into the run. Wired in B5.
    /// **Mutually exclusive with `record_trace`** (it perturbs combat
    /// RNG/state, so an invincible run must never be replayed for parity).
    pub invincible: bool,
    /// Record the action trace for replay/parity (wired in B6).
    pub record_trace: bool,
}

impl Default for FullRunConfig {
    fn default() -> Self {
        Self {
            max_steps: DEFAULT_MAX_STEPS,
            livelock_stall_limit: DEFAULT_LIVELOCK_STALL_LIMIT,
            invincible: false,
            record_trace: false,
        }
    }
}

/// Summary of a driven run.
#[derive(Clone, Debug)]
pub struct FullRunReport {
    pub outcome: RunOutcome,
    pub steps: usize,
    pub final_phase: Phase,
    pub final_hp: i32,
    pub final_gold: i32,
    pub act_index: u8,
    pub player_won: bool,
}

/// Drive `rm` with `policy` until it finishes, stalls, or hits a cap.
///
/// The `RunManager` must already have its content callbacks wired (a bare
/// [`RunManager::new`] returns [`ActionResult::NotImplemented`] for every
/// action). Panic-transparent by design — the caller's sweep wraps this
/// in `catch_unwind` to convert a panic into a recorded failing seed.
#[must_use]
pub fn drive_full_run(
    rm: &mut RunManager,
    policy: &mut dyn RunPolicy,
    cfg: &FullRunConfig,
) -> FullRunReport {
    debug_assert!(
        !(cfg.invincible && cfg.record_trace),
        "invincible mode perturbs combat state and must not be trace-recorded"
    );

    let mut steps = 0usize;
    let mut last_hash = run_hash(rm);
    let mut stall = 0usize;

    let outcome = loop {
        if rm.is_over() {
            break won_or_lost(rm);
        }
        if steps >= cfg.max_steps {
            break RunOutcome::MaxSteps;
        }

        policy.prepare(rm);
        let actions = rm.get_available_actions();
        if actions.is_empty() {
            break RunOutcome::EmptyActions {
                phase: rm.phase(),
                step: steps,
            };
        }

        let Some(idx) = policy.choose(rm, &actions) else {
            break RunOutcome::PolicyHalt {
                phase: rm.phase(),
                step: steps,
            };
        };
        let action = actions[idx];
        let phase = rm.phase();

        match rm.take_action(action) {
            ActionResult::Advanced => {}
            ActionResult::RunOver => break won_or_lost(rm),
            ActionResult::NotImplemented => {
                break RunOutcome::NotImplemented {
                    phase,
                    action,
                    step: steps,
                }
            }
            other => {
                break RunOutcome::Rejected {
                    phase,
                    action,
                    result: format!("{other:?}"),
                    step: steps,
                }
            }
        }

        steps += 1;

        // ── Invincible coverage mode (B5): keep the player pinned to
        //    huge HP for the duration of every combat so random play
        //    survives deep into the run. Re-applied each combat step
        //    (not just on entry) because the player mirror creature is
        //    created lazily *after* combat entry (combat_loop.rs ~138) and
        //    seeded from the original HP — an entry-only buff would be
        //    overwritten. `apply_invincible_buffs` is idempotent once HP
        //    is maxed, so it does not perturb the livelock hash in a truly
        //    stuck combat.
        if cfg.invincible {
            if let Some(combat) = rm.combat.as_mut() {
                apply_invincible_buffs(combat);
            }
        }

        // ── Livelock watchdog (B4): run-hash stall detection ────────────
        if cfg.livelock_stall_limit > 0 {
            let h = run_hash(rm);
            if h == last_hash {
                stall += 1;
                if stall >= cfg.livelock_stall_limit {
                    break RunOutcome::Livelock {
                        phase: rm.phase(),
                        step: steps,
                    };
                }
            } else {
                stall = 0;
                last_hash = h;
            }
        }
    };

    FullRunReport {
        outcome,
        steps,
        final_phase: rm.phase(),
        final_hp: rm.run_state.current_hp,
        final_gold: rm.run_state.gold,
        act_index: rm.run_state.act_index,
        player_won: rm.run_state.player_won,
    }
}

/// Pin the player to [`INVINCIBLE_HP`] so a random-play run penetrates
/// deep into the run for crash coverage instead of dying early — the
/// engine-level analog of the C# AutoSlay coverage trick.
///
/// Sets both the mirror creature's HP (the d03-unified source of truth
/// when `creature_uid` is set) and the legacy `PlayerCombatState` fields,
/// keeping them consistent so the mirror invariant holds either way.
///
/// Must run at *combat start*, before the player's first action. Never
/// used on a trace-recorded run — it perturbs combat state.
pub fn apply_invincible_buffs(combat: &mut CombatState) {
    if let Some(uid) = combat.player.creature_uid {
        if let Some(c) = combat.creatures.iter_mut().find(|c| c.id == uid.0) {
            c.max_hp = INVINCIBLE_HP;
            c.current_hp = INVINCIBLE_HP;
        }
    }
    combat.player.max_hp = INVINCIBLE_HP;
    combat.player.current_hp = INVINCIBLE_HP;
}

fn won_or_lost(rm: &RunManager) -> RunOutcome {
    if rm.run_state.player_won {
        RunOutcome::Won
    } else {
        RunOutcome::Lost
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_manager::RunManager;
    use crate::CharacterId;

    /// A bare `RunManager` (no content callbacks) returns
    /// `NotImplemented` for everything — but `MapChoice` surfaces no
    /// actions in the skeleton, so the driver stops at `EmptyActions`.
    /// Either way it terminates without panicking and reports a phase.
    #[test]
    fn bare_run_manager_terminates_cleanly() {
        let mut rm = RunManager::new(7, CharacterId::Ironclad);
        let mut policy = RandomPolicy::from_run_seed(7);
        let cfg = FullRunConfig::default();
        let report = drive_full_run(&mut rm, &mut policy, &cfg);
        // Must terminate (not loop forever) and not be Won (no content).
        assert!(!matches!(report.outcome, RunOutcome::Won));
        assert_eq!(report.final_phase, rm.phase());
    }

    /// A zero `max_steps` budget yields `MaxSteps` immediately (unless the
    /// run is already over, which a fresh run is not).
    #[test]
    fn zero_budget_hits_max_steps() {
        let mut rm = RunManager::new(1, CharacterId::Ironclad);
        let mut policy = RandomPolicy::from_run_seed(1);
        let cfg = FullRunConfig {
            max_steps: 0,
            ..FullRunConfig::default()
        };
        let report = drive_full_run(&mut rm, &mut policy, &cfg);
        assert_eq!(report.outcome, RunOutcome::MaxSteps);
        assert_eq!(report.steps, 0);
    }

    #[test]
    fn outcome_anomaly_classification() {
        assert!(RunOutcome::EmptyActions {
            phase: Phase::MapChoice,
            step: 0
        }
        .is_anomaly());
        assert!(!RunOutcome::NotImplemented {
            phase: Phase::MapChoice,
            action: RunAction::Unhandled,
            step: 0
        }
        .is_anomaly());
        assert!(!RunOutcome::Won.is_anomaly());
    }
}
