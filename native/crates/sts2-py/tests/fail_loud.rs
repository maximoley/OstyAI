//! Fail-loud parse tests for the wire-format dict.
//!
//! Bead `sts2-rs-92sk` (hint-T1). Every unsupported id / shape must
//! produce an error whose message contains the offending JSON path
//! (`player.relics[3]`, `deck.hand[1]: STRIKE_X`, …) so the Python
//! adapter can log it and degrade gracefully without the rollout
//! producing silent garbage.

#![allow(clippy::missing_panics_doc)]

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use sts2_rs::state_dict::{list_registry_names_impl, parse_state_dict, UnsupportedStateError};

// ---------------------------------------------------------------------------
// Fixture builders — return a "minimum-viable" dict that parses, so a
// single field can be perturbed per test to isolate the error path.
// ---------------------------------------------------------------------------

fn empty_list(py: Python<'_>) -> Bound<'_, PyList> {
    PyList::empty(py)
}

fn make_card<'py>(
    py: Python<'py>,
    id: &str,
    stable_idx: i32,
    kind: &str,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("id", id)?;
    d.set_item("stable_idx", stable_idx)?;
    d.set_item("kind", kind)?;
    d.set_item("upgraded", false)?;
    Ok(d)
}

fn make_deck(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("draw_pile", empty_list(py))?;
    d.set_item("discard_pile", empty_list(py))?;
    d.set_item("hand", empty_list(py))?;
    d.set_item("exhaust_pile", empty_list(py))?;
    Ok(d)
}

fn make_player(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("class", "ironclad")?;
    d.set_item("hp", 80)?;
    d.set_item("max_hp", 80)?;
    d.set_item("block", 0)?;
    d.set_item("energy", 3)?;
    d.set_item("max_energy", 3)?;
    d.set_item("energy_regen_per_turn", 3)?;
    d.set_item("relics", empty_list(py))?;
    d.set_item("powers", empty_list(py))?;
    d.set_item("potions", empty_list(py))?;
    Ok(d)
}

fn make_top(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("wire_version", 1)?;
    d.set_item("phase", "PlayerPlayPhase")?;
    d.set_item("round", 1)?;
    d.set_item("side", "Player")?;
    d.set_item("is_elite", false)?;
    d.set_item("turn", 1)?;
    d.set_item("combat_id", 0_i64)?;
    d.set_item("player", make_player(py)?)?;
    d.set_item("deck", make_deck(py)?)?;
    d.set_item("encounter", empty_list(py))?;
    Ok(d)
}

fn assert_unsupported_with_path(err: &PyErr, path_substring: &str) {
    Python::with_gil(|py| {
        assert!(
            err.is_instance_of::<UnsupportedStateError>(py),
            "expected UnsupportedStateError, got: {err}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains(path_substring),
            "error message `{msg}` missing path `{path_substring}`"
        );
    });
}

fn assert_value_error_with_path(err: &PyErr, path_substring: &str) {
    Python::with_gil(|py| {
        // ValueError or subclass (UnsupportedStateError is also a ValueError).
        let value_err_ty = py.get_type::<pyo3::exceptions::PyValueError>();
        assert!(
            err.is_instance(py, value_err_ty.as_any()),
            "expected ValueError(-subclass), got: {err}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains(path_substring),
            "error message `{msg}` missing path `{path_substring}`"
        );
    });
}

// ---------------------------------------------------------------------------
// Baseline: minimum-viable dict parses cleanly.
// ---------------------------------------------------------------------------

#[test]
fn baseline_minimal_dict_parses() {
    Python::with_gil(|py| {
        let d = make_top(py).unwrap();
        let parsed = parse_state_dict(&d).expect("baseline must parse");
        assert_eq!(parsed.wire_version, 1);
        assert_eq!(parsed.player.class, "ironclad");
        assert!(parsed.encounter.is_empty());
    });
}

// ---------------------------------------------------------------------------
// wire_version mismatch
// ---------------------------------------------------------------------------

#[test]
fn bad_wire_version_fails_loud() {
    Python::with_gil(|py| {
        let d = make_top(py).unwrap();
        d.set_item("wire_version", 2).unwrap();
        let err = parse_state_dict(&d).unwrap_err();
        assert_unsupported_with_path(&err, "$.wire_version");
    });
}

// ---------------------------------------------------------------------------
// Unknown relic
// ---------------------------------------------------------------------------

