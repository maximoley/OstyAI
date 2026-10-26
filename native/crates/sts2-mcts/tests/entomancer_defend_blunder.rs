//! Regression guard for a workbench bug report: Ironclad, post-Bloodletting
//! 2 energy, hand=[Defend, Defend, Strike], 11 block, facing Entomancer's BEES
//! telegraph (`MultiAttack { damage: 3, hits: 7 }` = 21 incoming). The user saw
//! the MCTS hint recommend *End Turn* and (rightly) found it nonsensical — two
//! Defends (10 block) fully wall the incoming 21 vs the bare 11 block (which
//! eats 10).
//!
//! Investigation result: the MCTS is NOT wrong on the faithful state. At the
//! production recommender config (sims=300, depth=200, c_puct=1.4 — see
//! `sts2-pycli/python/sts2_hint/recommender.py::RecommendConfig`) it ranks a
//! Defend strictly above End Turn, and End Turn lands *last*. The reported
//! symptom only appears when the recommender is handed a state with **0
//! energy** (no card is affordable, so End Turn is the only legal move and is
//! trivially "best") — i.e. an upstream energy/state desync, not a search bug.
//!
//! This test pins the correct behavior at the production config so an eval /
//! search change that reintroduces the "end turn over an affordable wall"
//! blunder fails CI.
//!
//! Note on rollout depth: at a *shallow* depth cap (e.g. 20) the 145-HP fight
//! never finishes in rollouts, every line depth-caps into a Tie, and the
//! heavily-squashed in-progress HCE can no longer separate "block 10" from
//! "eat 10" — at depth=20 End Turn does win. Production runs depth=200, which
//! lets rollouts terminate and `evaluate_victory` (HP-monotone) reward the
//! wall, so this guard pins the production config specifically.

mod common;

use common::build_handful_ex;
use sts2_core::cards::{CardId, TargetType};
use sts2_core::{Action, PotionKind};
use sts2_mcts::{Mcts, MctsConfig, TerminalEvalMode};

/// 145-HP enemy telegraphing 7×3 (Entomancer BEES). Hand is two Defends
/// (block 5 each) and a Strike (6 dmg). Player starts with 11 block and 2
/// energy (post-Bloodletting).
fn entomancer_fixture(player_hp: i32) -> sts2_core::CombatState {
    let mut state = build_handful_ex(
        1,
        &[
            (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
            (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
            (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        ],
        /* enemy_hp= */ 145,
        player_hp,
        sts2_core::Intent::MultiAttack { damage: 3, hits: 7 },
        &[] as &[PotionKind],
        /* energy= */ 2,
    );
    state.player.block = 11;
    state
}

/// Production recommender defaults (sims=300, depth=200, c_puct=1.4).
fn production_config() -> MctsConfig {
    MctsConfig {
        c_puct: 1.4,
        max_simulations: 300,
        max_rollout_depth: 200,
        terminal_eval_mode: TerminalEvalMode::RichBoth,
        seed: 1,
        ..MctsConfig::default()
    }
}

#[test]
fn entomancer_block_is_preferred_over_end_turn() {
    for &hp in &[20_i32, 35, 50, 70] {
        let mut mcts = Mcts::new(entomancer_fixture(hp), production_config());
        let best = mcts.search();
        let outcomes = mcts.root_outcomes();

        let end_q = outcomes
            .iter()
            .find(|o| matches!(o.action, Action::EndTurn))
            .map(|o| o.mean_value)
            .expect("EndTurn must be a legal root action");
        let best_card_q = outcomes
            .iter()
            .filter(|o| matches!(o.action, Action::PlayCard { .. }))
            .map(|o| o.mean_value)
            .fold(f32::NEG_INFINITY, f32::max);

        eprintln!("hp={hp}: best={best:?}  best_card_q={best_card_q:.4}  end_turn_q={end_q:.4}");

        assert!(
            !matches!(best, Action::EndTurn),
            "hp={hp}: MCTS recommended End Turn over an affordable Defend wall \
             against telegraphed 7×3 — the reported blunder. \
             end_turn_q={end_q:.4}, best_card_q={best_card_q:.4}"
        );
        assert!(
            best_card_q > end_q,
            "hp={hp}: a playable card must out-rank End Turn here \
             (best_card_q={best_card_q:.4} <= end_turn_q={end_q:.4})"
        );
    }
}

/// Root-cause pin: when the same board is handed to the search with **0
/// energy** (no affordable card), End Turn is the only legal action and is
/// trivially the recommendation. This is the state shape that actually
/// produces the user-visible "end turn" hint — an upstream energy desync, not
/// a search defect.
#[test]
fn zero_energy_leaves_only_end_turn() {
    let mut state = entomancer_fixture(50);
    state.player.energy = 0;

    let mut mcts = Mcts::new(state, production_config());
    let best = mcts.search();
    assert!(
        matches!(best, Action::EndTurn),
        "with 0 energy the only legal move is End Turn; got {best:?}"
    );
}
