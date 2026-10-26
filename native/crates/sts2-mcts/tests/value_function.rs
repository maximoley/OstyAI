//! Value-function regression tests for bead `sts2-rs-v2oh`.
//!
//! These tests pin behaviors that the OLD binary `±1.0` terminal reward
//! could not represent. Each one constructs a scenario where two
//! reasonable plays both win in the long run, but at meaningfully
//! different terminal state — different remaining HP, different
//! potion counts — and asserts that MCTS prefers the higher-quality
//! outcome via the continuous Q gradient added in v2oh.
//!
//! **Pre-v2oh expectation:** every win was `+1.0`, so the Q-distance
//! between "win at full HP" and "win at 1 HP" was strictly zero. These
//! tests would either fail or pass by coincidence on the old code.
//!
//! **Post-v2oh expectation:** `terminal_value` returns a continuous
//! score that decays with HP loss and potion consumption, so MCTS can
//! pick the cleaner finish.

mod common;

use common::build_handful_ex;
use smallvec::SmallVec;
use sts2_core::cards::{CardId, TargetType};
use sts2_core::{Action, Intent, PotionKind};
use sts2_mcts::{Mcts, MctsConfig, RootOutcome};

/// Look up the root outcome that matches a given action under the
/// "PlayCard hand_index N targeting enemy 0" specifier (or `EndTurn`).
/// Panics if not found — every test in this file expects the action
/// to be present at the root.
fn outcome_for_play_card(outs: &[RootOutcome], hand_index: u8, target: Option<u8>) -> &RootOutcome {
    outs.iter()
        .find(|o| {
            matches!(
                o.action,
                Action::PlayCard { hand_index: hi, target: t } if hi == hand_index && t == target
            )
        })
        .unwrap_or_else(|| panic!("no PlayCard(idx={hand_index}, target={target:?}) at root"))
}

fn outcome_for_end_turn(outs: &[RootOutcome]) -> &RootOutcome {
    outs.iter()
        .find(|o| matches!(o.action, Action::EndTurn))
        .expect("no EndTurn at root")
}

fn outcome_for_drink_potion(outs: &[RootOutcome], slot: u8) -> &RootOutcome {
    outs.iter()
        .find(|o| {
            matches!(
                o.action,
                Action::DrinkPotion { potion_slot, .. } if potion_slot == slot
            )
        })
        .unwrap_or_else(|| panic!("no DrinkPotion(slot={slot}) at root"))
}

/// Many simulations so the per-action Q estimate is tight enough to
/// resolve the sub-bucket gradients these tests exercise. The vanilla
/// convergence test uses 1000; we double it to keep variance below the
/// gaps we're asserting on.
const SIMS: u32 = 2000;

/// Kill-now vs end-turn-and-take-damage. The OLD reward gave both
/// terminal `+1`; v2oh's HP-graded terminal pushes the kill-now branch
/// well above the end-turn branch on Q.
#[test]
fn q_prefers_killing_strike_over_taking_a_big_hit() {
    let hand = [(CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy)];
    // Cultist at 6 HP with a *big* attack intent. Killing it now ends
    // combat with the player still at 30 HP. EndTurn lets the attack
    // land (20 damage) before the heuristic Strikes on the next
    // player turn — terminal at ~10 HP.
    let state = build_handful_ex(
        0,
        &hand,
        /*cultist_hp=*/ 6,
        /*player_hp=*/ 30,
        Intent::Attack { damage: 20 },
        &[],
        /*energy=*/ 1,
    );
    let cfg = MctsConfig {
        max_simulations: SIMS,
        seed: 0,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state, cfg);
    let _ = mcts.search();
    let outs = mcts.root_outcomes();
    let strike = outcome_for_play_card(&outs, 0, Some(0));
    let end_turn = outcome_for_end_turn(&outs);
    assert!(
        strike.mean_value > 0.5,
        "Strike-kill-now should bucket as Win (Q>0.5): got Q={}",
        strike.mean_value,
    );
    // Bead `sts2-rs-wr8f` (MCTS-Solver lite, Wave-4) compressed this
    // gap: Strike now resolves to a `proven_win` leaf and backs up as
    // a flat `+1.0` (skipping `evaluate_victory`'s HP-grading), while
    // EndTurn's sub-tree also has proven-win descendants that pin its
    // Q very close to `1.0`. The v2oh HP-aware margin is still visible
    // in the strict ordering — gap of ≥0.005 keeps the regression bite
    // without re-asserting the pre-solver continuous-reward
    // calibration the solver-lite legitimately supersedes.
    assert!(
        strike.mean_value > end_turn.mean_value + 0.005,
        "Strike should out-rank EndTurn even with solver-lite saturation, \
         got strike.Q={} end_turn.Q={}",
        strike.mean_value,
        end_turn.mean_value,
    );
}

