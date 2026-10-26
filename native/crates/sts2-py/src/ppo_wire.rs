//! `PyO3` surface for the wire-driven PPO run policy.
//!
//! The advisor lane's non-combat decisions (map / card reward / shop / rest /
//! event / boss relic) used to be answered entirely in Python: build the
//! observation from the bridge dict in `realgame_ppo.py`, featurize it in
//! `ppo/encoder.py`, run it through `onnxruntime`. That works, but it is why
//! the shipped advisor bundle carries `numpy` + `onnxruntime` — ~54 MB of the
//! 165 MB staged mod, and the bulk of its cold start.
//!
//! Every stage of that pipeline now exists in Rust: [`ppo::wire`] builds the
//! observation from the same bridge dict, [`ppo::encoder`] featurizes it, and
//! [`ppo::net`] runs it on `tract`. This module is the door between them.
//!
//! Two entry points, deliberately:
//!
//! * [`ppo_wire_obs`] returns the observation and candidate list *without*
//!   scoring them. It exists for the differential harness — comparing what
//!   Rust builds against what `realgame_ppo._build_*` builds, row by row over
//!   recorded logs, is the only way to know the port is faithful.
//! * [`ppo_wire_decide`] runs the whole pipeline and returns the ranked
//!   result, which is what the advisor actually needs.
//!
//! Both follow the lane's never-crash contract: a surface with nothing to
//! decide, an unloadable net, or an inference failure returns `None` and the
//! caller falls back to its own policy for that one decision.

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use sts2_runai::ppo::policy::PpoPolicy;
use sts2_runai::ppo::wire::{self, WireDecision};

/// Convert a Python dict (the bridge `state` object) into `serde_json::Value`.
fn state_value(state: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    pythonize::depythonize(state).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("ppo_wire: state is not JSON-shaped: {e}"))
    })
}

fn json_to_py<'py>(py: Python<'py>, v: &serde_json::Value) -> PyResult<Bound<'py, PyAny>> {
    pythonize::pythonize(py, v)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("pythonize failed: {e}")))
}

/// The pieces of a [`WireDecision`] every entry point returns, as a dict.
fn decision_common<'py>(py: Python<'py>, d: &WireDecision) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    // The observation as the training-time dict, so a Python-side comparison
    // is a plain `==` against `realgame_ppo._base_obs`'s output rather than a
    // field-by-field transcription.
    let obs = serde_json::to_value(&d.obs).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("obs serialize failed: {e}"))
    })?;
    out.set_item("obs", json_to_py(py, &obs)?)?;

    let actions = PyList::empty(py);
    for a in &d.actions {
        let av = serde_json::to_value(a).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("action serialize failed: {e}"))
        })?;
        actions.append(json_to_py(py, &av)?)?;
    }
    out.set_item("actions", actions)?;

    let responses = PyList::empty(py);
    for r in &d.responses {
        responses.append(json_to_py(py, r)?)?;
    }
    out.set_item("responses", responses)?;
    out.set_item("labels", d.labels.clone())?;
    Ok(out)
}

/// Build the PPO observation + candidate list for one bridge decision.
///
/// `surface` is the bridge `state["type"]` (`map_select`, `card_reward`,
/// `boss_relic`, `rest_site`, `event`, `shop`). Returns `None` when that
/// surface has nothing to decide or is not one the net covers — the same
/// signal `_PPO_NONCOMBAT_BUILDERS.get(...)` returning `None` carries.
///
/// The returned dict has `obs` / `actions` / `responses` / `labels`, matching
/// `realgame_ppo.PpoDecision` minus its `metrics` (whose scalars live inside
/// `obs` on this side).
#[pyfunction]
pub fn ppo_wire_obs<'py>(
    py: Python<'py>,
    state: &Bound<'py, PyAny>,
    surface: &str,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let value = state_value(state)?;
    let Some(decision) = wire::build_decision(&value, surface) else {
        return Ok(None);
    };
    Ok(Some(decision_common(py, &decision)?))
}

/// Build the observation **and** score it with `character`'s champion net.
///
/// Adds to [`ppo_wire_obs`]'s dict: `probs` (one per candidate, same order),
/// `value` (the critic's estimate), and `argmax` (the first-max index, the
/// tie-break numpy uses and the training env's `logp.argmax()` matched).
///
/// Returns `None` when the surface has nothing to decide, when fewer than two
/// candidates survived (the choice is forced), or when inference fails.
/// Nothing here raises on a bad decision — the caller is driving a live game.
#[pyfunction]
pub fn ppo_wire_decide<'py>(
    py: Python<'py>,
    state: &Bound<'py, PyAny>,
    surface: &str,
    character: &str,
) -> PyResult<Option<Bound<'py, PyDict>>> {
    let value = state_value(state)?;
    let Some(decision) = wire::build_decision(&value, surface) else {
        return Ok(None);
    };
    let character_id = character_id_from_str(character)?;
    // Loading is per-call today. The champion nets are ~8 MB and `tract`
    // parses them on load, so a caller scoring many decisions should hold the
    // policy itself; wiring that cache is a follow-up, not a correctness
    // issue, and the advisor makes one non-combat decision every few seconds.
    let policy = PpoPolicy::from_env_for(character_id);
    let Some((probs, critic, argmax)) = policy.evaluate_wire(&decision) else {
        return Ok(None);
    };
    let out = decision_common(py, &decision)?;
    out.set_item("probs", probs)?;
    out.set_item("value", critic)?;
    out.set_item("argmax", argmax)?;
    Ok(Some(out))
}

/// The bridge/pycli character spelling → `CharacterId`.
///
/// Accepts both dialects the lane uses: the bridge's SCREAMING_CASE
/// (`"IRONCLAD"`) and the adapter's Title-Case (`"Ironclad"`).
fn character_id_from_str(raw: &str) -> PyResult<sts2_core::potions::CharacterId> {
    use sts2_core::potions::CharacterId;
    Ok(match raw.to_ascii_uppercase().as_str() {
        "IRONCLAD" => CharacterId::Ironclad,
        "SILENT" => CharacterId::Silent,
        "DEFECT" => CharacterId::Defect,
        "REGENT" => CharacterId::Regent,
        "NECROBINDER" => CharacterId::Necrobinder,
        other => {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "ppo_wire: unknown character {other:?} (expected one of \
                 IRONCLAD/SILENT/DEFECT/REGENT/NECROBINDER)"
            )));
        }
    })
}
