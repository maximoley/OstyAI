//! Per-screen handler placeholder. v1 inlines the combat loop directly
//! into `ConsoleSimulator::play` (combat is the only screen). This module
//! exists as the future home for combat-specific helpers (e.g. legal-
//! action enumeration shared between human and random agents) so that
//! `mod.rs` stays focused on top-level dispatch.

#![allow(dead_code)]

use sts2_core::{Action, CombatState};

/// Enumerate every action the engine would currently accept. Used by the
/// `actions` REPL command and by `RandomAgent`. Delegates to
/// [`sts2_core::legal_actions`] so the CLI's dispatchable menu stays in
/// lock-step with what MCTS sees (notably: potions are included).
pub fn legal_actions(state: &CombatState) -> Vec<Action> {
    sts2_core::legal_actions(state).into_vec()
}
