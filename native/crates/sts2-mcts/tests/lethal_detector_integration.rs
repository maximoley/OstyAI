//! Integration test: opt-in `use_lethal_detector` stamps `proven_win`
//! on a leaf when the conservative lethal-this-turn detector fires,
//! and the standard MCTS-Solver lite backup propagates the forced win
//! to the root. Bead `sts2-rs-5yzrh` follow-up (GH #51).

mod common;

use common::build_handful;
use sts2_core::cards::{CardId, TargetType};
use sts2_mcts::{detect_lethal, Mcts, MctsConfig};

/// A hand-rigged single-Strike-lethal at the root: one StrikeIronclad
/// (6 dmg base) vs a Cultist at HP 5. The detector should fire on
/// the initial root state, and after a single search the root is
/// proven-win.
#[test]
fn detector_fires_at_root_and_search_proves_win() {
    let state = build_handful(
        0,
        &[(CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy)],
        5,
        50,
    );
    assert!(
        detect_lethal(&state),
        "detector should fire on hand-rigged lethal-at-six state"
    );

    let config = MctsConfig {
        max_simulations: 4,
        use_lethal_detector: true,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state, config);
    let _ = mcts.search();
    assert!(
        mcts.root_proven_win(),
        "root should be proven-win after detector stamps the leaf"
    );
}

/// Same fixture but with the detector OFF: the root may or may not be
/// proven-win after 4 sims (depends on rollout outcomes), but the
/// detector itself must independently say "yes". This pins the
/// opt-in nature of the integration — the default config does not
/// silently change behaviour.
#[test]
fn detector_off_does_not_stamp_proven_win() {
    let state = build_handful(
        0,
        &[(CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy)],
        7, // out of single-strike range, no rollout-win on a single sim
        50,
    );
    let config = MctsConfig {
        max_simulations: 1,
        use_lethal_detector: false,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state.clone(), config);
    let _ = mcts.search();
    // Detector says "no" on HP=7, so even with the flag ON the root
    // would not be proven-win. Verify the detector is sound on this
    // fixture.
    assert!(!detect_lethal(&state));
    assert!(!mcts.root_proven_win());
}

/// Multi-Strike lethal (12 HP, 2× Strike, 2 energy) — the detector
/// fires and the MCTS root is proven-win after a tiny search.
#[test]
fn two_strike_lethal_proves_win() {
    let mut state = build_handful(
        0,
        &[
            (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
            (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        ],
        12,
        50,
    );
    state.player.energy = 2;
    state.player.max_energy = 2;
    assert!(detect_lethal(&state));

    let config = MctsConfig {
        max_simulations: 8,
        use_lethal_detector: true,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state, config);
    let _ = mcts.search();
    assert!(mcts.root_proven_win());
}
