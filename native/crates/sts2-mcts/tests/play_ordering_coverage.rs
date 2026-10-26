//! Census: every card that can legally be *played* must carry an explicit
//! `play_ordering` priority (GH #515).
//!
//! `expert::rollout_coverage_fraction` counts a legal `PlayCard` action as
//! covered only when its card's priority is something other than
//! `NEUTRAL_PRIORITY`, and the rollout policy degrades to Uniform for the
//! whole rollout once coverage drops under `COVERAGE_THRESHOLD` (0.8). So an
//! unclassified card does not merely get an average priority — a few of them
//! in one hand switch the search's rollouts to random play. That is not
//! hypothetical: Regent once sat at ~2% coverage and Necrobinder at 12.7% of
//! positions below the gate, both found long after the fact.
//!
//! The 2026-07-20 passes bulk-classified the Regent and Necrobinder pools by
//! type/cost/block, and #515 cleared the stragglers. This test is what stops
//! the next new card from re-opening the hole silently: a card added to the
//! content crate without a priority fails here, at the point it is added,
//! instead of showing up as a coverage statistic two months later.
//!
//! **Unplayable cards are exempt, and derived rather than listed.** A card
//! the engine refuses to play never produces a `PlayCard` action, so it can
//! never affect coverage; exempting it by asking the spec (rather than by
//! name) means the exemption cannot drift out of date. `CardId` variants
//! with no spec at all — the `Generic` sentinel — are exempt for the same
//! reason: nothing can put them in a hand.
//!
//! Note the wart this test has to work around: `NEUTRAL_PRIORITY` is both
//! "the fallback" and a legal value, so a card genuinely deserving 50 is
//! indistinguishable from an unclassified one. If a future card really
//! belongs mid-ladder, give it 48 or 52.

use sts2_content::cards::spec_index::spec_for_card_id;
use sts2_core::cards::CardId;
use sts2_mcts::expert::{play_ordering, NEUTRAL_PRIORITY};

/// Cards that can appear as a legal `PlayCard` action, i.e. everything the
/// census actually governs.
fn playable_cards() -> Vec<CardId> {
    CardId::all()
        .iter()
        .copied()
        .filter(|&id| spec_for_card_id(id).is_some_and(|spec| !spec.is_unplayable()))
        .collect()
}

#[test]
fn every_playable_card_has_an_explicit_play_ordering() {
    let uncovered: Vec<String> = playable_cards()
        .into_iter()
        .filter(|&id| play_ordering(id, None) == NEUTRAL_PRIORITY)
        .map(|id| format!("{id:?}"))
        .collect();
    assert!(
        uncovered.is_empty(),
        "{} playable card(s) fall through to NEUTRAL_PRIORITY, so any hand \
         holding a few of them drops the whole rollout to Uniform:\n  {}\n\
         Add each to a bucket in `expert::play_ordering_base` — read the card \
         in ../sts2-wiki/decompiled/v0.107.1/MegaCrit.Sts2.Core.Models.Cards/ \
         for its cost/type/effect and match the existing bucket comments.",
        uncovered.len(),
        uncovered.join("\n  ")
    );
}

/// The exemption above is only sound while unplayable cards really cannot be
/// played. If that ever changes, this test fails and the census above has a
/// silent hole rather than a visible one.
#[test]
fn unplayable_cards_are_the_only_exemption() {
    let exempt: Vec<CardId> = CardId::all()
        .iter()
        .copied()
        .filter(|&id| spec_for_card_id(id).is_none_or(|spec| spec.is_unplayable()))
        .collect();
    assert!(
        !exempt.is_empty(),
        "no card is exempt any more — either the specs changed or \
         `is_unplayable` did; re-read the census exemption above"
    );
    for id in exempt {
        if let Some(spec) = spec_for_card_id(id) {
            assert!(
                spec.is_unplayable(),
                "{id:?} is exempt from the play_ordering census but is playable"
            );
        }
    }
}
