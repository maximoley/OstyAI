//! Issue #15 / bead `sts2-rs-cg5ci`: the build-time `SRC_HASH` stamp must be a
//! 16-char hex digest AND equal a recompute over the on-disk source (clean
//! tree). That equality is the content-identity invariant the parity gate
//! relies on — `assert_wheel_fresh()` will compare the loaded `.so`'s stamp to
//! exactly this recompute. If a build.rs ever hashes a different input set than
//! the recompute, this test fails before it can mislead the harness.

#[test]
fn src_hash_is_hex_and_matches_recompute() {
    let stamped = sts2_core::SRC_HASH;
    assert_eq!(
        stamped.len(),
        16,
        "SRC_HASH must be a 16-char FNV-1a hex digest, got {stamped:?}"
    );
    assert!(
        stamped.bytes().all(|b| b.is_ascii_hexdigit()),
        "SRC_HASH must be lowercase hex, got {stamped:?}"
    );

    let crate_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let recomputed = sts2_srchash::hash_crate(&crate_dir, &[]);
    assert_eq!(
        stamped, recomputed,
        "build-time stamp must equal a recompute over the current source (clean tree)"
    );
}
