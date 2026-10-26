//! AlphaZero M3, Task 6: integration-level test gate suite for the
//! run-level outer-loop MCTS (`sts2_runai::planning_mcts`).
//!
//! Integration tests only see the crate's PUBLIC surface — the
//! `#[cfg(test)]`-gated `StubLeafEval`/`FnLeafEval`/`StubSurrogate`/
//! `fresh_ironclad_run` fixtures in `planning.rs`/`planning_mcts.rs` are
//! `pub(crate)` and invisible here, so this file defines its own local
//! `Stub` surrogate and leaf evals, mirroring `planning_full_run.rs` and
//! `planning_run_draw_streams.rs` exactly (same fresh-Ironclad-run
//! construction via the public `sts2_content::run_callbacks` API).
//!
//! Four gates, matching
//! `docs/plans/2026-07-02-alphazero-m3-outer-loop-mcts-design.md` §8:
//!
//! - **Gate A** — determinism: two `search()` calls, same config, same
//!   action.
//! - **Gate B** — terminal backup: an always-lose surrogate drives a
//!   combat-entering action to a deterministic loss; the search must back
//!   up `0.0` for that action's root child, NOT the (deliberately high)
//!   leaf-eval constant — proving `RunOver` short-circuits `RunLeafEval`.
//! - **Gate C** — value-steering: a root with 2 legal actions (the
//!   post-Neow relic-reward decision, `[PickRelicReward, SkipRelic]`) and a
//!   leaf that scores low/high by a run-state predicate (`relics.len()`
//!   changed). Proven non-vacuous by *flipping* which side is low/high and
//!   showing the chosen action flips too — the steering is causal, not
//!   coincidental.
//! - **Gate D** — budget: `search()` returns a legal root action for a
//!   small simulation budget and returns promptly (no hang).

use sts2_content::run_callbacks::{
    initialize_run_setup, new_run_manager_for_character_with_ascension,
};
use sts2_core::potions::CharacterId;
use sts2_core::run_manager::{Phase, RunAction, RunManager};
use sts2_runai::eval::ranker_features::ContextFeatures;
use sts2_runai::eval::surrogate::CombatOutcome;
use sts2_runai::planning::{CombatSurrogate, PlanningRun};
use sts2_runai::planning_mcts::{RunLeafEval, RunMctsConfig, RunPlanner, UniformPriors};

/// Forced-outcome combat surrogate: every fight resolves to `win`
/// regardless of context — mirrors `planning_full_run.rs`'s `Stub`.
#[derive(Debug)]
struct Stub {
    win: f32,
}

impl CombatSurrogate for Stub {
    fn predict_outcome(&self, _ctx: &ContextFeatures, _encounter_idx: usize) -> CombatOutcome {
        CombatOutcome {
            win_prob: self.win,
            hp_frac_lost: 0.1,
        }
    }
}

/// Fixed-value leaf eval — the integration-test stand-in for the crate's
/// private `StubLeafEval`.
#[derive(Debug)]
struct ConstLeaf {
    v: f32,
}

impl RunLeafEval for ConstLeaf {
    fn value(&self, _pr: &PlanningRun<'_>) -> f32 {
        self.v
    }
}

/// Predicate-scored leaf eval — the integration-test stand-in for the
/// crate's private `FnLeafEval`, specialized to "score by a boolean
/// run-state predicate" for Gate C. Not `Debug` (the wrapped closure
/// isn't), same rationale as the crate-internal `FnLeafEval`.
#[allow(missing_debug_implementations)]
struct PredicateLeaf<F: Fn(&PlanningRun<'_>) -> bool> {
    deadly: F,
    low: f32,
    high: f32,
}

impl<F: Fn(&PlanningRun<'_>) -> bool> RunLeafEval for PredicateLeaf<F> {
    fn value(&self, pr: &PlanningRun<'_>) -> f32 {
        if (self.deadly)(pr) {
            self.low
        } else {
            self.high
        }
    }
}