#[test]
fn unknown_relic_fails_loud_with_path() {
    Python::with_gil(|py| {
        let d = make_top(py).unwrap();
        let player = d.get_item("player").unwrap().unwrap();
        let player = player.downcast::<PyDict>().unwrap();
        let relics = PyList::empty(py);
        let r0 = PyDict::new(py);
        r0.set_item("id", "BURNING_BLOOD").unwrap();
        r0.set_item("counter", py.None()).unwrap();
        relics.append(r0).unwrap();
        let r1 = PyDict::new(py);
        r1.set_item("id", "DEFINITELY_NOT_A_RELIC").unwrap();
        r1.set_item("counter", py.None()).unwrap();
        relics.append(r1).unwrap();
        player.set_item("relics", relics).unwrap();

        let err = parse_state_dict(&d).unwrap_err();
        assert_unsupported_with_path(&err, "$.player.relics[1]");
        assert!(err.to_string().contains("DEFINITELY_NOT_A_RELIC"));
    });
}

// ---------------------------------------------------------------------------
// Unknown card in hand
// ---------------------------------------------------------------------------

#[test]
fn unknown_card_in_hand_fails_loud_with_path() {
    Python::with_gil(|py| {
        let d = make_top(py).unwrap();
        let deck = d
            .get_item("deck")
            .unwrap()
            .unwrap()
            .downcast::<PyDict>()
            .unwrap()
            .clone();
        let hand = PyList::empty(py);
        hand.append(make_card(py, "STRIKE_IRONCLAD", 0, "card").unwrap())
            .unwrap();
        hand.append(make_card(py, "NOT_A_REAL_CARD", 1, "card").unwrap())
            .unwrap();
        deck.set_item("hand", hand).unwrap();
        let err = parse_state_dict(&d).unwrap_err();
        assert_unsupported_with_path(&err, "$.deck.hand[1]");
        assert!(err.to_string().contains("NOT_A_REAL_CARD"));
    });
}

// ---------------------------------------------------------------------------
// Unknown monster kind
// ---------------------------------------------------------------------------

#[test]
fn unknown_monster_kind_fails_loud_with_path() {
    Python::with_gil(|py| {
        let d = make_top(py).unwrap();
        let enc = PyList::empty(py);
        let m0 = PyDict::new(py);
        m0.set_item("kind", "GhibbetMaw").unwrap();
        m0.set_item("uid", 1_i64).unwrap();
        m0.set_item("hp", 10).unwrap();
        m0.set_item("max_hp", 10).unwrap();
        m0.set_item("block", 0).unwrap();
        m0.set_item("powers", empty_list(py)).unwrap();
        m0.set_item("intents", empty_list(py)).unwrap();
        m0.set_item("intent_history", empty_list(py)).unwrap();
        m0.set_item("is_dead", false).unwrap();
        m0.set_item("is_escaped", false).unwrap();
        enc.append(m0).unwrap();
        d.set_item("encounter", enc).unwrap();
        let err = parse_state_dict(&d).unwrap_err();
        assert_unsupported_with_path(&err, "$.encounter[0].kind");
        assert!(err.to_string().contains("GhibbetMaw"));
    });
}

// ---------------------------------------------------------------------------
// Unknown player power
// ---------------------------------------------------------------------------

#[test]
fn unknown_player_power_fails_loud_with_path() {
    Python::with_gil(|py| {
        let d = make_top(py).unwrap();
        let player = d
            .get_item("player")
            .unwrap()
            .unwrap()
            .downcast::<PyDict>()
            .unwrap()
            .clone();
        let powers = PyList::empty(py);
        let p0 = PyDict::new(py);
        p0.set_item("name", "STRENGTH").unwrap();
        p0.set_item("amount", 1).unwrap();
        powers.append(p0).unwrap();
        let p1 = PyDict::new(py);
        p1.set_item("name", "MEGA_FAKE_POWER").unwrap();
        p1.set_item("amount", 1).unwrap();
        powers.append(p1).unwrap();
        player.set_item("powers", powers).unwrap();
        let err = parse_state_dict(&d).unwrap_err();
        assert_unsupported_with_path(&err, "$.player.powers[1].name");
        assert!(err.to_string().contains("MEGA_FAKE_POWER"));
    });
}

// ---------------------------------------------------------------------------
// Unknown enemy power
// ---------------------------------------------------------------------------

#[test]
fn unknown_enemy_power_fails_loud_with_path() {
    Python::with_gil(|py| {
        let d = make_top(py).unwrap();
        let enc = PyList::empty(py);
        let m0 = PyDict::new(py);
        m0.set_item("kind", "Nibbit").unwrap();
        m0.set_item("uid", 1_i64).unwrap();
        m0.set_item("hp", 50).unwrap();
        m0.set_item("max_hp", 50).unwrap();
        m0.set_item("block", 0).unwrap();
        let pows = PyList::empty(py);
        let p0 = PyDict::new(py);
        p0.set_item("name", "ABSOLUTELY_FAKE").unwrap();
        p0.set_item("amount", 1).unwrap();
        pows.append(p0).unwrap();
        m0.set_item("powers", pows).unwrap();
        m0.set_item("intents", empty_list(py)).unwrap();
        m0.set_item("intent_history", empty_list(py)).unwrap();
        m0.set_item("is_dead", false).unwrap();
        m0.set_item("is_escaped", false).unwrap();
        enc.append(m0).unwrap();
        d.set_item("encounter", enc).unwrap();
        let err = parse_state_dict(&d).unwrap_err();
        assert_unsupported_with_path(&err, "$.encounter[0].powers[0].name");
    });
}

