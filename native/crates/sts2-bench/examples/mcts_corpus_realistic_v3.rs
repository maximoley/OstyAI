//! `mcts_corpus_realistic_v3` — outcome-balanced A/B corpus.
//!
//! Filed for gh issue #14 (`docs/perf/2026-05-20-issue14-outcome-balanced-corpus.md`).
//!
//! v2 stride-samples by tier from the deck-perfect Ironclad pool.
//! That pool is ~94 % `[provenance].player_outcome = "won"`, so the
//! resulting 200-state corpus inherits the same skew (the ls5xj slice
//! measured 188 won / 12 lost). Acceptance gates that quote a single
//! aggregate win-rate are therefore dominated by easy-won positions;
//! any "X pp lift on lost-run fixtures" is buried by the won-run
//! mass and binomial noise on the small lost-run subset.
//!
//! v3 stratifies on **(tier, player_outcome)** jointly so each tier
//! contributes equal halves of won / lost fixtures. The per-tier
//! totals match v2 (50 common / 100 elite / 50 boss → 200 total) so
//! existing per-tier infrastructure (`mcts_ab_diff`) keeps working
//! one-for-one; what changes is *within* each tier: 25 won + 25 lost
//! (common, boss) and 50 won + 50 lost (elite).
//!
//! `RealisticCorpusEntry` learns an optional `player_outcome` field so
//! downstream consumers (`mcts_ab_value_net.rs`, `mcts_ab.rs`, etc.)
//! can read v3 corpora without re-parsing the fixture TOML. The field
//! is `#[serde(default)]`, so v1/v2 JSON keeps deserializing.
//!
//! ## Running
//!
//! ```sh
//! cargo run --release --example mcts_corpus_realistic_v3
//! ```
//!
//! Persists to `crates/sts2-bench/data/mcts_corpus_realistic_v3.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use glob::glob;
use serde::{Deserialize, Serialize};
use sts2_cli::scenario_to_combat_state;
use sts2_cli::sim::scenario::toml_file::{load_scenario_file, ScenarioFile};
use sts2_content::encounters::EncounterTier;

const FIXTURE_ROOT: &str = "/tmp/run-fixtures-full/the-ironclad";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RealisticCorpus {
    version: u32,
    git_sha: String,
    generator: String,
    bucket_globs: BTreeMap<String, String>,
    entries: Vec<RealisticCorpusEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RealisticCorpusEntry {
    state_id: u32,
    tier: String,
    fixture_path: String,
    floor: u16,
    seed: u64,
    /// `"won"` / `"lost"` — copied from `[provenance].player_outcome`.
    /// Required by v3 (we just stratified on it); optional in the
    /// reader-side mirrors for back-compat with v1/v2.
    player_outcome: String,
}

fn floor_from_filename(path: &Path) -> Option<u16> {
    let name = path.file_name()?.to_str()?;
    let idx = name.find("floor")?;
    let after = &name[idx + "floor".len()..];
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn provenance_tier(s: &ScenarioFile) -> Option<&'static str> {
    let id_raw = s.provenance.as_ref()?.encounter.as_deref()?;
    let id = id_raw.strip_prefix("ENCOUNTER.").unwrap_or(id_raw);
    let def = sts2_content::encounters::act1_overgrowth::find(id)
        .or_else(|| sts2_content::encounters::act2_hive::find(id))
        .or_else(|| sts2_content::encounters::act3_glory::find(id))?;
    Some(match def.tier {
        EncounterTier::Weak | EncounterTier::Normal => "common",
        EncounterTier::Elite => "elite",
        EncounterTier::Boss => "boss",
        _ => return None,
    })
}

/// Returns `Some("won" | "lost")` when the provenance label is one of
/// the two values we stratify on. Other values (`"unknown"`,
/// `"abandoned"`, missing) are dropped — including them would muddy
/// the won/lost balance the corpus is supposed to enforce.
fn provenance_outcome(s: &ScenarioFile) -> Option<&'static str> {
    match s.provenance.as_ref()?.player_outcome.as_deref()? {
        "won" => Some("won"),
        "lost" => Some("lost"),
        _ => None,
    }
}

