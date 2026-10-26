//! Hard-example training-row factory (campaign #3, Task 2 — spec
//! `docs/superpowers/specs/2026-07-18-hard-example-combat-net-design.md`).
//!
//! For every combat-entry snapshot of a corpus seed
//! ([`fork_diagnosis::replay_all_fights`]), plays K reps of the fight
//! under the deployed honest eval-cheap combat MCTS with a per-rep
//! perturbed search seed, and emits one [`LabelRow`] per combat decision:
//! the wide feature vector at the decision state, the rep's fight outcome
//! (value label), end-HP fraction (aux label), and the root visit
//! distribution over canonical action slots (policy label).
//!
//! The fight driver is the production [`MctsCombatPolicy`] itself — the
//! same budget, env knobs, and seed derivation as deployed play — wrapped
//! around [`HeuristicPolicy`] for the (rare, counted) meta-fallback
//! decisions, which matches deployed behavior: the deployed `PpoPolicy`
//! also delegates combat fallbacks to an embedded `HeuristicPolicy`.

use serde::{Deserialize, Serialize};
use sts2_core::eval_features_wide;
use sts2_core::full_run::RunPolicy;
use sts2_core::run_manager::ActionResult;
use sts2_mcts::corpus::PolicyActionSpace;

use crate::fork_diagnosis::{fork_seed, FightSnapshot};
use crate::policy::{CombatBudget, HeuristicPolicy, MctsCombatPolicy};

/// One training row (a JSONL line in a label shard).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LabelRow {
    pub seed: u64,
    pub fight_idx: usize,
    /// True iff this is the LAST fight of a LOSING corpus run — the
    /// hard-example stratum the curriculum oversamples.
    pub is_fatal: bool,
    pub encounter: String,
    pub act: u8,
    pub rep: u32,
    /// 0-based decision index within this rep's playthrough.
    pub decision: usize,
    /// `sts2_core::eval_features_wide` at the decision state
    /// (`WIDE_FEATURE_DIM` floats).
    pub features: Vec<f32>,
    /// Whether THIS rep survived the fight — the value label.
    pub outcome: bool,
    /// `max(0, end_hp) / max_hp` after the fight — the aux-HP label.
    pub end_hp_frac: f32,
    /// Sparse normalized root visit distribution over
    /// [`PolicyActionSpace::Canonical`] slots. Empty when the decision
    /// bypassed search (single legal action / meta fallback).
    pub visits: Vec<(usize, f32)>,
}

/// Search-seed rep index: keeps rep streams distinct across fights of
/// the same run without colliding with the diagnosis campaign's plain
/// `0..reps` usage on the fatal fight. Reps per fight are far below 64.
#[must_use]
pub fn rep_index(fight_idx: usize, rep: u32) -> u32 {
    (u32::try_from(fight_idx).expect("fight_idx fits u32")) * 64 + rep
}

