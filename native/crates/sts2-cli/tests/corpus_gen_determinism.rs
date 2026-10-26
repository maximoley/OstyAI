//! Acceptance test for `sts2-rs-3b0p`: same `--seed N --n-positions K`
//! must produce byte-identical NPZ output across worker counts.
//!
//! Without this, the rayon parallelization in `sts2-corpus-gen` would
//! silently introduce per-thread RNG drift into the Texel corpus and
//! make downstream weight-fitting results non-reproducible. The
//! parallel driver achieves cross-`--workers`-N determinism by
//! seeding each combat solely from its `combat_idx` and merging
//! per-worker collectors in `combat_idx` order before truncating to
//! exactly `--n-positions` rows.
//!
//! The test is gated by `--release` because the debug build of
//! `sts2-corpus-gen` is ~30× slower per MCTS iteration; even at the
//! small budgets used here a debug run can take >60s.

use std::path::PathBuf;
use std::process::Command;

fn binary_path() -> PathBuf {
    // CARGO_BIN_EXE_<name> is populated by cargo at test-build time
    // when the binary lives in the same package as the integration
    // test. `sts2-corpus-gen` is one of `sts2-cli`'s bin targets.
    if let Some(p) = option_env!("CARGO_BIN_EXE_sts2-corpus-gen") {
        return PathBuf::from(p);
    }
    // Fallback: assume `cargo build --release -p sts2-cli --bin
    // sts2-corpus-gen` ran beforehand. Required for `cargo test
    // --release` invocations that skip the test binary's bin-dep
    // discovery (rare, but happens under cross-package test runs).
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target/release/sts2-corpus-gen")
}

fn run_corpus_gen(workers: usize, output: &PathBuf) {
    let bin = binary_path();
    assert!(
        bin.exists(),
        "corpus-gen binary not found at {} — run `cargo build --release -p sts2-cli --bin sts2-corpus-gen` first",
        bin.display(),
    );
    let status = Command::new(&bin)
        // Small budgets keep the test under ~15s even at low worker
        // counts. The parallelism behavior we're exercising is the
        // same as at production scale; we just need enough combats to
        // span multiple batches.
        .args([
            "--n-positions",
            "150",
            "--sims-per-move",
            "30",
            "--max-rollout-depth",
            "5",
            "--seed",
            "1234",
            "--workers",
            &workers.to_string(),
            "--output",
        ])
        .arg(output)
        .status()
        .expect("spawn sts2-corpus-gen");
    assert!(
        status.success(),
        "sts2-corpus-gen --workers {workers} exited with {status}",
    );
}

#[test]
fn slow_workers_1_and_8_produce_byte_identical_npz_for_same_seed() {
    // Use unique tempfile names so this test doesn't collide with a
    // parallel invocation of itself in CI (cargo test parallelism).
    let tmpdir = std::env::temp_dir();
    let out_j1 = tmpdir.join("sts2_corpus_3b0p_j1.npz");
    let out_j8 = tmpdir.join("sts2_corpus_3b0p_j8.npz");

    run_corpus_gen(1, &out_j1);
    run_corpus_gen(8, &out_j8);

    let bytes_j1 = std::fs::read(&out_j1).expect("read j1 NPZ");
    let bytes_j8 = std::fs::read(&out_j8).expect("read j8 NPZ");

    assert_eq!(
        bytes_j1.len(),
        bytes_j8.len(),
        "NPZ size mismatch — parallel run produced {} bytes vs serial {} bytes",
        bytes_j8.len(),
        bytes_j1.len(),
    );
    // Byte-by-byte equality is the bead acceptance criterion.
    assert!(
        bytes_j1 == bytes_j8,
        "NPZ bytes differ between --workers 1 and --workers 8 at the same --seed — \
         parallel corpus generation is non-deterministic across worker counts",
    );

    // Best-effort cleanup; ignore errors (tempdir might already be
    // gone if a sibling test cleared it).
    let _ = std::fs::remove_file(&out_j1);
    let _ = std::fs::remove_file(&out_j8);
}