fn deck_perfect(s: &ScenarioFile) -> bool {
    s.provenance
        .as_ref()
        .and_then(|p| p.deck_perfect)
        .unwrap_or(false)
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

/// Sample up to `target` fixtures evenly through `pool`, validating
/// each candidate by round-tripping it through `scenario_to_combat_state`.
/// Returns `(entries, seed_counter, state_id)` advanced past the new
/// rows.
fn sample_bucket(
    pool: &[PathBuf],
    tier: &str,
    outcome: &str,
    target: usize,
    mut seed_counter: u64,
    mut state_id: u32,
) -> (Vec<RealisticCorpusEntry>, u64, u32) {
    let mut out: Vec<RealisticCorpusEntry> = Vec::with_capacity(target);
    let stride = if pool.is_empty() {
        1
    } else {
        (pool.len() / target.max(1)).max(1)
    };
    let mut kept = 0usize;
    let mut i = 0usize;
    while kept < target && i < pool.len() {
        let path = &pool[i];
        i += stride;
        let Ok(scenario) = load_scenario_file(path) else {
            continue;
        };
        if scenario_to_combat_state(&scenario, seed_counter).is_err() {
            continue;
        }
        let floor = floor_from_filename(path).unwrap_or(0);
        out.push(RealisticCorpusEntry {
            state_id,
            tier: tier.to_string(),
            fixture_path: path.display().to_string(),
            floor,
            seed: seed_counter,
            player_outcome: outcome.to_string(),
        });
        state_id += 1;
        seed_counter += 1;
        kept += 1;
    }
    // Backfill linearly through any remaining pool entries if the
    // stride walk fell short (some fixtures may fail realization).
    if kept < target {
        for path in pool {
            if kept >= target {
                break;
            }
            let already = out
                .iter()
                .any(|e| e.fixture_path == path.display().to_string());
            if already {
                continue;
            }
            let Ok(scenario) = load_scenario_file(path) else {
                continue;
            };
            if scenario_to_combat_state(&scenario, seed_counter).is_err() {
                continue;
            }
            let floor = floor_from_filename(path).unwrap_or(0);
            out.push(RealisticCorpusEntry {
                state_id,
                tier: tier.to_string(),
                fixture_path: path.display().to_string(),
                floor,
                seed: seed_counter,
                player_outcome: outcome.to_string(),
            });
            state_id += 1;
            seed_counter += 1;
            kept += 1;
        }
    }
    if kept < target {
        eprintln!(
            "[v3] WARNING: tier={tier} outcome={outcome} pool ({}) short of target ({target}); \
             emitting {kept}",
            pool.len(),
        );
    } else {
        eprintln!("[v3] tier {tier} outcome {outcome}: kept {kept}/{target}");
    }
    (out, seed_counter, state_id)
}

fn main() -> std::io::Result<()> {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    // Per-tier targets (same totals as v2). The outcome split inside
    // each tier is forced to 50/50.
    let tier_targets: &[(&str, usize)] = &[("common", 50), ("elite", 100), ("boss", 50)];

    let pattern = format!("{FIXTURE_ROOT}/*.toml");
    eprintln!("[v3] scanning {pattern}");
    let all_paths: Vec<PathBuf> = match glob(&pattern) {
        Ok(it) => {
            let mut v: Vec<PathBuf> = it.filter_map(Result::ok).collect();
            v.sort();
            v
        }
        Err(e) => {
            eprintln!("[v3] glob failed: {e}");
            Vec::new()
        }
    };
    eprintln!("[v3] {} fixtures matched", all_paths.len());

    // (tier, outcome) -> sorted pool.
    let mut pools: BTreeMap<(&'static str, &'static str), Vec<PathBuf>> = BTreeMap::new();
    let mut stat_loaded = 0usize;
    let mut stat_imperfect = 0usize;
    let mut stat_unresolved_tier = 0usize;
    let mut stat_unresolved_outcome = 0usize;
    let mut stat_parse_err = 0usize;

    for path in &all_paths {
        let Ok(scenario) = load_scenario_file(path) else {
            stat_parse_err += 1;
            continue;
        };
        stat_loaded += 1;
        if !deck_perfect(&scenario) {
            stat_imperfect += 1;
            continue;
        }
        let Some(tier) = provenance_tier(&scenario) else {
            stat_unresolved_tier += 1;
            continue;
        };
        let Some(outcome) = provenance_outcome(&scenario) else {
            stat_unresolved_outcome += 1;
            continue;
        };
        pools.entry((tier, outcome)).or_default().push(path.clone());
    }
    eprintln!(
        "[v3] parsed {stat_loaded}, dropped {stat_imperfect} deck-imperfect, \
         {stat_unresolved_tier} unresolved-tier, \
         {stat_unresolved_outcome} unresolved-outcome (neither 'won' nor 'lost'), \
         {stat_parse_err} parse-err"
    );
    for ((tier, outcome), pool) in &pools {
        eprintln!("[v3] pool tier={tier} outcome={outcome}: {}", pool.len());
    }

    let mut entries: Vec<RealisticCorpusEntry> = Vec::new();
    let mut seed_counter: u64 = 1;
    let mut state_id: u32 = 0;
    let mut bucket_globs = BTreeMap::new();

    for (tier, total) in tier_targets {
        bucket_globs.insert((*tier).to_string(), pattern.clone());
        let per_outcome = *total / 2;
        for outcome in ["won", "lost"] {
            let pool = pools.get(&(*tier, outcome)).cloned().unwrap_or_default();
            let (rows, next_seed, next_state) =
                sample_bucket(&pool, tier, outcome, per_outcome, seed_counter, state_id);
            entries.extend(rows);
            seed_counter = next_seed;
            state_id = next_state;
        }
    }

    // Sanity-print the realized balance per tier.
    let mut by_tier_outcome: BTreeMap<(String, String), usize> = BTreeMap::new();
    for e in &entries {
        *by_tier_outcome
            .entry((e.tier.clone(), e.player_outcome.clone()))
            .or_default() += 1;
    }
    for ((tier, outcome), n) in &by_tier_outcome {
        eprintln!("[v3] realized tier={tier} outcome={outcome}: {n}");
    }

    let corpus = RealisticCorpus {
        version: 3,
        git_sha: git_short_sha(),
        generator: "mcts_corpus_realistic_v3".to_string(),
        bucket_globs,
        entries,
    };
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    let mut out = std::path::PathBuf::from(manifest);
    out.push("data");
    out.push("mcts_corpus_realistic_v3.json");
    let json = serde_json::to_string_pretty(&corpus).expect("serialize corpus");
    std::fs::write(&out, json)?;
    eprintln!(
        "[v3] wrote {} entries to {}",
        corpus.entries.len(),
        out.display(),
    );
    Ok(())
}
