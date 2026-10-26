//! Rust<->torch round-trip parity for the whole-run value net (AlphaZero
//! Milestone 4, Task 4). Loads a fixture emitted by
//! `train_run_value_net.py --emit-parity-fixture` (raw densified context,
//! PRE-standardization, so `win_prob_from_dense` exercises the same
//! standardize+forward as `win_prob`) and asserts the Rust forward matches
//! the torch forward within `1e-4`. Mirrors `surrogate_parity.rs` (AZ M1
//! Task 4) — same fixture shape (`{net, cases}`), same tolerance, same
//! RAW-ctx-dense convention.
//!
//! `act_probs` (the act-reached aux head) is `#[cfg(test)]`-gated inside
//! `run_value_net.rs` — diagnostics/parity only, not part of the deployed
//! inference surface — so it isn't reachable from this integration test.
//! The fixture still carries `act_probs` per case; Task 3's Python
//! self-check (`test_run_value_net_export.py`) is what asserts those sum
//! to 1 / are well-formed on the torch side. Only `win_prob` — the
//! deployed inference surface — is checked here.

use sts2_runai::eval::run_value_net::RunValueNet;

#[test]
fn rust_forward_matches_torch_fixture() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/run_value_net_parity.json"
    );
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("parity fixture absent — run trainer --emit-parity-fixture; skipping");
        return;
    };
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let net = RunValueNet::from_json_str(&v["net"].to_string()).unwrap();

    let mut max_win_diff = 0.0f32;
    let cases = v["cases"].as_array().unwrap();
    assert!(!cases.is_empty(), "fixture has zero cases");
    for case in cases {
        // The fixture stores the RAW densified context (pre-standardization);
        // `win_prob_from_dense` standardizes internally with the net's own
        // feat_mu/feat_sd, exactly like `win_prob`.
        let dense: Vec<f32> = case["ctx_dense"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap() as f32)
            .collect();
        let win_prob = net.win_prob_from_dense(&dense);
        let ew = case["win_prob"].as_f64().unwrap() as f32;
        max_win_diff = max_win_diff.max((win_prob - ew).abs());
        assert!((win_prob - ew).abs() < 1e-4, "win {win_prob} vs {ew}");
    }
    eprintln!(
        "run_value_net_parity: {} cases, max |Δwin|={max_win_diff:.3e}",
        cases.len()
    );
}
