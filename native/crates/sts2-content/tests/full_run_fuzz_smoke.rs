//! Full-run fuzzer smoke + determinism test — bead `sts2-rs` (AutoSlay
//! fuzzer, B3/B4).
//!
//! Drives a real content-wired Ironclad run with the uniform-random
//! [`RandomPolicy`] through the core [`drive_full_run`] loop and asserts:
//!
//! 1. The run terminates (no hang) with a sensible [`RunOutcome`].
//! 2. It makes progress (random play advances past the first action).
//! 3. Determinism: the same `(run seed, policy seed)` produces an
//!    identical [`FullRunReport`] and an identical final [`run_hash`] on a
//!    second construction — the run-level analog of the `combat_step`
//!    fuzz target's double-execution determinism check.

use sts2_content::run_callbacks::new_run_manager_for_character_from_string_seed;
use sts2_core::full_run::{drive_full_run, FullRunConfig, RandomPolicy, RunOutcome};
use sts2_core::{run_hash, CharacterId};

/// Build + drive one Ironclad run from a string seed with a random policy
/// derived deterministically from the run's integer seed.
fn drive(seed: &str) -> (sts2_core::FullRunReport, u64) {
    drive_with(seed, FullRunConfig::default())
}

fn drive_with(seed: &str, cfg: FullRunConfig) -> (sts2_core::FullRunReport, u64) {
    let mut rm = new_run_manager_for_character_from_string_seed(seed, CharacterId::Ironclad);
    let mut policy = RandomPolicy::from_run_seed(rm.seed);
    let report = drive_full_run(&mut rm, &mut policy, &cfg);
    let final_hash = run_hash(&rm);
    (report, final_hash)
}

#[test]
fn random_run_terminates_and_makes_progress() {
    let (report, _) = drive("FUZZSMOKE1");
    // Random play should advance at least a few actions before it loses,
    // stalls, or hits unported content — i.e. the driver actually drives.
    assert!(
        report.steps > 0,
        "random policy made no progress: {:?}",
        report.outcome
    );
    // It must terminate with *some* outcome (the `loop` always breaks);
    // reaching here at all proves no hang within the step cap.
}

#[test]
fn random_run_is_deterministic() {
    let (r1, h1) = drive("FUZZSMOKE2");
    let (r2, h2) = drive("FUZZSMOKE2");
    assert_eq!(r1.outcome, r2.outcome, "outcome diverged across replays");
    assert_eq!(r1.steps, r2.steps, "step count diverged");
    assert_eq!(r1.final_phase, r2.final_phase, "final phase diverged");
    assert_eq!(r1.final_hp, r2.final_hp, "final HP diverged");
    assert_eq!(r1.final_gold, r2.final_gold, "final gold diverged");
    assert_eq!(r1.player_won, r2.player_won, "player_won diverged");
    assert_eq!(h1, h2, "final run_hash diverged across identical replays");
}

#[test]
fn invincible_run_is_deterministic() {
    // Fast (bounded) check that the invincible code path runs without
    // panic and is deterministic. The unbounded "never loses" proof is
    // the `slow_` test below (invincibility makes random play drag full
    // acts to completion, ~20s).
    let cfg = FullRunConfig {
        invincible: true,
        max_steps: 500,
        ..FullRunConfig::default()
    };
    let (r1, h1) = drive_with("FUZZINVINC", cfg);
    let (r2, h2) = drive_with("FUZZINVINC", cfg);
    assert_eq!(r1.outcome, r2.outcome, "invincible outcome diverged");
    assert_eq!(h1, h2, "invincible final run_hash diverged");
}

/// Long soak proving invincibility prevents death: the player is pinned to
/// huge HP at every combat step, so the run can never reach `RunOver` as a
/// `Lost`. `slow_` (nightly only) — ~30s, vs the 500-step sibling above.
///
/// NOTE: this does *not* drive a run to completion, and must not be
/// described as if it did. Random play + invincibility gets wedged in an
/// act-2 combat it cannot win, so every cap — 5k through 100k — ends
/// `MaxSteps` in `Phase::Combat` at `act_index == 2`. The cap only sets how
/// long the soak runs.
///
/// The cap is explicit because step cost is **quadratic**, not linear
/// (measured, debug build: 5k → 5s, 10k → 28s, 20k → 131s, 100k → >25min).
/// It previously inherited `DEFAULT_MAX_STEPS` (100_000) and silently grew
/// into a >25-minute test that the `slow` nextest profile killed at its 360s
/// cap — i.e. this assertion had stopped running at all. Raise the cap only
/// with that curve in mind.
#[test]
fn slow_invincible_run_never_loses() {
    let cfg = FullRunConfig {
        invincible: true,
        max_steps: 10_000,
        ..FullRunConfig::default()
    };
    let (report, _) = drive_with("FUZZINVINC", cfg);
    assert_ne!(
        report.outcome,
        RunOutcome::Lost,
        "an invincible run reached RunOver as a LOSS — the player died despite HP pinning"
    );
}

#[test]
fn distinct_seeds_diverge() {
    // Sanity: different seeds should (almost always) produce different
    // run hashes. Two fixed, distinct seeds suffice for a regression guard.
    let (_, h_a) = drive("FUZZSMOKE_A");
    let (_, h_b) = drive("FUZZSMOKE_B");
    assert_ne!(h_a, h_b, "distinct seeds collided on final run_hash");
}
