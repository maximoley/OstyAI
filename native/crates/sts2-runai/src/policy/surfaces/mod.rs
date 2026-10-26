//! Per-surface decision logic for [`super::heuristic::HeuristicPolicy`]
//! (plan §2 architecture-per-surface; bead `sts2-rs-40ikw.5`).
//!
//! Each surface is a pure function over (`VisibleRunView`, `HeuristicV`)
//! so it unit-tests against synthetic views; the policy layer maps the
//! result back onto `RunAction` indices.

pub mod combat_choice;
pub mod events;
pub mod pathing;
pub mod picks;
pub mod rest;
pub mod shop;
