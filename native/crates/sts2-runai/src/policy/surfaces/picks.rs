//! Card-shaped picks: ΔV argmax over candidates applied to the view
//! (plan §2 "card-shaped choices"; bead `sts2-rs-40ikw.5`).
//!
//! Covers card rewards (incl. skip), boss relics, deck removal /
//! upgrade / transform targets, and the rest-site smith target. Every
//! candidate is applied with the *pure* `VisibleRunView::with_*`
//! helpers — `ΔV = V(view + candidate) − V(view)` — never by cloning or
//! simulating the `RunManager` (that's M2's rollout path).

use sts2_core::RelicId;

use crate::eval::HeuristicV;
use crate::view::{DeckCard, VisibleRunView};

/// ΔV of adding `card` to the deck.
#[must_use]
pub fn dv_add_card(view: &VisibleRunView, v: &HeuristicV, card: DeckCard) -> f64 {
    v.score(&view.with_card_added(card.card_id, card.upgraded)) - v.score(view)
}

/// ΔV of removing the deck card at `idx`.
#[must_use]
pub fn dv_remove_card(view: &VisibleRunView, v: &HeuristicV, idx: usize) -> f64 {
    v.score(&view.with_deck_card_removed(idx)) - v.score(view)
}

/// ΔV of upgrading the deck card at `idx`.
#[must_use]
pub fn dv_upgrade_card(view: &VisibleRunView, v: &HeuristicV, idx: usize) -> f64 {
    v.score(&view.with_deck_card_upgraded(idx)) - v.score(view)
}

/// ΔV of obtaining `relic_id`.
#[must_use]
pub fn dv_add_relic(view: &VisibleRunView, v: &HeuristicV, relic_id: RelicId) -> f64 {
    v.score(&view.with_relic_added(relic_id)) - v.score(view)
}

/// Card-reward decision: `Some(i)` picks `offers[i]`, `None` skips.
/// Skip when no option clears ΔV > 0 (taking a V-negative card is
/// strictly worse than skipping). Also returns the per-option scores
/// (skip last) for the audit log.
#[must_use]
pub fn best_card_reward(
    view: &VisibleRunView,
    v: &HeuristicV,
    offers: &[DeckCard],
) -> (Option<usize>, Vec<f64>) {
    let mut scores: Vec<f64> = offers
        .iter()
        .map(|&card| dv_add_card(view, v, card))
        .collect();
    let best = argmax(&scores).filter(|&i| scores[i] > 0.0);
    scores.push(0.0); // skip baseline
    (best, scores)
}

/// Boss-relic pick: argmax relic ΔV over the offered pool (no skip on
/// this screen). Returns `(best index, scores)`.
#[must_use]
pub fn best_boss_relic(
    view: &VisibleRunView,
    v: &HeuristicV,
    offers: &[RelicId],
) -> (usize, Vec<f64>) {
    let scores: Vec<f64> = offers
        .iter()
        .map(|&relic| dv_add_relic(view, v, relic))
        .collect();
    let best = argmax(&scores).unwrap_or(0);
    (best, scores)
}

/// Best removal target over an eligible deck-index subset (mandatory
/// removal surfaces — Precise Scissors etc. — and the shop remove slot).
/// Returns `(position within `eligible`, ΔV, scores)`.
#[must_use]
pub fn best_removal(
    view: &VisibleRunView,
    v: &HeuristicV,
    eligible: &[usize],
) -> Option<(usize, f64, Vec<f64>)> {
    let scores: Vec<f64> = eligible
        .iter()
        .map(|&deck_idx| dv_remove_card(view, v, deck_idx))
        .collect();
    argmax(&scores).map(|i| (i, scores[i], scores))
}

/// Best upgrade target over an eligible deck-index subset (Pomander's
/// mandatory upgrade, the smith pick). Returns `(position, ΔV, scores)`.
#[must_use]
pub fn best_upgrade(
    view: &VisibleRunView,
    v: &HeuristicV,
    eligible: &[usize],
) -> Option<(usize, f64, Vec<f64>)> {
    let scores: Vec<f64> = eligible
        .iter()
        .map(|&deck_idx| dv_upgrade_card(view, v, deck_idx))
        .collect();
    argmax(&scores).map(|i| (i, scores[i], scores))
}

