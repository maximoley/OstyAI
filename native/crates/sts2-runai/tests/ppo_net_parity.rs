// Needs the ONNX runtime (`ppo::net`), compiled out without the `ppo` feature.
#![cfg(feature = "ppo")]
//! Gate B (spec Phase 2 §4b): tract π/V vs torch < 1e-4 on the fixtures.
use std::fs;

use serde::Deserialize;
use sts2_core::potions::CharacterId;
use sts2_runai::ppo::encoder::featurize_with;
use sts2_runai::ppo::features::Features;
use sts2_runai::ppo::net::{features_for, onnx_model_path, PpoNet};
use sts2_runai::ppo::obs::{ActionDesc, PpoObs};

#[derive(Deserialize)]
struct Row {
    seed: i64,
    decision: i64,
    obs: PpoObs,
    actions: Vec<ActionDesc>,
    /// See `ppo_encoder_parity::Row::features` — π/V were recorded at this
    /// schema, so featurize at it rather than at the runner's environment.
    #[serde(default)]
    features: Vec<String>,
    probs: Vec<f64>,
    value: f64,
}

#[test]
fn tract_matches_torch_pi_and_v() {
    let texts: Vec<String> = sts2_runai::ppo::test_fixture_paths("ppo_encoder_parity")
        .iter()
        .filter_map(|p| fs::read_to_string(p).ok())
        .collect();
    let (false, Some(model)) = (texts.is_empty(), onnx_model_path()) else {
        eprintln!("skip: fixture or ONNX model absent");
        return;
    };
    let net = PpoNet::load(&model).expect("load");
    // Gate B can only score a row recorded at the schema the SERVED net was
    // trained at. Running it on any other schema is precisely the mis-serve
    // #545 exists to prevent — a gated feature rewrites an existing `cat`
    // slot, so the shapes match, nothing errors, and the π it produces is
    // simply wrong. Rows at another schema stay in the file because gate A
    // (`ppo_encoder_parity`) is net-independent and still pins them.
    let served = features_for(CharacterId::Ironclad);
    let mut max_dp = 0f64;
    let mut max_dv = 0f64;
    let mut scored = 0usize;
    let mut skipped = 0usize;
    for line in texts
        .iter()
        .flat_map(|t| t.lines())
        .filter(|l| !l.trim().is_empty())
    {
        let row: Row = serde_json::from_str(line).expect("row");
        let recorded = Features::parse(&row.features.join(","));
        if recorded != served {
            skipped += 1;
            continue;
        }
        scored += 1;
        let feat = featurize_with(&row.obs, &row.actions, recorded);
        let (probs, value) = net.infer(&feat).expect("infer");
        assert_eq!(probs.len(), row.probs.len());
        for (g, w) in probs.iter().zip(&row.probs) {
            max_dp = max_dp.max((f64::from(*g) - w).abs());
        }
        max_dv = max_dv.max((f64::from(value) - row.value).abs());
        assert!(
            max_dp < 1e-4,
            "π diverged {max_dp} @ seed {} d{}",
            row.seed,
            row.decision
        );
        assert!(
            max_dv < 1e-4,
            "V diverged {max_dv} @ seed {} d{}",
            row.seed,
            row.decision
        );
    }
    // Without this the gate passes loudest when it is doing nothing: promote a
    // champion at a schema no fixture was dumped at and every row would be
    // skipped, leaving a green test that scored zero rows.
    assert!(
        scored > 0,
        "gate B scored 0 of {} fixture rows — none were recorded at the \
         served champion's schema ({served:?}). Re-dump the fixtures with \
         STS2_PPO_FEATURES set to match champions.toml.",
        scored + skipped
    );
    println!("gate B: {scored} rows scored, {skipped} at another schema; max |Δπ|={max_dp:.2e} max |ΔV|={max_dv:.2e}");
}
