//! `TieredPolicy` — tier-1 heuristic with margin-gated tier-2/3 rollout
//! escalation (full-run-AI plan §2, §5/M2; bead `sts2-rs-40ikw.6`).
//!
//! Every decision is first scored by the wrapped
//! [`HeuristicPolicy`] (tier 1). When the decision is card-shaped and
//! the tier-1 top-2 margin falls below the surface's threshold
//! (`[tiers]` in `data/runai/heuristic_v.toml`; E7 sweeps these), the
//! top candidates are re-ranked by [`fight_delta`] rollout probes:
//!
//! - **Tier 2** (default 100-sim, K=16, T=8): card rewards, deck
//!   removal / upgrade / transform targets, smith picks.
//! - **Tier 3** (stronger budget): boss-relic picks.
//!
//! **Meta-move surfaces (M2.5, bead `sts2-rs-5jrk9`):** `fight_delta` can't
//! rank `Move` options (deck/relics/HP are identical across them), so Path
//! (`MapChoice`) and Rest (`RestSite`) escalate through dedicated probes
//! ([`path_delta`] grounds the committing edge's next encounter,
//! [`rest_delta`] grounds heal-vs-smith) blended with the tier-1 projected
//! score — see [`TieredPolicy::escalate_meta`]. The E2 importance leg
//! showed these own the act-2 frontier (Path +36, Rest +24). Gated by
//! `path_margin`/`rest_margin` (default `0.0` = off until E12/E13
//! calibrate).
//!
//! Escalation counts are tracked per tier + per surface and surfaced in
//! the arena summary (plan §3.3: report escalations per sweep).

use std::collections::BTreeMap;

use sts2_core::full_run::RunPolicy;
use sts2_core::map::MapPointType;
use sts2_core::run_manager::{Phase, RunAction, RunManager};

use crate::eval::Weights;
use crate::policy::surfaces::{pathing, picks, rest};
use crate::policy::{HeuristicPolicy, VScoreSource};
use crate::rollout::{
    candidates_for_surface, detect_card_surface, path_delta, rest_delta, CardSurface,
    EncounterPlan, FightCache, ProbeBudget,
};
use crate::view::VisibleRunView;

/// Escalation counters (plan §3.3 sweep reporting).
#[derive(Clone, Debug, Default)]
pub struct EscalationCounts {
    /// Decisions escalated at the tier-2 budget.
    pub tier2: u64,
    /// Decisions escalated at the tier-3 budget (boss relics).
    pub tier3: u64,
    /// Per-surface escalation counts (`CardSurface::token` keys).
    pub by_surface: BTreeMap<&'static str, u64>,
}

/// Tier-1 heuristic + margin-gated tier-2/3 rollout escalation.
#[derive(Debug)]
pub struct TieredPolicy {
    inner: HeuristicPolicy,
    weights: Weights,
    tier2: ProbeBudget,
    tier3: ProbeBudget,
    cache: FightCache,
    counts: EscalationCounts,
    decisions: u64,
    last_scores: Option<(Vec<f64>, f64)>,
}

impl Default for TieredPolicy {
    fn default() -> Self {
        Self::with_weights(Weights::default())
    }
}

