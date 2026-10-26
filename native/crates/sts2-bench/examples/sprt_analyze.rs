//! `sprt_analyze` — post-hoc Sequential Probability Ratio Test on two
//! [`AbReport`] files (bead `sts2-rs-s6umt`).
//!
//! Pairs states by `state_id` between a baseline and a branch report,
//! classifies each paired state as a branch-win / branch-loss / draw
//! from the branch's perspective, and runs Wald's binomial LLR against
//! the H0: `Δelo == elo0` vs H1: `Δelo == elo1` hypotheses.
//!
//! Outputs:
//!   - per-step LLR trace (state index, cumulative W/L/D, LLR)
//!   - the state-index at which LLR first crossed an acceptance bound
//!     (`ln((1-β)/α)` for H1, `ln(β/(1-α))` for H0)
//!   - final verdict (H1 accepted / H0 accepted / inconclusive)
//!   - implied compute-savings ratio (`states_to_decision / total_states`)
//!
//! Use this to validate the SPRT math on existing report pairs BEFORE
//! wiring inline early-termination into `mcts_ab_value_net`. The inline
//! path is a follow-up — it requires restructuring `mcts_ab_value_net`
//! to run two evaluators in a single invocation with shared per-state
//! ordering (it currently runs one evaluator and writes a JSON).
//!
//! ```sh
//! cargo run --release --example sprt_analyze -- \
//!     <baseline.json> <branch.json> \
//!     [--elo0 0] [--elo1 10] [--alpha 0.05] [--beta 0.05] [--budget 300]
//! ```
//!
//! Exits 0 if a decision was reached; 1 if inconclusive; 2 on bad input.

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::process::ExitCode;

use common::{AbReport, BudgetResult, PerStateRow};

/// Per-state outcome from the branch's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trinomial {
    BranchWin,
    BranchLoss,
    Draw,
}

#[derive(Debug, Clone, Copy)]
struct SprtConfig {
    /// Lower Elo bound (null hypothesis). Default 0 (no difference).
    elo0: f64,
    /// Upper Elo bound (alternative hypothesis). Default 10.
    elo1: f64,
    /// Type-I error rate. Default 0.05.
    alpha: f64,
    /// Type-II error rate. Default 0.05.
    beta: f64,
}

impl Default for SprtConfig {
    fn default() -> Self {
        Self {
            elo0: 0.0,
            elo1: 10.0,
            alpha: 0.05,
            beta: 0.05,
        }
    }
}

impl SprtConfig {
    /// Wald acceptance bound for H1 (Δelo ≥ elo1).
    fn upper_bound(&self) -> f64 {
        ((1.0 - self.beta) / self.alpha).ln()
    }
    /// Wald rejection bound (accept H0).
    fn lower_bound(&self) -> f64 {
        (self.beta / (1.0 - self.alpha)).ln()
    }
}

/// Elo → expected score (`p_win` under the logistic Elo model).
/// Standard form: `p = 1 / (1 + 10^(-elo/400))`.
fn elo_to_score(elo: f64) -> f64 {
    1.0 / (1.0 + (-elo / 400.0 * std::f64::consts::LN_10).exp())
}

