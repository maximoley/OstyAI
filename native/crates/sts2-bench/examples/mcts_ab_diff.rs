//! `mcts_ab_diff` — diff two A/B reports for bead `sts2-rs-yuyc`.
//!
//! ```sh
//! cargo run --release --example mcts_ab_diff -- <baseline.json> <branch.json>
//! ```
//!
//! Computes per-budget aggregate deltas (tie-rate, win-rate, PV[0]
//! agreement, wall-clock), prints a markdown table, and — if
//! `data/mcts_oracle.json` is present in the same directory as
//! `branch.json` — a calibration MSE against the 50k-sim oracle.
//!
//! Exits non-zero when a regression threshold trips:
//!   - tie-rate up by more than 5 percentage points at any budget, OR
//!   - calibration MSE (vs oracle) up by more than 0.01.

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::process::ExitCode;

use common::{AbReport, BudgetResult, CorpusTier, PerStateRow};

const TIE_RATE_REGRESSION_PP: f64 = 0.05; // 5 percentage points
const CALIB_MSE_REGRESSION: f64 = 0.01;

#[derive(Debug, Clone, Copy, Default)]
struct BudgetAgg {
    states: u32,
    sum_tie_rate: f64,
    sum_win_rate: f64,
    pv_match: u32,
    sum_wall_ms: u64,
}

impl BudgetAgg {
    fn tie_rate(self) -> f64 {
        if self.states == 0 {
            return 0.0;
        }
        self.sum_tie_rate / f64::from(self.states)
    }
    fn win_rate(self) -> f64 {
        if self.states == 0 {
            return 0.0;
        }
        self.sum_win_rate / f64::from(self.states)
    }
    fn pv_agreement(self) -> f64 {
        if self.states == 0 {
            return 0.0;
        }
        f64::from(self.pv_match) / f64::from(self.states)
    }
    fn mean_wall_ms(self) -> f64 {
        if self.states == 0 {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)]
        let n = f64::from(self.states);
        #[allow(clippy::cast_precision_loss)]
        let s = self.sum_wall_ms as f64;
        s / n
    }
}

fn rates(r: &BudgetResult) -> (f64, f64) {
    let total = r.wins + r.losses + r.ties;
    if total == 0 {
        return (0.0, 0.0);
    }
    #[allow(clippy::cast_precision_loss)]
    let t = f64::from(total);
    (f64::from(r.wins) / t, f64::from(r.ties) / t)
}

