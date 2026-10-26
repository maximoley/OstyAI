//! End-to-end smoke of the browser state-import routes over `Workbench::call`.
//! Uses a map-less paste (the degraded run-lane path), so it needs no live
//! game to borrow a map from — the graft correctness with a real map is
//! covered by `game::tests::start_imported_*`.

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

fn paste() -> String {
    json!({
        "type": "advice_request",
        "advice_for": "shop",
        "seed": "SMOKESEED",
        "act": 1,
        "floor": 4,
        "run_state": {
            "character": "IRONCLAD",
            "ascension": 0,
            "hp": 44,
            "max_hp": 80,
            "gold": 150,
            "potion_slot_count": 3,
            "master_deck": [
                {"id": "STRIKE_IRONCLAD"},
                {"id": "STRIKE_IRONCLAD"},
                {"id": "BASH"},
            ],
            "relics": [{"id": "BURNING_BLOOD"}],
            "potions": [],
        },
    })
    .to_string()
}

#[test]
fn preview_describes_without_starting_a_run() {
    let mut wb = new_wb();
    let p = call(
        &mut wb,
        "POST",
        "/api/live/import-state/preview",
        json!({"payload": paste()}),
    );
    assert_eq!(p["run"]["character"], json!("Ironclad"));
    assert_eq!(p["run"]["seed"], json!("SMOKESEED"));
    // A shop paste carries no fight: only the run lane.
    assert_eq!(p["has_combat"], json!(false));
    assert_eq!(p["lanes"], json!(["run"]));
    // Nothing was started.
    let err = wb.call("GET", "/api/live/state", &Value::Null).unwrap_err();
    assert_eq!(err.status, 400);
}

#[test]
fn import_replaces_the_run_and_persist_round_trips() {
    let mut wb = new_wb();
    let step = call(
        &mut wb,
        "POST",
        "/api/live/import-state",
        json!({"payload": paste()}),
    );
    // The graft's fidelity rides on the step; a shop paste is the run lane.
    assert_eq!(step["import"]["lane"], json!("run"));
    assert!(step["import"]["exact"]
        .as_array()
        .unwrap()
        .contains(&json!("deck")));

    // The run is live and its numbers are the pasted ones.
    let state = call(&mut wb, "GET", "/api/live/state", Value::Null);
    let player = &state["state"]["player"];
    assert_eq!(player["hp"], json!(44));
    assert_eq!(player["name"], json!("Ironclad"));

    // A reload rebuilds the imported run through the graft, not as a fresh
    // seed run: the exported persist carries the snapshot and imports back.
    let persist = wb.export_persist();
    assert!(
        !persist["imported"].is_null(),
        "persist carries the import snapshot"
    );
    let mut wb2 = new_wb();
    let restored = wb2.import_persist(&persist, |_, _| {}).expect("reload");
    assert_eq!(restored["state"]["player"]["hp"], json!(44));
    // And the reloaded session can still seek to its root.
    let back = call(&mut wb2, "POST", "/api/live/seek", json!({"node_id": 0}));
    // seek wraps its result under "step".
    assert_eq!(back["step"]["state"]["player"]["hp"], json!(44));
}

#[test]
fn a_bad_paste_is_422_and_costs_nothing() {
    let mut wb = new_wb();
    // Establish a run first.
    call(
        &mut wb,
        "POST",
        "/api/live/import-state",
        json!({"payload": paste()}),
    );
    let before = call(&mut wb, "GET", "/api/live/state", Value::Null);
    // A junk paste is refused without tearing the run down.
    let err = wb
        .call("POST", "/api/live/import-state", &json!({"payload": "{}"}))
        .unwrap_err();
    assert_eq!(err.status, 422);
    let after = call(&mut wb, "GET", "/api/live/state", Value::Null);
    assert_eq!(
        before["state"]["player"]["hp"],
        after["state"]["player"]["hp"]
    );
}

/// A real SpireMentor fight capture (the first case of the combat-lane golden,
/// `sts2-pycli/tests/fixtures/realgame_adapter/*.json` as pasted).
fn combat_paste() -> Option<String> {
    // Game-derived golden (rendered card text), not published — runtime read.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/state_import_combat.json"
    );
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("skipping: {path} not present (re-dump with dump_state_import_golden.py)");
        return None;
    };
    let doc: Value = serde_json::from_str(&text).expect("golden parses");
    let case = doc["cases"]
        .as_array()
        .and_then(|c| c.iter().find(|c| c["name"] == "single_enemy_ironclad"))
        .expect("the single-enemy capture");
    Some(case["paste"].as_str().expect("paste").to_string())
}

