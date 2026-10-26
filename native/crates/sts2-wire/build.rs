//! Stamp a content hash of this crate's own source into `STS2_WIRE_SRC_HASH`,
//! exposed as `sts2_wire::SRC_HASH`. Part of the `sts2_rs` content identity
//! (`sts2_srchash::ENGINE_CRATES`); see sts2-core/build.rs for the rationale.

fn main() {
    sts2_srchash::emit_build_stamp("sts2-wire", "STS2_WIRE_SRC_HASH");
}
