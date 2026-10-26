//! Public API smoke test for sts2-mcts.

mod common;

use common::build_vertical_slice;
use sts2_mcts::{par_search, Mcts, MctsConfig, UniformPolicy};

#[test]
fn public_api_compiles_and_does_not_panic() {
    let state = build_vertical_slice(0);
    let cfg = MctsConfig {
        max_simulations: 50,
        seed: 0,
        ..MctsConfig::default()
    };
    let mut mcts: Mcts<UniformPolicy> = Mcts::new(state.clone(), cfg);
    let _best = mcts.search();
    let _pv = mcts.get_pv();
    let _stats: Vec<_> = mcts.root_visits().to_vec();

    // search_n adds more sims.
    let _best2 = mcts.search_n(25);

    // with_policy compiles.
    let _mcts2 = Mcts::with_policy(state, cfg, UniformPolicy);
}

/// `root_outcomes()` must agree with `root_visits()` on visit counts and
/// mean value, and the win/loss/tie bucket counters must sum to visits
/// on every child.
#[test]
fn root_outcomes_invariant_buckets_sum_to_visits() {
    let state = build_vertical_slice(0);
    let cfg = MctsConfig {
        max_simulations: 100,
        seed: 0,
        ..MctsConfig::default()
    };
    let mut mcts: Mcts<UniformPolicy> = Mcts::new(state, cfg);
    let _best = mcts.search();
    let visits = mcts.root_visits().to_vec();
    let outcomes = mcts.root_outcomes();
    assert_eq!(visits.len(), outcomes.len());
    for (vt, o) in visits.iter().zip(outcomes.iter()) {
        assert_eq!(vt.1, o.visits, "visit counts disagree for {:?}", o.action);
        assert!(
            (vt.2 - o.mean_value).abs() < 1e-6,
            "mean disagrees for {:?}: {} vs {}",
            o.action,
            vt.2,
            o.mean_value,
        );
        assert_eq!(
            o.wins + o.losses + o.ties,
            o.visits,
            "wins+losses+ties != visits for {:?}",
            o.action,
        );
    }
}

/// `par_search` with `threads == 1` must match the single-threaded path
/// exactly, and total merged visits must equal the configured sim budget.
/// The bucket invariant carries over per merged child.
#[test]
fn par_search_single_worker_matches_single_threaded() {
    let state = build_vertical_slice(0);
    let cfg = MctsConfig {
        max_simulations: 200,
        seed: 17,
        ..MctsConfig::default()
    };
    let mut single = Mcts::<UniformPolicy>::new(state.clone(), cfg);
    let _ = single.search();
    let single_outs = single.root_outcomes();

    let par = par_search(state, cfg, 1);
    assert_eq!(par.outcomes.len(), single_outs.len());
    for (a, b) in single_outs.iter().zip(par.outcomes.iter()) {
        assert_eq!(a.action, b.action);
        assert_eq!(a.visits, b.visits);
        assert_eq!(a.wins, b.wins);
        assert_eq!(a.losses, b.losses);
        assert_eq!(a.ties, b.ties);
    }
    let total: u32 = par.outcomes.iter().map(|o| o.visits).sum();
    assert_eq!(total, cfg.max_simulations);
}

/// Multi-worker `par_search` preserves the bucket invariant per child
/// and the total visit budget across workers, and is deterministic
/// across repeat calls with the same `(seed, threads, sims)`.
#[test]
fn par_search_multi_worker_deterministic_and_budgeted() {
    let state = build_vertical_slice(0);
    let cfg = MctsConfig {
        max_simulations: 400,
        seed: 42,
        ..MctsConfig::default()
    };
    let a = par_search(state.clone(), cfg, 4);
    let b = par_search(state, cfg, 4);
    assert_eq!(a.outcomes.len(), b.outcomes.len());
    for (x, y) in a.outcomes.iter().zip(b.outcomes.iter()) {
        assert_eq!(x.action, y.action);
        assert_eq!(x.visits, y.visits, "non-determinism for {:?}", x.action);
        assert_eq!(x.wins, y.wins);
        assert_eq!(x.losses, y.losses);
        assert_eq!(x.ties, y.ties);
        assert_eq!(x.wins + x.losses + x.ties, x.visits);
    }
    let total: u32 = a.outcomes.iter().map(|o| o.visits).sum();
    assert_eq!(total, cfg.max_simulations);
}

/// GH #3: `search`, `par_search(threads=1)`, and `get_pv` must all
/// agree on the action chosen at the root. Bead `sts2-rs-trnu`
/// switched the root selection rule from `argmax(visits)` to
/// `argmax(Q) tiebreak visits`; this regression test pins that the
/// par-search root rule and the PV's first step are kept in sync
/// with `search`.
#[test]
fn search_par_search_get_pv_agree_on_root_action() {
    let state = build_vertical_slice(0);
    let cfg = MctsConfig {
        max_simulations: 300,
        seed: 123,
        ..MctsConfig::default()
    };
    let mut single = Mcts::<UniformPolicy>::new(state.clone(), cfg);
    let single_best = single.search();
    let pv = single.get_pv();
    assert!(!pv.is_empty(), "expected non-empty PV after search");
    assert_eq!(
        pv[0], single_best,
        "get_pv()[0] must equal the action returned by search()",
    );

    let par = par_search(state, cfg, 1);
    assert_eq!(
        par.best_action, single_best,
        "par_search(threads=1).best_action must equal search()",
    );
}
