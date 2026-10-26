//! `mcts_corpus_realistic` — DUAL sibling to `mcts_corpus.rs`
//! from the 2026-05-17 starter-deck audit.
//!
//! The canonical `mcts_corpus.json` is locked to Ironclad starter
//! deck + Act-1 encounters; downstream A/B / Texel / value-net
//! gates have repeatedly traced their +0.00 pp deltas back to
//! that distribution (see `sts2-rs-{nk9e,acbj,7njh,2dgj}`).
//! This generator emits a 50-state corpus drawn from the
//! `/tmp/run-fixtures-full/the-ironclad/` `.run-source` TOML
//! fixtures — real human decks, real relics, real HP — at floor
//! tiers proven by `sts2-rs-2dgj` to land inside the
//! `[30 %, 70 %]` win-rate band the gates need.
//!
//! ## Fixture-tier mapping (mirrors `mcts_corpus.rs`)
//!
//! | bucket | fixtures sampled from | count |
//! |---|---|---:|
//! | common | `floor00[3-9]*` + `floor01[0-9]*` (Act 1 + Act 1 boss) | 20 |
//! | elite  | `floor02[0-9]*` (Act 2 mid) | 20 |
//! | boss   | `floor0[4-9][0-9]*` (Act 3+, deepest available) | 10 |
//!
//! ## Output schema
//!
//! Persisted to `crates/sts2-bench/data/mcts_corpus_realistic.json`.
//! Schema is intentionally distinct from `mcts_corpus.json` because
//! the (encounter_id, engine_seed, warmup_seed, warmup_turns)
//! quadruple is insufficient to reproduce a fixture-derived state
//! — the deck/relics/HP all need to come from the source TOML.
//! Each entry records the fixture path + a seed; downstream
//! consumers (a future MIGRATE bead extending `mcts_ab.rs` etc.)
//! re-realize via `sts2_cli::scenario_to_combat_state`.
//!
//! Wiring this corpus into the existing A/B harnesses is a
//! follow-up MIGRATE bead — this example only delivers the
//! generator + manifest so the work can proceed in parallel
//! with the rest of the audit's MIGRATE rows.
//!
//! ## Running
//!
//! ```sh
//! cargo run --release --example mcts_corpus_realistic
//! ```
//!
//! Cheap (<2 s on the dev box at 50 entries).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use glob::glob;
use serde::{Deserialize, Serialize};
use sts2_cli::scenario_to_combat_state;
use sts2_cli::sim::scenario::toml_file::load_scenario_file;

/// Default fixture root. Matches the path the rest of the project
/// uses (`mcts_sweep.py`'s `_load_corpus_pool`, the 2dgj smoke
/// command in `docs/perf/2026-05-17-texel-corpus.md`).
const FIXTURE_ROOT: &str = "/tmp/run-fixtures-full/the-ironclad";

/// Output manifest. The schema deliberately differs from
/// `Corpus` in `examples/common/mod.rs` because realistic-deck
/// states aren't reproducible from `(encounter_id, seed)` alone.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RealisticCorpus {
    /// Schema version. Bump on breaking layout changes; downstream
    /// consumers should reject unknown versions.
    version: u32,
    git_sha: String,
    generator: String,
    /// `(bucket name → fixture-glob pattern)`. Persisted so a
    /// reader can re-derive the bucketing intent without re-reading
    /// this example's source.
    bucket_globs: BTreeMap<String, String>,
    entries: Vec<RealisticCorpusEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RealisticCorpusEntry {
    state_id: u32,
    /// "common" / "elite" / "boss" — same bucket labels as
    /// `mcts_corpus.json` so the A/B aggregation code translates
    /// 1:1 when the consumer bead lands.
    tier: String,
    /// Absolute path to the source TOML. Consumers load it via
    /// `sts2_cli::sim::scenario::toml_file::load_scenario_file`
    /// and realize via `sts2_cli::scenario_to_combat_state`.
    fixture_path: String,
    /// The floor number lifted from the fixture filename. Useful
    /// for downstream stratification + diagnostics.
    floor: u16,
    /// Seed passed to `scenario_to_combat_state` (drives spawn-HP
    /// rolls + combat RNG). Stable across runs of this example.
    seed: u64,
}

