//! AlphaZero M2, Task 7: full-run fast-forward integration test.
//!
//! End-to-end proof that a [`PlanningRun`] driven by a trivial "always take
//! the first legal action" policy reaches `Phase::RunOver` in bounded steps
//! without ever surfacing `Phase::Combat` to the caller — every fight is
//! fast-forwarded through the [`CombatSurrogate`] seam (Tasks 2/5/6). A
//! `win_prob = 1.0` stub must ride the run all the way to a win (or at
//! least never lose); a `win_prob = 0.0` stub must die at the very first
//! combat entry.
//!
//! Uses the same fresh-Ironclad-run construction as the M1 harvest binary
//! (`crates/sts2-cli/src/bin/combat_surrogate_data.rs:258-260`) and the
//! `#[cfg(test)]` `fresh_ironclad_run` helper in `planning.rs` — that
//! helper isn't visible here (it's private to the unit-test module inside
//! the crate), so this integration test rebuilds the same sequence against
//! the public `sts2_content::run_callbacks` API.

use sts2_content::run_callbacks::{
    initialize_run_setup, new_run_manager_for_character_with_ascension,
};
use sts2_core::potions::CharacterId;
use sts2_core::run_manager::{Phase, RunManager};
use sts2_runai::eval::ranker_features::ContextFeatures;
use sts2_runai::eval::surrogate::CombatOutcome;
use sts2_runai::planning::{CombatSurrogate, PlanningRun};

/// Forced-outcome combat surrogate: every fight resolves to `win`
/// regardless of the pre-combat context or encounter index.
#[derive(Debug)]
struct Stub {
    win: f32,
}

impl CombatSurrogate for Stub {
    fn predict_outcome(&self, _ctx: &ContextFeatures, _encounter_idx: usize) -> CombatOutcome {
        CombatOutcome {
            win_prob: self.win,
            hp_frac_lost: 0.1,
        }
    }
}

/// Fresh Ironclad run through the real content callbacks + Neow start,
/// mirroring `combat_surrogate_data.rs:587-589` (and the crate-private
/// `planning::tests::fresh_ironclad_run` it's pinned to) verbatim.
fn build_run(seed: u64) -> RunManager {
    sts2_runai::rollout::ensure_content_installed();
    let mut rm = new_run_manager_for_character_with_ascension(seed, CharacterId::Ironclad, 0)
        .with_neow_start();
    initialize_run_setup(&mut rm);
    rm
}

#[test]
fn always_win_reaches_run_over_bounded() {
    let rm = build_run(1);
    let stub = Stub { win: 1.0 };
    let mut pr = PlanningRun::new(rm, &stub, 0x1234);
    let mut steps = 0usize;
    loop {
        assert_ne!(
            pr.phase(),
            Phase::Combat,
            "combat surfaced to the caller at step {steps}"
        );
        let actions = pr.legal_actions();
        assert!(
            !actions.is_empty(),
            "wedged: no legal actions at step {steps}"
        );
        let out = pr.apply(actions[0], 0);
        steps += 1;
        assert!(steps < 5000, "run did not terminate in bounded steps");
        if let Some(won) = out.terminal {
            assert!(won, "an always-win surrogate must not lose the run");
            // The winning run terminates by beating the final boss, so the
            // terminal transition is a fast-forwarded combat — proves the win
            // came through the surrogate seam, not some other RunOver path.
            assert!(
                out.stochastic,
                "the terminal (boss-win) transition must be a resolved combat"
            );
            break;
        }
    }
    assert_eq!(pr.phase(), Phase::RunOver);
}

#[test]
fn always_lose_dies_at_first_combat() {
    let rm = build_run(1);
    let stub = Stub { win: 0.0 };
    let mut pr = PlanningRun::new(rm, &stub, 0x1234);
    let mut terminal = None;
    for _ in 0..500 {
        assert_ne!(pr.phase(), Phase::Combat, "combat surfaced to the caller");
        let actions = pr.legal_actions();
        assert!(
            !actions.is_empty(),
            "wedged: no legal actions before first combat"
        );
        let out = pr.apply(actions[0], 0);
        if let Some(won) = out.terminal {
            // The loss terminates AT the first combat, so the terminal
            // transition must itself be a resolved fight (guards against a
            // vacuous Some(false) arriving from a non-combat path).
            assert!(
                out.stochastic,
                "the terminal (forced-loss) transition must be a resolved combat"
            );
            terminal = Some(won);
            break;
        }
    }
    assert_eq!(terminal, Some(false), "forced-loss run must end in a loss");
}
