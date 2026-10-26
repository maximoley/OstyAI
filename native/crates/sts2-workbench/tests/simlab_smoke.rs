//! End-to-end smoke of the browser SimLab sandbox routes over `Workbench::call`.
//! The position logic itself is covered by `simlab::tests`; this checks the
//! `/api/simlab/*` dispatch, the capability flag, and the 422 path.

use serde_json::{json, Value};
use sts2_workbench::api::{Config, Workbench};

fn new_wb() -> Workbench {
    Workbench::new(Config {
        assets_base: "https://assets.test".to_string(),
        ..Config::default()
    })
}

fn call(wb: &mut Workbench, method: &str, route: &str, body: Value) -> Value {
    match wb.call(method, route, &body) {
        Ok(v) => v,
        Err(e) => panic!("{method} {route} -> {} {}", e.status, e.detail),
    }
}

#[test]
fn capabilities_advertise_the_sandbox_and_the_sweeps() {
    let mut wb = new_wb();
    let caps = call(&mut wb, "GET", "/api/capabilities", json!({}));
    assert_eq!(caps["features"]["simlab"], json!(true));
    // GH #707: the sweeps run in the wasm as a host-stepped job.
    assert_eq!(caps["features"]["simlab_sweeps"], json!(true));
    let meta = call(&mut wb, "GET", "/api/meta", json!({}));
    assert_eq!(meta["strategies"].as_array().unwrap().len(), 5);
    assert!(meta["deck_presets"]["ironclad_starter"].is_array());
}

/// The sweep routes end to end through `Workbench::call` + `sweep_tick`
/// (what the worker does): submit → poll → tick to done → results; stop.
#[test]
fn sweep_routes_run_a_study_through_the_dispatcher() {
    let mut wb = new_wb();
    let spec = json!({
        "name": "smoke", "encounters": ["FUZZY_WURM_CRAWLER_WEAK"], "ascensions": [0],
        "strategies": ["random", "attack_first"], "n_seeds": 2,
        "decks": [["ironclad_starter", ["STRIKE_IRONCLAD", "STRIKE_IRONCLAD", "STRIKE_IRONCLAD",
            "STRIKE_IRONCLAD", "STRIKE_IRONCLAD", "DEFEND_IRONCLAD", "DEFEND_IRONCLAD",
            "DEFEND_IRONCLAD", "DEFEND_IRONCLAD", "BASH"]]],
    });
    let bad = wb
        .call("POST", "/api/sweeps", &json!({"name": "x"}))
        .unwrap_err();
    assert_eq!(bad.status, 422);
    let id = call(&mut wb, "POST", "/api/sweeps", spec)["sweep_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(id.ends_with("-smoke"));
    let status = call(&mut wb, "GET", &format!("/api/sweeps/{id}"), json!({}));
    assert_eq!(status["status"], "running");
    assert_eq!(status["total"], 4);
    assert_eq!(
        wb.call("GET", &format!("/api/sweeps/{id}/results"), &json!({}))
            .unwrap_err()
            .status,
        404
    );
    let mut ticks = 0;
    while wb.has_running_sweep() && ticks < 100 {
        wb.sweep_tick(1);
        ticks += 1;
    }
    assert_eq!(ticks, 4);
    let status = call(&mut wb, "GET", &format!("/api/sweeps/{id}"), json!({}));
    assert_eq!(status["status"], "done");
    let list = call(&mut wb, "GET", "/api/sweeps", json!({}));
    assert_eq!(list["sweeps"][0]["sweep_id"], json!(id));
    let res = call(
        &mut wb,
        "GET",
        &format!("/api/sweeps/{id}/results?baseline=random"),
        json!({}),
    );
    let key = "FUZZY_WURM_CRAWLER_WEAK/0/ironclad_starter/attack_first";
    assert_eq!(res["buckets"][key]["n"], 2);
    assert!(res["buckets"][key]["delta_vs_baseline"].is_object());
    assert_eq!(
        wb.call("POST", &format!("/api/sweeps/{id}/stop"), &json!({}))
            .unwrap_err()
            .status,
        404
    );
    // A stored sweep comes back through import_sweep.
    let doc = wb.export_sweep(&id).unwrap();
    let mut fresh = new_wb();
    fresh.import_sweep(&doc).unwrap();
    assert_eq!(
        call(&mut fresh, "GET", &format!("/api/sweeps/{id}"), json!({}))["status"],
        "done"
    );
}

#[test]
fn catalog_and_meta_answer_from_the_wasm() {
    let mut wb = new_wb();
    let cat = call(&mut wb, "GET", "/api/simlab/catalog", json!({}));
    assert!(!cat["cards"].as_array().unwrap().is_empty());
    assert!(!cat["monsters"].as_array().unwrap().is_empty());
    let meta = call(&mut wb, "GET", "/api/meta", json!({}));
    assert!(!meta["cards"].as_array().unwrap().is_empty());
}

#[test]
fn a_new_fight_builds_moves_and_searches() {
    let mut wb = new_wb();
    // New fight → a turn-1 position.
    let new = call(
        &mut wb,
        "POST",
        "/api/simlab/position/new",
        json!({"character": "Ironclad", "encounter": "MAWLER_NORMAL", "ascension": 0, "seed": 1}),
    );
    let position = new["position"].clone();
    assert_eq!(position["character"], json!("Ironclad"));

    // Build round-trips it.
    let built = call(
        &mut wb,
        "POST",
        "/api/simlab/position/build",
        json!({"position": position}),
    );
    assert_eq!(built["wire"]["phase"], json!("PlayerPlayPhase"));

    // Legal first moves include End turn.
    let moves = call(
        &mut wb,
        "POST",
        "/api/simlab/position/moves",
        json!({"position": position}),
    );
    let labels: Vec<&str> = moves["moves"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["label"].as_str())
        .collect();
    assert!(labels.contains(&"End turn"), "labels: {labels:?}");

    // MCTS returns a ranked, non-empty verdict at a tiny budget.
    let mcts = call(
        &mut wb,
        "POST",
        "/api/simlab/position/mcts",
        json!({"position": position, "sims": 16, "seed": 1}),
    );
    assert_eq!(mcts["sims"], json!(16));
    assert!(!mcts["rows"].as_array().unwrap().is_empty());
}

#[test]
fn a_position_paste_imports_and_a_bad_one_is_422() {
    let mut wb = new_wb();
    let new = call(
        &mut wb,
        "POST",
        "/api/simlab/position/new",
        json!({"character": "Ironclad", "encounter": "MAWLER_NORMAL", "seed": 2}),
    );
    let text = serde_json::to_string(&new["position"]).unwrap();
    let imported = call(
        &mut wb,
        "POST",
        "/api/simlab/position/import",
        json!({"text": text}),
    );
    assert_eq!(imported["source"], json!("position"));

    // A shape with no fight in it is a 422, and changes nothing.
    let err = wb
        .call(
            "POST",
            "/api/simlab/position/import",
            &json!({"text": "{\"not\": \"a fight\"}"}),
        )
        .expect_err("junk paste is refused");
    assert_eq!(err.status, 422);
}

#[test]
fn a_broken_position_build_is_422() {
    let mut wb = new_wb();
    // An empty board can never become a fight.
    let err = wb
        .call(
            "POST",
            "/api/simlab/position/build",
            &json!({"position": {"schema": "simlab-position/1"}}),
        )
        .expect_err("empty board is refused");
    assert_eq!(err.status, 422);
}

/// GH #708: a SpireMentor fight paste becomes a sandbox board (the combat
/// adapter runs in the wasm), and a fight the engine cannot rebuild is a 422
/// that says why.
#[test]
fn a_spirementor_fight_paste_becomes_a_board() {
    // Game-derived golden (rendered card text), not published — runtime read.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/state_import_combat.json"
    );
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("skipping: {path} not present (re-dump with dump_state_import_golden.py)");
        return;
    };
    let doc: Value = serde_json::from_str(&text).expect("golden parses");
    let case = &doc["cases"][0];
    let paste = case["paste"].as_str().unwrap();
    let mut wb = new_wb();
    let r = call(
        &mut wb,
        "POST",
        "/api/simlab/position/import",
        json!({"text": paste}),
    );
    assert_eq!(r["source"], json!("SpireMentor Copy-state"));
    let hand = r["position"]["piles"]["hand"].as_array().expect("hand");
    assert_eq!(
        hand.len(),
        case["combat_wire"]["deck"]["hand"]
            .as_array()
            .unwrap()
            .len()
    );
    // It builds and searches like any other board.
    let built = call(
        &mut wb,
        "POST",
        "/api/simlab/position/build",
        json!({"position": r["position"]}),
    );
    assert!(built["wire"].is_object());

    // A fight paste with an unknown monster is refused with the reason.
    let mut broken: Value = serde_json::from_str(paste).unwrap();
    broken["enemies"][0]["name"] = json!("Not A Monster");
    broken["enemies"][0]["id"] = json!("NOT_A_MONSTER");
    let e = wb
        .call(
            "POST",
            "/api/simlab/position/import",
            &json!({"text": broken.to_string()}),
        )
        .unwrap_err();
    assert_eq!(e.status, 422);
    assert!(e.detail.contains("could not rebuild"), "{}", e.detail);
    assert!(e.detail.contains("unknown monster"), "{}", e.detail);
}

