//! `Mcts::rollout_stats` (Phase-0 instrument, 2026-08-15 high-asc
//! campaign) regression tests.
//!
//! A tiny `max_rollout_depth` forces every non-decisive rollout to
//! return via the depth-cap branch (`depth_cut_value`), while a huge
//! cap lets the (short, non-lethal) fixture combat run to a genuine
//! terminal state every time — so `depth_cap_hits` should be `0`.

mod common;

use common::build_handful;
use sts2_core::cards::{CardId, TargetType};
use sts2_mcts::{Mcts, MctsConfig, RolloutPolicyKind};

#[test]
fn rollout_stats_count_depth_cap_hits() {
    // Non-lethal opening (defends only vs a Cultist that outpaces
    // them) — every root child needs to run an actual rollout instead
    // of short-circuiting through the proven-win/loss solver path.
    let hand = [
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
    ];
    let state = build_handful(
        /*seed=*/ 0, &hand, /*cultist_hp=*/ 40, /*player_hp=*/ 30,
    );

    let config = MctsConfig {
        max_simulations: 50,
        max_rollout_depth: 1,
        seed: 0,
        rollout_policy: RolloutPolicyKind::Uniform,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state.clone(), config);
    let _ = mcts.search();
    let (rollouts, cap_hits) = mcts.rollout_stats();
    assert!(rollouts > 0, "expected at least one real rollout");
    assert!(cap_hits > 0, "depth cap 1 must be hit");

    // And at a generous cap on this short, non-lethal fight, hits ≈ 0
    // — the combat naturally terminates (win or loss) long before
    // 10,000 plies.
    let config2 = MctsConfig {
        max_simulations: 50,
        max_rollout_depth: 10_000,
        seed: 0,
        rollout_policy: RolloutPolicyKind::Uniform,
        ..MctsConfig::default()
    };
    let mut mcts2 = Mcts::new(state, config2);
    let _ = mcts2.search();
    let (r2, c2) = mcts2.rollout_stats();
    assert!(r2 > 0, "expected at least one real rollout");
    assert_eq!(c2, 0, "generous depth cap on a short fight must not be hit");
}
