//! Stamp a content hash of this crate's own source (including the
//! `src/vocab_pins/` files it bakes in) into `STS2_VOCAB_SRC_HASH`, exposed as
//! `sts2_vocab::SRC_HASH`. Part of the `sts2_rs` content identity
//! (`sts2_srchash::ENGINE_CRATES`); see sts2-core/build.rs for the rationale.

fn main() {
    sts2_srchash::emit_build_stamp("sts2-vocab", "STS2_VOCAB_SRC_HASH");
}