/// GH #709: the live-game tree's raw combat-log rows are rendered in the
/// wasm, the way the Python proxy rendered them.
#[test]
fn bridge_tree_combat_logs_render_in_the_wasm() {
    let mut wb = new_wb();
    let tree = json!({
        "nodes": [
            {"id": 0, "decision": "root"},
            {"id": 1, "decision": "combat_action", "player_id": "IRONCLAD",
             "combat_log_raw": [{
                "kind": "damage", "round": 1, "side": "player",
                "text": "IRONCLAD dealt 6 damage to NIBBIT",
                "source": {"kind": "card", "id": "STRIKE_IRONCLAD", "upgraded": false},
                "dealer": {"kind": "player", "id": "IRONCLAD", "uid": null},
                "receiver": {"kind": "monster", "id": "Nibbit", "uid": null},
                "unblocked": 6, "blocked": 0,
             }]},
            "not a node",
        ],
        "active_path": [0, 1],
        "active_leaf": 1,
    });
    let out = call(
        &mut wb,
        "POST",
        "/api/bridge/render-tree",
        json!({"tree": tree}),
    );
    assert_eq!(out["active_leaf"], json!(1));
    assert!(out["nodes"][0].get("combat_log").is_none());
    let n1 = &out["nodes"][1];
    assert!(n1.get("combat_log_raw").is_none());
    assert!(n1.get("player_id").is_none());
    let rows = n1["combat_log"].as_array().expect("rendered rows");
    assert_eq!(rows.len(), 1, "{n1}");
    assert!(
        rows[0]["text"].as_str().unwrap().contains("Strike"),
        "{}",
        rows[0]
    );
    assert_eq!(out["nodes"][2], json!("not a node"));
    // No tree at all is an empty object, never an error.
    assert_eq!(
        call(&mut wb, "POST", "/api/bridge/render-tree", json!({})),
        json!({})
    );
}
