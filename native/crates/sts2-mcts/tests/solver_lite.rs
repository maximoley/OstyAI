//! MCTS-Solver lite (bead `sts2-rs-wr8f`, Wave-4) regression tests.
//!
//! Forced-lethal position: 6 HP Cultist, single Strike for exactly 6
//! damage in hand. The Strike child resolves to a terminal-win state
//! immediately; `MctsNode::new` flags it `proven_win` and backup
//! propagates the flag root-ward.
//!
//! Acceptance: `rollouts_skipped >= 0.30 * max_simulations` once the
//! solver has surfaced the proven leaf and UCT keeps re-selecting it.

mod common;

use common::build_handful;
use sts2_core::cards::{CardId, TargetType};
use sts2_mcts::{Mcts, MctsConfig, RolloutPolicyKind};

#[test]
fn solver_lite_skips_rollouts_on_forced_lethal() {
    let hand = [
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
    ];
    let state = build_handful(
        /*seed=*/ 0, &hand, /*cultist_hp=*/ 6, /*player_hp=*/ 30,
    );
    let cfg = MctsConfig {
        max_simulations: 1000,
        seed: 0,
        rollout_policy: RolloutPolicyKind::Uniform,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state, cfg);
    let _ = mcts.search();

    assert!(
        mcts.root_proven_win(),
        "root should be proven_win once a Strike-kill child surfaces; got proven_win=false"
    );
    let skipped = mcts.rollouts_skipped();
    let threshold = (u64::from(cfg.max_simulations) * 30) / 100;
    assert!(
        skipped >= threshold,
        "rollouts_skipped ({skipped}) must be ≥30% of {} sims (>= {threshold})",
        cfg.max_simulations,
    );
}

#[test]
fn solver_lite_inert_on_non_lethal_position() {
    // No card kills this turn — the Cultist sits at 40 HP, hand has only
    // Defends. The solver should never flag the root proven (no terminal
    // child surfaces under any first action) and `rollouts_skipped`
    // stays at 0.
    let hand = [
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
    ];
    let state = build_handful(
        /*seed=*/ 0, &hand, /*cultist_hp=*/ 40, /*player_hp=*/ 30,
    );
    let cfg = MctsConfig {
        max_simulations: 200,
        seed: 0,
        rollout_policy: RolloutPolicyKind::Uniform,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state, cfg);
    let _ = mcts.search();

    assert!(
        !mcts.root_proven_win(),
        "root must not be proven_win on a non-lethal opening"
    );
}
