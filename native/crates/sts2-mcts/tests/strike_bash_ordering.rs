//! Ordering regression: never play Strike *before* Bash on the same turn.
//!
//! User-reported "dumb play": the agent plays Strike and then Bash on the
//! same turn. Either card alone is fine, but Bash applies 2 Vulnerable
//! (+50% damage taken), so the *only* sensible order when you intend to
//! play both is Bash first — that way the follow-up Strike lands into the
//! debuff. Strike-then-Bash strands the Vulnerable on a target that takes
//! no further hits this turn, throwing away ~50% of the Strike's value.
//!
//! This test pins the contract by constructing a position where the
//! ordering is *forced* by lethality, so the search can't sidestep it by
//! declining to play one of the cards:
//!
//!   - Hand: `[Strike(6, 1e), Bash(8 + 2 Vulnerable, 2e)]`, energy 3
//!     (exactly enough for both).
//!   - Enemy at 15 HP, low non-lethal intent; player at 60 HP so survival
//!     never confounds the damage calculus.
//!   - Bash → Strike: 8 + ceil(6 × 1.5) = 8 + 9 = 17 ≥ 15 → **kills**.
//!   - Strike → Bash: 6 + 8 = 14 → enemy survives at 1 HP, Vulnerable
//!     wasted. Empty draw pile means there is no recovery next turn.
//!
//! The Bash-first line is the unique kill, so a search that doesn't waste
//! Bash's Vulnerable both (a) plays both cards and (b) plays Bash first.
//! We drive the player's whole turn move-by-move, record the CardId of
//! every card played, and assert Strike never precedes Bash.
//!
//! Budget/seeds mirror `intent_aware_scenarios`: 1000 sims, 3 seeds,
//! tolerating ≤1 outlier on the "both cards played" sanity check while
//! holding the ordering contract hard (the bad order must never appear).

mod common;

use common::build_handful_ex;
use sts2_core::cards::{CardId, TargetType};
use sts2_core::{apply, is_terminal, legal_actions, Action, CombatPhase, CombatState, Intent};
use sts2_mcts::{Mcts, MctsConfig};

const SIMS: u32 = 1000;
const SEEDS: &[u64] = &[0, 1, 2];

/// Resolve the `CardId` behind a `PlayCard { hand_index }` against `state`
/// *before* the play mutates the hand.
fn played_card_id(state: &CombatState, hand_index: u8) -> Option<CardId> {
    let uid = *state.player.hand.get(hand_index as usize)?;
    state.cards.iter().find(|c| c.uid == uid).map(|c| c.card_id)
}

/// Drive the player's turn with MCTS, returning the ordered list of
/// `CardId`s actually played (EndTurn / combat-end terminates the walk).
fn played_sequence(seed: u64) -> Vec<CardId> {
    // index 0 = Strike, index 1 = Bash. The deliberately "wrong" hand
    // order (Strike before Bash) makes a naive left-to-right pick fail.
    let hand = [
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        (CardId::Bash, 8, 0, 2, TargetType::Enemy),
    ];
    let mut state = build_handful_ex(
        seed,
        &hand,
        /*cultist_hp=*/ 15,
        /*player_hp=*/ 60,
        Intent::Attack { damage: 5 },
        &[],
        /*energy=*/ 3,
    );

    let mut played = Vec::new();
    // Hard cap on iterations as a runaway guard; a turn here is ≤2 plays.
    for _ in 0..8 {
        if state.phase != CombatPhase::PlayerPlayPhase || is_terminal(&state) {
            break;
        }
        if legal_actions(&state).is_empty() {
            break;
        }
        let cfg = MctsConfig {
            max_simulations: SIMS,
            seed,
            ..MctsConfig::default()
        };
        let mut mcts = Mcts::new(state.clone(), cfg);
        let action = mcts.search();

        if let Action::PlayCard { hand_index, .. } = action {
            if let Some(id) = played_card_id(&state, hand_index) {
                played.push(id);
            }
        }

        let _ = apply(&mut state, action.clone());
        if matches!(action, Action::EndTurn) || is_terminal(&state) {
            break;
        }
    }
    played
}

/// MCTS must never play Strike before Bash when it plays both on a turn,
/// because that wastes Bash's Vulnerable. In this forced-lethal fixture
/// the only winning line is Bash → Strike.
#[test]
fn never_plays_strike_before_bash() {
    let mut wrong_order = Vec::new();
    let mut both_played = 0_usize;
    let mut sequences = Vec::with_capacity(SEEDS.len());

    for &seed in SEEDS {
        let seq = played_sequence(seed);
        let strike_pos = seq.iter().position(|c| *c == CardId::StrikeIronclad);
        let bash_pos = seq.iter().position(|c| *c == CardId::Bash);
        if let (Some(si), Some(bi)) = (strike_pos, bash_pos) {
            both_played += 1;
            if si < bi {
                wrong_order.push((seed, seq.clone()));
            }
        }
        sequences.push((seed, seq));
    }

    // Hard contract: the wasteful Strike-before-Bash order must never
    // appear on any seed where both cards are played.
    assert!(
        wrong_order.is_empty(),
        "MCTS played Strike before Bash, wasting Bash's Vulnerable (only Bash→Strike is lethal here): \
         offending seeds = {wrong_order:?}; all sequences = {sequences:?}",
    );

    // Sanity: the fixture is meant to force the Bash→Strike kill, so both
    // cards should actually get played. Tolerate ≤1 seed declining (UCB
    // tie-breaking at the budget knee) without weakening the order check.
    assert!(
        both_played >= 2,
        "expected the forced-lethal Bash→Strike line to play both cards on ≥2/3 seeds; \
         got {both_played}/{} (sequences = {sequences:?})",
        SEEDS.len(),
    );
}
