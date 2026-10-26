//! Integration test: the `sts2-srchash` CLI prints exactly
//! `recompute_engine_hash(rs_root)` — the SAME aggregate the wheel bakes into
//! `src_hash()`. wheel-venv.sh uses this CLI to content-address each worktree's
//! venv as `.venv-h/<H>` from SOURCE alone, before any wheel exists (the
//! chicken-and-egg of issue #15 / bead sts2-rs-cg5ci, Phase 2). If the CLI ever
//! drifts from the library hasher the venv name drifts from `src_hash()` and
//! content-addressing silently breaks; this test is the guard.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static CTR: AtomicUsize = AtomicUsize::new(0);

/// A minimal fake sts2-rs checkout root the library hasher accepts: the four
/// engine crate dirs (each with `src/` + `Cargo.toml`), the py `include_bytes!`
/// data file, `Cargo.lock`, and `rust-toolchain.toml`. Mirrors the lib's own
/// test fixture so the CLI is checked against the same shape it sees in anger.
fn tmp_rs_root() -> PathBuf {
    let n = CTR.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("sts2srchash_cli_{}_{n}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for c in sts2_srchash::ENGINE_CRATES {
        let cd = root.join("crates").join(c);
        fs::create_dir_all(cd.join("src")).unwrap();
        fs::write(cd.join("Cargo.toml"), b"[package]\n").unwrap();
        fs::write(cd.join("src/lib.rs"), b"// x").unwrap();
    }
    fs::write(root.join("Cargo.lock"), b"lock-v1").unwrap();
    fs::write(
        root.join("rust-toolchain.toml"),
        b"[toolchain]\nchannel=\"1.95\"\n",
    )
    .unwrap();
    root
}

/// Absolute path to the freshly-built CLI. Cargo sets `CARGO_BIN_EXE_<name>`
/// for integration tests of a package with a binary target — so this fails to
/// compile (loudly) if the `sts2-srchash` bin is ever dropped.
fn cli() -> &'static str {
    env!("CARGO_BIN_EXE_sts2-srchash")
}

#[test]
fn cli_prints_recompute_engine_hash() {
    let root = tmp_rs_root();
    let out = Command::new(cli())
        .arg(&root)
        .output()
        .expect("failed to run sts2-srchash");
    assert!(
        out.status.success(),
        "CLI exited non-zero: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed = String::from_utf8(out.stdout).unwrap();
    let printed = printed.trim();
    let expected = sts2_srchash::recompute_engine_hash(&root);
    assert_eq!(
        printed, expected,
        "CLI output must equal the library recompute (one hasher, no drift)"
    );
    assert_eq!(printed.len(), 16, "hash must be 16 hex chars");
}

#[test]
fn cli_errors_without_rs_root_arg() {
    let out = Command::new(cli())
        .output()
        .expect("failed to run sts2-srchash");
    assert!(
        !out.status.success(),
        "CLI must exit non-zero when given no rs_root argument"
    );
}