/// Binomial-style LLR after `(w, d, l)` paired outcomes.
///
/// Treats draws as half-wins and half-losses (standard chess-engine
/// convention; the "score" of a draw is 0.5). This is the binomial
/// approximation to GSPRT — exact-trinomial GSPRT would need full
/// `(W, D, L)` densities under each hypothesis. For STS2's per-state
/// paired comparisons where draws are common (both evaluators reach
/// the same MCTS recommendation), the binomial form is adequate and
/// matches fishtest's pre-2014 setup.
///
/// Returns `f64::NEG_INFINITY` / `f64::INFINITY` if one cell is zero
/// (degenerate); the caller treats those as "no signal yet".
fn binomial_llr(w: u32, d: u32, l: u32, cfg: &SprtConfig) -> f64 {
    let n = f64::from(w + d + l);
    if n < 1.0 {
        return 0.0;
    }
    let score = (f64::from(w) + 0.5 * f64::from(d)) / n;
    // Empirical variance of score around its sample mean — used as the
    // GSPRT-style variance term.
    let mean_sq = (f64::from(w) + 0.25 * f64::from(d)) / n;
    let var = (mean_sq - score * score).max(1e-9);

    let p0 = elo_to_score(cfg.elo0);
    let p1 = elo_to_score(cfg.elo1);

    // Wald LLR under Gaussian approximation:
    //   LLR ≈ (s - p0)² / (2 σ²) − (s - p1)² / (2 σ²)
    // (this is the standard chess-engine "simplified GSPRT" used by
    //   fishtest pre-2014; gives values directly comparable to the
    //   Wald bounds.)
    let llr = ((score - p0).powi(2) - (score - p1).powi(2)) / (2.0 * var);
    llr * n
}

fn index_by_state(report: &AbReport) -> BTreeMap<u32, &PerStateRow> {
    report.per_state.iter().map(|r| (r.state_id, r)).collect()
}

/// Reduce a `BudgetResult` to a single outcome label. With STS2's
/// 1-seed-per-state harness, exactly one of {wins, losses, ties} is
/// non-zero. We pick the dominant outcome; if multiple are non-zero
/// (multi-seed runs in future), majority wins, ties → draw.
fn outcome_label(r: &BudgetResult) -> char {
    if r.wins > r.losses && r.wins > r.ties {
        'W'
    } else if r.losses > r.wins && r.losses > r.ties {
        'L'
    } else {
        'D'
    }
}

fn classify(base: &BudgetResult, branch: &BudgetResult) -> Trinomial {
    match (outcome_label(base), outcome_label(branch)) {
        // Branch wins where baseline doesn't, or branch draws where baseline loses.
        ('L', 'W' | 'D') | ('D', 'W') => Trinomial::BranchWin,
        // Branch loses where baseline doesn't, or branch draws where baseline wins.
        ('W', 'L' | 'D') | ('D', 'L') => Trinomial::BranchLoss,
        // Identical outcome on both sides.
        _ => Trinomial::Draw,
    }
}

fn parse_args(args: &[String]) -> Result<(String, String, String, SprtConfig), String> {
    if args.len() < 2 {
        return Err("missing required positional args".into());
    }
    let mut cfg = SprtConfig::default();
    let mut budget: Option<String> = None;
    let mut i = 2;
    while i < args.len() {
        let arg = &args[i];
        let next = args.get(i + 1);
        match (arg.as_str(), next) {
            ("--elo0", Some(v)) => {
                cfg.elo0 = v.parse().map_err(|e| format!("--elo0: {e}"))?;
                i += 2;
            }
            ("--elo1", Some(v)) => {
                cfg.elo1 = v.parse().map_err(|e| format!("--elo1: {e}"))?;
                i += 2;
            }
            ("--alpha", Some(v)) => {
                cfg.alpha = v.parse().map_err(|e| format!("--alpha: {e}"))?;
                i += 2;
            }
            ("--beta", Some(v)) => {
                cfg.beta = v.parse().map_err(|e| format!("--beta: {e}"))?;
                i += 2;
            }
            ("--budget", Some(v)) => {
                budget = Some(v.clone());
                i += 2;
            }
            _ => return Err(format!("unknown flag or missing value: {arg}")),
        }
    }
    Ok((
        args[0].clone(),
        args[1].clone(),
        budget.unwrap_or_else(|| "300".into()),
        cfg,
    ))
}

