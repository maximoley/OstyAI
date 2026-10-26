//! Bead `sts2-rs-sh907`: PUCT first-visit prior-ordering.
//!
//! Pre-fix, the unvisited branch of the UCT loop returned
//! `f32::INFINITY` for every child with visits=0, so the `>` tiebreak
//! at line 997 picked whichever child appeared first in iteration
//! order. With `use_puct_prior=true` the LearnedPolicyPrior's
//! per-action probability never entered the comparison on the first
//! visit of any child.
//!
//! The fix replaces the unvisited PUCT branch with the standard
//! `c·p·sqrt(N_parent)` explore term (denom `1 + 0 = 1`), so the prior
//! orders first-visit selection. The UCT branch (no PUCT prior) keeps
//! the historical INFINITY tiebreak — there's no prior to inject so
//! falling through to iteration order is the only meaningful default.

mod common;

use common::build_vertical_slice;
use smallvec::SmallVec;
use sts2_core::{Action, CombatState};
use sts2_mcts::{Mcts, MctsConfig, Policy};

/// Test policy: returns a fixed score for every action, with a
/// distinctive bias toward whichever action is "tagged" by an external
/// closure. Used so the test can assert prior-ordered visits without
/// depending on action-specific game knowledge.
#[derive(Clone)]
struct SkewedPolicy {
    /// Action whose prior we'll set to 0.9 instead of the uniform
    /// share. All other actions get `(1 - 0.9) / (n - 1)`.
    favored_idx_fn: fn(&[Action]) -> usize,
}

impl Policy for SkewedPolicy {
    fn prior(&self, _state: &CombatState, actions: &[Action]) -> SmallVec<[f32; 16]> {
        let n = actions.len().max(1);
        let favored = (self.favored_idx_fn)(actions).min(n.saturating_sub(1));
        #[allow(clippy::cast_precision_loss)]
        let rest_share = if n > 1 { 0.1 / (n - 1) as f32 } else { 0.0 };
        actions
            .iter()
            .enumerate()
            .map(|(i, _)| if i == favored { 0.9 } else { rest_share })
            .collect()
    }
}

/// GAP #1 regression: the learned PUCT prior must steer selection under
/// chance-node search. Before the fix the chance-mode decision-node loop
/// ignored `Policy::prior` entirely (it only applied the always-on
/// game-knowledge edge priors), so a learned prior was inert and
/// `mcts_search_tree(use_prior=true)` / the recommender's `use_prior` did
/// nothing. The invariant: adding the prior measurably increases the favored
/// action's visit share vs the identical prior-free chance search.
#[test]
fn puct_prior_steers_root_visits_under_chance_nodes() {
    let cfg_with_prior = MctsConfig {
        max_simulations: 400,
        max_rollout_depth: 20,
        seed: 7,
        use_puct_prior: true,
        ..MctsConfig::default()
    };
    let cfg_no_prior = MctsConfig {
        use_puct_prior: false,
        ..cfg_with_prior
    };
    let policy = SkewedPolicy {
        favored_idx_fn: |actions| actions.len().saturating_sub(1),
    };

    let mut mcts_with = Mcts::with_policy(build_vertical_slice(7), cfg_with_prior, policy.clone());
    let _ = mcts_with.search();
    let visits_with = mcts_with.root_visits();

    let mut mcts_no = Mcts::with_policy(build_vertical_slice(7), cfg_no_prior, policy);
    let _ = mcts_no.search();
    let visits_no = mcts_no.root_visits();

    assert!(
        visits_with.len() >= 2 && visits_no.len() >= 2,
        "fixture must offer >=2 legal root actions; with={} no={}",
        visits_with.len(),
        visits_no.len(),
    );
    let favored_with = visits_with.last().expect("non-empty").1;
    let favored_no = visits_no.last().expect("non-empty").1;
    assert!(
        favored_with > favored_no,
        "learned PUCT prior must increase the favored action's visit share \
         under chance-node search; got with-prior={favored_with} \
         no-prior={favored_no} (with={visits_with:?}, no={visits_no:?})"
    );
}

/// Smoke: `use_puct_prior=false` with `UniformPolicy` is byte-stable
/// against pre-bead behavior — the UCT branch still uses INFINITY for
/// unvisited children. We assert search completes and produces the
/// invariant that visits sum to `max_simulations`.
#[test]
fn uct_branch_unchanged_visits_sum_to_sims() {
    let state = build_vertical_slice(7);
    let cfg = MctsConfig {
        max_simulations: 200,
        max_rollout_depth: 20,
        seed: 7,
        use_puct_prior: false,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state, cfg);
    let _ = mcts.search();
    let outcomes = mcts.root_outcomes();
    let total: u32 = outcomes.iter().map(|o| o.visits).sum();
    assert_eq!(total, cfg.max_simulations, "root visits must sum to sims");
}
