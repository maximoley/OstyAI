//! Integration test for bead `sts2-rs-8hs96` quiescence-pass wiring.
//!
//! Companion to `lethal_detector_integration.rs` — same pattern as the
//! prior `sts2-rs-3fk0l` work. Verifies that:
//!
//! - The `use_quiescence` config flag is honored without panicking.
//! - With `use_quiescence = false`, the search runs unchanged.
//! - A search on a state where the rollout may encounter sole-legal
//!   transitions completes successfully with the flag on.

mod common;

use common::build_vertical_slice;
use sts2_mcts::{Mcts, MctsConfig, UniformPolicy};

#[test]
fn quiescence_on_completes_without_panic() {
    let state = build_vertical_slice(0);
    let cfg = MctsConfig {
        max_simulations: 100,
        seed: 0,
        use_quiescence: true,
        ..MctsConfig::default()
    };
    let mut mcts: Mcts<UniformPolicy> = Mcts::new(state, cfg);
    let _best = mcts.search();
    let visits: u32 = mcts.root_visits().iter().map(|(_, v, _)| *v).sum();
    assert_eq!(visits, cfg.max_simulations);
}

#[test]
fn quiescence_off_matches_default_path() {
    let state = build_vertical_slice(7);
    let cfg_off = MctsConfig {
        max_simulations: 100,
        seed: 7,
        use_quiescence: false,
        ..MctsConfig::default()
    };
    let cfg_default = MctsConfig {
        max_simulations: 100,
        seed: 7,
        ..MctsConfig::default()
    };
    let mut a: Mcts<UniformPolicy> = Mcts::new(state.clone(), cfg_off);
    let _ = a.search();
    let mut b: Mcts<UniformPolicy> = Mcts::new(state, cfg_default);
    let _ = b.search();

    let av = a.root_visits().to_vec();
    let bv = b.root_visits().to_vec();
    assert_eq!(av.len(), bv.len());
    for (x, y) in av.iter().zip(bv.iter()) {
        assert_eq!(x.1, y.1, "visit counts diverged with quiescence off");
    }
}

#[test]
fn quiescence_repeated_search_is_deterministic() {
    let state = build_vertical_slice(13);
    let cfg = MctsConfig {
        max_simulations: 150,
        seed: 13,
        use_quiescence: true,
        ..MctsConfig::default()
    };
    let mut a: Mcts<UniformPolicy> = Mcts::new(state.clone(), cfg);
    let best_a = a.search();
    let mut b: Mcts<UniformPolicy> = Mcts::new(state, cfg);
    let best_b = b.search();
    assert_eq!(best_a, best_b);
}
