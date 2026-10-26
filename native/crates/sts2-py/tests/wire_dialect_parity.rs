//! The two `WireValue` dialects must parse the same dict into the same state.
//!
//! `sts2-wire` runs one parser over two hosts: a `PyDict` (this crate's
//! `PyWire`) and a `serde_json::Value`. That is only worth anything if the two
//! actually agree — otherwise the refactor swapped "one implementation" for
//! "two that happen to share source". These tests drive both from the *same*
//! input and compare the results, including the failure paths.
//!
//! The interesting cases are the coercions where Python and JSON differ by
//! default, each of which the `WireValue` doc block calls out:
//!
//! * `bool` where an integer is expected — Python's `bool` subclasses `int`,
//!   so `PyO3` accepts it. `serde_json` would not, unless made to.
//! * A `u64` above `i64::MAX` (`$.rng.seed`).
//! * A key present but explicitly null vs absent entirely.

#![allow(clippy::missing_panics_doc)]

use pyo3::prelude::*;
use pyo3::types::PyDict;

use sts2_rs::state_dict::parse_state_dict;

/// The committed `sts2-wire` fixture, as JSON text.
fn fixture_json() -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../sts2-wire/tests/fixtures/combat_action_ironclad.json"),
    )
    .expect("sts2-wire fixture readable")
}

/// The same document as a `PyDict`, via Python's own `json.loads` — so the
/// Python side is built by a real producer path rather than transcribed by
/// hand into something that only accidentally matches.
fn fixture_pydict<'py>(py: Python<'py>, text: &str) -> Bound<'py, PyDict> {
    let json = py.import("json").expect("json module");
    json.call_method1("loads", (text,))
        .expect("json.loads")
        .downcast_into::<PyDict>()
        .expect("top level is a dict")
}

/// The message a `PyErr` carries, without the `"TypeName: "` prefix that
/// `PyErr::to_string` prepends — that prefix is Python's exception rendering,
/// not part of the wire error surface, so comparing it against a `WireError`
/// display would compare two different things.
fn py_err_message(py: Python<'_>, err: &PyErr) -> String {
    err.value(py).to_string()
}

/// `ParsedState` is `Debug`-only (no `PartialEq`), and its `Debug` rendering
/// covers every field — which is exactly the comparison we want: a field the
/// two dialects disagree on shows up as a diff in the dump.
fn parsed_debug_via_python(text: &str) -> String {
    pyo3::prepare_freethreaded_python();
    Python::with_gil(|py| {
        let d = fixture_pydict(py, text);
        format!(
            "{:?}",
            parse_state_dict(&d).expect("python path must parse")
        )
    })
}

fn parsed_debug_via_json(text: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(text).expect("valid JSON");
    format!(
        "{:?}",
        sts2_wire::parse_json(&value).expect("json path must parse")
    )
}

#[test]
fn both_dialects_parse_the_fixture_identically() {
    let text = fixture_json();
    assert_eq!(
        parsed_debug_via_python(&text),
        parsed_debug_via_json(&text),
        "PyDict and serde_json parses of the same document must be field-for-field equal"
    );
}

/// Python's `bool` is an `int`, so `.extract::<i64>()` accepts `True`. The
/// JSON impl has to match that or it is stricter than the path it replaced —
/// and a producer emitting `true` for a count would parse under the advisor
/// and fail under a Rust caller.
#[test]
fn a_bool_in_an_int_slot_is_accepted_by_both() {
    let mut value: serde_json::Value = serde_json::from_str(&fixture_json()).unwrap();
    value["player"]["powers"][0]["amount"] = serde_json::json!(true);
    let text = serde_json::to_string(&value).unwrap();

    let py_dump = parsed_debug_via_python(&text);
    let json_dump = parsed_debug_via_json(&text);
    assert_eq!(py_dump, json_dump);
    assert!(
        py_dump.contains("amount: 1"),
        "`true` should read as 1 on both sides"
    );
}

/// No silent truncation on either side.
#[test]
fn a_float_in_an_int_slot_is_rejected_by_both() {
    let mut value: serde_json::Value = serde_json::from_str(&fixture_json()).unwrap();
    value["player"]["hp"] = serde_json::json!(53.5);
    let text = serde_json::to_string(&value).unwrap();

    pyo3::prepare_freethreaded_python();
    let py_err = Python::with_gil(|py| {
        let d = fixture_pydict(py, &text);
        py_err_message(py, &parse_state_dict(&d).unwrap_err())
    });
    let json_err = sts2_wire::parse_json(&serde_json::from_str(&text).unwrap())
        .unwrap_err()
        .to_string();

    assert_eq!(py_err, json_err);
    assert!(py_err.contains("$.player"), "got {py_err}");
}

