//! Export the versioned dense feature layout used by the value/surrogate nets.
use sts2_runai::eval::ranker_features::{
    boss_kind_table, card_index_table, context_dim, feature_schema_id, relic_index_table,
    SCALAR_NAMES,
};
fn main() {
    let value = serde_json::json!({
        "schema_id": feature_schema_id(),
        "context_dim": context_dim(),
        "cards": card_index_table().iter().map(|v| format!("{v:?}")).collect::<Vec<_>>(),
        "relics": relic_index_table().iter().map(|v| format!("{v:?}")).collect::<Vec<_>>(),
        "scalars": SCALAR_NAMES,
        "bosses": boss_kind_table().iter().map(|v| format!("{v:?}")).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&value).unwrap());
}
