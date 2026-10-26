//! Shop surface: marginal-ΔV-per-gold knapsack with a gold reserve
//! (plan §2 "shop"; bead `sts2-rs-40ikw.5`).
//!
//! The engine surfaces the shop iteratively (one purchase per
//! `take_action`, then re-enumerates), so the knapsack runs greedily
//! per step: each call picks the single affordable item with the best
//! ΔV-per-gold ratio above the bar, or leaves. Greedy-per-step over
//! re-evaluated views ≈ the greedy knapsack, and it re-prices later
//! items after each purchase for free (deck/relic interactions). The
//! remove-card slot competes in the same knapsack. The gold reserve and
//! ΔV bars are TOML-tunable (`[shop]` in `data/runai/heuristic_v.toml`).

use crate::eval::ranker_features::ContextFeatures;
use crate::eval::run_value::ValueNet;
use crate::eval::HeuristicV;
use crate::policy::surfaces::picks;
use crate::rollout::{fight_delta, CandidateMutation, EncounterPlan, FightCache, ProbeBudget};
use crate::view::VisibleRunView;
use sts2_core::run_manager::{RunAction, RunManager};

/// Can the shop delete `c`? The engine enumerates `ShopRemoveCard` over
/// `RunState::removal_offer_order()`, whose gate is C#
/// `CardModel.IsRemovable => !Keywords.Contains(CardKeyword.Eternal)`
/// (`CardModel.cs:738`) — basics included, Eternal curses excluded.
///
/// Until 2026-08-26 all three call sites below filtered on the legacy
/// `CardId::is_removable`, which excluded basic Strike/Defend and claimed in
/// a comment to "match the engine surface". It did not: the engine has
/// offered basics since 2026-06-19, so the heuristic policy could never
/// propose the standard Strike-thinning line, and Eternal curses it could
/// never buy were scored anyway.
///
/// [`crate::view::DeckCard`] is a lossy projection (id + upgraded), so this
/// uses the spec-only predicate and cannot see a `TezcatarasEmber`-granted
/// Eternal keyword. Proposing such a card costs one rejected action
/// ([`sts2_core::shop::PurchaseCardRemovalResult::NotRemovable`]), never a
/// corrupt index — `deck_idx` is a master-deck index either way.
fn removable_here(c: &crate::view::DeckCard) -> bool {
    sts2_content::cards::spec_index::card_is_removable_csharp(c.card_id)
}

/// One shop decision (engine-index spaces of the matching `RunAction`s).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShopDecision {
    BuyCard { idx: usize },
    BuyRelic { idx: usize },
    BuyPotion { idx: usize },
    RemoveCard { deck_idx: usize },
    Leave,
}

/// Enumerate the feasible shop purchases for the current view, each with
/// its marginal `ΔV_net` (net of the gold spend) and price.
///
/// "Feasible" = the same filters [`best_shop_action`]'s greedy knapsack
/// applies: positive price, affordable above the gold reserve, ΔV ≥
/// `min_dv`, and ΔV-per-gold ≥ `min_dv_per_gold`. The order is
/// cards → relics → potions → remove-slot, matching the original
/// enumeration. Both the greedy `best_shop_action` (argmax ratio) and the
/// sampled deck policy (softmax over ΔV) consume this single list, so the
/// candidate set and its filters are defined in exactly one place.
#[must_use]
pub(crate) fn feasible_shop_options(
    view: &VisibleRunView,
    v: &HeuristicV,
) -> Vec<(RunAction, f64, i32)> {
    let Some(shop) = view.offers.shop.as_ref() else {
        return Vec::new();
    };
    let cfg = &v.weights.shop;
    let spendable = view.gold - cfg.gold_reserve;

    let mut out: Vec<(RunAction, f64, i32)> = Vec::new();
    let mut consider = |action: RunAction, dv: f64, price: i32| {
        if price <= 0 || price > spendable || dv < cfg.min_dv {
            return;
        }
        if dv / f64::from(price) < cfg.min_dv_per_gold {
            return;
        }
        out.push((action, dv, price));
    };

    for (idx, offer) in shop.cards.iter().enumerate() {
        if let Some(o) = offer {
            // Net of the gold spend: V's gold term re-prices the cost.
            let dv = v.score(
                &view
                    .with_card_added(o.card.card_id, o.card.upgraded)
                    .with_gold_delta(-o.price),
            ) - v.score(view);
            consider(RunAction::ShopBuyCard { idx }, dv, o.price);
        }
    }
    for (idx, offer) in shop.relics.iter().enumerate() {
        if let Some(o) = offer {
            let dv = v.score(&view.with_relic_added(o.relic_id).with_gold_delta(-o.price))
                - v.score(view);
            consider(RunAction::ShopBuyRelic { idx }, dv, o.price);
        }
    }
    let belt_has_space = (view.potions.len() as i32) < view.max_potion_slots;
    if belt_has_space {
        for (idx, offer) in shop.potions.iter().enumerate() {
            if let Some(o) = offer {
                let dv = v.score(
                    &view
                        .with_potion_added(o.potion_kind)
                        .with_gold_delta(-o.price),
                ) - v.score(view);
                consider(RunAction::ShopBuyPotion { idx }, dv, o.price);
            }
        }
    }
    if !shop.removal_used {
        let eligible: Vec<usize> = view
            .deck
            .iter()
            .enumerate()
            .filter(|(_, c)| removable_here(c))
            .map(|(i, _)| i)
            .collect();
        if let Some((pos, dv_raw, _)) = picks::best_removal(view, v, &eligible) {
            let deck_idx = eligible[pos];
            let dv = dv_raw + v.gold_term(view.gold - shop.removal_cost) - v.gold_term(view.gold);
            consider(
                RunAction::ShopRemoveCard { deck_idx },
                dv,
                shop.removal_cost,
            );
        }
    }

    out
}

/// Map a feasible [`RunAction`] back to the matching [`ShopDecision`].
/// `LeaveShop` (and any non-shop action) maps to [`ShopDecision::Leave`].
#[must_use]
pub(crate) fn shop_decision_for(action: &RunAction) -> ShopDecision {
    match *action {
        RunAction::ShopBuyCard { idx } => ShopDecision::BuyCard { idx },
        RunAction::ShopBuyRelic { idx } => ShopDecision::BuyRelic { idx },
        RunAction::ShopBuyPotion { idx } => ShopDecision::BuyPotion { idx },
        RunAction::ShopRemoveCard { deck_idx } => ShopDecision::RemoveCard { deck_idx },
        _ => ShopDecision::Leave,
    }
}

