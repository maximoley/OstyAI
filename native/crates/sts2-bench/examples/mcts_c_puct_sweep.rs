//! `mcts_c_puct_sweep` — c_puct re-tune sweep harness for bead
//! `sts2-rs-y2eu` (W5).
//!
//! Sweeps `c_puct ∈ {0.7, 1.0, 1.41, 2.0, 3.0}` × `sims ∈ {500, 1000,
//! 2000}` × the seeded mid-Ironclad corpus, using ≥3 seeds per
//! (state, c_puct, sims) to average out the per-seed determinization
//! noise.
//!
//! ## Corpus selection (bead `sts2-rs-ek2o`, aze8.3)
//!
//! The starter-deck audit (`docs/audits/2026-05-17-starter-deck-audit.md`)
//! flagged this harness as MIGRATE: tuning `c_puct` against starter
//! decks risks fitting an exploration constant that's only optimal for
//! the saturated-eval starter-corpus distribution. The new
//! `mcts_corpus_realistic.json` (from `mcts_corpus_realistic.rs`,
//! bead `sts2-rs-4gei`) re-realizes 50 states from real `.run-source`
//! TOML fixtures via `sts2_cli::scenario_to_combat_state`.
//!
//! - `cargo run --release --example mcts_c_puct_sweep` (default — starter)
//! - `cargo run --release --example mcts_c_puct_sweep -- --corpus realistic`
//!   re-fits the sweep on the realistic-deck corpus.
//!
//! Output filename tags the corpus (`mcts_c_puct_sweep_<sha>.json` vs
//! `mcts_c_puct_sweep_realistic_<sha>.json`) so a single bench
//! invocation can produce a side-by-side comparison.
//!
//! Ranking signals:
//!   1. Calibration MSE vs the 50k-sim oracle (lower is better) —
//!      delta of mean root win-rate vs the oracle's win-rate, squared
//!      and averaged across all states. Same metric as `mcts_ab_diff`.
//!   2. PV agreement across sim budgets (higher is better) — fraction
//!      of (state, seed, sims-pair) tuples where the recommended root
//!      action matches across two sim budgets at the same c_puct.
//!
//! Decision rule (matches the bead text):
//!   - Winner if a c_puct beats the current default by ≥2pp absolute
//!     calibration MSE (averaged across budgets) OR ≥5pp PV agreement.
//!   - Tie-break by lower wall time at sims=1000.

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use common::{
    action_to_string, data_path, git_short_sha, realize_state, write_json_pretty, AbReport, Corpus,
};
use serde::{Deserialize, Serialize};
use sts2_cli::scenario_to_combat_state;
use sts2_cli::sim::scenario::toml_file::load_scenario_file;
use sts2_core::combat_loop::{start_combat, start_player_turn};
use sts2_core::{is_terminal, legal_actions, CombatPhase, CombatState};
use sts2_mcts::{Mcts, MctsConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CorpusKind {
    Starter,
    Realistic,
}

/// Reader-only mirror of `mcts_corpus_realistic.rs::RealisticCorpus`
/// (sts2-rs-4gei). Re-declared here so this harness doesn't need to
/// expose the generator example as a sibling lib target — same pattern
/// as `mcts_ab.rs` (sts2-rs-ch1h).
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
    #[allow(dead_code)]
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
                        "[c_puct_sweep] --corpus expects 'starter' or 'realistic', \
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
                        "[c_puct_sweep] unknown --corpus value {eq:?}; defaulting to realistic",
                    );
                    return CorpusKind::Realistic;
                }
            }
        }
    }
    CorpusKind::Realistic
}

/// Drive `state` from its post-`scenario_to_combat_state` Setup phase
/// up to the first `PlayerPlayPhase` decision point — same path the
/// `realize_state` helper takes for the starter corpus. The realistic
/// path needs this driver explicitly because `scenario_to_combat_state`
/// hands back the state pre-`start_combat`; the starter corpus's
/// `realize_state` already advances through both calls internally.
fn advance_to_play_phase(state: &mut CombatState) {
    let _ = start_combat(state);
    if matches!(
        state.phase,
        CombatPhase::Setup | CombatPhase::PlayerTurnStart
    ) {
        let _ = start_player_turn(state);
    }
}

