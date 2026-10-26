//! Smoke tests for the cliext C4 potion-seeding surface (bead
//! `sts2-rs-bzgs`). Drives the binary end-to-end via stdin so the
//! CLI surface, scenario resolver, parse extension, and engine
//! dispatch are all exercised together.
//!
//! Note on HP-drop expectation (acceptance deviation): the original
//! bead spec calls for "seed `FirePotion`, use on slot-0 enemy, assert
//! HP drop matches potion spec". At pin `01c094d` the potion-on-drink
//! interpreter (P7-8 umbrella `sts2-rs-h9u`) is not yet wired —
//! `dispatch::resolved_marker` returns `Resolved` without applying
//! damage. The smoke test below substitutes a weaker but verifiable
//! claim: the `potion <slot> <target>` verb parses, dispatches, and
//! the engine accepts the action (`Advanced` rather than
//! `InvalidTarget`).

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

fn run_with_stdin(args: &[&str], stdin_text: &str) -> (i32, String) {
    use std::io::Write;
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
        .expect("stdin")
        .write_all(stdin_text.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait sts2-cli");
    let mut combined = String::from_utf8_lossy(&out.stdout).to_string();
    combined.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), combined)
}

#[test]
fn banner_lists_inline_potions() {
    let (code, out) = run_with_stdin(
        &[
            "--potion",
            "FirePotion,BloodPotion",
            "--agent",
            "human",
            "--no-prompt",
            "--encounter",
            "nibbit",
        ],
        "quit\n",
    );
    assert_eq!(code, 0, "exit code; out=\n{out}");
    assert!(
        out.contains("potions=[FirePotion, BloodPotion]"),
        "banner missing potion list; out=\n{out}"
    );
}

#[test]
fn title_case_resolves_to_pascal() {
    let (code, out) = run_with_stdin(
        &[
            "--potion",
            "Fire Potion",
            "--agent",
            "human",
            "--no-prompt",
            "--encounter",
            "nibbit",
        ],
        "quit\n",
    );
    assert_eq!(code, 0);
    assert!(out.contains("potions=[FirePotion]"), "out=\n{out}");
}

#[test]
fn unknown_potion_exits_one_with_suggestion() {
    let (code, out) = run_with_stdin(&["--potion", "FierPotion", "--agent", "random"], "");
    assert_eq!(code, 1, "exit code; out=\n{out}");
    assert!(
        out.contains("did you mean 'FirePotion'"),
        "missing did-you-mean; out=\n{out}"
    );
}

#[test]
fn unknown_potion_exits_one() {
    let (code, out) = run_with_stdin(&["--potion", "UnknownPotion", "--agent", "random"], "");
    assert_eq!(code, 1, "exit code; out=\n{out}");
    assert!(out.contains("unknown potion"), "out=\n{out}");
}

#[test]
fn random_3_preset_deterministic_per_seed() {
    let (code_a, out_a) = run_with_stdin(
        &[
            "--potion-preset",
            "random-3",
            "--seed",
            "42",
            "--agent",
            "human",
            "--no-prompt",
        ],
        "quit\n",
    );
    let (code_b, out_b) = run_with_stdin(
        &[
            "--potion-preset",
            "random-3",
            "--seed",
            "42",
            "--agent",
            "human",
            "--no-prompt",
        ],
        "quit\n",
    );
    assert_eq!(code_a, 0);
    assert_eq!(code_b, 0);
    let banner_a = out_a.lines().next().unwrap_or("");
    let banner_b = out_b.lines().next().unwrap_or("");
    assert_eq!(banner_a, banner_b, "same seed must produce same draw");
    assert!(
        banner_a.matches(',').count() == 2,
        "expected 3 entries in banner; got: {banner_a}"
    );
}

#[test]
fn potion_verb_advances_engine() {
    // Drive: seed FirePotion, then `potion 0 0` (slot 0 -> enemy 0).
    // Acceptance (deviation noted in module docstring): asserts the
    // verb parses + dispatches via the engine to `Advanced` rather
    // than `InvalidTarget`. The actual HP-drop body lands with
    // `sts2-rs-h9u` (P7-8 on-drink interpreter).
    let (code, out) = run_with_stdin(
        &[
            "--potion",
            "FirePotion",
            "--agent",
            "human",
            "--no-prompt",
            "--echo-input",
            "--encounter",
            "nibbit",
        ],
        "potion 0 0\nquit\n",
    );
    assert_eq!(code, 0, "exit code; out=\n{out}");
    assert!(
        out.contains("<< potion 0 0"),
        "verb not echoed; out=\n{out}"
    );
    assert!(
        out.contains("-> Advanced"),
        "engine did not advance on potion verb; out=\n{out}"
    );
    // Negative: no InvalidTarget surfaced.
    assert!(
        !out.contains("invalid target"),
        "unexpected InvalidTarget; out=\n{out}"
    );
}