/// Deck indices of non-upgraded cards — a **valuation** candidate set
/// ("which card would we most like to see upgraded"), used to price a
/// rest-site Smith before its option list exists.
///
/// **NOT an engine index space.** `UpgradeDeckCard{idx}` indexes the
/// `Card::is_upgradable()` subset — see [`upgradable_deck_indices`] — which
/// additionally drops Curse/Status and the three Quest cards; and a Smith's
/// `SmithCard{idx}` indexes the engine's own pending-choice option list.
/// Feeding a position computed here back to the engine upgrades a *different*
/// card whenever the deck holds a non-upgradable card ahead of the target
/// (GH #399 review; it silently clamped instead of erroring). Use this only
/// where the result is a score, never where it is an action index.
#[must_use]
pub fn non_upgraded_deck_indices(view: &VisibleRunView) -> Vec<usize> {
    view.deck
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.upgraded)
        .map(|(i, _)| i)
        .collect()
}

/// Master-deck indices of the cards the engine will offer for
/// `UpgradeDeckCard` — i.e. the `Card::is_upgradable()` subset, in deck
/// order. Position within the returned slice **is** the engine's
/// `UpgradeDeckCard{idx}` index space: `get_available_actions`
/// (`run_manager.rs:954-968`) offers `0..count` over this filter and
/// `do_upgrade_deck_card_at` (`:3044-3053`) resolves `idx` by `nth()` over
/// the identical filter.
///
/// Takes the `RunManager` rather than the [`VisibleRunView`] because
/// `is_upgradable()` needs `card_type` and the Quest card ids, which
/// [`DeckCard`] does not carry. Index alignment holds: `view.deck` mirrors
/// `RunState::master_deck` order 1:1, so the returned indices are valid for
/// [`dv_upgrade_card`].
#[must_use]
pub fn upgradable_deck_indices(rm: &sts2_core::run_manager::RunManager) -> Vec<usize> {
    rm.run_state
        .master_deck
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_upgradable())
        .map(|(i, _)| i)
        .collect()
}

/// Index of the max score, ties broken to the LOWEST index (the
/// deterministic "first option" convention used across surfaces).
/// `None` for empty input.
#[must_use]
pub fn argmax(scores: &[f64]) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, &s) in scores.iter().enumerate() {
        let better = match best {
            None => true,
            Some((_, b)) => s > b,
        };
        if better {
            best = Some((i, s));
        }
    }
    best.map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sts2_content::run_callbacks::new_run_manager_for_character;
    use sts2_core::cards::CardId;
    use sts2_core::potions::CharacterId;

    fn fresh_view() -> VisibleRunView {
        let rm = new_run_manager_for_character(13, CharacterId::Ironclad);
        VisibleRunView::from_run_manager(&rm)
    }

    /// ΔV pick prefers an obviously good card over a curse-ish option
    /// (M1 acceptance test).
    #[test]
    fn card_reward_prefers_good_card_over_curse() {
        let view = fresh_view();
        let v = HeuristicV::default();
        let offers = vec![
            DeckCard {
                card_id: CardId::Decay,
                upgraded: false,
            },
            DeckCard {
                card_id: CardId::Inflame,
                upgraded: true,
            },
        ];
        let (best, scores) = best_card_reward(&view, &v, &offers);
        assert_eq!(best, Some(1), "scores: {scores:?}");
    }

    /// All-bad offerings are skipped.
    #[test]
    fn card_reward_skips_all_curses() {
        let view = fresh_view();
        let v = HeuristicV::default();
        let offers = vec![DeckCard {
            card_id: CardId::Decay,
            upgraded: false,
        }];
        let (best, _) = best_card_reward(&view, &v, &offers);
        assert_eq!(best, None);
    }

    /// Removal prefers dumping a curse over a useful card.
    #[test]
    fn removal_prefers_curse() {
        let view = fresh_view().with_card_added(CardId::Decay, false);
        let v = HeuristicV::default();
        let curse_idx = view.deck.len() - 1;
        let eligible: Vec<usize> = (0..view.deck.len()).collect();
        let (pos, dv, _) = best_removal(&view, &v, &eligible).expect("non-empty");
        assert_eq!(eligible[pos], curse_idx);
        assert!(dv > 0.0, "removing a curse must raise V");
    }

    #[test]
    fn upgrade_targets_are_positive_dv() {
        let view = fresh_view();
        let v = HeuristicV::default();
        let eligible = non_upgraded_deck_indices(&view);
        let (_, dv, _) = best_upgrade(&view, &v, &eligible).expect("non-empty");
        assert!(dv > 0.0);
    }
}
