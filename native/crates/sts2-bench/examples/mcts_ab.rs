//! `mcts_ab` — A/B harness for bead `sts2-rs-yuyc`.
//!
//! For each state in the chosen corpus, run `Mcts::search` at the
//! `BUDGETS` set and persist the per-state / per-budget outcome
//! breakdown to `data/mcts_ab[_realistic]_<git-short-sha>.json`.
//! Output is byte-stable across runs at the same git SHA (the
//! `wall_ms` field is rounded to whole milliseconds — informational
//! only, do not include in determinism hashes).
//!
//! ## Corpus selection (bead `sts2-rs-ch1h`)
//!
//! The starter-deck audit (`docs/audits/2026-05-17-starter-deck-audit.md`)
//! flagged this harness as MIGRATE: the starter `mcts_corpus.json`
//! already produced +0.00 pp on the 16-col-vs-12-col eval test
//! (sts2-rs-7wk2). The new `mcts_corpus_realistic.json` (from
//! `mcts_corpus_realistic.rs`) draws 50 states from real `.run-source`
//! TOML fixtures across floor-tier buckets proven by `sts2-rs-2dgj`
//! to land inside the [30 %, 70 %] band the gates need.
//!
//! - `cargo run --release --example mcts_ab` (default — realistic, flipped by `sts2-rs-a6wt`)
//! - `cargo run --release --example mcts_ab -- --corpus starter`
//!   loads the legacy starter-deck corpus (kept for back-compat /
//!   regression reproductions only; see
//!   `[[feedback_corpus_starter_deck_too_simple]]`).

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use common::{
    action_to_string, data_path, git_short_sha, realize_state, write_json_pretty, AbReport,
    BudgetResult, Corpus, CorpusTier, PerStateRow,
};
use serde::{Deserialize, Serialize};
use sts2_cli::scenario_to_combat_state;
use sts2_cli::sim::scenario::toml_file::load_scenario_file;
use sts2_core::{is_terminal, legal_actions, CombatState};
use sts2_mcts::{Mcts, MctsConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CorpusKind {
    Starter,
    Realistic,
}

/// Schema mirror of `mcts_corpus_realistic.rs::RealisticCorpus`. The
/// generator example owns the canonical struct; we re-declare a
/// reader-only copy here so this harness doesn't need to expose the
/// generator as a sibling lib target.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct RealisticCorpus {
    version: u32,
    git_sha: String,
    generator: String,
    bucket_globs: BTreeMap<String, String>,
    entries: Vec<RealisticCorpusEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RealisticCorpusEntry {
    state_id: u32,
    tier: String,
    fixture_path: String,
    floor: u16,
    seed: u64,
    // gh #14: v3 corpora stratify on player_outcome and persist the
    // label here. Optional + default so v1/v2 JSON still loads.
    #[serde(default)]
    player_outcome: Option<String>,
}

// sts2-rs-7wk2: rebaseline at sims=300/1000 against the 12-col Texel
// baseline in `data/mcts_ab_300_texel.json` (74.07 % winrate at
// sims=300). 300 matches the baseline's sole budget column; 1000
// confirms the new-feature lift is stable at higher search compute.
const BUDGETS: &[u32] = &[300, 1000];

/// One realized state for the inner MCTS loop. The starter arm
/// owns `entry_mut.state_hash` (mutated by `realize_state`); the
/// realistic arm doesn't track a hash (the source TOML is the
/// canonical identity).
struct RealizedEntry {
    state_id: u32,
    encounter: String,
    tier: CorpusTier,
    state_hash: u64,
    template: CombatState,
}

/// Map the realistic-corpus string tier to the canonical
/// `CorpusTier`. The generator already serializes these labels
/// 1:1 with `mcts_corpus.json`'s lowercase rename_all serialization.
fn parse_realistic_tier(s: &str) -> CorpusTier {
    match s {
        "elite" => CorpusTier::Elite,
        "boss" => CorpusTier::Boss,
        _ => CorpusTier::Common,
    }
}

/// Bead `sts2-rs-65q2` (aze8.1): `--corpus-path <FILENAME>` —
/// override the file the chosen schema is loaded from. Pairs with
/// `--corpus`; the schema selector picks which struct to
/// deserialize, this flag picks which file. Filename is resolved
/// via `common::data_path` (same dir as the built-in defaults), so
/// callers pass `mcts_corpus_realistic_balanced.json` rather than
/// an absolute path.
fn parse_corpus_path() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--corpus-path" {
            return args.next();
        } else if let Some(eq) = arg.strip_prefix("--corpus-path=") {
            return Some(eq.to_string());
        }
    }
    None
}