/// Fresh Ironclad run through the real content callbacks + Neow start,
/// mirroring `planning_full_run.rs::build_run` verbatim.
fn build_run(seed: u64) -> RunManager {
    sts2_runai::rollout::ensure_content_installed();
    let mut rm = new_run_manager_for_character_with_ascension(seed, CharacterId::Ironclad, 0)
        .with_neow_start();
    initialize_run_setup(&mut rm);
    rm
}

/// Drive `pr` past the opening Neow event (all 3 `EventChoice`s are
/// deterministic — a fixed narrative branch, no run-draw RNG) to the
/// relic-reward decision it grants: `[PickRelicReward, SkipRelic]`. Mirrors
/// the crate's own `stochastic_transition_makes_a_chance_node` unit-test
/// fixture (`planning_mcts.rs`), rebuilt here against the public API.
fn drive_to_relic_reward_decision(pr: &mut PlanningRun<'_>) {
    let opening = pr.legal_actions();
    assert!(!opening.is_empty(), "fresh run must offer the Neow choice");
    pr.apply(opening[0], 0);
    let legal = pr.legal_actions();
    assert_eq!(
        legal,
        vec![RunAction::PickRelicReward, RunAction::SkipRelic],
        "fixture drifted: expected the post-Neow relic-reward decision"
    );
}

/// Walk two lock-step `PlanningRun`s — one wired to an always-win `Stub`,
/// one to an always-lose `Stub` — forward one run-level action at a time
/// (always taking `legal_actions()[0]`, driving through any intervening
/// combat/event) until a legal action is found whose LOSE-walker outcome is
/// a deterministic loss (`terminal == Some(false)`). That action must be a
/// combat entry (the only way `apply` can produce a loss), found via public
/// `PlanningRun::apply`/`clone` alone — no reach into the private `rm`
/// field the crate's own `#[cfg(test)]` `enters_combat` helper uses.
///
/// Returns `(root_with_lose_stub, combat_action)`: `root` is positioned
/// immediately BEFORE `combat_action`, carrying the always-lose surrogate,
/// so `RunPlanner::search`'s internal expansions (which reuse `root`'s
/// embedded surrogate) will deterministically kill that branch.
fn walk_to_lethal_combat_choice(seed: u64) -> (PlanningRun<'static>, RunAction) {
    let win_stub: &'static Stub = Box::leak(Box::new(Stub { win: 1.0 }));
    let lose_stub: &'static Stub = Box::leak(Box::new(Stub { win: 0.0 }));
    let rm_win = build_run(seed);
    let rm_lose = rm_win.clone();
    let mut pr_win = PlanningRun::new(rm_win, win_stub, 0x5EED_5EED);
    let mut pr_lose = PlanningRun::new(rm_lose, lose_stub, 0x5EED_5EED);

    for _ in 0..500 {
        let actions = pr_win.legal_actions();
        assert!(!actions.is_empty(), "wedged before finding a lethal combat");

        for &action in &actions {
            let mut probe = pr_lose.clone();
            let out = probe.apply(action, 0);
            if out.terminal == Some(false) {
                return (pr_lose, action);
            }
        }

        // None of this position's actions is immediately lethal under the
        // always-lose stub — advance both walkers in lock-step by the same
        // (non-lethal, per the probe loop above) action and keep looking.
        let action = actions[0];
        let out_win = pr_win.apply(action, 0);
        let out_lose = pr_lose.apply(action, 0);
        assert_ne!(
            out_win.terminal,
            Some(false),
            "the always-win walker must never lose"
        );
        assert_eq!(
            out_lose.terminal, None,
            "already probed as non-lethal above"
        );
        assert_ne!(pr_win.phase(), Phase::RunOver, "run ended before combat");
    }
    panic!("did not find a lethal combat choice within 500 lock-step steps");
}

