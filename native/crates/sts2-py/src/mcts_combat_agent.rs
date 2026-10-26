//! `MctsCombatAgent` — a net-free stepwise combat agent for PPO
//! net-play (GH #220).
//!
//! In PPO net-play mode (`ppo.env.PpoRunEnv` with `--policy net` /
//! `net_greedy` / `random`) the agent is consulted **only** for
//! `Phase::Combat` decisions: every multi-option non-combat surface is
//! yielded to the PPO net, and single-option surfaces auto-resolve. The
//! combat search itself is a heuristic-leaf MCTS (see
//! `sts2_runai::policy::MctsCombatPolicy::mcts_pick`).
//!
//! It wraps the `MctsCombatPolicy` combat search under a
//! [`HeuristicPolicy`] meta, so no net weights are needed. Because the
//! meta is only ever reached on a combat MCTS→meta fallback (never for a
//! yielded PPO decision), a heuristic meta keeps the fallback graceful
//! (first-option) instead of panicking.

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use sts2_core::full_run::RunPolicy;
use sts2_core::run_manager::Phase;
use sts2_mcts::RootOutcome;
use sts2_runai::policy::{CombatBudget, HeuristicPolicy, MctsCombatPolicy};

use crate::run_manager::{phase_str, run_action_to_pydict, PyRunManager};

/// Python-facing stepwise agent that plays a net-free
/// `MctsCombatPolicy<HeuristicPolicy>` on a live `RunManager`, emitting one
/// per-decision record per `step()` call, for PPO net-play (combat MCTS +
/// PPO meta).
///
/// Defaults reproduce `run_arena`'s combat search exactly:
/// `sims=1000, boss_sims=4000, threads=1` (eval-cheap).
#[pyclass(name = "MctsCombatAgent", module = "sts2_rs")]
#[derive(Debug)]
pub struct PyMctsCombatAgent {
    policy: MctsCombatPolicy<HeuristicPolicy>,
}

#[pymethods]
impl PyMctsCombatAgent {
    #[new]
    #[pyo3(signature = (seed = 0, sims = 1000, boss_sims = 4000, threads = 1))]
    fn new(seed: u64, sims: u32, boss_sims: u32, threads: usize) -> Self {
        // `threads` is reserved for future parallelism; eval-cheap is
        // single-threaded by design (throughput parallelises across seeds).
        // Kept in the signature so `MctsCombatAgent` is a drop-in for
        // `HeuristicRunAgent`.
        let _ = threads;

        let mut budget = CombatBudget::eval_cheap();
        budget.sims = sims;
        budget.boss_sims = boss_sims;

        let policy = MctsCombatPolicy::new(HeuristicPolicy::new(), budget, seed);
        Self { policy }
    }

    /// Apply one decision to `rm` and return a step record dict, or `None`
    /// if the policy has no legal action to take.
    ///
    /// Identical shape/semantics to `HeuristicRunAgent.step` — see that
    /// method's doc comment for the key list. The `combat_search` record is
    /// non-`None` only on a combat decision where the MCTS ran (i.e. no
    /// fallback to the meta).
    fn step<'py>(
        &mut self,
        py: Python<'py>,
        rm: &mut PyRunManager,
    ) -> PyResult<Option<Bound<'py, PyDict>>> {
        let actions = rm.inner.get_available_actions();
        if actions.is_empty() {
            return Ok(None);
        }

        let phase = rm.inner.phase();
        let is_combat = phase == Phase::Combat;

        let Some(idx) = self.policy.choose(&rm.inner, &actions) else {
            return Ok(None);
        };
        let action = actions[idx];

        // Capture the combat search record AFTER `choose` (which runs the
        // MCTS and caches it in `last_combat_search`) but BEFORE
        // `take_action` (which advances the state). Only meaningful when the
        // MCTS ran, i.e. `is_combat` AND the policy didn't fall back to the
        // meta — same rationale as `HeuristicRunAgent::step`.
        let combat_search: Option<Bound<'py, PyDict>> = if is_combat {
            self.policy
                .last_combat_search()
                .map(|rec| -> PyResult<Bound<'py, PyDict>> {
                    let d = PyDict::new(py);

                    // Build the per-action list. Compute the best index once
                    // so exactly one entry carries `is_best = true` even if
                    // the same action appears more than once in root_outcomes.
                    let best_idx = rec
                        .outcomes
                        .iter()
                        .position(|o| o.action == rec.chosen)
                        .unwrap_or(0);
                    let list = PyList::empty(py);
                    for (i, o) in rec.outcomes.iter().enumerate() {
                        let is_best = i == best_idx;
                        let entry = root_outcome_to_pydict(py, o, is_best)?;
                        list.append(entry)?;
                    }
                    d.set_item("actions", list)?;

                    // The recommended action (chosen by Q-then-visits).
                    d.set_item("recommended", crate::action_to_pydict(py, &rec.chosen)?)?;
                    Ok(d)
                })
                .transpose()?
        } else {
            None
        };

        // Apply the chosen action.
        rm.ensure_play_log_flag();
        let result = rm.inner.take_action(action);

        let over = rm.inner.is_over();
        let won = rm.inner.run_state.player_won;

        let out = PyDict::new(py);
        out.set_item("phase", phase_str(phase))?;
        out.set_item("is_combat", is_combat)?;
        out.set_item("over", over)?;
        out.set_item("won", won)?;
        out.set_item("action", run_action_to_pydict(py, &action)?)?;
        out.set_item("combat_search", combat_search)?;
        out.set_item("result", format!("{result:?}"))?;
        Ok(Some(out))
    }
}

/// Serialise one [`RootOutcome`] into the same per-action dict shape that
/// `mcts_recommend` emits, reusing `action_score_pydict` — byte-copy of
/// `heuristic_agent.rs`'s helper.
fn root_outcome_to_pydict<'py>(
    py: Python<'py>,
    o: &RootOutcome,
    is_best: bool,
) -> PyResult<Bound<'py, PyDict>> {
    crate::action_score_pydict(
        py,
        &o.action,
        o.visits,
        o.wins,
        o.losses,
        o.ties,
        o.value_sum,
        /* low_confidence */ false,
        /* q_saturated */ false,
        is_best,
    )
}
