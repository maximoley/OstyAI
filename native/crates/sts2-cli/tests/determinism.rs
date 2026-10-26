//! Long-trajectory determinism digest test.
//!
//! Locks the SHA-256 of the JSONL produced by:
//!   `sts2-cli combat --agent random --seed 0 --steps 1000 --jsonl <file>`
//!
//! against the digest stored in `tests/fixtures/random_seed0_steps1000.sha256`.
//! Catches RNG-replay drift across the full action surface.
//!
//! Regenerate the digest (see `tests/fixtures/README.md`):
//!   `cargo build -p sts2-cli --release`
//!   `./target/release/sts2-cli combat --agent random --seed 0 --steps 1000 --jsonl /tmp/seed0.jsonl`
//!   `sha256sum /tmp/seed0.jsonl | awk '{print $1}' \
//!       > crates/sts2-cli/tests/fixtures/random_seed0_steps1000.sha256`

use std::path::PathBuf;
use std::process::Command;

use sha2::{Digest, Sha256};

fn binary_path() -> PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_sts2-cli") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target/debug/sts2-cli")
}

#[test]
fn random_seed0_steps1000_jsonl_digest_matches() {
    let expected = include_str!("fixtures/random_seed0_steps1000.sha256")
        .trim()
        .to_string();

    let tmp = std::env::temp_dir().join("sts2-cli-determinism-seed0.jsonl");
    // Best-effort cleanup of any prior run.
    let _ = std::fs::remove_file(&tmp);

    let status = Command::new(binary_path())
        .args([
            "combat", "--agent", "random", "--seed", "0", "--steps", "1000", "--jsonl",
        ])
        .arg(&tmp)
        .status()
        .expect("spawn sts2-cli");
    assert!(status.success(), "sts2-cli exited non-zero: {status:?}");

    let bytes = std::fs::read(&tmp).expect("read jsonl output");
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual = hex(&hasher.finalize());

    assert_eq!(
        actual, expected,
        "JSONL determinism digest mismatch.\n  expected: {expected}\n  actual:   {actual}\n  \
         If the JSONL schema or starter content changed intentionally, regenerate \
         tests/fixtures/random_seed0_steps1000.sha256 (see fixtures/README.md)."
    );
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}