fn load(path: &str) -> AbReport {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("failed to parse {path}: {e}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!(
            "usage: sprt_analyze <baseline.json> <branch.json> \
             [--elo0 0] [--elo1 10] [--alpha 0.05] [--beta 0.05] [--budget 300]"
        );
        return ExitCode::from(2);
    }
    let (base_path, branch_path, budget_key, cfg) = match parse_args(&args) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    let baseline = load(&base_path);
    let branch = load(&branch_path);
    let by_base = index_by_state(&baseline);

    let upper = cfg.upper_bound();
    let lower = cfg.lower_bound();

    println!("# SPRT analysis");
    println!("- baseline: {base_path}");
    println!("- branch:   {branch_path}");
    println!("- budget:   {budget_key}");
    println!(
        "- elo bounds: ({:.1}, {:.1}); alpha={:.3}, beta={:.3}",
        cfg.elo0, cfg.elo1, cfg.alpha, cfg.beta
    );
    println!("- Wald bounds: accept-H1 LLR ≥ {upper:.3}, accept-H0 LLR ≤ {lower:.3}");
    println!();

    // Pair states by state_id, ordered by branch's per_state ordering.
    let mut paired: Vec<(u32, Trinomial)> = Vec::new();
    for row in &branch.per_state {
        let Some(base_row) = by_base.get(&row.state_id) else {
            continue;
        };
        let (Some(base_r), Some(br_r)) = (
            base_row.results.get(&budget_key),
            row.results.get(&budget_key),
        ) else {
            continue;
        };
        paired.push((row.state_id, classify(base_r, br_r)));
    }
    if paired.is_empty() {
        eprintln!(
            "error: no paired states found at budget {budget_key}. \
             Check that both reports share state_ids and have that budget."
        );
        return ExitCode::from(2);
    }
    let total = paired.len();

    let (mut w, mut d, mut l) = (0u32, 0u32, 0u32);
    let mut decision_at: Option<(usize, &'static str)> = None;
    println!("| step | state_id | outcome | W | D | L | LLR |");
    println!("|-----:|---------:|:-------:|--:|--:|--:|----:|");
    for (step, (state_id, t)) in paired.iter().enumerate() {
        match t {
            Trinomial::BranchWin => w += 1,
            Trinomial::Draw => d += 1,
            Trinomial::BranchLoss => l += 1,
        }
        let llr = binomial_llr(w, d, l, &cfg);
        let label = match t {
            Trinomial::BranchWin => "W",
            Trinomial::Draw => "D",
            Trinomial::BranchLoss => "L",
        };
        if step < 50 || decision_at.is_none() {
            println!(
                "| {} | {} | {} | {} | {} | {} | {:.3} |",
                step + 1,
                state_id,
                label,
                w,
                d,
                l,
                llr
            );
        }
        // Minimum-sample guard: the empirical-variance LLR is unstable
        // at low N (variance floor of 1e-9 makes the first few steps
        // produce huge spurious LLRs). Standard chess-engine SPRT
        // practice (fishtest) uses min-games = 10–20 before reading the
        // bounds. We use 10.
        if decision_at.is_none() && step + 1 >= 10 {
            if llr >= upper {
                decision_at = Some((step + 1, "accept H1 (Δelo ≥ elo1)"));
            } else if llr <= lower {
                decision_at = Some((step + 1, "accept H0 (Δelo ≤ elo0)"));
            }
        }
    }
    println!();
    println!("## Final");
    println!("- total paired states: {total}");
    println!("- final W/D/L:        {w} / {d} / {l}");
    let final_llr = binomial_llr(w, d, l, &cfg);
    println!("- final LLR:          {final_llr:.3}");

    if let Some((step, verdict)) = decision_at {
        #[allow(clippy::cast_precision_loss)]
        let savings = 1.0 - (step as f64 / total as f64);
        println!("- decision:           {verdict} at step {step}/{total}");
        println!("- would-have-saved:   {:.1}% compute", savings * 100.0);
        ExitCode::from(0)
    } else {
        println!("- decision:           INCONCLUSIVE (LLR did not cross either bound)");
        println!("- would-have-saved:   0% (full corpus needed for this comparison)");
        ExitCode::from(1)
    }
}