// ---------------------------------------------------------------------------
// Schema violations
// ---------------------------------------------------------------------------

#[test]
fn hp_gt_max_hp_fails_loud() {
    Python::with_gil(|py| {
        let d = make_top(py).unwrap();
        let player = d
            .get_item("player")
            .unwrap()
            .unwrap()
            .downcast::<PyDict>()
            .unwrap()
            .clone();
        player.set_item("hp", 100).unwrap();
        player.set_item("max_hp", 80).unwrap();
        let err = parse_state_dict(&d).unwrap_err();
        assert_value_error_with_path(&err, "$.player");
        assert!(err.to_string().contains("hp"));
    });
}

#[test]
fn negative_energy_fails_loud() {
    Python::with_gil(|py| {
        let d = make_top(py).unwrap();
        let player = d
            .get_item("player")
            .unwrap()
            .unwrap()
            .downcast::<PyDict>()
            .unwrap()
            .clone();
        player.set_item("energy", -1).unwrap();
        let err = parse_state_dict(&d).unwrap_err();
        assert_value_error_with_path(&err, "$.player.energy");
    });
}

#[test]
fn missing_top_key_fails_loud() {
    Python::with_gil(|py| {
        let d = make_top(py).unwrap();
        // Delete a required key.
        d.del_item("phase").unwrap();
        let err = parse_state_dict(&d).unwrap_err();
        assert_value_error_with_path(&err, "phase");
    });
}

#[test]
fn invalid_phase_fails_loud() {
    Python::with_gil(|py| {
        let d = make_top(py).unwrap();
        d.set_item("phase", "MysteryPhase").unwrap();
        let err = parse_state_dict(&d).unwrap_err();
        assert_unsupported_with_path(&err, "$.phase");
    });
}

#[test]
fn pending_choice_non_none_fails_loud() {
    Python::with_gil(|py| {
        let d = make_top(py).unwrap();
        let pc = PyDict::new(py);
        pc.set_item("type", "card_select").unwrap();
        d.set_item("pending_choice", pc).unwrap();
        let err = parse_state_dict(&d).unwrap_err();
        assert_unsupported_with_path(&err, "$.pending_choice");
    });
}

#[test]
fn unknown_potion_fails_loud_with_path() {
    Python::with_gil(|py| {
        let d = make_top(py).unwrap();
        let player = d
            .get_item("player")
            .unwrap()
            .unwrap()
            .downcast::<PyDict>()
            .unwrap()
            .clone();
        let potions = PyList::empty(py);
        potions.append(py.None()).unwrap();
        potions.append("DEFINITELY_NOT_A_POTION").unwrap();
        potions.append(py.None()).unwrap();
        player.set_item("potions", potions).unwrap();
        let err = parse_state_dict(&d).unwrap_err();
        assert_unsupported_with_path(&err, "$.player.potions[1]");
    });
}

// ---------------------------------------------------------------------------
// list_registry_names — error path + determinism
// ---------------------------------------------------------------------------

#[test]
fn list_registry_names_unknown_kind_returns_value_error() {
    let err = list_registry_names_impl("not-a-registry").unwrap_err();
    Python::with_gil(|py| {
        assert!(err.is_instance_of::<pyo3::exceptions::PyValueError>(py));
    });
    assert!(err.to_string().contains("not-a-registry"));
}

#[test]
fn list_registry_names_round_trips_canonical_examples() {
    for (kind, expected) in [
        ("relic", "BURNING_BLOOD"),
        ("card", "STRIKE_IRONCLAD"),
        ("monster", "Nibbit"),
        ("power", "STRENGTH"),
        ("potion", "ATTACK_POTION"),
        ("orb", "LIGHTNING"),
    ] {
        let names = list_registry_names_impl(kind).unwrap();
        assert!(
            names.iter().any(|n| n == expected),
            "{kind} missing `{expected}`"
        );
        // Pinned registry order is append-only; only unpinned kinds sort.
        assert!(!names.is_empty());
        let mut s = names.clone();
        s.sort();
        if !["card", "potion"].contains(&kind) {
            assert_eq!(names, s, "{kind} not sorted");
        }
    }
}
