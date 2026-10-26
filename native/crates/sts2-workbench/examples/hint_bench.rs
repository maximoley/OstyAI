//! The native twin of the browser workbench's "⏱ bench" button, used for the
//! search timings on the docs site
//! (`sts2-rs/docs/perf/2026-10-14-site-performance.md`).
//!
//! Walks a seed from Neow to its first fight through the same route dispatcher
//! the wasm worker drives (`Workbench::call`), with the SAME walk rule as
//! `scripts/site-bench/wasm_hint_bench.mjs` and `chrome_bench_page.js` at the
//! repository root (first enabled option; a card
//! select takes the first `min_select` cards). It prints the route and a
//! position fingerprint so all harnesses can be checked to sit on the same
//! fight, then times `POST /api/bench {sims, seed: 7}` (the BenchButton's exact
//! body: the hint endpoint's single-tree BeliefDraw search, no TT, depth 200)
//! after one untimed warm-up, `reps` times per budget. `pv0` (first move of the
//! principal variation) must match across engines and across native/wasm.
//!
//!   cargo build --release -p sts2-workbench --example hint_bench
//!   taskset -c 20 target/release/examples/hint_bench BENCH1 7
use std::time::Instant;

use serde_json::{json, Value};
use sts2_workbench::api::{Config, Workbench};

const SIMS: [u64; 5] = [200, 800, 1000, 2000, 4000];

fn call(wb: &mut Workbench, m: &str, r: &str, b: &Value) -> Value {
    wb.call(m, r, b)
        .unwrap_or_else(|e| panic!("{m} {r} -> {} {}", e.status, e.detail))
}

fn next(step: &Value) -> (String, Value) {
    let st = &step["state"];
    if step["decision"] == "card_select" {
        let min = st["min_select"].as_u64().unwrap_or(1).max(1);
        let idx: Vec<String> = (0..min).map(|i| i.to_string()).collect();
        return ("select_cards".into(), json!({"indices": idx.join(",")}));
    }
    st["options"]
        .as_array()
        .and_then(|o| o.iter().find(|o| o["enabled"].as_bool().unwrap_or(true)))
        .map_or_else(
            || ("proceed".into(), json!({})),
            |o| {
                (
                    o["action"].as_str().unwrap_or("proceed").to_string(),
                    o["args"].clone(),
                )
            },
        )
}

fn fingerprint(step: &Value) -> String {
    let st = &step["state"];
    let enemies: Vec<String> = st["enemies"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| format!("{}:{}", e["name"].as_str().unwrap_or("?"), e["hp"]))
        .collect();
    let hand: Vec<String> = st["hand"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| c["id"].as_str().unwrap_or("?").to_string())
        .collect();
    format!("enemies={};hand={}", enemies.join(","), hand.join(","))
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seed = a.get(1).cloned().unwrap_or_else(|| "BENCH1".into());
    let reps: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(7).max(1);
    let mut wb = Workbench::new(Config {
        assets_base: "/assets".into(),
        ..Config::default()
    });
    let mut step = call(
        &mut wb,
        "POST",
        "/api/live/start",
        &json!({"character": "Ironclad", "seed": seed, "ascension": 0, "act1": "overgrowth"}),
    );
    let mut route = Vec::new();
    while step["decision"] != "combat_play" {
        assert!(route.len() < 60, "no fight reached: {}", step["decision"]);
        let (action, args) = next(&step);
        route.push(json!([action, args]));
        let resp = call(
            &mut wb,
            "POST",
            "/api/live/action",
            &json!({"action": action, "args": args}),
        );
        step = resp["step"].clone();
    }
    println!("host=native seed={seed} route={}", Value::Array(route));
    println!("position {}", fingerprint(&step));
    // One untimed 4,000-sim call before any budget, identical in all three
    // harnesses: the JS engines tier the wasm up (Liftoff -> TurboFan) while it
    // runs, so the small budgets are not timed on baseline code.
    let _ = call(
        &mut wb,
        "POST",
        "/api/bench",
        &json!({"sims": 4000, "seed": 7}),
    );
    for sims in SIMS {
        let body = json!({"sims": sims, "seed": 7});
        let t = Instant::now();
        let first = call(&mut wb, "POST", "/api/bench", &body); // warm-up, reported as cold
        let cold = t.elapsed().as_secs_f64() * 1e3;
        let mut ms: Vec<f64> = (0..reps)
            .map(|_| {
                let t = Instant::now();
                let _ = call(&mut wb, "POST", "/api/bench", &body);
                t.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        let min = ms.iter().copied().fold(f64::INFINITY, f64::min);
        let med = median(&mut ms);
        #[allow(clippy::cast_precision_loss)]
        let per = med / sims as f64;
        println!(
            "sims={sims} cold_ms={cold:.2} median_ms={med:.2} min_ms={min:.2} ms_per_sim={per:.5} pv0={}",
            first["pv"][0]
        );
    }
}
