//! Root-level potion opportunity-cost charge + root drink mask (GH #447
//! follow-up). The #447 sweep proved the LEAF potion channel is
//! noise-bound at the run budget (λ∈{4..16} moved nothing against the
//! ±0.075 per-child reproducibility floor), so cross-fight potion value
//! enters at the ROOT instead: `pick_root_action` subtracts
//! `MctsConfig::potion_charge_q[slot]` from each `DrinkPotion` child's
//! score, deterministic arithmetic immune to rollout noise. These tests
//! bracket the mechanism with charges far outside any tie band so they
//! cannot flake on search stochasticity.

mod common;

use common::build_handful_ex;
use sts2_core::cards::{CardId, TargetType};
use sts2_core::{Action, PotionKind};
use sts2_mcts::{Mcts, MctsConfig, TerminalEvalMode};

/// A winnable Cultist fight holding one Fire Potion: hand is a Strike and
/// a Defend, 3 energy, enemy at 18 HP telegraphing a small attack. The
/// search wins this with or without drinking, so the drink-vs-play choice
/// sits near the tie regime — exactly where the charge must decide.
fn potion_fixture(seed: u64) -> sts2_core::CombatState {
    build_handful_ex(
        seed,
        &[
            (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
            (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        ],
        /* enemy_hp= */ 18,
        /* player_hp= */ 60,
        sts2_core::Intent::Attack { damage: 6 },
        &[PotionKind::FirePotion],
        /* energy= */ 3,
    )
}

fn config(potion_charge_q: Option<[f32; 8]>, root_drink_mask: bool) -> MctsConfig {
    MctsConfig {
        c_puct: 1.4,
        max_simulations: 300,
        max_rollout_depth: 200,
        terminal_eval_mode: TerminalEvalMode::RichBoth,
        seed: 7,
        potion_charge_q,
        root_drink_mask,
        ..MctsConfig::default()
    }
}

fn pick(cfg: MctsConfig) -> (Action, Vec<sts2_mcts::RootOutcome>) {
    let mut mcts = Mcts::new(potion_fixture(3), cfg);
    let action = mcts
        .try_search_n(cfg.max_simulations)
        .expect("search completes");
    (action, mcts.root_outcomes())
}

/// A whole-win charge (1.0 Q) puts every drink outside any conceivable
/// band: the pick must not be a drink. A whole-win *bonus* (−1.0) puts
/// the drink above everything: the pick must be a drink. Bracketing both
/// directions proves the charge reaches the score arithmetic without
/// depending on the fixture's natural (noise-adjacent) preference.
#[test]
fn charge_brackets_the_drink_decision() {
    let (penalized, _) = pick(config(Some([1.0; 8]), false));
    assert!(
        !matches!(penalized, Action::DrinkPotion { .. }),
        "a 1.0-Q charge must price the drink out, got {penalized:?}"
    );

    let (bonused, _) = pick(config(Some([-1.0; 8]), false));
    assert!(
        matches!(bonused, Action::DrinkPotion { .. }),
        "a -1.0-Q charge (bonus) must force the drink, got {bonused:?}"
    );
}

/// An all-zero charge table is byte-identical to no table: same pick,
/// same per-child outcomes. Pins that the seam is a no-op at 0 (the
/// deployed default when the env seam is unset).
#[test]
fn zero_charge_is_byte_identical_to_none() {
    let (pick_none, outcomes_none) = pick(config(None, false));
    let (pick_zero, outcomes_zero) = pick(config(Some([0.0; 8]), false));
    assert_eq!(format!("{pick_none:?}"), format!("{pick_zero:?}"));
    let render = |os: &[sts2_mcts::RootOutcome]| {
        os.iter()
            .map(|o| format!("{:?} v{} q{}", o.action, o.visits, o.value_sum))
            .collect::<Vec<_>>()
    };
    assert_eq!(render(&outcomes_none), render(&outcomes_zero));
}

/// `root_drink_mask` removes drink children from the root entirely (the
/// randomized-drink jitter seam): no root outcome is a `DrinkPotion` and
/// the search still resolves a pick.
#[test]
fn root_drink_mask_removes_drink_children() {
    let (action, outcomes) = pick(config(None, true));
    assert!(
        !outcomes.is_empty(),
        "masked search must still expand non-drink children"
    );
    assert!(
        outcomes
            .iter()
            .all(|o| !matches!(o.action, Action::DrinkPotion { .. })),
        "mask must remove every DrinkPotion root child: {outcomes:?}"
    );
    assert!(!matches!(action, Action::DrinkPotion { .. }));
}
