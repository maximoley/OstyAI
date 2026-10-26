//! Rust<->torch round-trip parity for the combat surrogate (AlphaZero
//! Milestone 1, Task 4). Loads a fixture emitted by
//! `train_combat_surrogate_net.py --emit-parity-fixture` (raw densified
//! context, PRE-standardization, so `predict_from_dense` exercises the
//! same standardize+forward as `predict`) and asserts the Rust forward
//! matches the torch forward within `1e-4`.

use sts2_runai::eval::{CombatOutcome, SurrogateNet};

#[test]
fn rust_forward_matches_torch_fixture() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/surrogate_parity.json"
    );
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("parity fixture absent — run trainer --emit-parity-fixture; skipping");
        return;
    };
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let net = SurrogateNet::from_json_slice(v["net"].to_string().as_bytes()).unwrap();

    let mut max_win_diff = 0.0f32;
    let mut max_hp_diff = 0.0f32;
    let cases = v["cases"].as_array().unwrap();
    assert!(!cases.is_empty(), "fixture has zero cases");
    for case in cases {
        let idx = case["encounter_idx"].as_u64().unwrap() as usize;
        // The fixture stores the RAW densified context (pre-standardization);
        // `predict_from_dense` standardizes internally with the net's own
        // feat_mu/feat_sd, exactly like `predict`.
        let dense: Vec<f32> = case["ctx_dense"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap() as f32)
            .collect();
        let CombatOutcome {
            win_prob,
            hp_frac_lost,
        } = net.predict_from_dense(&dense, idx);
        let ew = case["win_prob"].as_f64().unwrap() as f32;
        let eh = case["hp_frac_lost"].as_f64().unwrap() as f32;
        max_win_diff = max_win_diff.max((win_prob - ew).abs());
        max_hp_diff = max_hp_diff.max((hp_frac_lost - eh).abs());
        assert!((win_prob - ew).abs() < 1e-4, "win {win_prob} vs {ew}");
        assert!(
            (hp_frac_lost - eh).abs() < 1e-4,
            "hp {hp_frac_lost} vs {eh}"
        );
    }
    eprintln!(
        "surrogate_parity: {} cases, max |Δwin|={max_win_diff:.3e}, max |Δhp|={max_hp_diff:.3e}",
        cases.len()
    );
}