/// Pick the best next shop action for the current view.
#[must_use]
pub fn best_shop_action(view: &VisibleRunView, v: &HeuristicV) -> ShopDecision {
    let mut best: Option<(ShopDecision, f64)> = None; // (decision, dv/price)
    for (action, dv, price) in feasible_shop_options(view, v) {
        let ratio = dv / f64::from(price);
        let better = match best {
            None => true,
            Some((_, b)) => ratio > b,
        };
        if better {
            best = Some((shop_decision_for(&action), ratio));
        }
    }
    best.map_or(ShopDecision::Leave, |(d, _)| d)
}

/// Net-valued shop oracle — the E2 oracle leg, Stage A (e0-baselines
/// Addendum 25 follow-up). Same greedy-ratio structure as
/// [`best_shop_action`], but each option's marginal ΔV is scored by the
/// **learned value net** (≈win-prob scale) instead of the hand-tuned
/// `HeuristicV`. Under the deployed agent the net is otherwise never
/// consulted for purchases (the ranker is card-only; V-pathing is
/// map-only), so this measures shop *valuation* headroom.
///
/// The HeuristicV-scaled bars (`min_dv`, `min_dv_per_gold`) do NOT
/// transfer to the win-prob scale, so the gate is net-native: a purchase
/// clears iff its marginal ΔV is strictly positive (it raises projected
/// win value net of the gold spent — the net sees the reduced gold via
/// `with_gold_delta`). The gold reserve (gold units) carries over
/// unchanged. Picks the best ΔV-per-gold ratio per step, greedily, just
/// like the heuristic.
#[must_use]
pub fn best_shop_action_by_value(
    view: &VisibleRunView,
    v: &HeuristicV,
    net: &ValueNet,
) -> ShopDecision {
    let Some(shop) = view.offers.shop.as_ref() else {
        return ShopDecision::Leave;
    };
    let spendable = view.gold - v.weights.shop.gold_reserve;
    let base_value = net.value(&ContextFeatures::from_view(view));
    let net_dv =
        |projected: &VisibleRunView| net.value(&ContextFeatures::from_view(projected)) - base_value;

    let mut best: Option<(ShopDecision, f64)> = None; // (decision, dv/price)
    let mut consider = |decision: ShopDecision, dv: f64, price: i32| {
        // Net-native gate: ΔV > 0 (raises win value net of the spend).
        if price <= 0 || price > spendable || dv <= 0.0 {
            return;
        }
        let ratio = dv / f64::from(price);
        let better = match best {
            None => true,
            Some((_, b)) => ratio > b,
        };
        if better {
            best = Some((decision, ratio));
        }
    };

    for (idx, offer) in shop.cards.iter().enumerate() {
        if let Some(o) = offer {
            let dv = net_dv(
                &view
                    .with_card_added(o.card.card_id, o.card.upgraded)
                    .with_gold_delta(-o.price),
            );
            consider(ShopDecision::BuyCard { idx }, dv, o.price);
        }
    }
    for (idx, offer) in shop.relics.iter().enumerate() {
        if let Some(o) = offer {
            let dv = net_dv(&view.with_relic_added(o.relic_id).with_gold_delta(-o.price));
            consider(ShopDecision::BuyRelic { idx }, dv, o.price);
        }
    }
    let belt_has_space = (view.potions.len() as i32) < view.max_potion_slots;
    if belt_has_space {
        for (idx, offer) in shop.potions.iter().enumerate() {
            if let Some(o) = offer {
                let dv = net_dv(
                    &view
                        .with_potion_added(o.potion_kind)
                        .with_gold_delta(-o.price),
                );
                consider(ShopDecision::BuyPotion { idx }, dv, o.price);
            }
        }
    }
    if !shop.removal_used {
        // Net-value each removable directly (picks::best_removal scores
        // with HeuristicV; the oracle must use the net for parity).
        let mut best_rem: Option<(usize, f64)> = None;
        for (deck_idx, _) in view
            .deck
            .iter()
            .enumerate()
            .filter(|(_, c)| removable_here(c))
        {
            let dv = net_dv(
                &view
                    .with_deck_card_removed(deck_idx)
                    .with_gold_delta(-shop.removal_cost),
            );
            let better = match best_rem {
                None => true,
                Some((_, b)) => dv > b,
            };
            if better {
                best_rem = Some((deck_idx, dv));
            }
        }
        if let Some((deck_idx, dv)) = best_rem {
            consider(ShopDecision::RemoveCard { deck_idx }, dv, shop.removal_cost);
        }
    }

    best.map_or(ShopDecision::Leave, |(d, _)| d)
}

