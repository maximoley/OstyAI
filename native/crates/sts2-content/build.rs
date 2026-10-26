//! Stamp a content hash of this crate's own source into `STS2_CONTENT_SRC_HASH`,
//! exposed as `sts2_content::SRC_HASH`. Part of the content-addressed
//! engine-identity fix (issue #15, bead `sts2-rs-cg5ci`); see sts2-core/build.rs
//! for the rationale. No `rerun-if-changed` is emitted, so cargo's default —
//! re-run this script whenever any file in the package changes — keeps the stamp
//! fresh without per-file directives.

fn main() {
    let crate_dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo"),
    );
    let hash = sts2_srchash::hash_crate(&crate_dir, &[]);
    println!("cargo:rustc-env=STS2_CONTENT_SRC_HASH={hash}");
}
