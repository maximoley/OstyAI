//! End-to-end test for the cliext F3 `--random-all` precedence rule
//! (bead `sts2-rs-c9s5`). Specifically guards the conflict-resolution
//! invariant that an explicit per-flag value beats `--random-all`,
//! which was the trickiest merge point of the F3 landing.

use std::path::PathBuf;
use std::process::Command;

fn binary_path() -> PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_sts2-cli") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target/debug/sts2-cli")
}

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(binary_path())
        .args(args)
        .output()
        .expect("spawn sts2-cli");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn random_all_is_reproducible_for_a_fixed_seed() {
    // Banner contains encounter alias + resolved deck + potions +
    // relics — if any random stream drifts the banners diverge.
    let (a_code, a_out) = run(&[
        "combat",
        "--seed",
        "424242",
        "--random-all",
        "--agent",
        "random",
        "--steps",
        "1",
    ]);
    let (b_code, b_out) = run(&[
        "combat",
        "--seed",
        "424242",
        "--random-all",
        "--agent",
        "random",
        "--steps",
        "1",
    ]);
    assert_eq!(a_code, 0, "first run failed: {a_out}");
    assert_eq!(b_code, 0, "second run failed: {b_out}");
    // Compare just the header lines (banner + deck + relics) — the
    // combat trace itself can diverge with different RNG fork order,
    // but the resolved scenario must be byte-identical.
    let header = |s: &str| {
        s.lines()
            .take_while(|l| !l.starts_with("── starting combat"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(header(&a_out), header(&b_out));
}

#[test]
fn explicit_deck_beats_random_all() {
    let (code, out) = run(&[
        "combat",
        "--seed",
        "7",
        "--random-all",
        "--deck",
        "Strike,Bash",
        "--agent",
        "random",
        "--steps",
        "1",
    ]);
    assert_eq!(code, 0, "run failed: {out}");
    // The deck-summary line must reflect the explicit 2-card deck,
    // not a 10-card random draw.
    let deck_line = out
        .lines()
        .find(|l| l.starts_with("deck ("))
        .expect("deck summary line absent from output");
    assert!(
        deck_line.starts_with("deck (2 cards):"),
        "expected explicit 2-card deck, got: {deck_line}"
    );
    assert!(
        deck_line.contains("STRIKE_IRONCLAD") && deck_line.contains("BASH"),
        "explicit deck tokens missing: {deck_line}"
    );
}

#[test]
fn explicit_encounter_beats_random_all() {
    let (code, out) = run(&[
        "combat",
        "--seed",
        "1",
        "--random-all",
        "--encounter",
        "nibbit",
        "--agent",
        "random",
        "--steps",
        "1",
    ]);
    assert_eq!(code, 0, "run failed: {out}");
    let banner = out.lines().next().unwrap_or("");
    assert!(
        banner.contains("encounter=nibbit"),
        "expected explicit encounter=nibbit in banner, got: {banner}"
    );
}
