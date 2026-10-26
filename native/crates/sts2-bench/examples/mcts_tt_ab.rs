//! `mcts_tt_ab` — bead `sts2-rs-7z97` (W6) targeted A/B for the
//! transposition table.
//!
//! For each corpus state at sims ∈ {500, 1000, 2000}, run MCTS twice
//! with a fixed seed:
//!   * baseline (`use_transposition_table = false`)
//!   * candidate (`use_transposition_table = true`)
//!
//! Reports per-budget aggregate root-level win-rate delta, allocation
//! delta, and TT-hit count. Intended as a one-shot measurement to
//! justify the default-OFF gating decision; not a regression gate.
//!
//! **This harness no longer measures anything (2026-08-12).**
//! `Mcts::with_policy` has forced `use_transposition_table = true` on
//! every construction since `755819594` (2026-06-22), so both arms below
//! run with the table ON and the reported delta is null by construction.
//! Reviving it means adding a real seam — making that assignment
//! conditional, or building the tree by a path that respects the config.
//! Until then do not read a null here as evidence about the
//! transposition table. The live hit rate is measured by
//! `mcts_permutation_probe` instead: 0.1518 hits/sim on the deployed
//! dialect over harvested positions, merging 12.4% of node-creation
//! attempts.
//!
//! ```sh
//! cargo run --release --example mcts_tt_ab
//! ```

#[path = "common/mod.rs"]
mod common;

use std::time::Instant;

use common::{data_path, realize_state, Corpus};
use sts2_core::{is_terminal, legal_actions};
use sts2_mcts::{Mcts, MctsConfig};

const BUDGETS: &[u32] = &[500, 1000, 2000];

#[derive(Default, Clone, Copy)]
struct Agg {
    states: u32,
    sum_win_rate: f64,
    sum_alloc: u64,
    sum_hits: u64,
    sum_wall_ms: u64,
}

fn winrate(w: u32, l: u32, t: u32) -> f64 {
    let total = w + l + t;
    if total == 0 {
        return 0.0;
    }
    f64::from(w) / f64::from(total)
}

fn main() {
    sts2_content::cards::dispatch::ensure_installed();

    let corpus_path = data_path("mcts_corpus.json");
    let bytes = std::fs::read(&corpus_path).expect("corpus present (run mcts_corpus first)");
    let corpus: Corpus = serde_json::from_slice(&bytes).expect("corpus JSON parses");

    println!("# MCTS transposition-table A/B (bead sts2-rs-7z97)");
    println!();
    println!("| budget | states | base win% | tt win% | Δ win pp | base alloc | tt alloc | Δ alloc % | tt hits/sim | base ms | tt ms |");
    println!("|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");

    for &sims in BUDGETS {
        let mut base_agg = Agg::default();
        let mut tt_agg = Agg::default();

        for entry in &corpus.entries {
            let mut entry_mut = entry.clone();
            let template = realize_state(&mut entry_mut);
            if is_terminal(&template) || legal_actions(&template).is_empty() {
                continue;
            }

            let seed = u64::from(entry.state_id)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(u64::from(sims));
            let cfg_off = MctsConfig {
                max_simulations: sims,
                // sts2-rs-bhet3: depth=20 (was 200). Transposition-
                // table A/B measures search quality at the same
                // depth-cut behavior the consumers see — depth=200
                // bypasses leaf eval (vbzji finding) and the original
                // 7z97 verdict ("inconclusive") was at that broken
                // regime.
                max_rollout_depth: 20,
                seed,
                use_transposition_table: false,
                regularized_pi_temperature: 0.1,
                ..MctsConfig::default()
            };
            let cfg_on = MctsConfig {
                use_transposition_table: true,
                regularized_pi_temperature: 0.1,
                ..cfg_off
            };

            // Baseline.
            let mut m = Mcts::new(template.clone(), cfg_off);
            let t0 = Instant::now();
            let _ = m.search();
            let ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
            let outs = m.root_outcomes();
            let (w, l, t): (u32, u32, u32) = outs.iter().fold((0, 0, 0), |(a, b, c), o| {
                (a + o.wins, b + o.losses, c + o.ties)
            });
            base_agg.states += 1;
            base_agg.sum_win_rate += winrate(w, l, t);
            base_agg.sum_alloc += m.nodes_allocated();
            base_agg.sum_wall_ms += ms;

            // TT-on.
            let mut m = Mcts::new(template, cfg_on);
            let t0 = Instant::now();
            let _ = m.search();
            let ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
            let outs = m.root_outcomes();
            let (w, l, t): (u32, u32, u32) = outs.iter().fold((0, 0, 0), |(a, b, c), o| {
                (a + o.wins, b + o.losses, c + o.ties)
            });
            tt_agg.states += 1;
            tt_agg.sum_win_rate += winrate(w, l, t);
            tt_agg.sum_alloc += m.nodes_allocated();
            tt_agg.sum_hits += m.tt_hits();
            tt_agg.sum_wall_ms += ms;
        }

        let n = f64::from(base_agg.states.max(1));
        let base_wr = base_agg.sum_win_rate / n;
        let tt_wr = tt_agg.sum_win_rate / n;
        #[allow(clippy::cast_precision_loss)]
        let base_alloc = base_agg.sum_alloc as f64 / n;
        #[allow(clippy::cast_precision_loss)]
        let tt_alloc = tt_agg.sum_alloc as f64 / n;
        let alloc_pct = if base_alloc > 0.0 {
            (tt_alloc - base_alloc) / base_alloc * 100.0
        } else {
            0.0
        };
        #[allow(clippy::cast_precision_loss)]
        let hits_per_sim = tt_agg.sum_hits as f64 / n / f64::from(sims);
        #[allow(clippy::cast_precision_loss)]
        let base_ms = base_agg.sum_wall_ms as f64 / n;
        #[allow(clippy::cast_precision_loss)]
        let tt_ms = tt_agg.sum_wall_ms as f64 / n;

        println!(
            "| {sims} | {} | {:.1}% | {:.1}% | {:+.2} | {:.0} | {:.0} | {:+.1}% | {:.4} | {:.1} | {:.1} |",
            base_agg.states,
            base_wr * 100.0,
            tt_wr * 100.0,
            (tt_wr - base_wr) * 100.0,
            base_alloc,
            tt_alloc,
            alloc_pct,
            hits_per_sim,
            base_ms,
            tt_ms,
        );
    }
}
