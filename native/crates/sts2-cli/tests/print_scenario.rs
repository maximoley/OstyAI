//! Round-trip test for `--print-scenario` (cliext E1, bead
//! `sts2-rs-92sq`).
//!
//! Dump the resolved scenario for a representative CLI invocation,
//! feed it back via `--scenario <tmpfile> --print-scenario`, and assert
//! the two dumps are byte-identical — the strict-subset TOML must be a
//! fixed point of the load + render pipeline.

use std::io::Write;
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

fn write_tmp(name: &str, body: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("sts2-92sq-{pid}-{ts}-{name}"));
    std::fs::File::create(&p)
        .unwrap()
        .write_all(body.as_bytes())
        .unwrap();
    p
}

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(binary_path()).args(args).output().unwrap();
    let code = out.status.code().unwrap_or(-1);
    let s = String::from_utf8_lossy(&out.stdout).into_owned();
    (code, s)
}

#[test]
fn print_scenario_round_trips() {
    let (code, dump1) = run(&[
        "combat",
        "--relic",
        "BurningBlood",
        "--player",
        "ironclad",
        "--print-scenario",
    ]);
    assert_eq!(code, 0, "first dump exited non-zero: {dump1}");
    assert!(dump1.contains("schema = 1"), "no schema line: {dump1}");
    assert!(dump1.contains("BurningBlood"), "no relic in dump: {dump1}");
    assert!(dump1.contains("Nibbit"), "default monster missing: {dump1}");

    let scenario = write_tmp("roundtrip.toml", &dump1);
    let path_str = scenario.to_str().unwrap();
    let (code2, dump2) = run(&["combat", "--scenario", path_str, "--print-scenario"]);
    std::fs::remove_file(&scenario).ok();
    assert_eq!(code2, 0, "second dump exited non-zero: {dump2}");
    assert_eq!(dump1, dump2, "round-trip not byte-identical");
}

#[test]
fn print_scenario_includes_deck_and_potions() {
    let (code, dump) = run(&[
        "combat",
        "--deck",
        "Strike,Strike,Bash+",
        "--potion",
        "FirePotion",
        "--player",
        "ironclad",
        "--print-scenario",
    ]);
    assert_eq!(code, 0, "exited non-zero: {dump}");
    assert!(dump.contains("deck = ["), "no deck line: {dump}");
    assert!(
        dump.contains("\"Bash+\""),
        "upgraded marker missing: {dump}"
    );
    assert!(dump.contains("potions = ["), "no potion line: {dump}");
    assert!(dump.contains("FirePotion"));

    let scenario = write_tmp("deck.toml", &dump);
    let path_str = scenario.to_str().unwrap();
    let (code2, dump2) = run(&["combat", "--scenario", path_str, "--print-scenario"]);
    std::fs::remove_file(&scenario).ok();
    assert_eq!(code2, 0);
    assert_eq!(dump, dump2, "deck round-trip not byte-identical");
}
