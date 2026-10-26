//! Emit a deterministic JSON description of the `sts2_rs` hint
//! wire-format dict (`docs/sts2-hint-state-schema.json`).
//!
//! **Bead:** `sts2-rs-lmg6` (hint-T5).
//!
//! Usage:
//!
//! ```bash
//! cargo run -p sts2-py --example dump_state_schema > docs/sts2-hint-state-schema.json
//! ```
//!
//! The artifact is checked into git so the sts2-cli Python adapter can
//! validate its `TypedDict` mirror against the Rust source-of-truth at CI
//! time without needing a working `import sts2_rs`.
//!
//! Determinism contract (acceptance criteria):
//!
//! * Two invocations on the same toolchain produce byte-identical output.
//! * The schema embeds the current [`state_dict::WIRE_VERSION`] so a
//!   schema-vs-adapter drift is detected at parse time, not at runtime.
//!
//! Determinism is achieved by:
//!
//! * Using [`BTreeMap`] (not `HashMap`) for every object the schema
//!   builds, so key order is sorted lexicographically.
//! * Reading the registry name lists through
//!   [`state_dict::registry_names_for`], which is itself backed by a
//!   `BTreeSet`.
//! * Pulling the schema-level constants (required keys, valid phases,
//!   etc.) from `state_dict` directly — those are `&'static [&'static
//!   str]` with insertion-order semantics from the Rust source. They
//!   are deterministic by definition but we still emit them in the
//!   order the parser checks them, since that matches the
//!   documentation in the module-level rustdoc.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};
use sts2_rs::state_dict::{
    self, REQUIRED_CARDSPEC_KEYS, REQUIRED_DECK_KEYS, REQUIRED_ENCOUNTER_KEYS,
    REQUIRED_PLAYER_KEYS, REQUIRED_TOP_KEYS, VALID_CARD_FLAGS, VALID_CARD_KINDS, VALID_CLASSES,
    VALID_PHASES, VALID_SIDES, WIRE_VERSION,
};

fn str_slice_to_value(xs: &[&'static str]) -> Value {
    Value::Array(xs.iter().map(|s| Value::String((*s).to_string())).collect())
}

fn registry(kind: &str) -> Value {
    let names = state_dict::registry_names_for(kind)
        .unwrap_or_else(|err| panic!("registry_names_for({kind}) failed: {err}"));
    Value::Array(names.into_iter().map(Value::String).collect())
}

fn btree_to_json_object(map: BTreeMap<&'static str, Value>) -> Value {
    // serde_json's `Map` preserves insertion order; feed it sorted keys
    // by iterating the BTreeMap so output is lexicographic regardless of
    // build flags.
    let mut out = Map::new();
    for (k, v) in map {
        out.insert(k.to_string(), v);
    }
    Value::Object(out)
}

fn build_schema() -> Value {
    // Top-level schema: each entry is a field of the wire dict. We use
    // BTreeMap keyed by &'static str so the resulting JSON object has
    // sorted keys; the lists inside preserve source order on purpose
    // (REQUIRED_* and VALID_* are documented in that order).
    let mut top: BTreeMap<&'static str, Value> = BTreeMap::new();

    top.insert("wire_version", json!(WIRE_VERSION));

    // Required keys on each shape the parser validates.
    let mut required_keys: BTreeMap<&'static str, Value> = BTreeMap::new();
    required_keys.insert("top", str_slice_to_value(REQUIRED_TOP_KEYS));
    required_keys.insert("player", str_slice_to_value(REQUIRED_PLAYER_KEYS));
    required_keys.insert("deck", str_slice_to_value(REQUIRED_DECK_KEYS));
    required_keys.insert("cardspec", str_slice_to_value(REQUIRED_CARDSPEC_KEYS));
    required_keys.insert(
        "encounter_entry",
        str_slice_to_value(REQUIRED_ENCOUNTER_KEYS),
    );
    top.insert("required_keys", btree_to_json_object(required_keys));

    // Closed-set enumerations the parser checks against.
    let mut enums: BTreeMap<&'static str, Value> = BTreeMap::new();
    enums.insert("phase", str_slice_to_value(VALID_PHASES));
    enums.insert("side", str_slice_to_value(VALID_SIDES));
    enums.insert("class", str_slice_to_value(VALID_CLASSES));
    enums.insert("card_kind", str_slice_to_value(VALID_CARD_KINDS));
    enums.insert("card_flag", str_slice_to_value(VALID_CARD_FLAGS));
    top.insert("enums", btree_to_json_object(enums));

    // Registry name tables (sorted, deduplicated). Adapter-side
    // TypedDict literals are validated against these lists at import
    // time in the Python sts2_hint package.
    let mut registries: BTreeMap<&'static str, Value> = BTreeMap::new();
    registries.insert("card", registry("card"));
    registries.insert("monster", registry("monster"));
    registries.insert("orb", registry("orb"));
    registries.insert("potion", registry("potion"));
    registries.insert("power", registry("power"));
    registries.insert("relic", registry("relic"));
    top.insert("registries", btree_to_json_object(registries));

    btree_to_json_object(top)
}

fn main() {
    let schema = build_schema();
    // `to_string_pretty` is deterministic on a serde_json::Value whose
    // objects use sorted (BTreeMap-backed) iteration. We emit a single
    // trailing newline so the committed file plays nicely with POSIX
    // tooling and git diffs.
    let mut out = serde_json::to_string_pretty(&schema)
        .expect("schema serialises (no non-string keys, no NaN floats)");
    out.push('\n');
    print!("{out}");
}
