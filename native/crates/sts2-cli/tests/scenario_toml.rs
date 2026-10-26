//! End-to-end smoke tests for the cliext D1 TOML scenario surface
//! (bead `sts2-rs-g2ex`). Exercises:
//!
//! * Each of the three shipped demo files (`docs/scenarios/*.toml`)
//!   parses and reaches `── starting combat ──` cleanly.
//! * CLI flags override scenario-file values (precedence rule).
//! * Missing files exit 1 with a clear message (not a panic).
//! * `schema != 1` exits 1 with the expected diagnostic.

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

/// Path to the repo-root `docs/scenarios/` directory shipped alongside
/// this bead.
fn demo_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("scenarios")
}

fn write_tmp(name: &str, body: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("sts2-g2ex-{pid}-{ts}-{name}"));
    std::fs::File::create(&p)
        .unwrap()
        .write_all(body.as_bytes())
        .unwrap();
    p
}

fn run_combat(args: &[&str]) -> (i32, String) {
    let out = Command::new(binary_path())
        .arg("combat")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run sts2-cli");
    let mut combined = String::from_utf8_lossy(&out.stdout).to_string();
    combined.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), combined)
}

#[test]
fn demo_single_cultist_runs() {
    let path = demo_dir().join("single-cultist.toml");
    let (code, out) = run_combat(&[
        "--scenario",
        path.to_str().unwrap(),
        "--agent",
        "random",
        "--steps",
        "5",
    ]);
    assert_eq!(code, 0, "out=\n{out}");
    assert!(out.contains("starting combat"), "out=\n{out}");
    assert!(
        out.contains(&format!("scenario={}", path.display())),
        "banner should mention scenario path; out=\n{out}"
    );
}

#[test]
fn demo_double_sentry_runs() {
    let path = demo_dir().join("double-sentry.toml");
    let (code, out) = run_combat(&[
        "--scenario",
        path.to_str().unwrap(),
        "--agent",
        "random",
        "--steps",
        "20",
    ]);
    assert_eq!(code, 0, "out=\n{out}");
    assert!(out.contains("starting combat"), "out=\n{out}");
}

#[test]
fn demo_nibbit_with_burning_blood_runs() {
    let path = demo_dir().join("nibbit-with-burning-blood.toml");
    let (code, out) = run_combat(&[
        "--scenario",
        path.to_str().unwrap(),
        "--agent",
        "random",
        "--steps",
        "5",
    ]);
    assert_eq!(code, 0, "out=\n{out}");
    // Scenario seeds BurningBlood; banner should render it.
    assert!(
        out.contains("Burning Blood"),
        "relic banner missing; out=\n{out}"
    );
}

#[test]
fn cli_relic_overrides_scenario_relics() {
    // single-cultist.toml has no relics; we use a custom file that DOES
    // declare relics, then pass `--relic` on the CLI to verify the CLI
    // wins (rather than concatenating).
    // Bead `sts2-rs-ar9u`: STS1 `Cultist` kind removed; using `Nibbit`.
    let body = "schema = 1\n\n[player]\nclass = \"Ironclad\"\n\nrelics = [\"Anchor\"]\n\n[[monsters]]\nkind = \"Nibbit\"\n";
    let path = write_tmp("relic-override.toml", body);
    let (code, out) = run_combat(&[
        "--scenario",
        path.to_str().unwrap(),
        "--relic",
        "BurningBlood",
        "--agent",
        "random",
        "--steps",
        "1",
    ]);
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 0, "out=\n{out}");
    assert!(
        out.contains("Burning Blood"),
        "CLI relic should appear; out=\n{out}"
    );
    assert!(
        !out.contains("Anchor"),
        "scenario file's Anchor must be overridden by CLI --relic; out=\n{out}"
    );
}

#[test]
fn missing_scenario_file_exits_one() {
    let (code, out) = run_combat(&["--scenario", "definitely-does-not-exist.toml"]);
    assert_eq!(code, 1, "out=\n{out}");
    assert!(out.contains("scenario error"), "out=\n{out}");
}

#[test]
fn unsupported_schema_exits_one() {
    // schema=2 was the wall before sts2-rs-i09d; bumped to schema=3
    // (above current max) so this test continues to assert that
    // unknown schemas are surfaced.
    let body = "schema = 3\n\n[[monsters]]\nkind = \"Nibbit\"\n";
    let path = write_tmp("schema3.toml", body);
    let (code, out) = run_combat(&["--scenario", path.to_str().unwrap()]);
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 1, "out=\n{out}");
    assert!(out.contains("unsupported schema"), "out=\n{out}");
}

#[test]
fn schema_two_fixture_from_scraper_loads_and_runs() {
    // The body below is the verbatim output of:
    //
    //   sts2-scrape fixtures emit <some.run> --floor 6
    //
    // from the sts2_run_scraper repo, edited down to a monster the
    // Rust engine currently supports (Nibbit) — the original run's
    // PHANTASMAL_GARDENER encounter isn't yet in this port. Schema=2
    // provenance + relic_counters fields must parse cleanly; the
    // engine doesn't act on provenance, but relic_counters must seed
    // into state.relics counter values (see sts2-cli unit tests).
    let body = r#"schema = 2

[provenance]
run_id     = "fake-run-uuid"
build_id   = "v0.101.0"
floor      = 6
act        = "ACT.UNDERDOCKS"
encounter  = "ENCOUNTER.NIBBIT_WEAK"
room_type  = "monster"
player_outcome = "won"
player_turns_taken = 4
seed = "SYDDKWZUC8"
deck_perfect   = true
relics_perfect = true

[player]
class     = "Ironclad"
ascension = 0
hp        = 64
max_hp    = 80
gold      = 137
deck      = ["Strike", "Strike", "Strike", "Defend+", "Defend", "Bash"]
relics    = ["BurningBlood", "Anchor"]
potions   = []

[player.relic_counters]
ANCHOR = 3

[[monsters]]
kind = "Nibbit"
"#;
    let path = write_tmp("schema2_fixture.toml", body);
    let (code, out) = run_combat(&["--scenario", path.to_str().unwrap()]);
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 0, "exit nonzero — out=\n{out}");
    assert!(
        out.contains("── starting combat ──"),
        "expected combat banner — out=\n{out}"
    );
}

#[test]
fn schema_two_with_upgrade_level_and_enchantment_tokens() {
    // Bead `sts2-rs-kbi1`: `Foo+N` and `Foo@KIND` tokens must round-trip
    // through the schema=2 deck-string list. We use `Strike+2@SHARP` to
    // exercise both extensions in one entry — the engine collapses
    // `+2` to `upgraded=true` (only `Card.upgraded: bool` exists today)
    // but `set_enchantment(Sharp, 1)` should populate the enchantment
    // stack so the resulting Card surfaces +1 Sharp damage downstream.
    let body = "schema = 2\n\n[player]\nclass = \"Ironclad\"\n\
                hp = 80\nmax_hp = 80\n\
                deck = [\"Strike+2@SHARP\", \"Defend\", \"Bash+@GOOPY\"]\n\
                relics = [\"BurningBlood\"]\n\n\
                [[monsters]]\nkind = \"Nibbit\"\n";
    let path = write_tmp("schema2_kbi1.toml", body);
    let (code, out) = run_combat(&["--scenario", path.to_str().unwrap()]);
    std::fs::remove_file(&path).ok();
    assert_eq!(code, 0, "exit nonzero — out=\n{out}");
    assert!(
        out.contains("── starting combat ──"),
        "expected combat banner — out=\n{out}"
    );
}
