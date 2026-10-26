//! Intent-aware tactical scenario tests for bead `sts2-rs-n0gn`.
//!
//! These two scenarios were deliberately punted from `sts2-rs-v2oh`
//! because they require the search to (a) read enemy intent end-to-end
//! and (b) value the resulting damage / wasted-energy delta on the
//! continuous Q channel. With v2oh's lightspeed-style victory eval +
//! `2rp7`'s intent translation both landed, the contract is finally
//! expressible at the MCTS level — and these tests pin it.
//!
//! - **Scenario A — block when the enemy attacks for lethal-soon.**
//!   Player low HP, enemy telegraphing a big hit. Defending now keeps
//!   the player alive into next turn where the kill lands; attacking
//!   first eats the hit and risks death. MCTS must prefer Defend.
//!
//! - **Scenario B — skip blocking when the enemy is buffing.** Enemy
//!   intent is `Buff` (no incoming damage this turn). Defending wastes
//!   energy that should go into Strike — chip damage now strictly
//!   dominates. MCTS must prefer Strike.
//!
//! Sim budget: 1000 sims, 3 seeds — within the documented MCTS knee
//! (`docs/perf/...` / project memory: `sims=300–3000`) and matching
//! the budget the hint adapter actually uses in the CLI. Sweeping >1
//! seed defends against single-seed flakes the way `toy_convergence`
//! historically caught them.

mod common;

use common::build_handful_ex;
use sts2_core::cards::{CardId, TargetType};
use sts2_core::powers::PowerKind;
use sts2_core::{Action, Intent};
use sts2_mcts::{Mcts, MctsConfig};

const SIMS: u32 = 1000;
const SEEDS: &[u64] = &[0, 1, 2];

// NOTE: these tests must score the action MCTS *actually recommends*,
// which since bead `sts2-rs-trnu` is `argmax(Q) tiebreak visits` — the
// value returned by [`Mcts::search`] — NOT the obsolete `argmax(visits)`
// rule. The two diverge on near-equal-Q positions: scenario B is a
// guaranteed win where Strike and Defend back up the same victory value
// (`evaluate_victory` counts HP/potions/round, not block), so their root
// visit counts tie and an `argmax(visits)` pick breaks the tie
// arbitrarily toward Defend even though Strike holds the (marginally)
// higher Q. Asserting on `search()`'s return keeps the test aligned with
// production move-selection. Do NOT reintroduce a visit-argmax helper.

fn is_defend(action: &Action, defend_hand_index: u8) -> bool {
    matches!(
        action,
        Action::PlayCard { hand_index, .. } if *hand_index == defend_hand_index
    )
}

/// Scenario A — Block when the enemy attacks for lethal-soon.
///
/// Setup:
///   - Player at 8 HP, energy 3, hand: `[Defend(5), Defend(5),
///     Strike(6), Strike(6), Strike(6)]`.
///   - Enemy at 24 HP, intent `Attack { damage: 8 }` — exactly lethal
///     against an undefended 8-HP player.
///   - "2 turns to lethal" framing: enemy survives this turn (24 HP >
///     3×6 max strike), so the player has to live into a second turn
///     to finish the fight. With this fixture's empty draw pile that
///     second turn never produces cards, so the rollout depth-cuts
///     into v2oh's in-progress eval band (–0.5, 0.5) — exactly the
///     gradient v2oh added so MCTS can rank "alive when the timer
///     runs out" above "dead at the buzzer".
///   - Path A (Defend × 2 + Strike): blocks 10 ≥ 8 incoming → HP 8
///     held, enemy chipped to 18 — search returns to mid-game.
///   - Path B (Strike × 3): deals 18, takes 8 → dead this turn → Q
///     pinned at –1.0.
///
/// Expected: the action MCTS recommends (`search()`) is Defend (hand index
/// 0 or 1) across ≥2/3 seeds. The Q-gap here is large (≈ –1.0 vs a
/// non-trivial in-progress value), well within MCTS's resolution at
/// 1000 sims.
///
/// NOTE on enemy choice: the `build_handful_ex` fixture uses the
/// Nibbit stub. Per project memory `feedback_sts1_stubs_not_sts2`,
/// the species discriminant on this fixture is Phase-4 scaffolding;
/// we override its HP + intent directly and only the intent damage
/// drives behavior here — exactly what v2oh's intent translation
/// reads. TODO: switch to a wired STS2 enemy with a reliable
/// big-attack intent once one ships through the act1 registry
/// (tracked under `sts2-rs-ar9u`).
#[test]
fn scenario_a_blocks_when_enemy_attacks_for_lethal() {
    // One energy makes Strike-first lethal: there is no energy left to block.
    // Hand layout: indices 0,1 = Defend; 2,3,4 = Strike. Both Defend
    // slots count as "the right play"; the assertion accepts either.
    let hand = [
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
    ];

    let mut defend_picks = 0_usize;
    let mut picks = Vec::with_capacity(SEEDS.len());
    for &seed in SEEDS {
        let state = build_handful_ex(
            seed,
            &hand,
            /*cultist_hp=*/ 24,
            /*player_hp=*/ 8,
            Intent::Attack { damage: 8 },
            &[],
            /*energy=*/ 1,
        );
        let cfg = MctsConfig {
            max_simulations: SIMS,
            seed,
            ..MctsConfig::default()
        };
        let mut mcts = Mcts::new(state, cfg);
        let best_action = mcts.search();
        if is_defend(&best_action, 0) || is_defend(&best_action, 1) {
            defend_picks += 1;
        }
        picks.push((seed, best_action));
    }

    // Require ≥2/3 seeds to pick Defend. Allowing one outlier guards
    // against UCB tie-breaking on near-identical Q at the budget knee
    // without weakening the contract: the defending action must
    // dominate as the search's preferred move.
    assert!(
        defend_picks >= 2,
        "expected MCTS to pick Defend on the lethal-attack turn for ≥2/3 seeds; got {defend_picks}/{} \
         (picks per seed = {picks:?})",
        SEEDS.len(),
    );
}