const C_PUCT_GRID: &[f32] = &[0.7, 1.0, std::f32::consts::SQRT_2, 2.0, 3.0];
const SIMS_GRID: &[u32] = &[500, 1000, 2000];
const SEEDS_PER_STATE: u32 = 3;

/// Current default — used to tag the "default" row in the report.
const DEFAULT_C_PUCT: f32 = std::f32::consts::SQRT_2;

/// Decision thresholds (from bead text).
const CALIB_WIN_DELTA: f64 = 0.02; // 2pp absolute calibration MSE drop
const PV_WIN_DELTA: f64 = 0.05; // 5pp PV agreement gain

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SweepRow {
    c_puct: f32,
    sims: u32,
    states: u32,
    seeds_per_state: u32,
    /// Mean per-state win-rate at this (c_puct, sims).
    mean_win_rate: f64,
    /// Mean calibration MSE vs oracle (per-state mean across seeds,
    /// then squared diff vs oracle, averaged across states).
    calib_mse: f64,
    /// Total wall time across all (state, seed) searches at this
    /// (c_puct, sims), milliseconds.
    total_wall_ms: u64,
    mean_wall_ms_per_search: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SweepReport {
    version: u32,
    git_sha: String,
    engine_version: String,
    rows: Vec<SweepRow>,
    /// PV-agreement matrix per c_puct: keyed by `"500_vs_1000"` etc.
    pv_agreement: BTreeMap<String, BTreeMap<String, f64>>,
}

#[derive(Debug, Clone, Copy, Default)]
struct PerCellAgg {
    state_count: u32,
    sum_win_rate_per_state: f64,
    sum_sq_diff_oracle: f64,
    total_wall_ms: u64,
    n_searches: u32,
}

fn rates(wins: u32, losses: u32, ties: u32) -> f64 {
    let total = wins + losses + ties;
    if total == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let t = f64::from(total);
    f64::from(wins) / t
}

fn try_load_oracle() -> Option<BTreeMap<u32, f64>> {
    let path = data_path("mcts_oracle.json");
    let bytes = std::fs::read(&path).ok()?;
    let report: AbReport = serde_json::from_slice(&bytes).ok()?;
    let mut out = BTreeMap::new();
    for row in report.per_state {
        // Oracle has a single "50000" budget per row.
        if let Some((_, res)) = row.results.iter().next() {
            out.insert(row.state_id, rates(res.wins, res.losses, res.ties));
        }
    }
    Some(out)
}

#[allow(clippy::too_many_lines)]
fn main() -> std::io::Result<()> {
    sts2_content::cards::dispatch::ensure_installed();
    let kind = parse_corpus_kind();

    let oracle = try_load_oracle();
    if oracle.is_none() {
        eprintln!(
            "[c_puct_sweep] WARN: data/mcts_oracle.json not found — calibration MSE will be 0. \
             Run `cargo run --release --example mcts_oracle` first for the full ranking signal."
        );
    }
    let oracle = oracle.unwrap_or_default();

    // Realize each corpus state once (cloned per search).
    let mut templates: Vec<(u32, CombatState)> = Vec::new();
    let n_entries_total = match kind {
        CorpusKind::Starter => {
            let corpus_path = data_path("mcts_corpus.json");
            let bytes = std::fs::read(&corpus_path).unwrap_or_else(|e| {
                panic!(
                    "failed to read {} ({}). Run `cargo run --release --example mcts_corpus` \
                     first.",
                    corpus_path.display(),
                    e,
                )
            });
            let corpus: Corpus =
                serde_json::from_slice(&bytes).expect("starter corpus JSON parses");
            eprintln!(
                "[c_puct_sweep] realizing {} starter-corpus states…",
                corpus.entries.len()
            );
            let total = corpus.entries.len();
            templates.reserve(total);
            for entry in corpus.entries {
                let mut e = entry.clone();
                let st = realize_state(&mut e);
                if is_terminal(&st) || legal_actions(&st).is_empty() {
                    eprintln!(
                        "[c_puct_sweep] skipping degenerate state_id={} ({})",
                        entry.state_id, entry.encounter,
                    );
                    continue;
                }
                templates.push((entry.state_id, st));
            }
            total
        }
        CorpusKind::Realistic => {
            // bead `sts2-rs-ek2o`: re-realize each entry from its source
            // `.run-source` TOML via the a2fh library fn. The relic /
            // potion dispatch tables must be installed before any
            // combat-loop call inside `scenario_to_combat_state` — the
            // library does this internally but we mirror `mcts_ab.rs`'s
            // explicit calls so failure modes line up across the two
            // harnesses.
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
            eprintln!(
                "[c_puct_sweep] realizing {} realistic-corpus states…",
                corpus.entries.len()
            );
            let total = corpus.entries.len();
            templates.reserve(total);
            for entry in corpus.entries {
                let path = PathBuf::from(&entry.fixture_path);
                let scenario = load_scenario_file(&path).unwrap_or_else(|e| {
                    panic!(
                        "failed to load realistic fixture {} ({}). The corpus may be stale — \
                         regenerate via `cargo run --release --example mcts_corpus_realistic`.",
                        path.display(),
                        e,
                    )
                });
                let mut state =
                    scenario_to_combat_state(&scenario, entry.seed).unwrap_or_else(|e| {
                        panic!(
                            "scenario_to_combat_state failed for {} ({}). Likely a content-port \
                             gap; the realistic corpus generator should have filtered this entry.",
                            path.display(),
                            e,
                        )
                    });
                advance_to_play_phase(&mut state);
                if is_terminal(&state) || legal_actions(&state).is_empty() {
                    eprintln!(
                        "[c_puct_sweep] skipping degenerate state_id={} (realistic, \
                         fixture={})",
                        entry.state_id,
                        path.display(),
                    );
                    continue;
                }
                templates.push((entry.state_id, state));
            }
            total
        }
    };
    eprintln!(
        "[c_puct_sweep] {} non-degenerate states (of {n_entries_total} total)",
        templates.len(),
    );

    // Per-cell aggregator + raw samples for PV agreement across sims.
    let mut cells: BTreeMap<(u32, u32), PerCellAgg> = BTreeMap::new();
    // (c_puct_idx, state_id, seed) -> sims -> recommended action string
    type PvKey = (u32, u32, u32);
    let mut recommended: BTreeMap<PvKey, BTreeMap<u32, String>> = BTreeMap::new();

    // sts2-rs-ek2o: count realistic-corpus searches dropped to
    // content-gap panics (BodySlam / Headbutt / etc. expecting a target
    // the deck-context doesn't supply). Silenced via `catch_unwind` so
    // a single panicky fixture doesn't kill the whole sweep — mirrors
    // the corpus_gen `--source toml` pattern. Suppress the per-panic
    // stderr spew with a custom hook so the log stays readable; the
    // aggregate count lands in the report header instead.
    let mut dropped_searches: u64 = 0;
    if matches!(kind, CorpusKind::Realistic) {
        std::panic::set_hook(Box::new(|_| {}));
    }

    for (cp_idx, &c_puct) in C_PUCT_GRID.iter().enumerate() {
        let cp_idx_u = u32::try_from(cp_idx).unwrap();
        eprintln!("[c_puct_sweep] === c_puct={c_puct:.4} ===");
        for &sims in SIMS_GRID {
            let cell_start = Instant::now();
            for (state_id, template) in &templates {
                // Per-state mean win-rate across SEEDS_PER_STATE seeds.
                let mut sum_win_rate = 0.0_f64;
                let mut wr_samples: u32 = 0;
                for seed_n in 0..SEEDS_PER_STATE {
                    // Deterministic seed mixing (state_id, sims, seed_n,
                    // c_puct_idx). Same shape as mcts_ab harness.
                    let seed = u64::from(*state_id)
                        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                        .wrapping_add(u64::from(sims))
                        .wrapping_add(u64::from(seed_n).wrapping_mul(0xBF58_476D_1CE4_E5B9))
                        .wrapping_add(u64::from(cp_idx_u).wrapping_mul(0x94D0_49BB_1331_11EB));
                    let cfg = MctsConfig {
                        c_puct,
                        max_simulations: sims,
                        // sts2-rs-bhet3: depth=20 (was 200). c_puct
                        // tuning is a play-strength sweep; depth=200
                        // bypasses the leaf evaluator entirely (vbzji
                        // finding).
                        max_rollout_depth: 20,
                        seed,
                        ..MctsConfig::default()
                    };
                    // catch_unwind isolates BodySlam / Headbutt /
                    // content-gap panics on the realistic corpus. The
                    // search owns its state clone, so unwind safety is
                    // local. A dropped seed contributes no sample to
                    // the per-state mean (denominator excludes panics).
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut mcts = Mcts::new(template.clone(), cfg);
                        let t0 = Instant::now();
                        let rec = mcts.search();
                        let wall_ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
                        let outcomes = mcts.root_outcomes();
                        let wins: u32 = outcomes.iter().map(|o| o.wins).sum();
                        let losses: u32 = outcomes.iter().map(|o| o.losses).sum();
                        let ties: u32 = outcomes.iter().map(|o| o.ties).sum();
                        (rates(wins, losses, ties), wall_ms, action_to_string(&rec))
                    }));
                    let Ok((wr, wall_ms, rec_str)) = outcome else {
                        dropped_searches += 1;
                        continue;
                    };
                    sum_win_rate += wr;
                    wr_samples += 1;

                    let cell = cells.entry((cp_idx_u, sims)).or_default();
                    cell.total_wall_ms += wall_ms;
                    cell.n_searches += 1;

                    recommended
                        .entry((cp_idx_u, *state_id, seed_n))
                        .or_default()
                        .insert(sims, rec_str);
                }
                if wr_samples == 0 {
                    // Every seed for this (state, c_puct, sims) cell
                    // dropped — exclude the state from this cell's
                    // calibration MSE / mean win-rate accumulators so
                    // a content-gap state doesn't poison the cell mean.
                    continue;
                }
                let mean_wr = sum_win_rate / f64::from(wr_samples);
                let cell = cells.entry((cp_idx_u, sims)).or_default();
                cell.state_count += 1;
                cell.sum_win_rate_per_state += mean_wr;
                if let Some(&o_wr) = oracle.get(state_id) {
                    let d = mean_wr - o_wr;
                    cell.sum_sq_diff_oracle += d * d;
                }
            }
            eprintln!(
                "[c_puct_sweep]   sims={sims} done in {:.1}s",
                cell_start.elapsed().as_secs_f64()
            );
        }
    }
    if dropped_searches > 0 {
        eprintln!(
            "[c_puct_sweep] {dropped_searches} searches dropped to content-gap panics \
             (likely Headbutt/BodySlam targeting on realistic fixtures)",
        );
    }

    // PV-agreement across sim budgets, per c_puct. Pairs:
    // (500,1000), (500,2000), (1000,2000), and overall (any matching pair).
    let pairs: Vec<(u32, u32)> = vec![(500, 1000), (500, 2000), (1000, 2000)];
    let mut pv_agreement: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for (cp_idx, &c_puct) in C_PUCT_GRID.iter().enumerate() {
        let cp_idx_u = u32::try_from(cp_idx).unwrap();
        let cp_key = format!("{c_puct:.4}");
        let entry = pv_agreement.entry(cp_key).or_default();
        for &(a, b) in &pairs {
            let mut total = 0_u32;
            let mut matches = 0_u32;
            for (key, by_sims) in &recommended {
                if key.0 != cp_idx_u {
                    continue;
                }
                let (Some(ra), Some(rb)) = (by_sims.get(&a), by_sims.get(&b)) else {
                    continue;
                };
                total += 1;
                if ra == rb {
                    matches += 1;
                }
            }
            let agree = if total == 0 {
                0.0
            } else {
                f64::from(matches) / f64::from(total)
            };
            entry.insert(format!("{a}_vs_{b}"), agree);
        }
        // Overall = mean of the three pairs.
        let mean: f64 = entry.values().sum::<f64>() / pairs.len() as f64;
        entry.insert("overall".to_string(), mean);
    }

    // Build rows.
    let mut rows: Vec<SweepRow> = Vec::new();
    for (cp_idx, &c_puct) in C_PUCT_GRID.iter().enumerate() {
        let cp_idx_u = u32::try_from(cp_idx).unwrap();
        for &sims in SIMS_GRID {
            let cell = cells.get(&(cp_idx_u, sims)).copied().unwrap_or_default();
            let n = if cell.state_count == 0 {
                1
            } else {
                cell.state_count
            };
            let mean_win_rate = cell.sum_win_rate_per_state / f64::from(n);
            let calib_mse = cell.sum_sq_diff_oracle / f64::from(n);
            #[allow(clippy::cast_precision_loss)]
            let mean_wall = if cell.n_searches == 0 {
                0.0
            } else {
                cell.total_wall_ms as f64 / f64::from(cell.n_searches)
            };
            rows.push(SweepRow {
                c_puct,
                sims,
                states: cell.state_count,
                seeds_per_state: SEEDS_PER_STATE,
                mean_win_rate,
                calib_mse,
                total_wall_ms: cell.total_wall_ms,
                mean_wall_ms_per_search: mean_wall,
            });
        }
    }

    // ─── Print markdown report ────────────────────────────────────────
    let corpus_label = match kind {
        CorpusKind::Starter => "starter",
        CorpusKind::Realistic => "realistic",
    };
    println!("# MCTS c_puct sweep (bead sts2-rs-y2eu)");
    println!();
    println!(
        "Corpus: {corpus_label} — {n_entries_total} states ({} non-degenerate × {} seeds each).",
        templates.len(),
        SEEDS_PER_STATE,
    );
    println!("Oracle present: {}.", !oracle.is_empty());
    println!();
    println!("## Per (c_puct, sims) cell");
    println!();
    println!("| c_puct | sims | mean win% | calib MSE | mean wall ms / search |");
    println!("|---:|---:|---:|---:|---:|");
    for row in &rows {
        let marker = if (row.c_puct - DEFAULT_C_PUCT).abs() < 1e-4 {
            " *"
        } else {
            ""
        };
        println!(
            "| {:.4}{} | {} | {:.1}% | {:.4} | {:.1} |",
            row.c_puct,
            marker,
            row.sims,
            row.mean_win_rate * 100.0,
            row.calib_mse,
            row.mean_wall_ms_per_search,
        );
    }
    println!();
    println!("`*` = current `MctsConfig::c_puct` default.");
    println!();

    println!("## PV agreement across sim budgets");
    println!();
    println!("| c_puct | 500↔1000 | 500↔2000 | 1000↔2000 | overall |");
    println!("|---:|---:|---:|---:|---:|");
    for &c_puct in C_PUCT_GRID {
        let key = format!("{c_puct:.4}");
        let row = pv_agreement.get(&key).cloned().unwrap_or_default();
        let g = |k: &str| row.get(k).copied().unwrap_or(0.0) * 100.0;
        let marker = if (c_puct - DEFAULT_C_PUCT).abs() < 1e-4 {
            " *"
        } else {
            ""
        };
        println!(
            "| {:.4}{} | {:.0}% | {:.0}% | {:.0}% | {:.0}% |",
            c_puct,
            marker,
            g("500_vs_1000"),
            g("500_vs_2000"),
            g("1000_vs_2000"),
            g("overall"),
        );
    }
    println!();

    // ─── Decision ─────────────────────────────────────────────────────
    // Per c_puct, average calib_mse across the 3 sim budgets and pick
    // the overall PV agreement. Compare to default's averages.
    let mut per_cp: BTreeMap<u32, (f64, f64, f64)> = BTreeMap::new();
    for (cp_idx, &c_puct) in C_PUCT_GRID.iter().enumerate() {
        let cp_idx_u = u32::try_from(cp_idx).unwrap();
        let calib_avg: f64 = SIMS_GRID
            .iter()
            .map(|&s| {
                cells.get(&(cp_idx_u, s)).map_or(0.0, |c| {
                    let n = if c.state_count == 0 { 1 } else { c.state_count };
                    c.sum_sq_diff_oracle / f64::from(n)
                })
            })
            .sum::<f64>()
            / SIMS_GRID.len() as f64;
        let pv_overall = pv_agreement
            .get(&format!("{c_puct:.4}"))
            .and_then(|m| m.get("overall").copied())
            .unwrap_or(0.0);
        let wall_at_1000 = cells.get(&(cp_idx_u, 1000)).map_or(0.0, |c| {
            if c.n_searches == 0 {
                0.0
            } else {
                #[allow(clippy::cast_precision_loss)]
                let s = c.total_wall_ms as f64;
                s / f64::from(c.n_searches)
            }
        });
        per_cp.insert(cp_idx_u, (calib_avg, pv_overall, wall_at_1000));
    }
    let default_idx = C_PUCT_GRID
        .iter()
        .position(|c| (c - DEFAULT_C_PUCT).abs() < 1e-4)
        .map(|i| u32::try_from(i).unwrap())
        .expect("DEFAULT_C_PUCT in C_PUCT_GRID");
    let (def_calib, def_pv, def_wall) = per_cp[&default_idx];

    println!("## Decision");
    println!();
    println!(
        "Default `c_puct = {DEFAULT_C_PUCT:.4}`: calib MSE (avg over sims) = {def_calib:.4}, \
         PV overall = {:.0}%, wall@1000 = {def_wall:.1}ms.",
        def_pv * 100.0
    );
    println!();
    let mut winners: Vec<(f32, f64, f64, f64)> = Vec::new();
    for (cp_idx, &c_puct) in C_PUCT_GRID.iter().enumerate() {
        let cp_idx_u = u32::try_from(cp_idx).unwrap();
        if cp_idx_u == default_idx {
            continue;
        }
        let (calib, pv, wall) = per_cp[&cp_idx_u];
        let calib_better = (def_calib - calib) >= CALIB_WIN_DELTA;
        let pv_better = (pv - def_pv) >= PV_WIN_DELTA;
        if calib_better || pv_better {
            winners.push((c_puct, calib, pv, wall));
            println!(
                "- `c_puct = {c_puct:.4}` BEATS default: calib Δ = {:+.4} (need ≥{:+.4} drop), \
                 PV Δ = {:+.1}pp (need ≥{:+.1}pp gain), wall@1000 = {wall:.1}ms.",
                def_calib - calib,
                CALIB_WIN_DELTA,
                (pv - def_pv) * 100.0,
                PV_WIN_DELTA * 100.0,
            );
        }
    }
    if winners.is_empty() {
        println!(
            "**No winner.** Keeping default `c_puct = {DEFAULT_C_PUCT:.4}` \
             (≈√2). The sweep harness lands as future tooling."
        );
    } else {
        // Tie-break by wall time at sims=1000 (lower better).
        winners.sort_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal));
        let (c, calib, pv, wall) = winners[0];
        println!();
        println!(
            "**Winner: c_puct = {c:.4}** (calib {calib:.4}, PV {:.0}%, wall@1000 {wall:.1}ms). \
             Update `MctsConfig::c_puct` default.",
            pv * 100.0
        );
    }

    // ─── Persist JSON for archival ────────────────────────────────────
    let sha = git_short_sha();
    let report = SweepReport {
        version: 1,
        git_sha: sha.clone(),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        rows,
        pv_agreement,
    };
    // sts2-rs-ek2o: tag the realistic output with `_realistic` so a
    // single bench session can produce starter + realistic artifacts
    // side-by-side without one clobbering the other.
    let corpus_suffix = match kind {
        CorpusKind::Starter => "",
        CorpusKind::Realistic => "_realistic",
    };
    let path = data_path(&format!("mcts_c_puct_sweep{corpus_suffix}_{sha}.json"));
    write_json_pretty(&path, &report)?;
    eprintln!("[c_puct_sweep] wrote {}", path.display());
    Ok(())
}
