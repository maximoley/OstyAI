//! AlphaZero M4, Task 2: `NetLeafEval` integration test — proves
//! `RunValueNet` (Task 1) is correctly wired into the run-level MCTS leaf
//! seam (`planning_mcts::RunLeafEval`) purely through the crate's public
//! surface.
//!
//! Mirrors `planning_full_run.rs`'s fresh-Ironclad-run construction: the
//! `#[cfg(test)]` `fresh_ironclad_run`/`StubSurrogate` helpers in
//! `planning.rs` are `pub(crate)`, so this integration test (a separate
//! compilation unit) rebuilds the same sequence against the public
//! `sts2_content::run_callbacks` API instead.

use sts2_content::run_callbacks::{
    initialize_run_setup, new_run_manager_for_character_with_ascension,
};
use sts2_core::potions::CharacterId;
use sts2_core::run_manager::RunManager;
use sts2_runai::eval::ranker_features::{
    context_dim, feature_schema_id, n_cards, n_relics, ContextFeatures,
};
use sts2_runai::eval::run_value_net::RunValueNet;
use sts2_runai::eval::surrogate::CombatOutcome;
use sts2_runai::planning::{CombatSurrogate, PlanningRun};
use sts2_runai::planning_mcts::{
    NetLeafEval, RunLeafEval, RunMctsConfig, RunPlanner, UniformPriors,
};

/// Forced-outcome combat surrogate. `PlanningRun::new` requires one, but
/// neither test here drives the run far enough to hit a real combat
/// entry, so the exact outcome is inert — mirrors `planning_full_run.rs`'s
/// `Stub`.
#[derive(Debug)]
struct Stub;

impl CombatSurrogate for Stub {
    fn predict_outcome(&self, _ctx: &ContextFeatures, _encounter_idx: usize) -> CombatOutcome {
        CombatOutcome {
            win_prob: 0.5,
            hp_frac_lost: 0.1,
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

/// A tiny, hand-crafted `RunValueNet` weights JSON whose ONLY nonzero
/// path runs from the `hp_frac` scalar (`SCALAR_NAMES[2]`, dense index
/// `2*n_cards() + n_relics() + 2`) through a single hidden unit to the
/// value head, with a positive sign throughout:
/// `win_prob = sigmoid(hp_frac)`. Every other feature (every card/relic
/// slot and every other scalar) has an all-zero `w1` row, so this
/// isolates hp_frac as the sole driver of the leaf value — proving
/// `NetLeafEval` actually threads `pr.context_features()` through the
/// net rather than returning some constant.
fn hp_frac_net_json() -> String {
    let cdim = context_dim();
    let hp_frac_row = 2 * n_cards() + n_relics() + 2;
    let mut w1 = vec![vec![0.0f32]; cdim];
    w1[hp_frac_row] = vec![1.0f32];
    serde_json::json!({
        "schema_id": feature_schema_id(),
        "engine": "test",
        "context_dim": cdim,
        "hidden": 1,
        "feat_mu": vec![0.0f32; cdim],
        "feat_sd": vec![1.0f32; cdim],
        "w1": w1,
        "b1": [0.0f32],
        "w_val": [1.0f32],
        "b_val": 0.0f32,
        "w_act": [[0.0f32, 0.0, 0.0, 0.0]],
        "b_act": [0.0f32, 0.0, 0.0, 0.0],
        "win_calib": null,
    })
    .to_string()
}

#[test]
fn net_leaf_eval_scores_healthier_root_higher() {
    let net = RunValueNet::from_json_str(&hp_frac_net_json()).expect("tiny net loads");
    let stub = Stub;

    let mut healthy_rm = build_run(1);
    healthy_rm.run_state.current_hp = healthy_rm.run_state.max_hp;
    let healthy = PlanningRun::new(healthy_rm, &stub, 0xA11CE);

    let mut damaged_rm = build_run(1);
    damaged_rm.run_state.current_hp = 1;
    let damaged = PlanningRun::new(damaged_rm, &stub, 0xA11CE);

    let leaf = NetLeafEval { net: &net };
    let v_healthy = leaf.value(&healthy);
    let v_damaged = leaf.value(&damaged);

    assert!((0.0..=1.0).contains(&v_healthy), "got {v_healthy}");
    assert!((0.0..=1.0).contains(&v_damaged), "got {v_damaged}");
    assert!(
        v_healthy > v_damaged,
        "positive hp_frac weight must score the healthier root higher: \
         healthy={v_healthy} damaged={v_damaged}"
    );
}

#[test]
fn net_leaf_eval_drives_deterministic_legal_search() {
    let net = RunValueNet::from_json_str(&hp_frac_net_json()).expect("tiny net loads");
    let stub = Stub;
    let rm = build_run(1);
    let pr = PlanningRun::new(rm, &stub, 0x1234);
    let legal = pr.legal_actions();

    let leaf = NetLeafEval { net: &net };
    let cfg = RunMctsConfig {
        max_simulations: 200,
        max_depth: 6,
        seed: 42,
        ..Default::default()
    };
    let a1 = RunPlanner::search(&pr, cfg, &leaf, &UniformPriors);
    let a2 = RunPlanner::search(&pr, cfg, &leaf, &UniformPriors);
    assert_eq!(a1, a2, "same seed/budget must yield same action");
    let a1 = a1.expect("search must find a root action");
    assert!(legal.contains(&a1), "returned action must be legal at root");
}
