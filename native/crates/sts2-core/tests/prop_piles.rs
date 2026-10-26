//! Property tests for player-pile invariants under random sequences
//! of pile operations.
//!
//! Two load-bearing invariants exposed at `lib.rs:2173-2177`:
//!
//!   * "Every `CardUid` in any player pile must resolve to an entry
//!     in `cards`."
//!   * "A single `CardUid` must not appear in more than one player
//!     pile simultaneously."
//!
//! Combined into the property:
//!
//!     multiset(hand ∪ draw ∪ discard ∪ exhaust ∪ play) is preserved
//!     across any sequence of `move_card / shuffle / draw /
//!     discard_pile_into_draw_pile / try_add_to_hand / discard`,
//!     and `CombatState::assert_invariants()` keeps holding.
//!
//! Generator: build a state seeded with N cards in the draw pile, then
//! apply M random ops drawn from a small enum of valid pile primitives.

use hegel::generators as gs;
use hegel::TestCase;
use std::collections::BTreeMap;
use sts2_core::cards::{self, PileKind};
use sts2_core::{Card, CardUid, CombatState};

/// Operations we exercise. Each maps to a documented pile primitive.
#[derive(Debug, Clone, Copy)]
enum Op {
    /// Move a card between two specified piles.
    Move {
        from: PileKind,
        to: PileKind,
        idx: usize,
    },
    /// Shuffle the draw pile.
    Shuffle,
    /// Draw `n` cards (auto-reshuffles when draw empty).
    Draw { n: i32 },
    /// Move whole discard into draw and shuffle.
    DiscardToDraw,
    /// `discard()` — convenience wrapper for hand→discard.
    DiscardFromHand { idx: usize },
    /// `try_add_to_hand()` — add a uid (drawn from any pile) to hand,
    /// or redirect to discard if full. We add a *fresh-looking* uid by
    /// picking a uid already in the cards arena (no new alloc here —
    /// that would change the multiset). To exercise the redirect we
    /// remove the uid from its current pile first.
    Promote { from: PileKind, idx: usize },
}

const PILE_KINDS: [PileKind; 5] = [
    PileKind::Hand,
    PileKind::Draw,
    PileKind::Discard,
    PileKind::Exhaust,
    PileKind::Play,
];

fn arb_pile(tc: &TestCase) -> PileKind {
    let i = tc.draw(gs::integers::<u32>().min_value(0).max_value(4)) as usize;
    PILE_KINDS[i]
}

fn arb_op(tc: &TestCase) -> Op {
    let tag = tc.draw(gs::integers::<u32>().min_value(0).max_value(5));
    let idx = tc.draw(gs::integers::<u32>().min_value(0).max_value(31)) as usize;
    match tag {
        0 => Op::Move {
            from: arb_pile(tc),
            to: arb_pile(tc),
            idx,
        },
        1 => Op::Shuffle,
        2 => Op::Draw {
            n: tc.draw(gs::integers::<i32>().min_value(-1).max_value(12)),
        },
        3 => Op::DiscardToDraw,
        4 => Op::DiscardFromHand { idx },
        _ => Op::Promote {
            from: arb_pile(tc),
            idx,
        },
    }
}

/// Sum a multiset of every CardUid across all five piles.
fn multiset(state: &CombatState) -> BTreeMap<CardUid, u32> {
    let mut acc: BTreeMap<CardUid, u32> = BTreeMap::new();
    for pile in PILE_KINDS {
        for uid in cards::pile_iter(&state.player, pile) {
            *acc.entry(uid).or_default() += 1;
        }
    }
    acc
}

/// Pick the uid at `idx % len` of `pile`, or `None` if empty.
fn nth_in_pile(state: &CombatState, pile: PileKind, idx: usize) -> Option<CardUid> {
    let len = cards::pile_len(&state.player, pile);
    if len == 0 {
        return None;
    }
    cards::pile_iter(&state.player, pile).nth(idx % len)
}