// ── Gate A — determinism ────────────────────────────────────────────────

#[test]
fn gate_a_search_is_deterministic_at_the_integration_boundary() {
    let stub = Stub { win: 1.0 };
    let rm = build_run(1);
    let pr = PlanningRun::new(rm, &stub, 0x1234);
    let leaf = ConstLeaf { v: 0.5 };
    let cfg = RunMctsConfig {
        max_simulations: 300,
        max_depth: 6,
        seed: 42,
        ..Default::default()
    };

    let a1 = RunPlanner::search(&pr, cfg, &leaf, &UniformPriors);
    let a2 = RunPlanner::search(&pr, cfg, &leaf, &UniformPriors);
    assert_eq!(
        a1, a2,
        "two searches from the same root with equal config must agree"
    );
    assert!(a1.is_some(), "a fresh run must have a legal root action");
}

// ── Gate B — terminal backup ─────────────────────────────────────────────

#[test]
fn gate_b_terminal_loss_backs_up_zero_not_the_leaf_constant() {
    let (root, lethal_action) = walk_to_lethal_combat_choice(2);

    // Deliberately HIGH constant leaf: if terminal handling were broken
    // (e.g. `evaluate` fell through to `leaf.value` instead of returning
    // `0.0` for `Some(false)`), the lethal action's root Q would land near
    // this constant instead of near 0.0 — the gap between them is what
    // makes this assertion non-vacuous.
    let leaf = ConstLeaf { v: 0.99 };
    let cfg = RunMctsConfig {
        max_simulations: 200,
        max_depth: 2,
        seed: 7,
        ..Default::default()
    };

    let (chosen, stats) = RunPlanner::search_with_stats(&root, cfg, &leaf, &UniformPriors);
    assert!(chosen.is_some(), "root must have a legal action");

    let q_lethal = stats
        .root_q
        .iter()
        .find(|&&(a, _)| a == lethal_action)
        .map(|&(_, q)| q)
        .expect("the lethal action must have been expanded as a root child");
    assert!(
        q_lethal < 0.05,
        "lethal action's root Q must back up the terminal LOSS value (~0.0), \
         got {q_lethal} (leaf constant was 0.99 — this would leak through if \
         terminal handling were broken)"
    );

    // Bonus (best-effort, not load-bearing): if a sibling root action
    // survived (didn't immediately die), its Q should reflect the high
    // leaf constant, and the search should prefer it over the lethal one.
    if let Some(&(_, q_other)) = stats.root_q.iter().find(|&&(a, _)| a != lethal_action) {
        if q_other > 0.5 {
            assert_ne!(
                chosen,
                Some(lethal_action),
                "search must not choose a branch it knows dies over a surviving one"
            );
        }
    }
}

// ── Gate C — value-steering sanity (the key gate) ────────────────────────

