//! `mcts_oracle` — 50k-sim ground truth for bead `sts2-rs-yuyc`.
//!
//! Same shape as `mcts_ab`, but with a single `max_simulations =
//! 50000` budget. Writes to `data/mcts_oracle.json` (starter) or
//! `data/mcts_oracle_realistic.json` (realistic).
//!
//! **This binary is slow** — at the current ~140 searches/sec /
//! 1000-sim baseline, 50k sims ≈ 7s per state × 50 states ≈ 6 min.
//! The output is cached: re-running with an existing oracle file
//! skips any state whose (`state_hash`, sims=50000) tuple already
//! has an entry. Delete the file to force a rebuild.
//!
//! ## Corpus selection (bead `sts2-rs-32k2`, aze8.6)
//!
//! Unblocks `mcts_c_puct_sweep`'s calibration-MSE arm on the
//! realistic corpus (which the `ek2o` smoke flagged as
//! `oracle absent → MSE = 0`). The realistic arm re-realizes each
//! state from its `.run-source` TOML via `sts2_cli::
//! scenario_to_combat_state` (a2fh) and wraps the per-state 50k-sim
//! search in `catch_unwind` to absorb content-gap panics (BodySlam
//! / Headbutt targeting) the same way `ek2o` and the corpus_gen
//! `--source toml` path do.
//!
//! ```sh
//! cargo run --release --example mcts_oracle                         # realistic (default, flipped by sts2-rs-a6wt)
//! cargo run --release --example mcts_oracle -- --corpus starter     # legacy starter-deck (back-compat)
//! ```

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
use sts2_core::combat_loop::{start_combat, start_player_turn};
use sts2_core::{is_terminal, legal_actions, CombatPhase, CombatState};
use sts2_mcts::{Mcts, MctsConfig};

const ORACLE_SIMS: u32 = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CorpusKind {
    Starter,
    Realistic,
}

/// Reader-only mirror of `mcts_corpus_realistic.rs::RealisticCorpus`
/// (sts2-rs-4gei). Re-declared here so this harness doesn't need to
/// expose the generator example as a sibling lib target — same
/// pattern as `mcts_ab.rs` (ch1h) and `mcts_c_puct_sweep.rs` (ek2o).
#[derive(Debug, Clone, Deserialize, Serialize)]
struct RealisticCorpus {
    #[allow(dead_code)]
    version: u32,
    #[allow(dead_code)]
    git_sha: String,
    #[allow(dead_code)]
    generator: String,
    #[allow(dead_code)]
    bucket_globs: BTreeMap<String, String>,
    entries: Vec<RealisticCorpusEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RealisticCorpusEntry {
    state_id: u32,
    tier: String,
    fixture_path: String,
    #[allow(dead_code)]
    floor: u16,
    seed: u64,
    // gh #14: v3 corpora stratify on player_outcome and persist the
    // label here. Optional + default so v1/v2 JSON still loads.
    #[serde(default)]
    #[allow(dead_code)]
    player_outcome: Option<String>,
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
                        "[mcts_oracle] --corpus expects 'starter' or 'realistic', \
                         got {other:?}; defaulting to realistic",
                    );
                    return CorpusKind::Realistic;
                }
            }
        } else if let Some(eq) = arg.strip_prefix("--corpus=") {
            match eq {
                "starter" => return CorpusKind::Starter,
                "realistic" => return CorpusKind::Realistic,
                _ => {
                    eprintln!(
                        "[mcts_oracle] unknown --corpus value {eq:?}; defaulting to realistic",
                    );
                    return CorpusKind::Realistic;
                }
            }
        }
    }
    CorpusKind::Realistic
}

fn parse_realistic_tier(s: &str) -> CorpusTier {
    match s {
        "elite" => CorpusTier::Elite,
        "boss" => CorpusTier::Boss,
        _ => CorpusTier::Common,
    }
}

/// One realized state ready for the oracle search. Mirrors
/// `mcts_ab.rs::RealizedEntry`. The realistic arm sets `state_hash =
/// 0` since the source TOML path is the canonical identity (the
/// cache key still works — `(state_id, 0)` is unique per realistic
/// state); the starter arm uses `realize_state`'s deterministic hash
/// for cross-run cache hits.
struct RealizedEntry {
    state_id: u32,
    encounter: String,
    tier: CorpusTier,
    state_hash: u64,
    template: CombatState,
}

/// Advance `state` to the first `PlayerPlayPhase` decision point —
/// the realistic path needs this because `scenario_to_combat_state`
/// hands back the state pre-`start_combat`. Starter `realize_state`
/// already advances both internally.
fn advance_to_play_phase(state: &mut CombatState) {
    let _ = start_combat(state);
    if matches!(
        state.phase,
        CombatPhase::Setup | CombatPhase::PlayerTurnStart
    ) {
        let _ = start_player_turn(state);
    }
}

