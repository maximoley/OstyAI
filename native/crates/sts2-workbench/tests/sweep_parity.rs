//! SimLab sweep parity (GH #707): the rows and the results payload of two
//! small sweeps, replayed through the port and compared with what the Python
//! runner + server answered for the same specs.
//!
//! The fixture is `tests/golden/simlab_sweep.json`, written by
//! `sts2-pycli/scripts/dump_simlab_sweep_golden.py` (its docstring is the
//! contract). Rows are compared exactly (minus `wall_ms`); the results
//! payload exactly except the KDE curve, which is compared within a
//! tolerance — the two sides sum the kernel in a different order.

use serde_json::{json, Value};
use sts2_workbench::simlab_sweep::{play_combat, results, SweepSpec};

const FIXTURE: &str = include_str!("golden/simlab_sweep.json");

/// Python `round(x, 4)` — see `golden_parity.rs`.
fn py_round4(f: f64) -> f64 {
    format!("{f:.4}").parse().unwrap_or(f)
}

/// The dump's normalization: sorted keys (serde_json's `Map` already is),
/// floats to 4 decimals, `throughput` / `wall_ms` dropped.
fn normalize(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| k.as_str() != "throughput" && k.as_str() != "wall_ms")
                .map(|(k, v)| (k.clone(), normalize(v)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(normalize).collect()),
        Value::Number(n) if n.is_f64() => json!(py_round4(n.as_f64().unwrap_or(0.0))),
        other => other.clone(),
    }
}

/// Compare two results payloads: the KDE curves within `tol`, the rest exactly.
fn assert_results_match(ours: &Value, theirs: &Value, label: &str) {
    let strip = |v: &Value| {
        let mut v = normalize(v);
        if let Some(buckets) = v.get_mut("buckets").and_then(Value::as_object_mut) {
            for b in buckets.values_mut() {
                if let Some(o) = b.as_object_mut() {
                    o.remove("kde");
                }
            }
        }
        v
    };
    assert_eq!(strip(ours), strip(theirs), "{label}: results differ");
    let ob = ours["buckets"].as_object().expect("buckets");
    let tb = theirs["buckets"].as_object().expect("buckets");
    for (key, t) in tb {
        let o = &ob[key];
        for axis in ["x", "y"] {
            let ox = o["kde"][axis].as_array().expect("kde");
            let tx = t["kde"][axis].as_array().expect("kde");
            assert_eq!(ox.len(), tx.len(), "{label}: {key} kde.{axis} length");
            for (a, b) in ox.iter().zip(tx) {
                let (a, b) = (a.as_f64().unwrap_or(0.0), b.as_f64().unwrap_or(0.0));
                assert!(
                    (a - b).abs() <= 2e-4,
                    "{label}: {key} kde.{axis}: {a} vs {b}"
                );
            }
        }
    }
}

#[test]
fn sweep_rows_and_results_match_the_python_runner() {
    let doc: Value = serde_json::from_str(FIXTURE).expect("fixture parses");
    let sweeps = doc["sweeps"].as_array().expect("sweeps");
    assert_eq!(sweeps.len(), 2);
    for sweep in sweeps {
        let label = sweep["name"].as_str().unwrap_or("?");
        let spec = SweepSpec::from_json(&sweep["spec"]).expect("spec parses");
        let cells = spec.validate().expect("spec validates");
        let clock = || 0.0;
        let mut rows = Vec::new();
        for cell in &cells {
            for seed in 0..spec.n_seeds {
                let row = play_combat(
                    cell,
                    u64::from(seed),
                    spec.mcts_sims,
                    spec.mcts_depth,
                    &clock,
                )
                .unwrap_or_else(|e| panic!("{label}: {} seed {seed}: {e}", cell.strategy));
                rows.push(row);
            }
        }
        let expected = sweep["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), expected.len(), "{label}: row count");
        for (i, (ours, theirs)) in rows.iter().zip(expected).enumerate() {
            assert_eq!(
                normalize(ours),
                normalize(theirs),
                "{label}: row {i} ({}/{}/seed {}) differs",
                theirs["strategy"],
                theirs["deck"],
                theirs["seed"]
            );
        }
        let plain = results(&rows, "", "").expect("results");
        assert_results_match(&plain, &sweep["results"], label);
        let baseline = sweep["baseline"].as_str().unwrap_or("");
        let with_baseline = results(&rows, baseline, "").expect("results with baseline");
        assert_results_match(&with_baseline, &sweep["results_baseline"], label);
    }
}
