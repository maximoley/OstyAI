//! `mcts_corpus_realistic_v2` — 200-state tier-stratified A/B
//! corpus. Bead `sts2-rs-acbj` (step 6).
//!
//! Sibling to `mcts_corpus_realistic.rs` (50 states). Three
//! differences:
//!
//! 1. **Tier resolution from provenance, not floor band.** Each
//!    fixture is bucketed by looking up
//!    `[provenance].encounter` against `ACT{1,2,3}_ENCOUNTERS`.
//!    Fixtures whose encounter id doesn't resolve are dropped
//!    (no `unknown` bucket — those positions are unlabeled and
//!    would bias the per-tier breakdown).
//!
//! 2. **`deck_perfect = true` filter.** ~7% of the scraped pool
//!    has incomplete decks (scraper couldn't recover the exact
//!    deck from the event log). Per-tier A/B numbers are
//!    sensitive to deck-quality drift, so the filter is on by
//!    default here (unlike v1 which silently included those
//!    fixtures).
//!
//! 3. **200-state mix: 50 common / 100 elite / 50 boss.** v1's
//!    50 states had only ~10 boss states (binomial ±15 pp at
//!    1000 seeds — too wide to call a +3 pp boss-tier effect).
//!    v2 lifts each bucket to >= 50 so per-tier binomial CIs
//!    drop into the ±6 pp band.
//!
//! Persisted to `crates/sts2-bench/data/mcts_corpus_realistic_v2.json`.
//! Same `RealisticCorpus*` schema as v1 so existing consumers
//! (`mcts_ab` loader) need no schema changes.
//!
//! ## Running
//!
//! ```sh
//! cargo run --release --example mcts_corpus_realistic_v2
//! ```
//!
//! Cheap (<10 s on the dev box at 200 entries — most of the time
//! is the per-fixture `scenario_to_combat_state` validation pass).

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
}

fn floor_from_filename(path: &Path) -> Option<u16> {
    let name = path.file_name()?.to_str()?;
    let idx = name.find("floor")?;
    let after = &name[idx + "floor".len()..];
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Resolve `[provenance].encounter` to one of "common" / "elite" /
/// "boss". Returns `None` if the encounter id isn't found in any
/// ACT*_ENCOUNTERS roster (those fixtures are dropped from the
/// corpus rather than bucketed into a noisy "unknown" tier).
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

fn main() -> std::io::Result<()> {
    sts2_content::cards::dispatch::ensure_installed();

    // Targets per tier.
    let targets: &[(&str, usize)] = &[("common", 150), ("elite", 300), ("boss", 150)];
    let total_target: usize = targets.iter().map(|(_, n)| *n).sum();

    // Walk the entire fixture pool, parse provenance, bucket by
    // resolved tier (skipping non-deck_perfect + unresolved-tier
    // fixtures). Sort within each bucket for determinism.
    let pattern = format!("{FIXTURE_ROOT}/*.toml");
    eprintln!("[v2] scanning {pattern}");
    let all_paths: Vec<PathBuf> = match glob(&pattern) {
        Ok(it) => {
            let mut v: Vec<PathBuf> = it.filter_map(Result::ok).collect();
            v.sort();
            v
        }
        Err(e) => {
            eprintln!("[v2] glob failed: {e}");
            Vec::new()
        }
    };
    eprintln!("[v2] {} fixtures matched", all_paths.len());

    let mut by_tier: BTreeMap<&'static str, Vec<PathBuf>> = BTreeMap::new();
    let mut stat_loaded = 0usize;
    let mut stat_imperfect = 0usize;
    let mut stat_unresolved = 0usize;
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
            stat_unresolved += 1;
            continue;
        };
        by_tier.entry(tier).or_default().push(path.clone());
    }
    eprintln!(
        "[v2] parsed {stat_loaded}, dropped {stat_imperfect} deck-imperfect, \
         {stat_unresolved} unresolved-tier, {stat_parse_err} parse-err"
    );
    for (tier, paths) in &by_tier {
        eprintln!("[v2] tier {tier}: pool size = {}", paths.len());
    }

    let mut entries: Vec<RealisticCorpusEntry> = Vec::with_capacity(total_target);
    let mut state_id: u32 = 0;
    let mut seed_counter: u64 = 1;
    let mut bucket_globs = BTreeMap::new();

    for (tier, target) in targets {
        bucket_globs.insert((*tier).to_string(), pattern.clone());
        let pool = by_tier.get(tier).cloned().unwrap_or_default();
        if pool.len() < *target {
            eprintln!(
                "[v2] WARNING: tier={tier} pool ({}) < target ({target}); emitting what we have",
                pool.len(),
            );
        }
        // Stride evenly through the deck_perfect tier pool so the
        // sampled fixtures cover the full alphabetic (i.e. run-id)
        // range, not just the first N. Each unique run_id appears
        // at most once per tier — the leading 8 hex digits of the
        // run_id prefix vary uniformly across the pool.
        let stride = if pool.is_empty() {
            1
        } else {
            (pool.len() / (*target).max(1)).max(1)
        };
        let mut kept = 0usize;
        let mut i = 0usize;
        while kept < *target && i < pool.len() {
            let path = &pool[i];
            i += stride;
            // Validate the fixture realizes end-to-end before we
            // commit it to the corpus — same as v1.
            let Ok(scenario) = load_scenario_file(path) else {
                continue;
            };
            if scenario_to_combat_state(&scenario, seed_counter).is_err() {
                continue;
            }
            let floor = floor_from_filename(path).unwrap_or(0);
            entries.push(RealisticCorpusEntry {
                state_id,
                tier: (*tier).to_string(),
                fixture_path: path.display().to_string(),
                floor,
                seed: seed_counter,
            });
            state_id += 1;
            seed_counter += 1;
            kept += 1;
        }
        // Backfill from the rest of the pool if stride-walk fell
        // short (e.g. some fixtures failed scenario_to_combat_state).
        if kept < *target {
            for path in &pool {
                if kept >= *target {
                    break;
                }
                let already = entries
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
                entries.push(RealisticCorpusEntry {
                    state_id,
                    tier: (*tier).to_string(),
                    fixture_path: path.display().to_string(),
                    floor,
                    seed: seed_counter,
                });
                state_id += 1;
                seed_counter += 1;
                kept += 1;
            }
        }
        eprintln!("[v2] tier {tier}: kept {kept}/{target}");
    }

    let corpus = RealisticCorpus {
        version: 2,
        git_sha: git_short_sha(),
        generator: "mcts_corpus_realistic_v2".to_string(),
        bucket_globs,
        entries,
    };
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    let mut out = std::path::PathBuf::from(manifest);
    out.push("data");
    out.push("mcts_corpus_realistic_v2.json");
    let json = serde_json::to_string_pretty(&corpus).expect("serialize corpus");
    std::fs::write(&out, json)?;
    eprintln!(
        "[v2] wrote {} entries to {}",
        corpus.entries.len(),
        out.display(),
    );
    Ok(())
}