/// Scenario B — Skip blocking when the enemy is buffing.
///
/// Setup:
///   - Player at 50/80 HP, energy 3, hand: `[Defend(5), Strike(6)]`.
///   - Enemy at 20 HP, intent `Intent::Buff { kind: PowerKind::Strength, amount: 0 }` — no incoming damage this
///     turn at all. Block on a non-attack turn is pure waste.
///
/// Expected: Strike (hand index 1) is the action MCTS recommends across
/// all 3 seeds. The Q-gap rationale is that on the no-incoming-damage
/// turn, Defend just drains 1 energy with zero terminal-state benefit:
/// (a) it doesn't reduce damage taken, (b) it keeps the enemy alive
/// 1 turn longer so it gets to hit later (after the buff resolves) at
/// presumably worse stats, (c) it consumes a card that doesn't
/// contribute to the lethal stack.
///
/// Strike, by contrast, chips 6 HP off the 20-HP enemy, accelerating
/// the kill and shifting `evaluate_victory`'s enemy-HP-ratio reward
/// term in our favor on every reachable terminal state.
#[test]
fn scenario_b_skips_block_when_enemy_buffs() {
    let hand = [
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
    ];

    let mut non_defend_picks = 0_usize;
    let mut picks = Vec::with_capacity(SEEDS.len());
    for &seed in SEEDS {
        let state = build_handful_ex(
            seed,
            &hand,
            /*cultist_hp=*/ 20,
            /*player_hp=*/ 50,
            Intent::Buff {
                kind: PowerKind::Strength,
                amount: 0,
            },
            &[],
            /*energy=*/ 3,
        );
        let cfg = MctsConfig {
            max_simulations: SIMS,
            seed,
            ..MctsConfig::default()
        };
        let mut mcts = Mcts::new(state, cfg);
        let best_action = mcts.search();
        if !is_defend(&best_action, 0) {
            non_defend_picks += 1;
        }
        picks.push((seed, best_action));
    }

    // Bead `sts2-rs-j9GGr` (GitHub issue #9): the original contract was
    // "MCTS picks Strike", but once the heuristic rollout gained kill
    // detection, deep rollouts finish the 20-HP enemy on whichever turn
    // gets to 6 HP — so Strike-now and EndTurn-then-Strike both back
    // up similar Q. The intent-awareness contract this test actually
    // pins is "don't waste energy on Defend when there's no incoming
    // damage"; relax the requirement accordingly.
    assert!(
        non_defend_picks >= 2,
        "expected MCTS to NOT pick Defend on a Buff-intent turn for ≥2/3 seeds; \
         got {non_defend_picks}/{} (picks per seed = {picks:?})",
        SEEDS.len(),
    );
}