/// `$.rng.seed` is a genuine `u64`. The fixture carries one above `i64::MAX`
/// precisely so a regression to an `as_i64` read fails here rather than in
/// production.
#[test]
fn a_u64_seed_survives_both_dialects() {
    let text = fixture_json();
    let dump = parsed_debug_via_json(&text);
    assert!(
        dump.contains("18446744073709551557"),
        "seed above i64::MAX must survive: {dump}"
    );
    assert_eq!(parsed_debug_via_python(&text), dump);
}

/// A key set to `None`/`null` must read the same as one that is absent, for
/// every optional field — and must NOT read as a missing *required* key.
#[test]
fn explicit_null_matches_absent_on_both_sides() {
    let base: serde_json::Value = serde_json::from_str(&fixture_json()).unwrap();

    let mut with_null = base.clone();
    with_null["player"]["gold"] = serde_json::Value::Null;
    let mut without = base.clone();
    without["player"].as_object_mut().unwrap().remove("gold");

    let null_text = serde_json::to_string(&with_null).unwrap();
    let absent_text = serde_json::to_string(&without).unwrap();

    assert_eq!(
        parsed_debug_via_json(&null_text),
        parsed_debug_via_json(&absent_text),
        "explicit null and absent must agree (json)"
    );
    assert_eq!(
        parsed_debug_via_python(&null_text),
        parsed_debug_via_python(&absent_text),
        "explicit null and absent must agree (python)"
    );
    assert_eq!(
        parsed_debug_via_json(&null_text),
        parsed_debug_via_python(&null_text)
    );
}

/// A *required* key set to null still satisfies `require_keys` (which tests
/// key presence, not value nullity) and then fails on the type — same message,
/// same class, both sides. This is the behaviour `PyDict::get_item` gave, and
/// it is easy to lose when reimplementing the accessor.
#[test]
fn a_null_required_key_fails_the_same_way_on_both_sides() {
    let mut value: serde_json::Value = serde_json::from_str(&fixture_json()).unwrap();
    value["phase"] = serde_json::Value::Null;
    let text = serde_json::to_string(&value).unwrap();

    pyo3::prepare_freethreaded_python();
    let py_err = Python::with_gil(|py| {
        let d = fixture_pydict(py, &text);
        py_err_message(py, &parse_state_dict(&d).unwrap_err())
    });
    let json_err = sts2_wire::parse_json(&serde_json::from_str(&text).unwrap())
        .unwrap_err()
        .to_string();

    assert_eq!(py_err, json_err);
    assert!(
        py_err.contains("`phase` must be a string"),
        "a null required key should fail on type, not on presence: {py_err}"
    );
}

/// Unmodelled content raises `UnsupportedStateError` on the Python side and
/// reports `WireError::Unsupported` on the other — the split `fail_loud.rs`
/// asserts must survive the trip through `wire_err`.
#[test]
fn the_error_class_split_survives_both_dialects() {
    let mut value: serde_json::Value = serde_json::from_str(&fixture_json()).unwrap();
    value["player"]["relics"][1]["id"] = serde_json::json!("DEFINITELY_NOT_A_RELIC");
    let text = serde_json::to_string(&value).unwrap();

    pyo3::prepare_freethreaded_python();
    Python::with_gil(|py| {
        let d = fixture_pydict(py, &text);
        let err = parse_state_dict(&d).unwrap_err();
        assert!(
            err.is_instance_of::<sts2_rs::state_dict::UnsupportedStateError>(py),
            "expected UnsupportedStateError, got {err}"
        );
        let json_err = sts2_wire::parse_json(&serde_json::from_str(&text).unwrap()).unwrap_err();
        assert!(json_err.is_unsupported());
        assert_eq!(py_err_message(py, &err), json_err.to_string());
    });
}

/// The whole point of the split: the same document reaches the same
/// `CombatState` whether it arrived as a Python dict or as JSON text.
#[test]
fn both_dialects_rebuild_the_same_combat_state() {
    let text = fixture_json();

    pyo3::prepare_freethreaded_python();
    let via_python = Python::with_gil(|py| {
        let d = fixture_pydict(py, &text);
        let state = sts2_rs::state_dict::state_from_dict(&d).expect("python rebuild");
        format!("{state:?}")
    });
    let via_json = format!(
        "{:?}",
        sts2_wire::state_from_json_str(&text).expect("json rebuild")
    );

    assert_eq!(via_python, via_json);
}
