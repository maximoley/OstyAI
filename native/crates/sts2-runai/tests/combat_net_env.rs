//! `STS2_RUNAI_COMBAT_NET` wiring contract (hard-example combat net
//! campaign, Task 1 — spec
//! `docs/superpowers/specs/2026-07-18-hard-example-combat-net-design.md`).
//!
//! One env state per `#[test]`: nextest gives every test its own
//! process, so `std::env::set_var` + the `OnceLock` cache inside
//! `combat_net_from_env` cannot leak across tests.

use sts2_core::WIDE_FEATURE_DIM;
use sts2_runai::policy::{combat_net_from_env, resolve_leaf_eval, LeafKind};

/// Minimal trainer-shaped weights JSON (`arch` + `feature_mean/std` +
/// `weights` table) accepted by `LearnedValueNet::from_weights_path`:
/// hidden=2, trunk_depth=2, canonical 640-slot policy head, all zeros.
fn tiny_weights_json() -> String {
    let hidden = 2usize;
    let policy_dim = 640usize;
    let row = |cols: usize| vec![0.0f32; cols];
    let mat = |rows: usize, cols: usize| vec![row(cols); rows];
    serde_json::json!({
        "arch": {
            "input_dim": WIDE_FEATURE_DIM,
            "hidden": hidden,
            "policy_dim": policy_dim,
            "trunk_depth": 2,
        },
        "feature_mean": row(WIDE_FEATURE_DIM),
        "feature_std": vec![1.0f32; WIDE_FEATURE_DIM],
        "weights": {
            "W1": mat(WIDE_FEATURE_DIM, hidden),
            "b1": row(hidden),
            "W2": mat(hidden, hidden),
            "b2": row(hidden),
            "W_v": mat(hidden, 1),
            "b_v": row(1),
            "W_p": mat(hidden, policy_dim),
            "b_p": row(policy_dim),
        },
    })
    .to_string()
}

#[test]
fn env_unset_resolves_to_budget_leaf() {
    std::env::remove_var("STS2_RUNAI_COMBAT_NET");
    assert!(combat_net_from_env().is_none());
    assert!(!resolve_leaf_eval(LeafKind::TexelWide).is_learned());
    assert!(!resolve_leaf_eval(LeafKind::Texel).is_learned());
}

#[test]
fn env_changes_are_tracked_for_two_phase_drivers() {
    // fork-fatal's G-panel contract: prefix replays run with the var
    // REMOVED (budget leaf), forks with it RESTORED (learned leaf) —
    // the cache must follow the env, not freeze the first answer.
    let dir = std::env::temp_dir().join(format!("combat-net-2ph-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tiny.weights.json");
    std::fs::write(&path, tiny_weights_json()).unwrap();

    std::env::remove_var("STS2_RUNAI_COMBAT_NET");
    assert!(combat_net_from_env().is_none(), "phase A: no net");
    std::env::set_var("STS2_RUNAI_COMBAT_NET", &path);
    assert!(combat_net_from_env().is_some(), "phase B: net restored");
    assert!(resolve_leaf_eval(LeafKind::TexelWide).is_learned());
    std::env::remove_var("STS2_RUNAI_COMBAT_NET");
    assert!(combat_net_from_env().is_none(), "removal tracked again");
}

#[test]
fn env_set_resolves_to_learned_leaf() {
    let dir = std::env::temp_dir().join(format!("combat-net-env-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tiny.weights.json");
    std::fs::write(&path, tiny_weights_json()).unwrap();

    std::env::set_var("STS2_RUNAI_COMBAT_NET", &path);
    let net = combat_net_from_env().expect("weights fixture must load");
    // Cached: second call hands back the same Arc.
    let again = combat_net_from_env().expect("cached");
    assert!(std::sync::Arc::ptr_eq(&net, &again));
    // The env override wins regardless of the budget's LeafKind.
    assert!(resolve_leaf_eval(LeafKind::TexelWide).is_learned());
    assert!(resolve_leaf_eval(LeafKind::Texel).is_learned());
}