fn parse_corpus_kind() -> CorpusKind {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--corpus" {
            match args.next().as_deref() {
                Some("starter") => return CorpusKind::Starter,
                Some("realistic") => return CorpusKind::Realistic,
                other => {
                    eprintln!(
                        "[mcts_ab] --corpus expects 'starter' or 'realistic', got {other:?}; \
                         defaulting to realistic",
                    );
                    return CorpusKind::Realistic;
                }
            }
        } else if let Some(eq) = arg.strip_prefix("--corpus=") {
            match eq {
                "starter" => return CorpusKind::Starter,
                "realistic" => return CorpusKind::Realistic,
                _ => {
                    eprintln!("[mcts_ab] unknown --corpus value {eq:?}; defaulting to realistic");
                    return CorpusKind::Realistic;
                }
            }
        }
    }
    CorpusKind::Realistic
}

fn load_starter(corpus_filename: &str) -> Vec<RealizedEntry> {
    let corpus_path = data_path(corpus_filename);
    let bytes = std::fs::read(&corpus_path).unwrap_or_else(|e| {
        panic!(
            "failed to read {} ({}). Run `cargo run --release --example mcts_corpus` first.",
            corpus_path.display(),
            e
        )
    });
    let corpus: Corpus = serde_json::from_slice(&bytes).expect("corpus JSON parses");
    corpus
        .entries
        .into_iter()
        .map(|entry| {
            let mut entry_mut = entry.clone();
            let template = realize_state(&mut entry_mut);
            RealizedEntry {
                state_id: entry.state_id,
                encounter: entry.encounter,
                tier: entry.tier,
                state_hash: entry_mut.state_hash,
                template,
            }
        })
        .collect()
}

fn load_realistic(corpus_filename: &str) -> Vec<RealizedEntry> {
    let corpus_path = data_path(corpus_filename);
    let bytes = std::fs::read(&corpus_path).unwrap_or_else(|e| {
        panic!(
            "failed to read {} ({}). Run `cargo run --release --example mcts_corpus_realistic` first.",
            corpus_path.display(),
            e
        )
    });
    let corpus: RealisticCorpus =
        serde_json::from_slice(&bytes).expect("realistic corpus JSON parses");
    // Content-side dispatch (relics/potions) must be installed before
    // any combat-loop call inside `scenario_to_combat_state`; the
    // library does this internally but we keep the explicit call here
    // so the harness's failure mode is the same shape as the starter
    // arm if dispatch ever regresses.
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    let mut out: Vec<RealizedEntry> = Vec::with_capacity(corpus.entries.len());
    for entry in corpus.entries {
        let path = PathBuf::from(&entry.fixture_path);
        let scenario = load_scenario_file(&path).unwrap_or_else(|e| {
            panic!(
                "failed to load realistic fixture {} ({}). \
                 The corpus may be stale — regenerate via `cargo run --release \
                 --example mcts_corpus_realistic`.",
                path.display(),
                e
            )
        });
        let mut template = scenario_to_combat_state(&scenario, entry.seed).unwrap_or_else(|e| {
            panic!(
                "scenario_to_combat_state failed for {} ({}). \
                 Likely a content-port gap (relic/card/monster); the realistic \
                 corpus generator should have filtered this entry out.",
                path.display(),
                e
            )
        });
        // The library returns the state pre-`start_combat` (callers
        // own the combat-loop progression). Drive it through to
        // `PlayerPlayPhase` so MCTS sees a real decision point — the
        // starter corpus's `realize_state` does the same via
        // `Mcts::warmup_random` plus an explicit start_combat/start_turn.
        // Without this, every realistic entry's PV would be "EndTurn"
        // ad infinitum (the only legal action in Setup phase).
        let _ = sts2_core::combat_loop::start_combat(&mut template);
        let _ = sts2_core::combat_loop::start_player_turn(&mut template);
        let state_hash = sts2_core::hashing::state_hash(&template);
        out.push(RealizedEntry {
            state_id: entry.state_id,
            encounter: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("<unknown>")
                .to_string(),
            tier: parse_realistic_tier(&entry.tier),
            state_hash,
            template,
        });
    }
    out
}

