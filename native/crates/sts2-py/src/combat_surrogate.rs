//! `CombatSurrogate` — a PyO3 handle on the combat surrogate net, plus the
//! fast-forward seam the PPO screening env drives it through.
//!
//! Why this exists: a PPO run-policy campaign spends ~87% of its wall clock
//! in collection, and collection is dominated by the combat MCTS (1000 sims
//! per decision, 4000 at a boss). The surrogate replaces the *playing* of a
//! fight with one forward pass — the rewards still come out of the deployed
//! bridge, because [`RunManager::resolve_combat_fast`] stamps the fight
//! terminal and then runs the ordinary `exit_combat_and_transition`.
//!
//! This is a **screening** backend. See `docs/ppo-screen-playbook.md`: the
//! measured surrogate barely beat a per-`(encounter, ascension)` lookup table,
//! so before believing any surrogate-world result about a deck-shaped
//! question, run the deck-sensitivity gate. A run policy trained against it
//! also optimizes against its errors.
//!
//! The fast-forward body lives in `sts2_runai::planning::fast_forward_combat`,
//! shared with the outer-loop planner — the survival draw's seeding is what
//! makes a fast-forwarded fight reproducible per `(seed, node)`, and a second
//! copy of it here would drift silently.

use std::path::Path;
use std::sync::Arc;

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use sts2_core::run_manager::Phase;
use sts2_runai::eval::surrogate::SurrogateNet;
use sts2_runai::planning::fast_forward_combat;

/// A loaded combat surrogate net.
///
/// Construction reads and validates the weights JSON once; clone the handle
/// (it is `Arc`-backed) rather than reloading per run. In the PPO env each
/// worker process builds exactly one.
#[pyclass(name = "CombatSurrogate", module = "sts2_rs")]
pub struct PyCombatSurrogate {
    net: Arc<SurrogateNet>,
}

impl std::fmt::Debug for PyCombatSurrogate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `SurrogateNet` is a weights blob; print what identifies it.
        f.debug_struct("PyCombatSurrogate")
            .field("schema_id", &self.net.schema_id())
            .finish_non_exhaustive()
    }
}

#[pymethods]
impl PyCombatSurrogate {
    /// Load a surrogate net from its weights JSON (the artifact
    /// `train_combat_surrogate_net.py` exports and
    /// `eval_combat_surrogate_net.py` gates).
    #[new]
    fn new(path: &str) -> PyResult<Self> {
        let net = SurrogateNet::from_weights_path(Path::new(path))
            .map_err(|e| PyValueError::new_err(format!("{path}: failed to load surrogate: {e}")))?;
        Ok(Self { net: Arc::new(net) })
    }

    /// The net's schema id — the ranker-feature layout it was trained
    /// against. Worth logging next to a screen result: a surrogate harvested
    /// under a different content pin is not the same oracle.
    fn schema_id(&self) -> String {
        self.net.schema_id().to_string()
    }

    fn __repr__(&self) -> String {
        format!("CombatSurrogate(schema_id={})", self.net.schema_id())
    }
}

impl PyCombatSurrogate {
    pub(crate) fn net(&self) -> &SurrogateNet {
        &self.net
    }
}

/// Fast-forward the combat `rm` is currently in. Returns the surrogate's
/// prediction plus what the seeded survival draw made of it.
///
/// `outcome_base` seeds that draw. The PPO env passes a per-run value so two
/// arms of a paired screen see the same fight outcomes wherever they reach the
/// same fight — the surrogate world keeps the pairing the MCTS world has.
pub(crate) fn resolve_current_combat<'py>(
    py: Python<'py>,
    rm: &mut sts2_core::run_manager::RunManager,
    surrogate: &PyCombatSurrogate,
    outcome_base: u64,
) -> PyResult<Bound<'py, PyDict>> {
    if rm.phase() != Phase::Combat {
        return Err(PyRuntimeError::new_err(format!(
            "surrogate_resolve_combat requires phase=combat, got {:?}",
            rm.phase()
        )));
    }
    let hp_before = rm.run_state.current_hp;
    let outcome = fast_forward_combat(rm, surrogate.net(), outcome_base, 0);
    let d = PyDict::new(py);
    d.set_item("win_prob", outcome.win_prob)?;
    d.set_item("hp_frac_lost", outcome.hp_frac_lost)?;
    // The realized branch, which `win_prob` alone does not tell you: the draw
    // is Bernoulli, so a 0.97-win_prob fight still kills the run 3% of the
    // time and the env's own audit needs to see which happened.
    d.set_item(
        "survived",
        rm.phase() != Phase::RunOver || rm.run_state.player_won,
    )?;
    d.set_item("hp_before", hp_before)?;
    d.set_item("hp_after", rm.run_state.current_hp)?;
    Ok(d)
}
