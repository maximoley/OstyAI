//! Stamp a content hash of this crate's build inputs into
//! `STS2_RUNAI_SRC_HASH`, exposed as `sts2_runai::SRC_HASH`: its `src`, its
//! `Cargo.toml` and the HeuristicV weights it bakes in from outside the package
//! (`data/runai/heuristic_v.toml`). Part of the `sts2_rs` content identity
//! (`sts2_srchash::ENGINE_CRATES`); see sts2-core/build.rs for the rationale.

fn main() {
    sts2_srchash::emit_build_stamp("sts2-runai", "STS2_RUNAI_SRC_HASH");
}
