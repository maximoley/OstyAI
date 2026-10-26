//! Hand-curated blunder fixtures (bead `sts2-rs-64h9r` Half B).
//!
//! Walks `tests/parity/blunders/*.toml`, runs the recorder on each,
//! and asserts that `decisions[decision_index].recommended_label`
//! does **not** contain the `must_not_pick` substring from the
//! fixture's `[blunder_assertion]` table.
//!
//! Substring matching is deliberate: the recorder's labels include
//! the hand-index suffix (e.g. `Play DefendIronclad+ (h0)`) which
//! shifts run-to-run with draw order; the assertion just names the
//! action prefix (`Play DefendIronclad+`).
//!
//! The existing `ScenarioFile` loader silently ignores the
//! `[blunder_assertion]` table because it's not in the
//! `#[derive(Deserialize)] pub struct ScenarioFile` schema and the
//! struct is not `#[serde(deny_unknown_fields)]`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sts2_cli::replay::{audit_fight, record_from_fixture, RecorderConfig, RedFlagKind};

#[derive(Debug, Deserialize)]
struct FixtureWithAssertion {
    blunder_assertion: BlunderAssertion,
}

#[derive(Debug, Deserialize)]
struct BlunderAssertion {
    class: String,
    must_not_pick: String,
    #[serde(default = "default_sims")]
    sims: u32,
    #[serde(default = "default_rollout_depth")]
    rollout_depth: u32,
    #[serde(default = "default_seed")]
    seed: u64,
    #[serde(default)]
    decision_index: usize,
}

fn default_sims() -> u32 {
    2000
}
fn default_rollout_depth() -> u32 {
    15
}
fn default_seed() -> u64 {
    1
}

fn blunders_dir() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest)
        .join("..")
        .join("..")
        .join("tests")
        .join("parity")
        .join("blunders")
}

fn enumerate_fixtures() -> Vec<PathBuf> {
    // Bead `sts2-rs-nhvwo` (q3yiy.1): walk recursively so
    // per-class fixtures under `blunders/per_class/<class>.toml`
    // are picked up alongside the flat-level fixtures.
    let dir = blunders_dir();
    let mut out: Vec<PathBuf> = Vec::new();
    walk_toml(&dir, &mut out);
    out.sort();
    out
}

fn walk_toml(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else {
        panic!("blunders dir missing: {}", dir.display());
    };
    for entry in rd.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            walk_toml(&path, out);
        } else if path.extension().is_some_and(|x| x == "toml") {
            out.push(path);
        }
    }
}

#[test]
fn slow_blunder_fixtures_all_pass() {
    let fixtures = enumerate_fixtures();
    assert!(
        !fixtures.is_empty(),
        "no .toml fixtures under tests/parity/blunders"
    );

    for path in &fixtures {
        let raw = fs::read_to_string(path).expect("read fixture");
        let parsed: FixtureWithAssertion =
            toml::from_str(&raw).unwrap_or_else(|e| panic!("{}: parse: {e}", path.display()));
        let a = &parsed.blunder_assertion;

        let cfg = RecorderConfig {
            sims: a.sims,
            rollout_depth: a.rollout_depth,
            seed: a.seed,
            ..RecorderConfig::default()
        };

        let record = record_from_fixture(path, &cfg)
            .unwrap_or_else(|e| panic!("{}: record_from_fixture: {e}", path.display()));

        let dec = record.decisions.get(a.decision_index).unwrap_or_else(|| {
            panic!(
                "{}: decision_index={} out of range (have {} decisions)",
                path.display(),
                a.decision_index,
                record.decisions.len()
            )
        });

        assert!(
            !dec.recommended_label.contains(&a.must_not_pick),
            "{}: decision_index={} picked blunder `{}` (full label: `{}`)",
            path.display(),
            a.decision_index,
            a.must_not_pick,
            dec.recommended_label
        );

        // sts2-rs-q3yiy: cross-validate the audit detector by negation.
        // Since MCTS avoided the blunder above, the audit's flag list
        // for this fight must NOT contain a flag of the fixture's
        // `class` kind at `decision_index`. If the detector is broken
        // and miscategorizes the rational pick as a blunder, this fires.
        let expected_kind = parse_kind(&a.class).unwrap_or_else(|| {
            panic!(
                "{}: unknown blunder class `{}` (must match a RedFlagKind::as_str())",
                path.display(),
                a.class
            )
        });
        let flags = audit_fight(&record);
        let misclassified: Vec<_> = flags
            .iter()
            .filter(|f| f.kind == expected_kind && f.decision_index as usize == a.decision_index)
            .collect();
        assert!(
            misclassified.is_empty(),
            "{}: detector flagged rational pick at decision_index={} as `{}` ({} flag(s)); detector likely regressed",
            path.display(),
            a.decision_index,
            a.class,
            misclassified.len()
        );
    }
}

fn parse_kind(s: &str) -> Option<RedFlagKind> {
    RedFlagKind::ALL.into_iter().find(|k| k.as_str() == s)
}
