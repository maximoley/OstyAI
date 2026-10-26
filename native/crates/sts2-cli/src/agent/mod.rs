//! Player-decision interface. Both the human REPL (`HumanAgent`) and the
//! `RandomAgent` (bead sts2-rs-ogw) produce the same `AgentChoice` so the
//! simulator loop is agent-agnostic.

use sts2_core::{Action, CombatState};

pub mod human;
pub mod mcts;
pub mod random;

/// What an [`Agent`] returns when the simulator asks for a decision.
#[derive(Debug, Clone)]
pub enum AgentChoice {
    /// Apply this action to the engine.
    Action(Action),
    /// Human typed `quit` (or stdin closed). Exit the REPL.
    QuitRepl,
    /// Non-action command (e.g. `hand`, `state`, `help`) was handled
    /// in-band; loop again without applying anything. Currently no
    /// `Agent` produces this — kept because the dispatch loop in
    /// `sim/mod.rs` matches against it and removing the arm + variant
    /// in lockstep is in `sts2-rs-na9oh` scope only if the variant has
    /// no consumers.
    #[allow(dead_code)]
    Continue,
    /// Scripted assertion failed. Set `ctx.failed_test = true`.
    AssertFailed(String),
}

/// Player-decision interface.
pub trait Agent {
    /// Called when the engine is awaiting a player decision.
    /// May write progress / prompts / errors to `out`.
    fn next_action<W: std::io::Write>(
        &mut self,
        state: &CombatState,
        out: &mut W,
    ) -> std::io::Result<AgentChoice>;
}