/// GH #708: a fight paste offers both lanes; "auto" resumes the exact fight
/// on the rebuilt run, playable, seekable, and persisted through the graft.
#[test]
fn a_fight_paste_resumes_the_exact_fight_and_survives_seek_and_persist() {
    let Some(combat) = combat_paste() else { return };
    let mut wb = new_wb();
    let p = call(
        &mut wb,
        "POST",
        "/api/live/import-state/preview",
        json!({"payload": combat.clone()}),
    );
    assert_eq!(p["has_combat"], json!(true));
    assert_eq!(p["lanes"], json!(["combat", "run"]));
    assert!(p["fidelity"]["exact"]
        .as_array()
        .unwrap()
        .contains(&json!("combat board (hand, piles, powers, intents)")));

    // The exact board: the capture's hand (5 cards), 3 energy, the Shrinker
    // Beetle at 40/40 telegraphing its debuff.
    let step = call(
        &mut wb,
        "POST",
        "/api/live/import-state",
        json!({"payload": combat.clone(), "lane": "auto"}),
    );
    assert_eq!(step["import"]["lane"], json!("combat"));
    assert_eq!(step["decision"], json!("combat_play"), "{step}");
    let state = &step["state"];
    assert_eq!(state["player"]["hp"], json!(80));
    assert_eq!(state["energy"], json!(3));
    assert_eq!(state["hand"].as_array().map(Vec::len), Some(5), "{state}");
    assert_eq!(state["enemies"][0]["hp"], json!(40));
    let hand_ids: Vec<&str> = state["hand"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    assert!(
        hand_ids.iter().any(|id| id.contains("STRIKE")),
        "{hand_ids:?}"
    );

    // Play the recorded action of the capture (card 2 at the beetle) and end
    // the turn: the fight advances on the run.
    let after = call(
        &mut wb,
        "POST",
        "/api/live/action",
        json!({"action": "play_card", "args": {"card_index": 2, "target_index": 0}}),
    );
    let s2 = &after["step"]["state"];
    assert!(
        s2["enemies"][0]["hp"].as_i64().unwrap() < 40 || s2["energy"].as_i64().unwrap() < 3,
        "{s2}"
    );
    let after = call(
        &mut wb,
        "POST",
        "/api/live/action",
        json!({"action": "end_turn", "args": {}}),
    );
    assert_eq!(after["step"]["decision"], json!("combat_play"));

    // Seek back to the import root: the exact board again, through the graft.
    let root = call(&mut wb, "POST", "/api/live/seek", json!({"node_id": 0}));
    assert_eq!(root["step"]["state"]["energy"], json!(3));
    assert_eq!(root["step"]["state"]["enemies"][0]["hp"], json!(40));

    // A reload (persist round trip) rebuilds the fight, not a fresh run.
    let persist = wb.export_persist();
    assert!(persist["imported"]["combat"].is_object(), "{persist}");
    let mut fresh = new_wb();
    let restored = fresh
        .import_persist(&persist, |_, _| {})
        .expect("persist round-trips");
    assert_eq!(restored["decision"], json!("combat_play"));
    assert_eq!(restored["state"]["enemies"][0]["hp"], json!(40));

    // The run lane of the same paste parks on the map instead.
    let step = call(
        &mut wb,
        "POST",
        "/api/live/import-state",
        json!({"payload": combat.clone(), "lane": "run"}),
    );
    assert_eq!(step["import"]["lane"], json!("run"));
    assert_ne!(step["decision"], json!("combat_play"));
    assert!(wb.export_persist()["imported"]["combat"].is_null());

    // Asking for the fight of a shop paste is a 409; a made-up lane a 400.
    let e = wb
        .call(
            "POST",
            "/api/live/import-state",
            &json!({"payload": paste(), "lane": "combat"}),
        )
        .unwrap_err();
    assert_eq!(e.status, 409);
    assert!(e.detail.contains("carries no fight"), "{}", e.detail);
    let e = wb
        .call(
            "POST",
            "/api/live/import-state",
            &json!({"payload": paste(), "lane": "nope"}),
        )
        .unwrap_err();
    assert_eq!(e.status, 400);
}
