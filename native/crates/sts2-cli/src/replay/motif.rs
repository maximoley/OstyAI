//! Position motifs for the combat decision-quality suite (GH #446).
//!
//! The module moved to [`sts2_mcts::motif`] so the live combat budget
//! (`sts2-runai`, which `sts2-cli` depends on — the reverse edge would be a
//! cycle) can condition the simulation budget on the same predicates the
//! position book is tagged with. This path stays as the historical import
//! for the bench binaries.

pub use sts2_mcts::motif::*;