/// Two paths both kill the enemy this turn, with the same player HP at
/// terminal — but one consumes a potion. v2oh's `evaluate_victory`
/// rewards potions held at terminal, so the search must prefer the
/// "Strike without drinking" path.
#[test]
fn q_prefers_saving_a_potion_over_drinking_it() {
    // 6-HP Cultist with no attack intent (Defend = no incoming
    // damage this turn). 1 energy, hand: single Strike that kills.
    // Player carries one BlockPotion in inventory.
    //
    // Root options:
    //   * PlayCard(Strike) → kill enemy → terminal at 30 HP, 1 potion held.
    //   * DrinkPotion(BlockPotion) → BlockPotion grants block but with
    //     no incoming damage and the cultist still alive, the heuristic
    //     rollout will play Strike next → terminal at 30 HP, 0 potions.
    //   * EndTurn → enemy defends, no damage; heuristic kills next turn.
    //     Same HP, same potions held — but extra turn.
    //
    // Q-gap: Strike should rank above DrinkPotion by the +4·potion
    // weight in evaluate_victory (≈ +0.013 normalized; resolvable with
    // enough sims).
    let hand = [(CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy)];
    let state = build_handful_ex(
        0,
        &hand,
        /*cultist_hp=*/ 6,
        /*player_hp=*/ 30,
        Intent::Defend { block: 0 },
        &[PotionKind::BlockPotion],
        /*energy=*/ 1,
    );
    let cfg = MctsConfig {
        max_simulations: SIMS,
        seed: 0,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state, cfg);
    let _ = mcts.search();
    let outs = mcts.root_outcomes();
    let strike = outcome_for_play_card(&outs, 0, Some(0));
    let potion = outcome_for_drink_potion(&outs, 0);
    assert!(
        strike.mean_value > potion.mean_value,
        "Strike should out-rank DrinkPotion when both win at full HP \
         (saved-potion bonus): strike.Q={} potion.Q={}",
        strike.mean_value,
        potion.mean_value,
    );
}

/// Sanity: the OLD reward function would have given both paths Q=1.0
/// (both rollouts terminate in a `+1` win). Confirm that under v2oh
/// the Q values are *bounded above by* the victory-eval ceiling
/// — i.e. wins genuinely span a range, not just a flat constant.
/// Guards against accidentally re-binarizing the reward in a refactor.
///
/// GitHub issue #10: under the default `RichBoth` mode, wins are now
/// `0.5 + evaluate_victory(state)` ∈ [+1.0, +1.5] (the bead v2oh signal
/// scaled into the post-issue-#4 magnitude band). The pre-#10 expected
/// upper bound was `< 1.0` (v2oh's raw `evaluate_victory` ceiling); the
/// post-#10 ceiling is `< 1.5` (and the gradient still resolves wins
/// with damage below that).
#[test]
fn winning_q_is_below_1_when_player_takes_damage_along_the_way() {
    let hand = [
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
    ];
    // Player at 30 HP, cultist at 12 HP with Attack(10). 1 energy, so
    // the player can only Strike once per turn → fight takes 2 player
    // turns. The cultist will land its Attack(10) at least once. Even
    // in the heuristic's best line, the player ends combat at ≤ 20 HP,
    // pushing the victory eval below the 1.0 ceiling.
    let state = build_handful_ex(
        0,
        &hand,
        /*cultist_hp=*/ 12,
        /*player_hp=*/ 30,
        Intent::Attack { damage: 10 },
        &[],
        /*energy=*/ 1,
    );
    let cfg = MctsConfig {
        max_simulations: SIMS,
        seed: 0,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state, cfg);
    let _ = mcts.search();
    let outs = mcts.root_outcomes();
    let max_q = outs
        .iter()
        .map(|o| o.mean_value)
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        max_q > 0.5 && max_q < 1.5,
        "wins-with-damage should span (0.5, 1.5) under RichBoth; got max Q = {max_q}",
    );
}

/// Q-gradient must show up across the root edges even when every
/// branch eventually wins. Without this, the v2oh ranking just falls
/// back to UCB visit allocation, which is what we were trying to get
/// away from.
#[test]
fn root_actions_have_distinguishable_q_when_outcomes_differ() {
    // Three Strikes + Defend in hand, 3 energy, cultist at 30 HP
    // intent Attack(15). Player at 40 HP. Different first-move
    // choices lead to noticeably different player HP at terminal:
    //   * 3× Strike (kill in 2 turns w/ help): take 15, win at ~25.
    //   * 2× Strike + Defend: block 5 → 10 dmg through → win at ~30.
    //   * Defend-first: cultist still at 30 next turn, fight drags,
    //     more damage taken.
    let hand: &[(CardId, i32, i32, i8, TargetType)] = &[
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
    ];
    let state = build_handful_ex(
        0,
        hand,
        /*cultist_hp=*/ 30,
        /*player_hp=*/ 40,
        Intent::Attack { damage: 15 },
        &[],
        /*energy=*/ 3,
    );
    let cfg = MctsConfig {
        max_simulations: SIMS,
        seed: 0,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state, cfg);
    let _ = mcts.search();
    let outs: Vec<RootOutcome> = mcts.root_outcomes();
    let qs: SmallVec<[f32; 8]> = outs.iter().map(|o| o.mean_value).collect();
    let spread = qs.iter().copied().fold(f32::NEG_INFINITY, f32::max)
        - qs.iter().copied().fold(f32::INFINITY, f32::min);
    assert!(
        spread > 0.01,
        "expect non-trivial Q spread across root actions under v2oh; got {spread} \
         (qs = {qs:?})",
    );
}