fn load_starter() -> Vec<RealizedEntry> {
    let corpus_path = data_path("mcts_corpus.json");
    let bytes = std::fs::read(&corpus_path).unwrap_or_else(|e| {
        panic!(
            "failed to read {} ({}). Run mcts_corpus first.",
            corpus_path.display(),
            e,
        )
    });
    let corpus: Corpus = serde_json::from_slice(&bytes).expect("starter corpus JSON parses");
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

fn load_realistic() -> Vec<RealizedEntry> {
    // Realistic-corpus content gaps are isolated via catch_unwind on
    // the per-state search below; dispatch installation here matches
    // mcts_ab / mcts_c_puct_sweep so the failure mode is consistent.
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    let corpus_path = data_path("mcts_corpus_realistic.json");
    let bytes = std::fs::read(&corpus_path).unwrap_or_else(|e| {
        panic!(
            "failed to read {} ({}). Run `cargo run --release --example \
             mcts_corpus_realistic` first.",
            corpus_path.display(),
            e,
        )
    });
    let corpus: RealisticCorpus =
        serde_json::from_slice(&bytes).expect("realistic corpus JSON parses");

    let mut out: Vec<RealizedEntry> = Vec::with_capacity(corpus.entries.len());
    for entry in corpus.entries {
        let path = PathBuf::from(&entry.fixture_path);
        let scenario = load_scenario_file(&path).unwrap_or_else(|e| {
            panic!(
                "failed to load realistic fixture {} ({}). \
                 The corpus may be stale — regenerate via `cargo run \
                 --release --example mcts_corpus_realistic`.",
                path.display(),
                e,
            )
        });
        let mut template = scenario_to_combat_state(&scenario, entry.seed).unwrap_or_else(|e| {
            panic!(
                "scenario_to_combat_state failed for {} ({}). \
                     Likely a content-port gap; the realistic corpus \
                     generator should have filtered this entry.",
                path.display(),
                e,
            )
        });
        advance_to_play_phase(&mut template);
        // Encounter label = first non-minion enemy species (informational
        // — the realistic corpus doesn't track per-state encounter IDs
        // because multi-monster fixtures don't map cleanly onto the
        // single-name slot the starter Corpus schema uses).
        let encounter = template
            .creatures
            .iter()
            .filter(|c| c.current_hp > 0)
            .find_map(|c| c.species.map(|s| format!("{s:?}")))
            .unwrap_or_else(|| "<realistic>".to_string());
        out.push(RealizedEntry {
            state_id: entry.state_id,
            encounter,
            tier: parse_realistic_tier(&entry.tier),
            state_hash: 0, // realistic states keyed by state_id alone
            template,
        });
    }
    out
}

fn main() -> std::io::Result<()> {
    sts2_content::cards::dispatch::ensure_installed();
    let kind = parse_corpus_kind();
    let oracle_filename = match kind {
        CorpusKind::Starter => "mcts_oracle.json",
        CorpusKind::Realistic => "mcts_oracle_realistic.json",
    };

    let entries = match kind {
        CorpusKind::Starter => load_starter(),
        CorpusKind::Realistic => load_realistic(),
    };
    eprintln!(
        "[mcts_oracle] {} corpus: {} realized states",
        match kind {
            CorpusKind::Starter => "starter",
            CorpusKind::Realistic => "realistic",
        },
        entries.len(),
    );

    // Load existing oracle (if present) so we can skip cached rows.
    let oracle_path = data_path(oracle_filename);
    let mut cached: BTreeMap<(u32, u64), PerStateRow> = BTreeMap::new();
    if let Ok(prev_bytes) = std::fs::read(&oracle_path) {
        if let Ok(prev) = serde_json::from_slice::<AbReport>(&prev_bytes) {
            for row in prev.per_state {
                if row.results.contains_key(&ORACLE_SIMS.to_string()) {
                    cached.insert((row.state_id, row.state_hash), row);
                }
            }
        }
    }

    // Silence per-panic stderr on the realistic arm; the dropped count
    // lands in the run summary instead. Starter arm has no content-gap
    // panics (curated 50-state corpus).
    if matches!(kind, CorpusKind::Realistic) {
        std::panic::set_hook(Box::new(|_| {}));
    }

    let mut per_state: Vec<PerStateRow> = Vec::with_capacity(entries.len());
    let mut dropped_searches: u64 = 0;

    for entry in entries {
        let hash = entry.state_hash;

        if let Some(row) = cached.get(&(entry.state_id, hash)) {
            eprintln!(
                "[mcts_oracle] cache hit state_id={} hash={hash:016x}",
                entry.state_id,
            );
            per_state.push(row.clone());
            continue;
        }

        if is_terminal(&entry.template) || legal_actions(&entry.template).is_empty() {
            let mut results: BTreeMap<String, BudgetResult> = BTreeMap::new();
            results.insert(
                ORACLE_SIMS.to_string(),
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
            per_state.push(PerStateRow {
                state_id: entry.state_id,
                encounter: entry.encounter,
                tier: entry.tier,
                state_hash: hash,
                results,
            });
            continue;
        }

        let config = MctsConfig {
            c_puct: std::f32::consts::SQRT_2,
            max_simulations: ORACLE_SIMS,
            // sts2-rs-bhet3: depth=20 (was 200). The 50k-sim oracle
            // anchors calibration MSE for other benches — it must
            // exercise the same leaf evaluator they do. depth=200
            // bypasses leaf eval (vbzji finding).
            max_rollout_depth: 20,
            // Distinct seed-mixer constant from `mcts_ab` so oracle
            // and A/B don't share PRNG state.
            seed: u64::from(entry.state_id)
                .wrapping_mul(0xBF58_476D_1CE4_E5B9)
                .wrapping_add(u64::from(ORACLE_SIMS)),
            ..MctsConfig::default()
        };

        // sts2-rs-32k2: catch_unwind on the realistic arm. Starter
        // never panics here (curated corpus), so the wrapper is a
        // no-op for that path.
        let search_outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut mcts = Mcts::new(entry.template.clone(), config);
            let start = Instant::now();
            let recommended = mcts.search();
            let wall_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            let outcomes = mcts.root_outcomes();
            let pv = mcts.get_pv();
            let pv_depth = u32::try_from(pv.len()).unwrap_or(u32::MAX);
            let wins: u32 = outcomes.iter().map(|o| o.wins).sum();
            let losses: u32 = outcomes.iter().map(|o| o.losses).sum();
            let ties: u32 = outcomes.iter().map(|o| o.ties).sum();
            let visits: u32 = outcomes.iter().map(|o| o.visits).sum();
            (
                wins,
                losses,
                ties,
                visits,
                pv_depth,
                pv,
                recommended,
                wall_ms,
            )
        }));
        let Ok((wins, losses, ties, visits, pv_depth, pv, recommended, wall_ms)) = search_outcome
        else {
            dropped_searches += 1;
            eprintln!(
                "[mcts_oracle] DROPPED state_id={} (content-gap panic; \
                 likely Headbutt/BodySlam targeting on realistic fixture)",
                entry.state_id,
            );
            // Still emit a row so the file's state_id coverage matches
            // the corpus; mark as <dropped> so the consumer (e.g.
            // mcts_c_puct_sweep) can skip these in calibration MSE.
            let mut results: BTreeMap<String, BudgetResult> = BTreeMap::new();
            results.insert(
                ORACLE_SIMS.to_string(),
                BudgetResult {
                    wins: 0,
                    losses: 0,
                    ties: 0,
                    visits: 0,
                    pv_depth: 0,
                    pv: Vec::new(),
                    recommended: "<dropped>".to_string(),
                    wall_ms: 0,
                },
            );
            per_state.push(PerStateRow {
                state_id: entry.state_id,
                encounter: entry.encounter,
                tier: entry.tier,
                state_hash: hash,
                results,
            });
            continue;
        };

        let mut results: BTreeMap<String, BudgetResult> = BTreeMap::new();
        eprintln!(
            "[mcts_oracle] state_id={} encounter={} hash={hash:016x} wall_ms={wall_ms}",
            entry.state_id, entry.encounter,
        );

        results.insert(
            ORACLE_SIMS.to_string(),
            BudgetResult {
                wins,
                losses,
                ties,
                visits,
                pv_depth,
                pv: pv.iter().map(action_to_string).collect(),
                recommended: action_to_string(&recommended),
                // Zeroed on disk for byte-stable diffs; see mcts_ab.rs.
                wall_ms: 0,
            },
        );

        per_state.push(PerStateRow {
            state_id: entry.state_id,
            encounter: entry.encounter,
            tier: entry.tier,
            state_hash: hash,
            results,
        });
    }

    per_state.sort_by_key(|r| r.state_id);

    let report = AbReport {
        version: 1,
        git_sha: git_short_sha(),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        per_state,
    };

    write_json_pretty(&oracle_path, &report)?;
    eprintln!(
        "[mcts_oracle] wrote {} states @ {} sims to {} (dropped {dropped_searches} to panics)",
        report.per_state.len(),
        ORACLE_SIMS,
        oracle_path.display(),
    );
    Ok(())
}
