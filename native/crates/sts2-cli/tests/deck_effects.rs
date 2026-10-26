//! Integration tests for cliext deck-preset effects — bead
//! `sts2-rs-ymuj` (F1).
//!
//! Validates that non-starter cards loaded via `--deck` or
//! `--deck-preset` actually fire their `PlayCardFn`. Pre-dates this
//! test the CLI installed only the core `card_effects` registry
//! (Strike/Defend/Bash/AscendersBane); per-character pools were never
//! installed, so cards like Anger / `PommelStrike` / `TwinStrike` fell
//! through to `NotImplemented` and played as no-ops. Fix: commit
//! `e1b23f0` swapped `sts2_core::card_effects::ensure_installed()`
//! for `sts2_content::cards::dispatch::ensure_installed()` in
//! `scenario::starter::build_state`.
//!
//! Tests here invoke the CLI binary end-to-end with a deterministic
//! seed, scripted stdin (human agent), and parse stdout for the
//! monster HP line to assert effect-firing.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn binary_path() -> PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_sts2-cli") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target/debug/sts2-cli")
}

/// Run the CLI with a scripted stdin and return full stdout.
fn run(args: &[&str], stdin: &str) -> String {
    let mut child = Command::new(binary_path())
        .arg("combat")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sts2-cli");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait_with_output");
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Parse the first monster's `HP X/Y` from a combat-state print and
/// return X (current HP). Returns `None` if not found.
fn first_monster_hp(state_print: &str) -> Option<i32> {
    for line in state_print.lines() {
        let line = line.trim_start();
        // Format: "slot 0: Cultist      HP 50/50 block 0 …"
        if !line.starts_with("slot 0: ") {
            continue;
        }
        let hp_idx = line.find("HP ")?;
        let after = &line[hp_idx + 3..];
        let slash = after.find('/')?;
        return after[..slash].trim().parse::<i32>().ok();
    }
    None
}

/// All current-HP readings the binary printed across the run (one per
/// combat-state snapshot). Lets us look at first and last samples.
fn all_monster_hps(out: &str) -> Vec<i32> {
    out.split("combat state")
        .filter_map(first_monster_hp)
        .collect()
}

/// Generic helper: place ONE card in the hand by seeding a tiny deck
/// containing just `card_name`, drive a scripted turn that plays it on
/// enemy 0, and return (`initial_hp`, `final_hp`) of that enemy.
fn play_once(card_name: &str, encounter: &str, seed: &str) -> (i32, i32) {
    // Deck of exactly one card → start_combat draws it into hand[0].
    let deck_arg = card_name;
    let stdin = "state\nplay 0 0\nstate\nquit\n";
    let out = run(
        &[
            "--seed",
            seed,
            "--encounter",
            encounter,
            "--deck",
            deck_arg,
            "--agent",
            "human",
            "--no-prompt",
            "--echo-input",
        ],
        stdin,
    );
    let hps = all_monster_hps(&out);
    assert!(
        hps.len() >= 2,
        "expected ≥2 monster-HP samples in stdout for {card_name}, got {} (full out:\n{out})",
        hps.len()
    );
    (hps[0], *hps.last().unwrap())
}

#[test]
fn anger_deals_damage() {
    // Anger: 6 damage to target enemy. Nibbit starting HP 45 fixed
    // (STS2-native pick after bead `sts2-rs-ar9u` removed the STS1
    // cultist alias).
    let (before, after) = play_once("Anger", "nibbit", "7");
    assert!(
        after < before,
        "Anger should reduce enemy HP: before={before} after={after}"
    );
    let dmg = before - after;
    assert!(
        (4..=8).contains(&dmg),
        "Anger damage out of expected range: {dmg} (before={before} after={after})"
    );
}

#[test]
fn pommel_strike_deals_damage() {
    // PommelStrike: 9 damage + draw 1.
    let (before, after) = play_once("PommelStrike", "nibbit", "11");
    let dmg = before - after;
    assert!(
        (7..=11).contains(&dmg),
        "PommelStrike damage out of expected range: {dmg}"
    );
}

#[test]
fn iron_wave_deals_damage_and_blocks() {
    // IronWave: 5 damage + 5 block. We can only check damage here
    // (player block read requires a different parse path).
    let (before, after) = play_once("IronWave", "nibbit", "13");
    let dmg = before - after;
    assert!(
        (3..=7).contains(&dmg),
        "IronWave damage out of expected range: {dmg}"
    );
}

#[test]
fn twin_strike_deals_double_damage() {
    // TwinStrike: 5 damage twice (10 total).
    let (before, after) = play_once("TwinStrike", "nibbit", "17");
    let dmg = before - after;
    assert!(
        (8..=12).contains(&dmg),
        "TwinStrike should hit twice (~10 dmg), got: {dmg}"
    );
}

#[test]
fn upgraded_card_marker_parses() {
    // Smoke: --deck Strike+ should not error and should run combat.
    let out = run(
        &[
            "--seed",
            "1",
            "--encounter",
            "nibbit",
            "--deck",
            "Strike+",
            "--agent",
            "random",
            "--steps",
            "3",
            "--no-prompt",
        ],
        "",
    );
    assert!(
        out.contains("STRIKE_IRONCLAD"),
        "expected upgraded Strike in deck banner; got:\n{out}"
    );
}

#[test]
fn unknown_deck_card_errors_with_suggestion() {
    let child = Command::new(binary_path())
        .arg("combat")
        .args([
            "--seed",
            "1",
            "--encounter",
            "nibbit",
            "--deck",
            "StrikeIornclad", // typo
            "--agent",
            "random",
            "--steps",
            "1",
            "--no-prompt",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("did you mean")
            || stderr.to_lowercase().contains("strike")
            || stderr.contains("unknown"),
        "expected did-you-mean / suggestion in stderr; got:\n{stderr}"
    );
}