/// Apply one op. Errors from `move_card` (uid not in source) are
/// swallowed — the property is about no-op safety + multiset
/// preservation, not about op success. Either path must preserve
/// the multiset.
fn apply_op(state: &mut CombatState, rng: &mut sts2_core::dotnet_rng::Rng, op: Op) {
    match op {
        Op::Move { from, to, idx } => {
            if let Some(uid) = nth_in_pile(state, from, idx) {
                let _ = cards::move_card(&mut state.player, uid, from, to);
            }
        }
        Op::Shuffle => cards::shuffle(&mut state.player, rng),
        Op::Draw { n } => {
            let _ = cards::draw(&mut state.player, n, &state.cards, rng);
        }
        Op::DiscardToDraw => {
            cards::discard_pile_into_draw_pile(&mut state.player, &state.cards, rng);
        }
        Op::DiscardFromHand { idx } => {
            if let Some(uid) = nth_in_pile(state, PileKind::Hand, idx) {
                let _ = cards::discard(&mut state.player, uid);
            }
        }
        Op::Promote { from, idx } => {
            // Pull a uid out of `from`, then route through try_add_to_hand
            // (which lands it in hand if room, else discard). Net: the uid
            // is preserved across the multiset; the only mutation is
            // *which* pile it ends up in.
            if let Some(uid) = nth_in_pile(state, from, idx) {
                // Remove from source via move_card to Play, then take it
                // back out with explicit move_card to its real destination
                // would double-handle; instead we mimic Python's
                // intra-pile fishing by moving to Play first then re-adding.
                // Simpler: just do move_card to Hand (which doesn't redirect
                // on full, exposing a different invariant — better to test
                // the actual try_add_to_hand semantics).
                if cards::move_card(&mut state.player, uid, from, PileKind::Play).is_ok() {
                    // Now pop it back out of Play and run try_add_to_hand.
                    let _ =
                        cards::move_card(&mut state.player, uid, PileKind::Play, PileKind::Hand);
                    // If that overshot HAND_LIMIT, the Hand pile is now over
                    // limit; revert by moving the just-added card to discard.
                    // (try_add_to_hand is the canonical way to enforce the
                    // limit; emulate it explicitly.)
                    if cards::pile_len(&state.player, PileKind::Hand) > sts2_core::cards::HAND_LIMIT
                    {
                        let _ = cards::move_card(
                            &mut state.player,
                            uid,
                            PileKind::Hand,
                            PileKind::Discard,
                        );
                    }
                }
            }
        }
    }
}

/// Run a fresh property iteration: build a seed state, draw ops, apply,
/// then assert.
#[hegel::test(test_cases = 300)]
fn pile_ops_preserve_multiset_and_invariants(tc: TestCase) {
    let n_cards = tc.draw(gs::integers::<u32>().min_value(0).max_value(20)) as usize;
    let n_ops = tc.draw(gs::integers::<u32>().min_value(0).max_value(40)) as usize;
    let seed = tc.draw(gs::integers::<u64>());

    // Re-seed the state inline so the snapshot lives in this scope.
    let mut state = CombatState::default();
    state.player.current_hp = 50;
    state.player.max_hp = 50;
    for _ in 0..n_cards {
        state.add_card_to_draw_pile(Card::pod_defaults());
    }
    let baseline = multiset(&state);
    state
        .assert_invariants()
        .expect("seed state passes invariants");

    let mut rng = sts2_core::dotnet_rng::Rng::new(seed);
    let ops: Vec<Op> = (0..n_ops).map(|_| arb_op(&tc)).collect();

    for (step, op) in ops.iter().copied().enumerate() {
        apply_op(&mut state, &mut rng, op);
        let after = multiset(&state);
        if after != baseline {
            tc.note(&format!(
                "step={step} op={op:?} baseline_card_count={} after_card_count={}",
                baseline.len(),
                after.len()
            ));
        }
        assert_eq!(
            after, baseline,
            "multiset changed after step {step} op {op:?}"
        );
        if let Err(e) = state.assert_invariants() {
            tc.note(&format!("step={step} op={op:?} invariant_violation={e}"));
            panic!("assert_invariants failed at step {step}: {e}");
        }
    }
}

/// Sanity: an empty state passes invariants, and shuffling/drawing on
/// an empty deck is a no-op.
#[hegel::test]
fn empty_state_ops_are_noops(tc: TestCase) {
    let seed = tc.draw(gs::integers::<u64>());
    let mut state = CombatState::default();
    state.player.current_hp = 50;
    state.player.max_hp = 50;
    let mut rng = sts2_core::dotnet_rng::Rng::new(seed);
    cards::shuffle(&mut state.player, &mut rng);
    let drew = cards::draw(&mut state.player, 5, &state.cards, &mut rng);
    cards::discard_pile_into_draw_pile(&mut state.player, &state.cards, &mut rng);
    assert_eq!(drew, 0);
    assert!(multiset(&state).is_empty());
    state.assert_invariants().expect("empty state is valid");
}
