//! Scripted-replay golden tests. Feed a fixture script through the
//! sts2-cli binary's stdin and diff stdout (whitespace-normalized)
//! against a `.golden` file alongside the script.
//!
//! Re-bless goldens by running with the env var `BLESS_GOLDEN=1`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn binary_path() -> PathBuf {
    // CARGO_BIN_EXE_<name> is set by cargo when running integration tests.
    // Fallback to target/debug if not set (manual `cargo test --test`).
    if let Some(p) = option_env!("CARGO_BIN_EXE_sts2-cli") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target/debug/sts2-cli")
}

fn normalize(s: &str) -> String {
    s.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
        + "\n"
}

fn run_script(script: &Path, args: &[&str]) -> String {
    let stdin_data = std::fs::read_to_string(script).expect("script readable");
    let mut child = Command::new(binary_path())
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
        .write_all(stdin_data.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait_with_output");
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn assert_golden(name: &str, args: &[&str]) {
    let dir = fixtures_dir();
    let script = dir.join(format!("{name}.txt"));
    let golden = dir.join(format!("{name}.golden"));
    let actual = normalize(&run_script(&script, args));

    if std::env::var("BLESS_GOLDEN").is_ok() {
        std::fs::write(&golden, &actual).expect("write golden");
        eprintln!("blessed: {}", golden.display());
        return;
    }

    let expected = if let Ok(s) = std::fs::read_to_string(&golden) {
        normalize(&s)
    } else {
        // First-run convenience: write the golden if missing.
        std::fs::write(&golden, &actual).expect("write missing golden");
        eprintln!(
            "wrote initial golden at {} — re-run to verify",
            golden.display()
        );
        return;
    };

    if actual != expected {
        let diff_path = std::env::temp_dir().join(format!("sts2-cli-{name}.actual"));
        std::fs::write(&diff_path, &actual).ok();
        panic!(
            "golden mismatch for {name}\n  expected: {} ({} bytes)\n  actual:   {} ({} bytes)\n  re-bless via BLESS_GOLDEN=1",
            golden.display(),
            expected.len(),
            diff_path.display(),
            actual.len(),
        );
    }
}

#[test]
fn human_basic_strike_defend_seed0() {
    // Bead `sts2-rs-ar9u` migrated this fixture from the (removed)
    // STS1 `cultist` alias to `nibbit` (the lone STS2-native single
    // legacy alias). Regenerate the golden after migration via
    // `BLESS_GOLDEN=1 cargo test -p sts2-cli --test scripted_replay
    // human_basic_strike_defend_seed0`.
    assert_golden(
        "basic_strike_defend",
        &[
            "combat",
            "--seed",
            "0",
            "--no-prompt",
            "--echo-input",
            "--encounter",
            "nibbit",
        ],
    );
}

#[test]
fn random_seed42_30steps_nibbit() {
    // Bead `sts2-rs-ar9u`: cultist alias removed; this fixture now
    // runs against `--encounter nibbit`.
    assert_golden(
        "random_seed42",
        &[
            "combat",
            "--agent",
            "random",
            "--seed",
            "42",
            "--steps",
            "30",
            "--encounter",
            "nibbit",
        ],
    );
}

#[test]
fn human_nibbit_first_turn_bare_int_and_card_alias() {
    // Default encounter is Nibbit (lightspeed-parity demo). Exercises:
    //   * BUTT(12) intent priming via creature.ai (sts2-core combat_loop).
    //   * `card 0` → alias for `info 0`.
    //   * Bare-int dispatch (`0`, `1`, `e`) against the auto-prompt menu.
    assert_golden(
        "nibbit_first_turn",
        &["combat", "--seed", "0", "--no-prompt", "--echo-input"],
    );
}