fn main() -> std::io::Result<()> {
    sts2_content::cards::dispatch::ensure_installed();

    let kind = parse_corpus_kind();
    // Bead `sts2-rs-65q2` (aze8.1): `--corpus-path <FILENAME>`
    // overrides the file the chosen schema is loaded from, so
    // callers can point at a custom-generated corpus (e.g. the
    // t4tn-balanced 100K NPZ) without touching the example.
    let corpus_path_override = parse_corpus_path();
    let corpus_filename: String = corpus_path_override.unwrap_or_else(|| match kind {
        CorpusKind::Starter => "mcts_corpus.json".to_string(),
        CorpusKind::Realistic => "mcts_corpus_realistic.json".to_string(),
    });
    // Realistic mode exercises card dispatch on cards the starter
    // pile never touches (BodySlam/Rampage/Headbutt with missing
    // target context). The per-state catch_unwind handles the abort
    // — this hook just suppresses the default-handler stderr spew
    // so the run log stays readable. Starter mode never panics so
    // the hook is a no-op there.
    if matches!(kind, CorpusKind::Realistic) {
        std::panic::set_hook(Box::new(|_| {}));
    }
    let realized = match kind {
        CorpusKind::Starter => load_starter(&corpus_filename),
        CorpusKind::Realistic => load_realistic(&corpus_filename),
    };

    let mut per_state: Vec<PerStateRow> = Vec::with_capacity(realized.len());

    for entry in &realized {
        let template = &entry.template;

        // Defensive: skip MCTS if the post-warmup state has no legal
        // actions. `warmup_random` already rolls back on terminal so
        // this only fires if the engine produced an empty
        // legal-action set in a non-terminal state (e.g. unhandled
        // PendingChoice) — emit a zero row instead of panicking the
        // run. Recording the state in the report still lets the diff
        // tool flag the regression on the next branch.
        let degenerate = is_terminal(template) || legal_actions(template).is_empty();

        let mut results: BTreeMap<String, BudgetResult> = BTreeMap::new();
        for &sims in BUDGETS {
            if degenerate {
                results.insert(
                    sims.to_string(),
                    BudgetResult {
                        wins: 0,
                        losses: 0,
                        ties: 0,
                        visits: 0,
                        pv_depth: 0,
                        pv: Vec::new(),
                        recommended: "<degenerate>".to_string(),
                        wall_ms: 0,
                    },
                );
                continue;
            }
            // Per-budget seed is deterministic in (state_id, sims) so
            // re-running yields byte-identical outputs.
            let config = MctsConfig {
                c_puct: std::f32::consts::SQRT_2,
                max_simulations: sims,
                // sts2-rs-bhet3: depth=20 (was 200). vbzji showed
                // depth=200 lets rollouts terminate naturally before
                // the cap, bypassing the leaf evaluator entirely. This
                // is a play-strength A/B → leaf eval must fire.
                max_rollout_depth: 20,
                seed: u64::from(entry.state_id)
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    .wrapping_add(u64::from(sims)),
                ..MctsConfig::default()
            };
            // Wrap the MCTS pass in catch_unwind so a content-gap
            // panic (e.g. BodySlam/Rampage/Headbutt expect an attack
            // target the deck-context doesn't supply) drops a single
            // state instead of poisoning the rest of the run.
            // panic=unwind is workspace-wide post-2vwf so this is
            // legal here. Each panicked state emits a recognizable
            // row that `mcts_ab_diff` can detect.
            let template_clone = template.clone();
            let start = Instant::now();
            let search_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut mcts = Mcts::new(template_clone, config);
                let recommended = mcts.search();
                let outcomes = mcts.root_outcomes();
                let pv = mcts.get_pv();
                (recommended, outcomes, pv)
            }));
            let wall_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            let Ok((recommended, outcomes, pv)) = search_result else {
                eprintln!(
                    "[mcts_ab] state_id={} sims={sims} PANICKED (content gap); \
                     emitting <panic> row",
                    entry.state_id,
                );
                results.insert(
                    sims.to_string(),
                    BudgetResult {
                        wins: 0,
                        losses: 0,
                        ties: 0,
                        visits: 0,
                        pv_depth: 0,
                        pv: Vec::new(),
                        recommended: "<panic>".to_string(),
                        wall_ms: 0,
                    },
                );
                continue;
            };
            let pv_depth = u32::try_from(pv.len()).unwrap_or(u32::MAX);

            // Aggregate root-level outcome counters. Each entry in
            // `outcomes` is a (action, wins, losses, ties, visits)
            // record; summing across actions gives the total at the
            // root, which is the canonical "trials" number.
            let wins: u32 = outcomes.iter().map(|o| o.wins).sum();
            let losses: u32 = outcomes.iter().map(|o| o.losses).sum();
            let ties: u32 = outcomes.iter().map(|o| o.ties).sum();
            let visits: u32 = outcomes.iter().map(|o| o.visits).sum();

            eprintln!(
                "[mcts_ab] state_id={} sims={sims} wall_ms={wall_ms}",
                entry.state_id
            );
            results.insert(
                sims.to_string(),
                BudgetResult {
                    wins,
                    losses,
                    ties,
                    visits,
                    pv_depth,
                    pv: pv.iter().map(action_to_string).collect(),
                    recommended: action_to_string(&recommended),
                    // Zeroed on disk so `diff -q` of two runs at the
                    // same git SHA is empty. The wall_ms above is the
                    // ground truth, logged to stderr.
                    wall_ms: 0,
                },
            );
        }

        per_state.push(PerStateRow {
            state_id: entry.state_id,
            encounter: entry.encounter.clone(),
            tier: entry.tier,
            state_hash: entry.state_hash,
            results,
        });
    }

    per_state.sort_by_key(|r| r.state_id);

    let sha = git_short_sha();
    let report = AbReport {
        version: 1,
        git_sha: sha.clone(),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        per_state,
    };

    // When `--corpus-path` overrides the default file, embed the
    // file stem in the output name so `mcts_ab_diff` can keep
    // separate runs apart (e.g. realistic-default vs t4tn-balanced
    // landing in the same data dir).
    let default_starter = "mcts_corpus.json";
    let default_realistic = "mcts_corpus_realistic.json";
    let is_default = match kind {
        CorpusKind::Starter => corpus_filename == default_starter,
        CorpusKind::Realistic => corpus_filename == default_realistic,
    };
    let filename = match (kind, is_default) {
        (CorpusKind::Starter, true) => format!("mcts_ab_{sha}.json"),
        (CorpusKind::Realistic, true) => format!("mcts_ab_realistic_{sha}.json"),
        (_, false) => {
            // Strip the `.json` suffix off the override so the
            // resulting filename is unambiguous and stable.
            let stem = corpus_filename
                .strip_suffix(".json")
                .unwrap_or(&corpus_filename);
            format!("mcts_ab_{stem}_{sha}.json")
        }
    };
    let path = data_path(&filename);
    write_json_pretty(&path, &report)?;
    eprintln!(
        "[mcts_ab] wrote {} states × {} budgets to {}",
        report.per_state.len(),
        BUDGETS.len(),
        path.display()
    );
    Ok(())
}
