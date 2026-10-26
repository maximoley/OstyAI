//! State-import combat-lane parity (GH #708): the engine wire dict the port
//! builds for a SpireMentor fight paste, against the desktop chain's answer
//! (`realgame_adapter.to_headless_combat_play` + `sts2_hint.adapter.translate`)
//! for every real capture under `sts2-pycli/tests/fixtures/realgame_adapter/`.
//!
//! The fixture is `tests/golden/state_import_combat.json`, written by
//! `sts2-pycli/scripts/dump_state_import_golden.py`. The wire dicts are
//! compared exactly; the engine must also accept every one of them.

use serde_json::Value;
use sts2_workbench::state_import::parse_pasted_state;

/// The golden is game-derived (it embeds rendered card text) and is not
/// published; read it at runtime so the crate compiles without it.
fn fixture() -> Option<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/state_import_combat.json"
    );
    std::fs::read_to_string(path).ok()
}

#[test]
fn combat_wire_matches_the_python_adapter_for_every_real_capture() {
    let Some(text) = fixture() else {
        eprintln!("skipping: tests/golden/state_import_combat.json not present (re-dump with dump_state_import_golden.py)");
        return;
    };
    let doc: Value = serde_json::from_str(&text).expect("fixture parses");
    let cases = doc["cases"].as_array().expect("cases");
    assert!(cases.len() >= 9, "expected the nine real captures");
    for case in cases {
        let name = case["name"].as_str().unwrap_or("?");
        let paste = case["paste"].as_str().expect("paste");
        let imported = parse_pasted_state(paste).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(imported.surface, case["surface"], "{name}: surface");
        assert_eq!(
            imported.has_combat(),
            case["has_combat"].as_bool().unwrap_or(false),
            "{name}: has_combat"
        );
        let ours = imported
            .run
            .combat
            .as_ref()
            .map_or(Value::Null, |c| c.wire.clone());
        assert_eq!(ours, case["combat_wire"], "{name}: the engine wire differs");
        if !ours.is_null() {
            let cs = sts2_wire::state_from_json(&ours)
                .unwrap_or_else(|e| panic!("{name}: the engine refused the wire: {e}"));
            assert_eq!(
                i64::from(cs.player.current_hp),
                ours["player"]["hp"].as_i64().unwrap_or(-1),
                "{name}: hp"
            );
        }
        // The fidelity claims: the Python list is a subset of ours (ours may
        // add the names of dropped entities the desktop tally kept silent).
        let exact = imported.fidelity()["exact"].clone();
        assert_eq!(exact, case["exact"], "{name}: exact claims");
        for w in case["warnings"].as_array().into_iter().flatten() {
            let w = w.as_str().unwrap_or("");
            assert!(
                imported.warnings.iter().any(|ours| ours == w),
                "{name}: missing warning {w:?}; ours: {:?}",
                imported.warnings
            );
        }
    }
}