fn floor_from_filename(path: &Path) -> Option<u16> {
    let name = path.file_name()?.to_str()?;
    let idx = name.find("floor")?;
    let after = &name[idx + "floor".len()..];
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Glob the fixture root, sort for determinism, and downsample to
/// `take` entries by striding evenly through the pool. Returns the
/// stride-picked list followed by the rest of the pool in original
/// order — callers can iterate past the initial `take` to backfill
/// dropped fixtures (e.g. unresolved-relic content gaps) without
/// changing the deterministic primary pick.
fn pick_fixtures(pattern: &str, take: usize) -> Vec<PathBuf> {
    let matched: Vec<PathBuf> = match glob(pattern) {
        Ok(paths) => paths.filter_map(Result::ok).collect(),
        Err(e) => {
            eprintln!("[mcts_corpus_realistic] glob({pattern}) failed: {e}");
            Vec::new()
        }
    };
    let mut paths = matched;
    paths.sort();
    if paths.len() <= take {
        return paths;
    }
    // Stride evenly: index i picks `paths[i * stride]` for the
    // primary `take` picks. Append the remainder in original order
    // so callers have a backfill pool for dropped fixtures.
    let stride = paths.len() / take;
    let primary: Vec<PathBuf> = (0..take).map(|i| paths[i * stride].clone()).collect();
    let primary_set: std::collections::HashSet<&PathBuf> = primary.iter().collect();
    let mut out = primary.clone();
    for p in &paths {
        if !primary_set.contains(p) {
            out.push(p.clone());
        }
    }
    out
}

fn git_short_sha() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn main() -> std::io::Result<()> {
    sts2_content::cards::dispatch::ensure_installed();

    // Bucket globs mirror `mcts_corpus.rs`'s common/elite/boss
    // intent: progressively-deeper-floor fixtures stand in for
    // progressively-harder encounter tiers, since the
    // `.run-source` corpus naturally pairs deeper-floor decks
    // with deeper-act encounters.
    let buckets = [
        ("common", format!("{FIXTURE_ROOT}/*floor00[3-9]*.toml")),
        ("elite", format!("{FIXTURE_ROOT}/*floor02[0-9]*.toml")),
        ("boss", format!("{FIXTURE_ROOT}/*floor0[4-9][0-9]*.toml")),
    ];
    let take = [20, 20, 10];

    let mut entries = Vec::with_capacity(50);
    let mut bucket_globs = BTreeMap::new();
    let mut state_id: u32 = 0;
    let mut seed_counter: u64 = 1;

    for ((label, pattern), n) in buckets.iter().zip(take.iter()) {
        bucket_globs.insert((*label).to_string(), pattern.clone());
        let pool = pick_fixtures(pattern, *n);
        if pool.len() < *n {
            eprintln!(
                "[mcts_corpus_realistic] {label}: pool too small ({} fixtures matched < {n} \
                 requested); emitting what we have. Pattern: {pattern}",
                pool.len(),
            );
        }
        let mut kept = 0usize;
        for path in pool {
            if kept >= *n {
                break;
            }
            let floor = floor_from_filename(&path).unwrap_or(0);
            // Realize each state once to (a) prove the fixture is
            // loadable end-to-end and (b) catch unresolved content
            // gaps (unknown relic, etc.) at corpus-generation time
            // rather than at A/B-run time. The state itself is
            // discarded — the consumer rebuilds from `fixture_path`.
            let scenario = load_scenario_file(&path)
                .map_err(|e| std::io::Error::other(format!("load {}: {e}", path.display())))?;
            match scenario_to_combat_state(&scenario, seed_counter) {
                Ok(_state) => {}
                Err(e) => {
                    eprintln!(
                        "[mcts_corpus_realistic] dropping fixture {} (build state failed: {e})",
                        path.display(),
                    );
                    continue;
                }
            }
            entries.push(RealisticCorpusEntry {
                state_id,
                tier: (*label).to_string(),
                fixture_path: path.display().to_string(),
                floor,
                seed: seed_counter,
            });
            state_id += 1;
            seed_counter += 1;
            kept += 1;
        }
        if kept < *n {
            eprintln!(
                "[mcts_corpus_realistic] {label}: only kept {kept}/{n} after content-gap \
                 drops (unresolved relics / cards). Future bead: widen the bucket's glob \
                 pattern or pre-filter fixtures by relic blocklist.",
            );
        }
    }

    let corpus = RealisticCorpus {
        version: 1,
        git_sha: git_short_sha(),
        generator:
            "sts2-bench/examples/mcts_corpus_realistic.rs (DUAL sibling, starter-deck audit)"
                .to_string(),
        bucket_globs,
        entries,
    };

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("data")
        .join("mcts_corpus_realistic.json");
    let json = serde_json::to_string_pretty(&corpus).map_err(std::io::Error::other)?;
    std::fs::write(&path, json)?;

    eprintln!(
        "[mcts_corpus_realistic] wrote {} entries to {} \
         (bucket breakdown: common={} elite={} boss={})",
        corpus.entries.len(),
        path.display(),
        corpus.entries.iter().filter(|e| e.tier == "common").count(),
        corpus.entries.iter().filter(|e| e.tier == "elite").count(),
        corpus.entries.iter().filter(|e| e.tier == "boss").count(),
    );

    Ok(())
}