fn load(path: &str) -> AbReport {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

fn index_by_state(report: &AbReport) -> BTreeMap<u32, &PerStateRow> {
    report.per_state.iter().map(|r| (r.state_id, r)).collect()
}

fn try_load_oracle(near: &str) -> Option<AbReport> {
    let dir = std::path::Path::new(near).parent()?;
    let oracle = dir.join("mcts_oracle.json");
    let bytes = std::fs::read(&oracle).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: mcts_ab_diff <baseline.json> <branch.json>");
        return ExitCode::from(2);
    }
    let baseline = load(&args[0]);
    let branch = load(&args[1]);
    let by_base = index_by_state(&baseline);
    let by_branch = index_by_state(&branch);

    // Collect the union of budget keys seen across both files. In
    // practice the A/B harness emits {500, 1000, 2000} on every state,
    // but tolerate other shapes for forward-compat.
    let mut budgets: Vec<String> = Vec::new();
    for r in baseline.per_state.iter().chain(branch.per_state.iter()) {
        for k in r.results.keys() {
            if !budgets.iter().any(|b| b == k) {
                budgets.push(k.clone());
            }
        }
    }
    // Sort budgets numerically when possible, else lexicographically.
    budgets.sort_by(|a, b| match (a.parse::<u32>(), b.parse::<u32>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        _ => a.cmp(b),
    });

    // Bead `sts2-rs-acbj` (step 6): per-tier aggregation. Same
    // aggregation loop as before, parameterized by an optional
    // tier filter. `None` reproduces the aggregate; `Some(tier)`
    // restricts to that tier's per-state rows.
    let aggregate = |tier_filter: Option<CorpusTier>| -> (BTreeMap<String, BudgetAgg>, BTreeMap<String, BudgetAgg>) {
        let mut base_aggs: BTreeMap<String, BudgetAgg> = BTreeMap::new();
        let mut branch_aggs: BTreeMap<String, BudgetAgg> = BTreeMap::new();
        for state_id in by_branch.keys() {
            let Some(b_row) = by_base.get(state_id) else {
                continue;
            };
            let br_row = by_branch[state_id];
            if let Some(t) = tier_filter {
                if br_row.tier != t {
                    continue;
                }
            }
            for budget in &budgets {
                let (Some(b_res), Some(br_res)) =
                    (b_row.results.get(budget), br_row.results.get(budget))
                else {
                    continue;
                };
                let (b_win, b_tie) = rates(b_res);
                let (br_win, br_tie) = rates(br_res);
                let b_pv0 = b_res
                    .pv
                    .first()
                    .map_or(b_res.recommended.as_str(), String::as_str);
                let br_pv0 = br_res
                    .pv
                    .first()
                    .map_or(br_res.recommended.as_str(), String::as_str);
                let pv_match = u32::from(b_pv0 == br_pv0);

                let entry_b = base_aggs.entry(budget.clone()).or_default();
                entry_b.states += 1;
                entry_b.sum_tie_rate += b_tie;
                entry_b.sum_win_rate += b_win;
                entry_b.sum_wall_ms += b_res.wall_ms;

                let entry_br = branch_aggs.entry(budget.clone()).or_default();
                entry_br.states += 1;
                entry_br.sum_tie_rate += br_tie;
                entry_br.sum_win_rate += br_win;
                entry_br.pv_match += pv_match;
                entry_br.sum_wall_ms += br_res.wall_ms;
            }
        }
        (base_aggs, branch_aggs)
    };
    let (base_aggs, branch_aggs) = aggregate(None);

    // Optional calibration MSE vs oracle.
    let oracle = try_load_oracle(&args[1]);
    let mut calib_branch: BTreeMap<String, f64> = BTreeMap::new();
    let mut calib_base: BTreeMap<String, f64> = BTreeMap::new();
    if let Some(orc) = &oracle {
        let by_orc = index_by_state(orc);
        for state_id in by_orc.keys() {
            let Some(o_row) = by_orc.get(state_id) else {
                continue;
            };
            // Oracle uses sims=50000; one budget per row.
            let Some((_, o_res)) = o_row.results.iter().next() else {
                continue;
            };
            let (o_win, _o_tie) = rates(o_res);
            for budget in &budgets {
                if let Some(br_row) = by_branch.get(state_id) {
                    if let Some(br_res) = br_row.results.get(budget) {
                        let (br_win, _) = rates(br_res);
                        let d = br_win - o_win;
                        *calib_branch.entry(budget.clone()).or_default() += d * d;
                    }
                }
                if let Some(b_row) = by_base.get(state_id) {
                    if let Some(b_res) = b_row.results.get(budget) {
                        let (b_win, _) = rates(b_res);
                        let d = b_win - o_win;
                        *calib_base.entry(budget.clone()).or_default() += d * d;
                    }
                }
            }
        }
        // Normalize MSE by state count per budget.
        for budget in &budgets {
            if let Some(agg) = branch_aggs.get(budget) {
                if agg.states > 0 {
                    if let Some(v) = calib_branch.get_mut(budget) {
                        *v /= f64::from(agg.states);
                    }
                }
            }
            if let Some(agg) = base_aggs.get(budget) {
                if agg.states > 0 {
                    if let Some(v) = calib_base.get_mut(budget) {
                        *v /= f64::from(agg.states);
                    }
                }
            }
        }
    }

    println!("# MCTS A/B diff");
    println!();
    println!("- baseline: `{}` ({})", args[0], baseline.git_sha);
    println!("- branch:   `{}` ({})", args[1], branch.git_sha);
    println!();
    println!(
        "| budget | base tie% | branch tie% | Δ tie pp | base win% | branch win% | Δ win pp | PV[0] agree | branch wall ms (avg) |"
    );
    println!("|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    let mut tripped_tie = false;
    for budget in &budgets {
        let b = base_aggs.get(budget).copied().unwrap_or_default();
        let br = branch_aggs.get(budget).copied().unwrap_or_default();
        let d_tie = br.tie_rate() - b.tie_rate();
        let d_win = br.win_rate() - b.win_rate();
        if d_tie > TIE_RATE_REGRESSION_PP {
            tripped_tie = true;
        }
        println!(
            "| {} | {:.1}% | {:.1}% | {:+.1} | {:.1}% | {:.1}% | {:+.1} | {:.0}% | {:.1} |",
            budget,
            b.tie_rate() * 100.0,
            br.tie_rate() * 100.0,
            d_tie * 100.0,
            b.win_rate() * 100.0,
            br.win_rate() * 100.0,
            d_win * 100.0,
            br.pv_agreement() * 100.0,
            br.mean_wall_ms(),
        );
    }

    // Bead `sts2-rs-acbj` (step 6): per-tier subtables. Sized for
    // a quick visual scan, not a primary regression gate (per-tier
    // CIs are wide on a 50-state corpus; the 200-state acbj v2
    // corpus tightens these).
    println!();
    println!("## Per-tier breakdown");
    for tier in [CorpusTier::Common, CorpusTier::Elite, CorpusTier::Boss] {
        let (b_tier, br_tier) = aggregate(Some(tier));
        // Skip the tier entirely if no states match (older 50-state
        // corpus may have zero of a given tier per budget).
        let n_states = br_tier.values().map(|a| a.states).max().unwrap_or(0);
        if n_states == 0 {
            continue;
        }
        println!();
        println!("### tier = {tier:?} (n={n_states} states)");
        println!();
        println!(
            "| budget | base tie% | branch tie% | Δ tie pp | base win% | branch win% | Δ win pp | PV[0] agree |"
        );
        println!("|---:|---:|---:|---:|---:|---:|---:|---:|");
        for budget in &budgets {
            let b = b_tier.get(budget).copied().unwrap_or_default();
            let br = br_tier.get(budget).copied().unwrap_or_default();
            let d_tie = br.tie_rate() - b.tie_rate();
            let d_win = br.win_rate() - b.win_rate();
            println!(
                "| {} | {:.1}% | {:.1}% | {:+.1} | {:.1}% | {:.1}% | {:+.1} | {:.0}% |",
                budget,
                b.tie_rate() * 100.0,
                br.tie_rate() * 100.0,
                d_tie * 100.0,
                b.win_rate() * 100.0,
                br.win_rate() * 100.0,
                d_win * 100.0,
                br.pv_agreement() * 100.0,
            );
        }
    }

    let mut tripped_calib = false;
    if oracle.is_some() {
        println!();
        println!("## Calibration vs oracle (win-rate MSE, lower = closer to 50k-sim ground truth)");
        println!();
        println!("| budget | base MSE | branch MSE | Δ |");
        println!("|---:|---:|---:|---:|");
        for budget in &budgets {
            let b = calib_base.get(budget).copied().unwrap_or(0.0);
            let br = calib_branch.get(budget).copied().unwrap_or(0.0);
            let d = br - b;
            if d > CALIB_MSE_REGRESSION {
                tripped_calib = true;
            }
            println!("| {budget} | {b:.4} | {br:.4} | {d:+.4} |");
        }
    } else {
        println!();
        println!("_(no oracle file alongside branch; skipping calibration MSE)_");
    }

    if tripped_tie || tripped_calib {
        eprintln!("mcts_ab_diff: regression detected (tie={tripped_tie}, calib={tripped_calib})");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