/// Fight-grounded shop oracle — the E2 oracle leg, Stage C (the plan's
/// tier-3 shop oracle; Addendum 26 follow-up). Each affordable purchase
/// is scored by how much it improves the upcoming elite+boss FIGHTS,
/// measured with `fight_delta` rollouts — the same probe that ranks boss
/// relics. This replaces HeuristicV's flat per-relic value (one
/// `relic_default` guess) and the gold-confounded value net (Addendum 26,
/// `best_shop_action_by_value`) with a per-item combat-outcome
/// measurement on the actual deck.
///
/// Combat value `cv = candidate.score() − baseline.score()` is a PAIRED
/// difference (`fight_delta` seeds the probe fights off the encounter
/// identity, identical across candidates), so a single relic's marginal
/// value is detectable despite per-fight variance. Probes are bounded to
/// the top-`top_n` candidates by a cheap HeuristicV pre-rank (mirrors
/// `TieredPolicy::escalate`). The gold trade is a `cv`-per-gold knapsack
/// with a `min_cv` noise floor and the gold reserve. Potions are left out
/// (combat rollouts don't reliably exercise potion use); with no upcoming
/// fight to ground against, falls back to the HeuristicV knapsack.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn best_shop_action_grounded(
    view: &VisibleRunView,
    v: &HeuristicV,
    rm: &RunManager,
    budget: &ProbeBudget,
    cache: &FightCache,
    decision_key: u64,
    top_n: usize,
    min_cv: f64,
) -> ShopDecision {
    let Some(shop) = view.offers.shop.as_ref() else {
        return ShopDecision::Leave;
    };
    let plan = EncounterPlan::for_decision(rm, decision_key);
    // No upcoming fight to ground against → defer to the heuristic knapsack.
    if plan.elite.is_none() && plan.boss.is_none() {
        return best_shop_action(view, v);
    }
    let spendable = view.gold - v.weights.shop.gold_reserve;

    // Affordable candidates: (decision, mutation, price, heuristic ΔV).
    // The heuristic ΔV is ONLY a pre-rank to bound the probe count.
    let base_v = v.score(view);
    let hv_dv = |projected: &VisibleRunView| v.score(projected) - base_v;
    let mut cands: Vec<(ShopDecision, CandidateMutation, i32, f64)> = Vec::new();
    for (idx, offer) in shop.cards.iter().enumerate() {
        if let Some(o) = offer {
            if o.price > 0 && o.price <= spendable {
                let dv = hv_dv(
                    &view
                        .with_card_added(o.card.card_id, o.card.upgraded)
                        .with_gold_delta(-o.price),
                );
                cands.push((
                    ShopDecision::BuyCard { idx },
                    CandidateMutation::AddCard {
                        card_id: o.card.card_id,
                        upgraded: o.card.upgraded,
                    },
                    o.price,
                    dv,
                ));
            }
        }
    }
    for (idx, offer) in shop.relics.iter().enumerate() {
        if let Some(o) = offer {
            if o.price > 0 && o.price <= spendable {
                let dv = hv_dv(&view.with_relic_added(o.relic_id).with_gold_delta(-o.price));
                cands.push((
                    ShopDecision::BuyRelic { idx },
                    CandidateMutation::AddRelic {
                        relic_id: o.relic_id,
                    },
                    o.price,
                    dv,
                ));
            }
        }
    }
    if !shop.removal_used && shop.removal_cost > 0 && shop.removal_cost <= spendable {
        let eligible: Vec<usize> = view
            .deck
            .iter()
            .enumerate()
            .filter(|(_, c)| removable_here(c))
            .map(|(i, _)| i)
            .collect();
        if let Some((pos, dv_raw, _)) = picks::best_removal(view, v, &eligible) {
            let deck_idx = eligible[pos];
            let dv = dv_raw + v.gold_term(view.gold - shop.removal_cost) - v.gold_term(view.gold);
            cands.push((
                ShopDecision::RemoveCard { deck_idx },
                CandidateMutation::RemoveCard { deck_idx },
                shop.removal_cost,
                dv,
            ));
        }
    }
    if cands.is_empty() {
        return ShopDecision::Leave;
    }
    // Pre-rank by heuristic ΔV; probe only the top-N (bounds rollout cost).
    cands.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    cands.truncate(top_n.max(1));

    let base_score = fight_delta(rm, &CandidateMutation::Baseline, &plan, budget, cache).score();
    let mut best: Option<(ShopDecision, f64)> = None; // (decision, cv/price)
    for (decision, mutation, price, _) in &cands {
        // Paired combat-value improvement over the status quo.
        let cv = fight_delta(rm, mutation, &plan, budget, cache).score() - base_score;
        if cv < min_cv {
            continue;
        }
        let ratio = cv / f64::from(*price);
        let better = match best {
            None => true,
            Some((_, b)) => ratio > b,
        };
        if better {
            best = Some((*decision, ratio));
        }
    }
    best.map_or(ShopDecision::Leave, |(d, _)| d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{DeckCard, ShopCardOffer, ShopPotionOffer, ShopRelicOffer, ShopView};
    use sts2_content::run_callbacks::new_run_manager_for_character;
    use sts2_core::cards::CardId;
    use sts2_core::potions::{CharacterId, PotionKind};
    use sts2_core::RelicId;

    fn view_with_shop(gold: i32, card_price: i32) -> VisibleRunView {
        let rm = new_run_manager_for_character(29, CharacterId::Ironclad);
        let mut view = crate::view::VisibleRunView::from_run_manager(&rm);
        view.gold = gold;
        view.offers.shop = Some(ShopView {
            cards: vec![Some(ShopCardOffer {
                card: DeckCard {
                    card_id: CardId::Inflame,
                    upgraded: true,
                },
                price: card_price,
            })],
            relics: vec![],
            potions: vec![],
            removal_cost: 75,
            removal_used: true,
        });
        view
    }

    /// The knapsack respects the gold reserve (M1 acceptance test):
    /// an affordable, good item is NOT bought when paying it would dip
    /// below the reserve.
    #[test]
    fn knapsack_respects_gold_reserve() {
        let v = HeuristicV::default();
        let reserve = v.weights.shop.gold_reserve;
        // gold = reserve + 49 < reserve + price → must leave.
        let view = view_with_shop(reserve + 49, 50);
        assert_eq!(best_shop_action(&view, &v), ShopDecision::Leave);
        // With enough gold above the reserve the same item is bought.
        let view = view_with_shop(reserve + 51, 50);
        assert_eq!(
            best_shop_action(&view, &v),
            ShopDecision::BuyCard { idx: 0 }
        );
    }

    /// Sold-out / empty inventory leaves.
    #[test]
    fn empty_shop_leaves() {
        let rm = new_run_manager_for_character(31, CharacterId::Ironclad);
        let mut view = crate::view::VisibleRunView::from_run_manager(&rm);
        view.gold = 999;
        view.offers.shop = Some(ShopView {
            cards: vec![None],
            relics: vec![None],
            potions: vec![None],
            removal_cost: 75,
            removal_used: true,
        });
        assert_eq!(
            best_shop_action(&view, &HeuristicV::default()),
            ShopDecision::Leave
        );
    }

    /// A constant value net (every state scores the same) yields zero
    /// marginal ΔV for every purchase, so the net-native gate (ΔV > 0)
    /// rejects them all and the oracle leaves — even with an affordable,
    /// reserve-clearing card on offer. Confirms the gate is net-driven,
    /// not the HeuristicV-scaled bars.
    #[test]
    fn net_oracle_leaves_when_no_positive_dv() {
        use crate::eval::ranker_features::{context_dim, feature_schema_id};
        let cdim = context_dim();
        // w1 all-zero + w2 all-zero ⇒ value == b2 for any input (constant).
        let json = serde_json::json!({
            "schema_id": feature_schema_id(),
            "context_dim": cdim,
            "hidden": 2,
            "feat_mu": vec![0.0f32; cdim],
            "feat_sd": vec![1.0f32; cdim],
            "y_mu": 0.0,
            "y_sd": 1.0,
            "w1": vec![vec![0.0f32; 2]; cdim],
            "b1": [1.0f32, 1.0],
            "w2": [0.0f32, 0.0],
            "b2": 0.5f32,
        })
        .to_string();
        let net = ValueNet::from_json_slice(json.as_bytes()).expect("loads");
        let v = HeuristicV::default();
        let reserve = v.weights.shop.gold_reserve;
        // Plenty of gold above the reserve and a juicy card — the
        // HeuristicV knapsack would buy it; the constant net must not.
        let view = view_with_shop(reserve + 500, 50);
        assert_eq!(
            best_shop_action_by_value(&view, &v, &net),
            ShopDecision::Leave
        );
    }

    // ── Shared fixtures ──────────────────────────────────────────────

    /// Fresh Ironclad view. Cached because building a run manager also
    /// generates an act map — too slow to redo per property case.
    fn fresh_view() -> VisibleRunView {
        static BASE: std::sync::OnceLock<VisibleRunView> = std::sync::OnceLock::new();
        BASE.get_or_init(|| {
            let rm = new_run_manager_for_character(29, CharacterId::Ironclad);
            VisibleRunView::from_run_manager(&rm)
        })
        .clone()
    }

    /// A shop screen with every row present but sold out and the remove
    /// slot spent — each test populates only the rows it is about.
    fn empty_shop() -> ShopView {
        ShopView {
            cards: vec![None],
            relics: vec![None],
            potions: vec![None],
            removal_cost: 75,
            removal_used: true,
        }
    }

    fn card_offer(card_id: CardId, upgraded: bool, price: i32) -> ShopCardOffer {
        ShopCardOffer {
            card: DeckCard { card_id, upgraded },
            price,
        }
    }

    /// Dense-feature index of a [`crate::eval::ranker_features::SCALAR_NAMES`]
    /// entry: the scalar block sits after the deck-histogram and relic
    /// blocks in the value net's input vector.
    fn scalar_dense_index(name: &str) -> usize {
        use crate::eval::ranker_features::{n_cards, n_relics, SCALAR_NAMES};
        let pos = SCALAR_NAMES
            .iter()
            .position(|n| *n == name)
            .expect("known scalar name");
        2 * n_cards() + n_relics() + pos
    }

    /// A value net that evaluates to exactly `100 + Σ w·scalar`: one
    /// hidden unit whose bias keeps the ReLU open for every view the
    /// tests build, identity output, no standardization. This lets a test
    /// state the net's preferences in win-value units directly ("+1 per
    /// relic owned", "−1 per curse in the deck") and predict the oracle's
    /// marginal ΔV exactly.
    fn linear_net(terms: &[(&str, f32)]) -> ValueNet {
        use crate::eval::ranker_features::{context_dim, feature_schema_id};
        let cdim = context_dim();
        let mut w1 = vec![vec![0.0f32]; cdim];
        for (name, w) in terms {
            w1[scalar_dense_index(name)] = vec![*w];
        }
        let json = serde_json::json!({
            "schema_id": feature_schema_id(),
            "context_dim": cdim,
            "hidden": 1,
            "feat_mu": vec![0.0f32; cdim],
            "feat_sd": vec![1.0f32; cdim],
            "y_mu": 0.0,
            "y_sd": 1.0,
            "w1": w1,
            "b1": [100.0f32],
            "w2": [1.0f32],
            "b2": 0.0f32,
        })
        .to_string();
        ValueNet::from_json_slice(json.as_bytes()).expect("loads")
    }

    // ── feasible_shop_options / best_shop_action ─────────────────────

    /// No shop on screen (any other phase) → no candidates at all, and
    /// the greedy knapsack leaves.
    #[test]
    fn no_shop_screen_yields_no_options() {
        let v = HeuristicV::default();
        let view = fresh_view();
        assert!(view.offers.shop.is_none());
        assert!(feasible_shop_options(&view, &v).is_empty());
        assert_eq!(best_shop_action(&view, &v), ShopDecision::Leave);
    }

    /// The candidate list is ordered cards → relics → potions →
    /// remove-slot. Both consumers (greedy argmax and the sampled deck
    /// policy's softmax) index into this list, so the order is part of
    /// the contract, not an accident of enumeration.
    #[test]
    fn feasible_options_are_ordered_cards_relics_potions_removal() {
        let v = HeuristicV::default();
        let reserve = v.weights.shop.gold_reserve;
        // A curse makes the remove-slot a positive-ΔV option.
        let mut view = fresh_view().with_card_added(CardId::Decay, false);
        view.gold = reserve + 500;
        view.offers.shop = Some(ShopView {
            cards: vec![Some(card_offer(CardId::Inflame, true, 50))],
            relics: vec![Some(ShopRelicOffer {
                relic_id: RelicId::Anchor,
                price: 50,
            })],
            potions: vec![Some(ShopPotionOffer {
                potion_kind: PotionKind::BlockPotion,
                price: 50,
            })],
            removal_cost: 50,
            removal_used: false,
        });

        let kinds: Vec<ShopDecision> = feasible_shop_options(&view, &v)
            .iter()
            .map(|(a, _, _)| shop_decision_for(a))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ShopDecision::BuyCard { idx: 0 },
                ShopDecision::BuyRelic { idx: 0 },
                ShopDecision::BuyPotion { idx: 0 },
                ShopDecision::RemoveCard {
                    deck_idx: view.deck.len() - 1
                },
            ],
            "all four rows feasible, in the documented order"
        );
    }

    /// The knapsack maximizes ΔV *per gold*, not raw ΔV: a 250g relic
    /// worth far more V than a 50g card still loses to the card.
    #[test]
    fn greedy_picks_best_ratio_not_biggest_dv() {
        let v = HeuristicV::default();
        let reserve = v.weights.shop.gold_reserve;
        let mut view = fresh_view();
        view.gold = reserve + 500;
        view.offers.shop = Some(ShopView {
            cards: vec![Some(card_offer(CardId::Inflame, true, 50))],
            relics: vec![Some(ShopRelicOffer {
                relic_id: RelicId::Anchor,
                price: 250,
            })],
            ..empty_shop()
        });

        let opts = feasible_shop_options(&view, &v);
        assert_eq!(opts.len(), 2, "both affordable above the reserve");
        let (_, card_dv, card_price) = opts[0];
        let (_, relic_dv, relic_price) = opts[1];
        // Non-vacuous: the relic really is the bigger absolute upgrade.
        assert!(
            relic_dv > card_dv,
            "fixture must give the relic the larger ΔV ({relic_dv} <= {card_dv})"
        );
        assert!(card_dv / f64::from(card_price) > relic_dv / f64::from(relic_price));
        assert_eq!(
            best_shop_action(&view, &v),
            ShopDecision::BuyCard { idx: 0 }
        );
    }

    /// Both TOML bars gate independently: a purchase the default weights
    /// accept is rejected when `min_dv` (absolute) or `min_dv_per_gold`
    /// (ratio) is raised above it.
    #[test]
    fn min_dv_and_min_dv_per_gold_bars_reject_purchases() {
        let base = HeuristicV::default();
        let reserve = base.weights.shop.gold_reserve;
        let view = view_with_shop(reserve + 500, 50);
        assert_eq!(
            best_shop_action(&view, &base),
            ShopDecision::BuyCard { idx: 0 },
            "control: the default bars accept this card"
        );

        let mut high_dv = HeuristicV::default();
        high_dv.weights.shop.min_dv = 10.0;
        assert!(feasible_shop_options(&view, &high_dv).is_empty());
        assert_eq!(best_shop_action(&view, &high_dv), ShopDecision::Leave);

        let mut high_ratio = HeuristicV::default();
        // Below the absolute bar's reach (ΔV clears `min_dv`) but far
        // above any ΔV-per-gold a 50g card can produce.
        high_ratio.weights.shop.min_dv_per_gold = 1.0;
        assert!(feasible_shop_options(&view, &high_ratio).is_empty());
        assert_eq!(best_shop_action(&view, &high_ratio), ShopDecision::Leave);
    }

    /// A zero-price slot is never a candidate (the ratio is undefined —
    /// the surface requires a positive price).
    #[test]
    fn zero_price_offers_are_not_candidates() {
        let v = HeuristicV::default();
        let view = view_with_shop(v.weights.shop.gold_reserve + 500, 0);
        assert!(feasible_shop_options(&view, &v).is_empty());
        assert_eq!(best_shop_action(&view, &v), ShopDecision::Leave);
    }

    /// Potions only compete for gold while the belt has a free slot.
    #[test]
    fn full_potion_belt_drops_potion_offers() {
        let v = HeuristicV::default();
        let reserve = v.weights.shop.gold_reserve;
        let mut view = fresh_view();
        view.gold = reserve + 500;
        view.offers.shop = Some(ShopView {
            potions: vec![Some(ShopPotionOffer {
                potion_kind: PotionKind::BlockPotion,
                price: 50,
            })],
            ..empty_shop()
        });
        assert!(view.max_potion_slots > 0);
        view.potions.clear();
        assert_eq!(
            best_shop_action(&view, &v),
            ShopDecision::BuyPotion { idx: 0 },
            "empty belt: the potion is worth buying"
        );

        view.potions = vec![PotionKind::BlockPotion; view.max_potion_slots as usize];
        assert!(feasible_shop_options(&view, &v).is_empty());
        assert_eq!(best_shop_action(&view, &v), ShopDecision::Leave);
    }

    /// Every shop `RunAction` maps to its decision; anything else (the
    /// engine's `LeaveShop`, or a non-shop action) is `Leave`.
    #[test]
    fn shop_decision_for_maps_every_action() {
        assert_eq!(
            shop_decision_for(&RunAction::ShopBuyCard { idx: 2 }),
            ShopDecision::BuyCard { idx: 2 }
        );
        assert_eq!(
            shop_decision_for(&RunAction::ShopBuyRelic { idx: 1 }),
            ShopDecision::BuyRelic { idx: 1 }
        );
        assert_eq!(
            shop_decision_for(&RunAction::ShopBuyPotion { idx: 0 }),
            ShopDecision::BuyPotion { idx: 0 }
        );
        assert_eq!(
            shop_decision_for(&RunAction::ShopRemoveCard { deck_idx: 7 }),
            ShopDecision::RemoveCard { deck_idx: 7 }
        );
        assert_eq!(
            shop_decision_for(&RunAction::LeaveShop),
            ShopDecision::Leave
        );
        assert_eq!(
            shop_decision_for(&RunAction::RestOption { idx: 0 }),
            ShopDecision::Leave
        );
    }

    // ── best_shop_action_by_value (net oracle) ───────────────────────

    /// No shop screen → the net oracle leaves without consulting the net.
    #[test]
    fn net_oracle_no_shop_leaves() {
        let view = fresh_view();
        assert_eq!(
            best_shop_action_by_value(&view, &HeuristicV::default(), &linear_net(&[])),
            ShopDecision::Leave
        );
    }

    /// The net oracle is a ΔV-per-gold argmax too: with a net that pays a
    /// flat +1 per relic owned, the two relics tie on ΔV and the cheaper
    /// one (listed second — so "first wins" cannot explain it) is bought.
    #[test]
    fn net_oracle_picks_best_ratio_relic() {
        let v = HeuristicV::default();
        let net = linear_net(&[("relic_count", 1.0)]);
        let mut view = fresh_view();
        view.gold = v.weights.shop.gold_reserve + 500;
        view.offers.shop = Some(ShopView {
            relics: vec![
                Some(ShopRelicOffer {
                    relic_id: RelicId::Anchor,
                    price: 100,
                }),
                Some(ShopRelicOffer {
                    relic_id: RelicId::Vajra,
                    price: 50,
                }),
            ],
            ..empty_shop()
        });
        assert_eq!(
            best_shop_action_by_value(&view, &v, &net),
            ShopDecision::BuyRelic { idx: 1 }
        );
    }

    /// The gold reserve carries over to the net oracle in gold units:
    /// a purchase whose price would dip below the reserve is skipped even
    /// though the net says it raises win value. The bound is inclusive —
    /// price == spendable buys.
    #[test]
    fn net_oracle_respects_gold_reserve() {
        let v = HeuristicV::default();
        let reserve = v.weights.shop.gold_reserve;
        let net = linear_net(&[("relic_count", 1.0)]);
        let shop = ShopView {
            relics: vec![Some(ShopRelicOffer {
                relic_id: RelicId::Anchor,
                price: 50,
            })],
            ..empty_shop()
        };

        let mut view = fresh_view();
        view.offers.shop = Some(shop);
        view.gold = reserve + 49;
        assert_eq!(
            best_shop_action_by_value(&view, &v, &net),
            ShopDecision::Leave
        );
        view.gold = reserve + 50;
        assert_eq!(
            best_shop_action_by_value(&view, &v, &net),
            ShopDecision::BuyRelic { idx: 0 }
        );
    }

    /// The belt-capacity gate applies to the net oracle as well.
    #[test]
    fn net_oracle_buys_potions_only_with_belt_space() {
        let v = HeuristicV::default();
        let net = linear_net(&[("potion_count", 1.0)]);
        let mut view = fresh_view();
        view.gold = v.weights.shop.gold_reserve + 500;
        view.potions.clear();
        view.offers.shop = Some(ShopView {
            potions: vec![Some(ShopPotionOffer {
                potion_kind: PotionKind::BlockPotion,
                price: 50,
            })],
            ..empty_shop()
        });
        assert_eq!(
            best_shop_action_by_value(&view, &v, &net),
            ShopDecision::BuyPotion { idx: 0 }
        );

        view.potions = vec![PotionKind::BlockPotion; view.max_potion_slots as usize];
        assert_eq!(
            best_shop_action_by_value(&view, &v, &net),
            ShopDecision::Leave
        );
    }

    /// The oracle net-values each removable deck slot itself (rather than
    /// deferring to `picks::best_removal`'s HeuristicV ranking): a net
    /// that only dislikes curses removes the curse, not the other
    /// removable starter card.
    #[test]
    fn net_oracle_removes_the_card_the_net_dislikes() {
        let v = HeuristicV::default();
        let net = linear_net(&[("deck_curses", -1.0)]);
        let mut view = fresh_view().with_card_added(CardId::Decay, false);
        view.gold = v.weights.shop.gold_reserve + 500;
        view.offers.shop = Some(ShopView {
            removal_cost: 50,
            removal_used: false,
            ..empty_shop()
        });
        let curse_idx = view.deck.len() - 1;
        // Non-vacuous: another removable (non-curse) card is also on the
        // table, and it is enumerated FIRST — only the net's valuation
        // can select the curse.
        assert!(view.deck[..curse_idx].iter().any(removable_here));
        assert_eq!(
            best_shop_action_by_value(&view, &v, &net),
            ShopDecision::RemoveCard {
                deck_idx: curse_idx
            }
        );

        // Spent remove slot → nothing left to buy.
        if let Some(shop) = view.offers.shop.as_mut() {
            shop.removal_used = true;
        }
        assert_eq!(
            best_shop_action_by_value(&view, &v, &net),
            ShopDecision::Leave
        );
    }

    // ── best_shop_action_grounded (fight-probe oracle) ───────────────

    /// Cheap probe budget: 1 sim/move, 1 paired seed, 1-turn horizon.
    /// The grounded tests below assert *structure* (which candidates get
    /// probed, which gates fire), never a rollout outcome, so the budget
    /// only has to be non-degenerate.
    fn tiny_budget() -> ProbeBudget {
        ProbeBudget {
            sims: 1,
            k: 1,
            t_turns: 1,
            ..ProbeBudget::default()
        }
    }

    #[test]
    fn grounded_no_shop_leaves() {
        let rm = new_run_manager_for_character(29, CharacterId::Ironclad);
        let view = VisibleRunView::from_run_manager(&rm);
        assert_eq!(
            best_shop_action_grounded(
                &view,
                &HeuristicV::default(),
                &rm,
                &tiny_budget(),
                &FightCache::default(),
                7,
                3,
                0.0,
            ),
            ShopDecision::Leave
        );
    }

    /// With no upcoming elite or boss to ground against, the grounded
    /// oracle defers to the HeuristicV knapsack instead of probing.
    #[test]
    fn grounded_without_upcoming_fight_falls_back_to_heuristic() {
        let v = HeuristicV::default();
        let mut rm = new_run_manager_for_character(29, CharacterId::Ironclad);
        // No encounter table for act 0 → empty plan.
        rm.run_state.act_index = 0;
        let plan = EncounterPlan::for_decision(&rm, 7);
        assert!(plan.elite.is_none() && plan.boss.is_none());

        let view = view_with_shop(v.weights.shop.gold_reserve + 500, 50);
        let heuristic = best_shop_action(&view, &v);
        assert_eq!(
            heuristic,
            ShopDecision::BuyCard { idx: 0 },
            "fixture must give the fallback something to buy"
        );
        assert_eq!(
            best_shop_action_grounded(
                &view,
                &v,
                &rm,
                &tiny_budget(),
                &FightCache::default(),
                7,
                3,
                0.0,
            ),
            heuristic
        );
    }

    /// Nothing affordable above the reserve → leave before any rollout
    /// runs (the empty-candidate short-circuit; the fight cache stays
    /// untouched, which is what proves no probe fired).
    #[test]
    fn grounded_leaves_when_nothing_affordable() {
        let v = HeuristicV::default();
        let rm = new_run_manager_for_character(29, CharacterId::Ironclad);
        let plan = EncounterPlan::for_decision(&rm, 7);
        assert!(
            plan.elite.is_some() || plan.boss.is_some(),
            "act-1 fixture must have something to ground against"
        );

        let view = view_with_shop(v.weights.shop.gold_reserve + 10, 50);
        let cache = FightCache::default();
        assert_eq!(
            best_shop_action_grounded(&view, &v, &rm, &tiny_budget(), &cache, 7, 3, 0.0),
            ShopDecision::Leave
        );
        assert!(cache.is_empty(), "no probe should have run");
    }

    /// `min_cv` is a noise floor over the paired combat-value delta: an
    /// unreachable floor rejects every probed candidate and the oracle
    /// leaves — after actually probing (the cache is warm).
    #[test]
    fn grounded_min_cv_floor_rejects_every_candidate() {
        let v = HeuristicV::default();
        let rm = new_run_manager_for_character(29, CharacterId::Ironclad);
        let view = view_with_shop(v.weights.shop.gold_reserve + 500, 50);
        let cache = FightCache::default();
        assert_eq!(
            best_shop_action_grounded(&view, &v, &rm, &tiny_budget(), &cache, 7, 1, f64::INFINITY,),
            ShopDecision::Leave
        );
        // One cache entry per (state, encounter): baseline + the single
        // top-ranked candidate — i.e. the probes really ran.
        assert_eq!(cache.len(), 2 * plan_encounters(&rm));
    }

    /// Encounters in the decision's grounding plan (elite and/or boss) —
    /// each probed candidate contributes exactly this many fight-cache
    /// entries.
    fn plan_encounters(rm: &RunManager) -> usize {
        let plan = EncounterPlan::for_decision(rm, 7);
        let n = usize::from(plan.elite.is_some()) + usize::from(plan.boss.is_some());
        assert!(n > 0, "fixture must ground against at least one fight");
        n
    }

    /// `top_n` bounds the probe count by pre-ranking on HeuristicV ΔV:
    /// with `top_n == 1` only the best-pre-ranked candidate can be
    /// returned, whatever the rollouts say (the floor is disabled here).
    #[test]
    fn grounded_probes_only_the_top_pre_ranked_candidate() {
        let v = HeuristicV::default();
        let rm = new_run_manager_for_character(29, CharacterId::Ironclad);
        let mut view = VisibleRunView::from_run_manager(&rm);
        view.gold = v.weights.shop.gold_reserve + 500;
        view.offers.shop = Some(ShopView {
            cards: vec![
                Some(card_offer(CardId::StrikeIronclad, false, 50)),
                Some(card_offer(CardId::Inflame, true, 50)),
            ],
            ..empty_shop()
        });
        // Pre-rank claim: the upgraded scaling card beats another Strike.
        let opts = feasible_shop_options(&view, &v);
        let dv_of = |idx: usize| {
            opts.iter()
                .find(|(a, _, _)| shop_decision_for(a) == ShopDecision::BuyCard { idx })
                .map_or(f64::NEG_INFINITY, |(_, dv, _)| *dv)
        };
        assert!(dv_of(1) > dv_of(0));

        let cache = FightCache::default();
        assert_eq!(
            best_shop_action_grounded(
                &view,
                &v,
                &rm,
                &tiny_budget(),
                &cache,
                7,
                1,
                f64::NEG_INFINITY
            ),
            ShopDecision::BuyCard { idx: 1 }
        );
        // Exactly the baseline + ONE candidate were probed: the second
        // (worse-pre-ranked) card never reached a rollout.
        assert_eq!(cache.len(), 2 * plan_encounters(&rm));
    }

    /// Raising `top_n` widens the probe set: two affordable cards mean
    /// three rollout batches (baseline + both candidates), and the winner
    /// is whichever the paired combat-value comparison prefers — so the
    /// test pins the probe accounting and the outcome domain, not the
    /// rollout result.
    #[test]
    fn grounded_top_n_two_probes_both_candidates() {
        let v = HeuristicV::default();
        let rm = new_run_manager_for_character(29, CharacterId::Ironclad);
        let mut view = VisibleRunView::from_run_manager(&rm);
        view.gold = v.weights.shop.gold_reserve + 500;
        view.offers.shop = Some(ShopView {
            cards: vec![
                Some(card_offer(CardId::StrikeIronclad, false, 50)),
                Some(card_offer(CardId::Inflame, true, 50)),
            ],
            ..empty_shop()
        });

        let cache = FightCache::default();
        let decision = best_shop_action_grounded(
            &view,
            &v,
            &rm,
            &tiny_budget(),
            &cache,
            7,
            2,
            f64::NEG_INFINITY,
        );
        assert!(
            decision == ShopDecision::BuyCard { idx: 0 }
                || decision == ShopDecision::BuyCard { idx: 1 },
            "got {decision:?}"
        );
        assert_eq!(cache.len(), 3 * plan_encounters(&rm));
    }

    /// Relics are grounded candidates: an affordable relic is probed as
    /// an `AddRelic` mutation instead of taking HeuristicV's flat
    /// `relic_default` valuation.
    #[test]
    fn grounded_probes_a_relic_candidate() {
        let v = HeuristicV::default();
        let rm = new_run_manager_for_character(29, CharacterId::Ironclad);
        let mut view = VisibleRunView::from_run_manager(&rm);
        view.gold = v.weights.shop.gold_reserve + 500;
        view.offers.shop = Some(ShopView {
            relics: vec![Some(ShopRelicOffer {
                relic_id: RelicId::Vajra,
                price: 150,
            })],
            ..empty_shop()
        });

        let cache = FightCache::default();
        assert_eq!(
            best_shop_action_grounded(
                &view,
                &v,
                &rm,
                &tiny_budget(),
                &cache,
                7,
                3,
                f64::NEG_INFINITY
            ),
            ShopDecision::BuyRelic { idx: 0 }
        );
        assert_eq!(cache.len(), 2 * plan_encounters(&rm));
    }

    /// The remove-card slot is a grounded candidate too — it gets probed
    /// as a `RemoveCard` mutation (not merely priced by HeuristicV) and
    /// can win the knapsack when it is the only affordable option.
    #[test]
    fn grounded_probes_the_remove_slot() {
        let v = HeuristicV::default();
        let rm = new_run_manager_for_character(29, CharacterId::Ironclad);
        let mut view = VisibleRunView::from_run_manager(&rm);
        view.gold = v.weights.shop.gold_reserve + 500;
        view.offers.shop = Some(ShopView {
            removal_cost: 50,
            removal_used: false,
            ..empty_shop()
        });

        let cache = FightCache::default();
        let decision = best_shop_action_grounded(
            &view,
            &v,
            &rm,
            &tiny_budget(),
            &cache,
            7,
            3,
            f64::NEG_INFINITY,
        );
        let ShopDecision::RemoveCard { deck_idx } = decision else {
            panic!("the remove slot is the only candidate, got {decision:?}");
        };
        assert!(removable_here(&view.deck[deck_idx]));
        // Baseline + the removal candidate were both rolled out.
        assert_eq!(cache.len(), 2 * plan_encounters(&rm));
    }

    // ── Properties ───────────────────────────────────────────────────

    use hegel::generators as gs;
    use hegel::TestCase;

    const PROP_CARDS: [CardId; 4] = [
        CardId::Inflame,
        CardId::Bash,
        CardId::Decay,
        CardId::StrikeIronclad,
    ];
    const PROP_RELICS: [RelicId; 2] = [RelicId::Anchor, RelicId::Vajra];
    const PROP_POTIONS: [PotionKind; 2] = [PotionKind::BlockPotion, PotionKind::FirePotion];

    /// Random shop screen: 0..=2 slots per row, each sold out or priced
    /// anywhere in 0..=400 (including free and unaffordable), a random
    /// remove-slot state and a random belt fill.
    fn arb_shop_view(tc: &TestCase, view: &mut VisibleRunView) {
        let n = |tc: &TestCase| tc.draw(gs::integers::<u32>().min_value(0).max_value(2)) as usize;
        let price = |tc: &TestCase| tc.draw(gs::integers::<i32>().min_value(0).max_value(400));
        let cards = (0..n(tc))
            .map(|_| {
                if tc.draw(gs::booleans()) {
                    None
                } else {
                    let i = tc.draw(gs::integers::<u32>().min_value(0).max_value(3)) as usize;
                    Some(card_offer(
                        PROP_CARDS[i],
                        tc.draw(gs::booleans()),
                        price(tc),
                    ))
                }
            })
            .collect();
        let relics = (0..n(tc))
            .map(|_| {
                if tc.draw(gs::booleans()) {
                    None
                } else {
                    let i = tc.draw(gs::integers::<u32>().min_value(0).max_value(1)) as usize;
                    Some(ShopRelicOffer {
                        relic_id: PROP_RELICS[i],
                        price: price(tc),
                    })
                }
            })
            .collect();
        let potions = (0..n(tc))
            .map(|_| {
                if tc.draw(gs::booleans()) {
                    None
                } else {
                    let i = tc.draw(gs::integers::<u32>().min_value(0).max_value(1)) as usize;
                    Some(ShopPotionOffer {
                        potion_kind: PROP_POTIONS[i],
                        price: price(tc),
                    })
                }
            })
            .collect();
        let belt = tc.draw(
            gs::integers::<i32>()
                .min_value(0)
                .max_value(view.max_potion_slots.max(0)),
        );
        view.potions = vec![PotionKind::BlockPotion; belt as usize];
        view.offers.shop = Some(ShopView {
            cards,
            relics,
            potions,
            removal_cost: price(tc),
            removal_used: tc.draw(gs::booleans()),
        });
    }

    /// Price of the slot a decision points at (panics on an out-of-range
    /// or sold-out slot — which is itself part of the property).
    fn decision_price(view: &VisibleRunView, d: ShopDecision) -> Option<i32> {
        let shop = view.offers.shop.as_ref()?;
        match d {
            ShopDecision::BuyCard { idx } => Some(shop.cards[idx].expect("live card slot").price),
            ShopDecision::BuyRelic { idx } => {
                Some(shop.relics[idx].expect("live relic slot").price)
            }
            ShopDecision::BuyPotion { idx } => {
                Some(shop.potions[idx].expect("live potion slot").price)
            }
            ShopDecision::RemoveCard { .. } => Some(shop.removal_cost),
            ShopDecision::Leave => None,
        }
    }

    /// Whatever the inventory, a purchase never dips below the gold
    /// reserve, never points at a sold-out/out-of-range slot, never
    /// never points at an Eternal card, and never happens at all once gold
    /// is at or below the reserve (the M1 reserve contract). Basics ARE
    /// eligible — the shop deletes them like anything else.
    #[hegel::test(test_cases = 500)]
    fn prop_pick_is_affordable_and_well_formed(tc: TestCase) {
        let v = HeuristicV::default();
        let reserve = v.weights.shop.gold_reserve;
        let mut view = fresh_view();
        view.gold = tc.draw(gs::integers::<i32>().min_value(0).max_value(1200));
        arb_shop_view(&tc, &mut view);

        let d = best_shop_action(&view, &v);
        tc.note(&format!("gold={} → {d:?}", view.gold));
        if view.gold <= reserve {
            assert_eq!(d, ShopDecision::Leave, "no spending below the reserve");
            return;
        }
        if let ShopDecision::RemoveCard { deck_idx } = d {
            let shop = view.offers.shop.as_ref().expect("shop");
            assert!(!shop.removal_used, "spent remove slot must not be picked");
            assert!(deck_idx < view.deck.len());
            assert!(removable_here(&view.deck[deck_idx]));
        }
        if let Some(price) = decision_price(&view, d) {
            assert!(price > 0, "zero/negative price is never a purchase");
            assert!(
                price <= view.gold - reserve,
                "price {price} spends into the {reserve}g reserve (gold {})",
                view.gold
            );
        }
    }

    /// Extra gold can never turn a purchase into a walk-out: the gold
    /// term is concave, so paying the same price out of a bigger pile
    /// costs strictly less V — every option that cleared the bars stays
    /// feasible.
    #[hegel::test(test_cases = 500)]
    fn prop_more_gold_never_forces_leave(tc: TestCase) {
        let v = HeuristicV::default();
        let mut view = fresh_view();
        view.gold = tc.draw(gs::integers::<i32>().min_value(0).max_value(600));
        arb_shop_view(&tc, &mut view);
        let extra = tc.draw(gs::integers::<i32>().min_value(0).max_value(600));

        let poor = best_shop_action(&view, &v);
        let rich = best_shop_action(&view.with_gold_delta(extra), &v);
        tc.note(&format!(
            "gold={} (+{extra}): {poor:?} → {rich:?}",
            view.gold
        ));
        if poor != ShopDecision::Leave {
            assert_ne!(
                rich,
                ShopDecision::Leave,
                "richer must still find something worth buying"
            );
        }
    }
}