/// Play `snap`'s fight once (rep `rep`) under the deployed combat search
/// and emit one [`LabelRow`] per combat decision. `is_fatal` is threaded
/// from the caller (needs corpus outcome + last-fight knowledge).
pub fn label_fight(
    snap: &FightSnapshot,
    rep: u32,
    base_seed: u64,
    is_fatal: bool,
) -> Vec<LabelRow> {
    /// A single combat never legitimately needs this many decisions.
    const CAP: usize = 3000;
    let mut rm = snap.rm.clone();
    let search_seed = fork_seed(base_seed, rep_index(snap.fight_idx, rep));
    let mut policy = MctsCombatPolicy::new(
        HeuristicPolicy::new(),
        CombatBudget::eval_cheap(),
        search_seed,
    );

    let mut rows: Vec<LabelRow> = Vec::new();
    let mut steps = 0usize;
    loop {
        let combat_live = rm.combat.as_ref().is_some_and(|c| !c.is_over);
        if rm.is_over() || !combat_live || steps >= CAP {
            break;
        }
        policy.prepare(&mut rm);
        let actions = rm.get_available_actions();
        if actions.is_empty() {
            break;
        }
        let features = rm.combat.as_ref().map(|c| eval_features_wide(c).to_vec());
        let Some(idx) = policy.choose(&rm, &actions) else {
            break;
        };
        if let Some(features) = features {
            let visits = policy
                .last_combat_search()
                .zip(rm.combat.as_ref())
                .map(|(rec, combat)| {
                    let total: u32 = rec.outcomes.iter().map(|o| o.visits).sum();
                    if total == 0 {
                        return Vec::new();
                    }
                    // Canonical slots are card-id-only, so root actions
                    // that play the same card at different targets (or
                    // any two potions) collide on one slot — sum them,
                    // matching `CorpusBuilder::record_position_with_visits`.
                    let mut acc: std::collections::BTreeMap<usize, f32> =
                        std::collections::BTreeMap::new();
                    #[allow(clippy::cast_precision_loss)]
                    for o in rec.outcomes.iter().filter(|o| o.visits > 0) {
                        if let Some(slot) = PolicyActionSpace::Canonical.slot(combat, &o.action) {
                            *acc.entry(slot).or_insert(0.0) += o.visits as f32 / total as f32;
                        }
                    }
                    acc.into_iter().collect()
                })
                .unwrap_or_default();
            rows.push(LabelRow {
                seed: base_seed,
                fight_idx: snap.fight_idx,
                is_fatal,
                encounter: snap.encounter.clone(),
                act: snap.act,
                rep,
                decision: rows.len(),
                features,
                outcome: false,   // filled below
                end_hp_frac: 0.0, // filled below
                visits,
            });
        }
        if !matches!(
            rm.take_action(actions[idx]),
            ActionResult::Advanced | ActionResult::RunOver
        ) {
            break;
        }
        steps += 1;
    }

    let survived = rm.run_state.current_hp > 0;
    #[allow(clippy::cast_precision_loss)]
    let end_hp_frac = rm.run_state.current_hp.max(0) as f32 / rm.run_state.max_hp.max(1) as f32;
    for r in &mut rows {
        r.outcome = survived;
        r.end_hp_frac = end_hp_frac;
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_row_jsonl_round_trip() {
        let row = LabelRow {
            seed: 7,
            fight_idx: 3,
            is_fatal: true,
            encounter: "MONSTER.Aeonglass".into(),
            act: 3,
            rep: 2,
            decision: 5,
            features: vec![0.0; 4],
            outcome: true,
            end_hp_frac: 0.42,
            visits: vec![(12, 0.75), (40, 0.25)],
        };
        let line = serde_json::to_string(&row).unwrap();
        let back: LabelRow = serde_json::from_str(&line).unwrap();
        assert_eq!(back.visits, row.visits);
        assert_eq!(back.fight_idx, 3);
        assert!(back.is_fatal && back.outcome);
    }

    #[test]
    fn rep_indices_never_collide_across_fights_or_with_diagnosis_reps() {
        let mut seen = std::collections::HashSet::new();
        // Diagnosis campaign uses raw rep 0..16 on the fatal fight.
        for r in 0..16u32 {
            seen.insert(r);
        }
        for fight in 1..40usize {
            for rep in 0..16u32 {
                assert!(
                    seen.insert(rep_index(fight, rep)),
                    "collision at fight {fight} rep {rep}"
                );
            }
        }
        // fight 0 rep r == raw rep r by construction — same fight, same
        // stream family is fine; assert the intended equality instead.
        assert_eq!(rep_index(0, 3), 3);
    }

    /// Drive a fresh Ironclad run with a trivial first-legal-action
    /// driver up to its first combat entry — the same snapshot shape
    /// `fork_diagnosis::replay_all_fights` hands the label factory,
    /// without the PPO prefix-replay cost.
    fn first_fight(seed: u64) -> FightSnapshot {
        use sts2_core::run_manager::{Phase, RunManager};
        crate::rollout::ensure_content_installed();
        let mut rm: RunManager =
            sts2_content::run_callbacks::new_run_manager_for_character_with_ascension(
                seed,
                sts2_core::potions::CharacterId::Ironclad,
                0,
            )
            .with_neow_start();
        sts2_content::run_callbacks::initialize_run_setup(&mut rm);
        for step in 0..400 {
            if rm.phase() == Phase::Combat && rm.combat.as_ref().is_some_and(|c| !c.is_over) {
                break;
            }
            let actions = rm.get_available_actions();
            assert!(!actions.is_empty(), "no legal actions at step {step}");
            assert!(matches!(rm.take_action(actions[0]), ActionResult::Advanced));
        }
        let combat = rm.combat.as_ref().expect("reached combat");
        let encounter = combat
            .creatures
            .iter()
            .find(|c| c.id == combat.enemy_uids[0].0)
            .map_or_else(|| "NONE.NONE".to_string(), |c| c.monster_id.0.to_string());
        FightSnapshot {
            act: rm.run_state.act_index,
            floor: rm.run_state.total_floor,
            // Deliberately non-zero so the rep_index-derived search seed
            // is the real `fight_idx * 64 + rep` stream, not raw `rep`.
            fight_idx: 2,
            encounter,
            entry_hp: rm.run_state.current_hp,
            rm,
        }
    }

    /// End-to-end shape gate on the label factory: one row per combat
    /// decision, labels back-filled from the rep's OUTCOME (not the
    /// decision state), and a sparse root-visit distribution over
    /// canonical slots. Runs a real eval-cheap search (1000 sims/decision),
    /// so it is the crate's slowest unit test by design — it is the only
    /// thing that exercises the deployed label path.
    #[test]
    fn label_fight_emits_one_labeled_row_per_decision() {
        use sts2_core::WIDE_FEATURE_DIM;
        use sts2_mcts::action_index::CANONICAL_ACTION_DIM;

        let snap = first_fight(4242);
        let rows = label_fight(&snap, 1, 4242, true);
        assert!(!rows.is_empty(), "a live fight must yield decisions");

        for (i, r) in rows.iter().enumerate() {
            assert_eq!(r.decision, i, "decision index is the row's position");
            assert_eq!(r.seed, 4242);
            assert_eq!(r.rep, 1);
            assert_eq!(r.fight_idx, snap.fight_idx);
            assert_eq!(r.act, snap.act);
            assert_eq!(r.encounter, snap.encounter);
            assert!(r.is_fatal, "is_fatal is threaded from the caller verbatim");
            assert_eq!(r.features.len(), WIDE_FEATURE_DIM);
            assert!(r.features.iter().all(|f| f.is_finite()));

            assert!(r.visits.iter().all(|(s, _)| *s < CANONICAL_ACTION_DIM));
            assert!(r.visits.iter().all(|(_, p)| *p > 0.0 && *p <= 1.0));
            if !r.visits.is_empty() {
                // Mass can only be LOST — actions whose canonical slot
                // does not resolve are dropped — never gained.
                let mass: f32 = r.visits.iter().map(|(_, p)| *p).sum();
                assert!(mass > 0.0 && mass <= 1.0 + 1e-3, "visit mass {mass}");
            }
        }
        assert!(
            rows.iter().any(|r| !r.visits.is_empty()),
            "at least one decision must carry a root visit distribution"
        );

        // The value/aux labels are the REP's outcome, identical on every
        // row of the rep and back-filled after the fight resolves.
        let outcome = rows[0].outcome;
        let frac = rows[0].end_hp_frac;
        assert!(rows.iter().all(|r| r.outcome == outcome));
        assert!(rows.iter().all(|r| (r.end_hp_frac - frac).abs() < 1e-9));
        assert_eq!(outcome, frac > 0.0, "survival == positive end-HP fraction");
        assert!((0.0..=1.0).contains(&frac), "end_hp_frac {frac}");
    }

    /// [`LabelRow::visits`] is documented as a *distribution over
    /// canonical slots*, which implies unique slots per row.
    /// `PolicyActionSpace::Canonical` is card-id-only (640 slots), so
    /// two root actions that play the same card at different targets
    /// project onto ONE slot. Regression: [`label_fight`] once emitted
    /// one entry per root action without aggregating, and the dense
    /// consumer (`scripts/hardex_dataset.py`) assigned instead of
    /// summing, silently dropping all but the last colliding entry's
    /// policy-target mass. Colliding slots are now summed, matching
    /// `sts2_mcts::corpus::CorpusBuilder::record_position_with_visits`
    /// (`dist[slot] += count / total`).
    #[test]
    fn label_fight_visits_carry_one_entry_per_canonical_slot() {
        let rows = label_fight(&first_fight(4242), 1, 4242, true);
        for r in &rows {
            let mut slots: Vec<usize> = r.visits.iter().map(|(s, _)| *s).collect();
            let n = slots.len();
            slots.sort_unstable();
            slots.dedup();
            assert_eq!(
                slots.len(),
                n,
                "decision {}: colliding canonical slots must be summed, not repeated",
                r.decision
            );
        }
    }
}
