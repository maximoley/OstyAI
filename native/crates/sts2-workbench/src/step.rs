//! `_live_step`: wrap a bare live state in the Step-shaped, enriched dict
//! the frontend consumes (`sts2-pycli/python/workbench/server.py`).

use serde_json::{json, Value};

use crate::enrich::{enrich, EnrichCtx};
use crate::options::normalize_options;

/// Wrap `state` in a `Step` (`idx 0`, `source: "live"`), enrich it, and
/// attach the normalized `options` for non-combat decisions.
#[must_use]
pub fn live_step(state: Value, ctx: &EnrichCtx<'_>) -> Value {
    let step = json!({
        "idx": 0,
        "decision": state.get("decision").cloned().unwrap_or(json!("unknown")),
        "state": state,
        "scores": [],
        "pv": Value::Null,
        "chosen_action": Value::Null,
        "source": "live",
        // `floor` / `act` must ride along: the docked minimap refetches the
        // act graph only when these change.
        "meta": {
            "turn": step_meta(&state_of(&state), "round"),
            "step_no": 0,
            "floor": step_meta(&state_of(&state), "floor"),
            "act": step_meta(&state_of(&state), "act"),
            "act_name": step_meta(&state_of(&state), "act_name"),
            "bg_seed": step_meta(&state_of(&state), "bg_seed"),
            "bg_row": step_meta(&state_of(&state), "bg_row"),
            "bg_col": step_meta(&state_of(&state), "bg_col"),
        },
    });
    // Python normalizes the RAW state (`_enrich` works on a deep copy), so a
    // card_select option's label is the card's unresolved loc key
    // (`ULTIMATE_STRIKE.title`), not the display name the enriched card
    // carries. Same here: options come from the state as serialized.
    let options = step.get("state").map(normalize_options).unwrap_or_default();
    let mut enriched = enrich(step, ctx);
    if let Some(st) = enriched.get_mut("state").and_then(Value::as_object_mut) {
        st.insert("options".into(), Value::Array(options));
    }
    enriched
}

fn state_of(state: &Value) -> Value {
    state.clone()
}

fn step_meta(state: &Value, key: &str) -> Value {
    state.get(key).cloned().unwrap_or(Value::Null)
}