impl TieredPolicy {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from a weights file (thresholds + probe budgets come from
    /// its `[tiers]` section; tier-1 V uses the same weights).
    #[must_use]
    pub fn with_weights(weights: Weights) -> Self {
        let t = &weights.tiers;
        let tier2 = ProbeBudget {
            sims: t.tier2_sims,
            k: t.tier2_k,
            t_turns: t.tier2_t_turns,
            ..ProbeBudget::default()
        };
        let tier3 = ProbeBudget {
            sims: t.tier3_sims,
            k: t.tier3_k,
            t_turns: t.tier3_t_turns,
            ..ProbeBudget::default()
        };
        Self {
            inner: HeuristicPolicy::with_weights(weights.clone()),
            weights,
            tier2,
            tier3,
            cache: FightCache::default(),
            counts: EscalationCounts::default(),
            decisions: 0,
            last_scores: None,
        }
    }

    /// Override the probe budgets (tests / CLI debug overrides).
    #[must_use]
    pub fn with_budgets(mut self, tier2: ProbeBudget, tier3: ProbeBudget) -> Self {
        self.tier2 = tier2;
        self.tier3 = tier3;
        self
    }

    /// Escalation counters accumulated so far.
    #[must_use]
    pub fn counts(&self) -> &EscalationCounts {
        &self.counts
    }

    fn threshold_for(&self, surface: CardSurface) -> f64 {
        let t = &self.weights.tiers;
        match surface {
            CardSurface::CardReward => t.card_reward_margin,
            CardSurface::BossRelic => t.boss_relic_margin,
            CardSurface::RemoveDeck | CardSurface::UpgradeDeck | CardSurface::TransformDeck => {
                t.deck_mutation_margin
            }
            CardSurface::Smith => t.smith_margin,
        }
    }

    /// Probe the top tier-1 candidates with `fight_delta` and return
    /// the action index of the rollout-best one. `scores` is the
    /// tier-1 per-option score vector, position-aligned with the
    /// candidate list (both follow the surface's option order).
    fn escalate(
        &mut self,
        rm: &RunManager,
        candidates: &[(crate::rollout::CandidateMutation, usize)],
        scores: &[f64],
        budget: ProbeBudget,
        decision_key: u64,
    ) -> Option<usize> {
        let plan = EncounterPlan::for_decision(rm, decision_key);
        plan.elite.or(plan.boss)?;

        // Candidate ordering for the probe shortlist: the tier-1 score
        // ranking.
        let mut order: Vec<usize> = (0..candidates.len()).collect();
        order.sort_by(|&a, &b| {
            let sa = scores.get(a).copied().unwrap_or(f64::NEG_INFINITY);
            let sb = scores.get(b).copied().unwrap_or(f64::NEG_INFINITY);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });
        order.truncate(self.weights.tiers.top_candidates.max(2));

        let mut best: Option<(usize, f64)> = None;
        for &ci in &order {
            let (candidate, action_idx) = &candidates[ci];
            let delta = crate::rollout::fight_delta(rm, candidate, &plan, &budget, &self.cache);
            let s = delta.score();
            // Ties break toward the higher tier-1 rank (earlier in
            // `order`) — strictly-greater keeps the first.
            let better = match best {
                None => true,
                Some((_, bs)) => s > bs,
            };
            if better {
                best = Some((*action_idx, s));
            }
        }
        best.map(|(idx, _)| idx)
    }

    /// Meta-move escalation (M2.5, bead `sts2-rs-5jrk9`): Path
    /// (`MapChoice`) and Rest (`RestSite`, non-smith) — the surfaces the
    /// card-shaped tier-2/3 stack bypasses, since `fight_delta` can't rank
    /// `Move` options. Margin-gated like the card surfaces; returns the
    /// chosen action index, falling back to `tier1_idx` when this isn't a
    /// meta surface, the margin clears the threshold, or scores/encounters
    /// are unavailable. A fired escalation bumps the tier-2 + per-surface
    /// counters.
    fn escalate_meta(
        &mut self,
        rm: &RunManager,
        actions: &[RunAction],
        tier1_idx: usize,
        tier1_scores: Option<&(Vec<f64>, f64)>,
        decision_key: u64,
    ) -> usize {
        let t = &self.weights.tiers;
        let (threshold, token) = match rm.phase() {
            Phase::MapChoice => (t.path_margin, "path"),
            Phase::RestSite if !matches!(actions.first(), Some(RunAction::SmithCard { .. })) => {
                (t.rest_margin, "rest")
            }
            _ => return tier1_idx,
        };
        let Some((scores, margin)) = tier1_scores else {
            return tier1_idx;
        };
        if *margin >= threshold {
            return tier1_idx;
        }

        let view = VisibleRunView::from_run_manager(rm);
        // Mix the run seed into the decision key (so two runs don't share
        // encounter samples decision-for-decision), as the card path does.
        let key = decision_key ^ (rm.seed.rotate_left(24));

        let chosen = match rm.phase() {
            Phase::MapChoice => self.escalate_path(rm, &view, actions, key),
            Phase::RestSite => self.escalate_rest(rm, &view, actions, scores, key),
            _ => None,
        };
        match chosen {
            Some(idx) => {
                self.counts.tier2 += 1;
                *self.counts.by_surface.entry(token).or_insert(0) += 1;
                idx.min(actions.len().saturating_sub(1))
            }
            None => tier1_idx,
        }
    }

    /// Path escalation: ground each candidate first edge's committing
    /// COMBAT room with `path_delta`'s measured HP loss, fed into
    /// `projection::score_path_grounded` (V-units, tail-aware — Addendum 7
    /// fix for the broken additive win-rate blend). Non-combat edges keep
    /// the pure projection score. Returns the best column's action index.
    fn escalate_path(
        &self,
        rm: &RunManager,
        view: &VisibleRunView,
        actions: &[RunAction],
        key: u64,
    ) -> Option<usize> {
        let map = view.map.as_ref()?;
        let mut grounded: BTreeMap<i32, f64> = BTreeMap::new();
        for a in actions {
            let RunAction::Move { col } = a else { continue };
            let Some(room) = pathing::next_room_for_col(map, *col) else {
                continue;
            };
            if matches!(room, MapPointType::Monster | MapPointType::Elite) {
                let pd = path_delta(rm, room, key, &self.tier2, &self.cache);
                if pd.probes > 0 {
                    grounded.insert(*col, pd.hp_lost_mean);
                }
            }
        }
        if grounded.is_empty() {
            return None; // nothing grounded → defer to tier-1
        }
        let (best_col, _) = pathing::scored_first_cols_grounded(view, self.inner.v(), &grounded)?;
        actions
            .iter()
            .position(|a| matches!(a, RunAction::Move { col } if *col == best_col))
    }

    /// Rest escalation: heal vs smith grounded against the shared
    /// elite+boss horizon (`rest_delta`), blended with the tier-1
    /// `[heal_dv, smith_dv]` scores. Both arms are horizon probes in the
    /// same units, so the additive blend is sound (Addendum 7: rest-only
    /// is neutral/positive at full escalation). Returns the best option's
    /// action index.
    fn escalate_rest(
        &self,
        rm: &RunManager,
        view: &VisibleRunView,
        actions: &[RunAction],
        scores: &[f64],
        key: u64,
    ) -> Option<usize> {
        let plan = EncounterPlan::for_decision(rm, key);
        plan.elite.or(plan.boss)?;
        let heal_amount =
            (f64::from(view.max_hp) * self.weights.projection.rest_heal_frac).floor() as i32;
        let eligible = picks::non_upgraded_deck_indices(view);
        // best_upgrade returns a position WITHIN `eligible`; map it back to
        // the master-deck index UpgradeCard expects.
        let smith_idx = picks::best_upgrade(view, self.inner.v(), &eligible)
            .and_then(|(pos, _, _)| eligible.get(pos).copied());
        let (heal_d, smith_d) =
            rest_delta(rm, heal_amount, smith_idx, &plan, &self.tier2, &self.cache);
        let w = self.weights.tiers.meta_probe_weight;

        // Option order matches the tier-1 [heal_dv, smith_dv] vector; map
        // each to its RunAction::RestOption index, blend, argmax.
        let mut out: Vec<(usize, f64)> = Vec::new();
        if let Some(ai) = rest_option_position(actions, rest::REST_IDX) {
            let t1 = scores.first().copied().unwrap_or(f64::NEG_INFINITY);
            out.push((ai, t1 + w * heal_d.score()));
        }
        if let (Some(ai), Some(sd)) = (rest_option_position(actions, rest::SMITH_IDX), smith_d) {
            let t1 = scores.get(1).copied().unwrap_or(f64::NEG_INFINITY);
            out.push((ai, t1 + w * sd.score()));
        }
        out.into_iter()
            .reduce(|a, b| if b.1 > a.1 { b } else { a })
            .map(|(idx, _)| idx)
    }
}

/// Index of `RunAction::RestOption { idx }` in `actions`.
fn rest_option_position(actions: &[RunAction], opt_idx: usize) -> Option<usize> {
    actions
        .iter()
        .position(|a| matches!(a, RunAction::RestOption { idx } if *idx == opt_idx))
}

impl RunPolicy for TieredPolicy {
    fn choose(&mut self, rm: &RunManager, actions: &[RunAction]) -> Option<usize> {
        let decision_key = self.decisions;
        self.decisions += 1;

        let tier1_idx = self.inner.choose(rm, actions)?;
        let tier1_scores = self.inner.take_v_scores();
        self.last_scores.clone_from(&tier1_scores);

        let Some(surface) = detect_card_surface(rm, actions) else {
            // Not card-shaped — try the M2.5 meta-move surfaces (Path /
            // Rest); falls back to the tier-1 pick when inapplicable.
            return Some(self.escalate_meta(
                rm,
                actions,
                tier1_idx,
                tier1_scores.as_ref(),
                decision_key,
            ));
        };
        let Some((scores, margin)) = tier1_scores else {
            return Some(tier1_idx);
        };
        if margin >= self.threshold_for(surface) {
            return Some(tier1_idx);
        }

        let view = VisibleRunView::from_run_manager(rm);
        let candidates = candidates_for_surface(rm, &view, surface, actions);
        if candidates.len() < 2 {
            return Some(tier1_idx);
        }

        let (budget, tier3) = if surface == CardSurface::BossRelic {
            (self.tier3, true)
        } else {
            (self.tier2, false)
        };
        // Mix the run seed into the decision key so two runs don't
        // share elite samples decision-for-decision.
        let key = decision_key ^ (rm.seed.rotate_left(24));
        match self.escalate(rm, &candidates, &scores, budget, key) {
            Some(idx) => {
                if tier3 {
                    self.counts.tier3 += 1;
                } else {
                    self.counts.tier2 += 1;
                }
                *self.counts.by_surface.entry(surface.token()).or_insert(0) += 1;
                Some(idx.min(actions.len().saturating_sub(1)))
            }
            None => Some(tier1_idx),
        }
    }
}

impl VScoreSource for TieredPolicy {
    fn take_v_scores(&mut self) -> Option<(Vec<f64>, f64)> {
        self.last_scores.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::LeafKind;
    use sts2_content::run_callbacks::new_run_manager_for_character;
    use sts2_core::potions::CharacterId;
    use sts2_core::run_manager::Phase;

    fn tiny() -> ProbeBudget {
        ProbeBudget {
            sims: 2,
            k: 1,
            t_turns: 1,
            leaf: LeafKind::TexelWide,
            horizon: crate::rollout::LabelHorizon::default(),
            chain_normals: 0,
        }
    }

    /// Drive to the first card-shaped decision with first options.
    fn walk_to_card_surface(seed: u64) -> Option<RunManager> {
        let mut rm = new_run_manager_for_character(seed, CharacterId::Ironclad);
        for _ in 0..600 {
            if rm.is_over() {
                return None;
            }
            let actions = rm.get_available_actions();
            if actions.is_empty() {
                return None;
            }
            if detect_card_surface(&rm, &actions).is_some() {
                return Some(rm);
            }
            let _ = rm.take_action(actions[0]);
        }
        None
    }

    /// Margin below threshold escalates (counter increments); margin
    /// above does not. Same decision, only the thresholds differ.
    #[test]
    fn escalation_gate_respects_margin_threshold() {
        let mut seed = 1;
        let rm = loop {
            if let Some(rm) = walk_to_card_surface(seed) {
                break rm;
            }
            seed += 1;
            assert!(seed < 60, "no card-shaped decision found in 60 seeds");
        };
        let actions = rm.get_available_actions();

        // Threshold ∞ → any finite margin escalates.
        let mut w_low = Weights::default();
        w_low.tiers.card_reward_margin = f64::INFINITY;
        w_low.tiers.boss_relic_margin = f64::INFINITY;
        w_low.tiers.deck_mutation_margin = f64::INFINITY;
        w_low.tiers.smith_margin = f64::INFINITY;
        let mut escalating = TieredPolicy::with_weights(w_low).with_budgets(tiny(), tiny());
        let idx = escalating.choose(&rm, &actions).expect("pick");
        assert!(idx < actions.len());
        assert_eq!(
            escalating.counts().tier2 + escalating.counts().tier3,
            1,
            "below-threshold margin must escalate exactly once"
        );

        // Threshold -∞ → margin can never be below it; no escalation.
        let mut w_high = Weights::default();
        w_high.tiers.card_reward_margin = f64::NEG_INFINITY;
        w_high.tiers.boss_relic_margin = f64::NEG_INFINITY;
        w_high.tiers.deck_mutation_margin = f64::NEG_INFINITY;
        w_high.tiers.smith_margin = f64::NEG_INFINITY;
        let mut frozen = TieredPolicy::with_weights(w_high).with_budgets(tiny(), tiny());
        let idx2 = frozen.choose(&rm, &actions).expect("pick");
        assert!(idx2 < actions.len());
        assert_eq!(
            frozen.counts().tier2 + frozen.counts().tier3,
            0,
            "above-threshold margin must not escalate"
        );
    }

    /// Non-card surfaces (the opening map move) never escalate.
    #[test]
    fn map_moves_never_escalate() {
        let rm = new_run_manager_for_character(41, CharacterId::Ironclad);
        assert_eq!(rm.phase(), Phase::MapChoice);
        let actions = rm.get_available_actions();
        let mut w = Weights::default();
        w.tiers.card_reward_margin = f64::INFINITY;
        let mut policy = TieredPolicy::with_weights(w).with_budgets(tiny(), tiny());
        let idx = policy.choose(&rm, &actions).expect("pick");
        assert!(matches!(actions[idx], RunAction::Move { .. }));
        assert_eq!(policy.counts().tier2 + policy.counts().tier3, 0);
    }

    /// M2.5: with `path_margin` raised, the opening map move escalates
    /// via `path_delta`, counts under the `path` surface, and still
    /// returns a legal `Move`.
    #[test]
    fn path_escalates_when_margin_raised() {
        let rm = new_run_manager_for_character(41, CharacterId::Ironclad);
        assert_eq!(rm.phase(), Phase::MapChoice);
        let actions = rm.get_available_actions();
        let mut w = Weights::default();
        w.tiers.path_margin = f64::INFINITY; // margin always below ∞ → escalate
        let mut policy = TieredPolicy::with_weights(w).with_budgets(tiny(), tiny());
        let idx = policy.choose(&rm, &actions).expect("pick");
        assert!(
            matches!(actions[idx], RunAction::Move { .. }),
            "escalated pick must still be a legal move"
        );
        assert_eq!(policy.counts().tier2, 1, "one tier-2 (path) escalation");
        assert_eq!(policy.counts().by_surface.get("path").copied(), Some(1));
    }
}