#[test]
fn gate_c_value_signal_steers_puct_toward_the_high_value_branch() {
    // Root: the post-Neow relic-reward decision, `[PickRelicReward,
    // SkipRelic]` — a real 2-legal-action decision, not a synthetic
    // fixture. `PickRelicReward` draws from run-draw RNG (a Chance node);
    // `SkipRelic` doesn't (a plain Decision node) — see
    // `planning_mcts.rs::stochastic_transition_makes_a_chance_node`.
    let stub = Stub { win: 1.0 };
    let rm = build_run(3);
    let mut pr = PlanningRun::new(rm, &stub, 0x77AA_11BB);
    drive_to_relic_reward_decision(&mut pr);
    let baseline_relics = pr.run_state().relics.len();

    // The run-state predicate: "this branch actually picked up the relic"
    // — true for every descendant of `PickRelicReward` (relic count only
    // grows), false for every descendant of `SkipRelic`. Robust to WHICH
    // specific relic got drawn (DPW may widen several distinct outcomes),
    // since it keys off count, not identity.
    let took_relic = |p: &PlanningRun<'_>| p.run_state().relics.len() != baseline_relics;

    let cfg = RunMctsConfig {
        max_simulations: 600,
        max_depth: 1,
        c_puct: std::f32::consts::SQRT_2,
        seed: 11,
        ..Default::default()
    };

    // Direction 1: taking the relic is "deadly" (low value); skipping is
    // "safe" (high value). Search must prefer SkipRelic.
    let leaf_relic_is_deadly = PredicateLeaf {
        deadly: took_relic,
        low: 0.05,
        high: 0.95,
    };
    let (chosen_a, stats_a) =
        RunPlanner::search_with_stats(&pr, cfg, &leaf_relic_is_deadly, &UniformPriors);
    assert_eq!(
        chosen_a,
        Some(RunAction::SkipRelic),
        "with PickRelicReward scored LOW, search must prefer SkipRelic; root_q={:?}",
        stats_a.root_q
    );
    let q_pick_a = root_q_for(&stats_a, RunAction::PickRelicReward);
    let q_skip_a = root_q_for(&stats_a, RunAction::SkipRelic);
    assert!(
        q_skip_a > q_pick_a,
        "SkipRelic's root Q ({q_skip_a}) must exceed PickRelicReward's ({q_pick_a}) \
         when the relic branch is scored low"
    );

    // Direction 2 (the causality proof): FLIP which branch is low/high.
    // If the steering in direction 1 were coincidental (e.g. tie-break
    // order, or a bug that always prefers the first-expanded action)
    // rather than caused by the leaf values, this would reproduce the
    // SAME choice. It must not.
    let leaf_skip_is_deadly = PredicateLeaf {
        deadly: took_relic,
        low: 0.95,
        high: 0.05,
    };
    let (chosen_b, stats_b) =
        RunPlanner::search_with_stats(&pr, cfg, &leaf_skip_is_deadly, &UniformPriors);
    assert_eq!(
        chosen_b,
        Some(RunAction::PickRelicReward),
        "with SkipRelic scored LOW (values flipped), search must prefer \
         PickRelicReward; root_q={:?}",
        stats_b.root_q
    );
    let q_pick_b = root_q_for(&stats_b, RunAction::PickRelicReward);
    let q_skip_b = root_q_for(&stats_b, RunAction::SkipRelic);
    assert!(
        q_pick_b > q_skip_b,
        "PickRelicReward's root Q ({q_pick_b}) must exceed SkipRelic's ({q_skip_b}) \
         when the relic branch is scored high"
    );

    assert_ne!(
        chosen_a, chosen_b,
        "flipping the leaf's low/high assignment must flip the search's \
         preferred root action — otherwise the steering isn't causal"
    );
}

fn root_q_for(stats: &sts2_runai::planning_mcts::SearchStats, action: RunAction) -> f32 {
    stats
        .root_q
        .iter()
        .find(|&&(a, _)| a == action)
        .map_or_else(
            || panic!("action {action:?} was not an expanded root child"),
            |&(_, q)| q,
        )
}

// ── Gate D — budget ──────────────────────────────────────────────────────

#[test]
fn gate_d_small_budget_returns_a_legal_action_promptly() {
    let stub = Stub { win: 1.0 };
    let rm = build_run(4);
    let pr = PlanningRun::new(rm, &stub, 0x9);
    let legal = pr.legal_actions();
    let leaf = ConstLeaf { v: 0.5 };
    let cfg = RunMctsConfig {
        max_simulations: 4,
        max_depth: 2,
        seed: 1,
        ..Default::default()
    };

    let start = std::time::Instant::now();
    let chosen = RunPlanner::search(&pr, cfg, &leaf, &UniformPriors);
    let elapsed = start.elapsed();

    let action = chosen.expect("a tiny budget must still return a legal action");
    assert!(
        legal.contains(&action),
        "returned action must be legal at the root"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "a 4-simulation search must not hang, took {elapsed:?}"
    );
}
