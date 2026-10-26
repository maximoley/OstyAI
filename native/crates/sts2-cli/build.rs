//! Stamp a content hash of this crate's own source into `STS2_CLI_SRC_HASH`,
//! exposed as `sts2_cli::SRC_HASH`. Part of the `sts2_rs` content identity
//! (`sts2_srchash::ENGINE_CRATES`: the wheel links this crate's scenario
//! loader); see sts2-core/build.rs for the rationale.

fn main() {
    sts2_srchash::emit_build_stamp("sts2-cli", "STS2_CLI_SRC_HASH");
}
