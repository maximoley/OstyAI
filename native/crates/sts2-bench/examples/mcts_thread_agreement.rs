//! `mcts_thread_agreement` — does root parallelism cost decision quality?
//!
//! `par_search` SPLITS the sim budget across workers (`lib.rs:3747`,
//! `base = total / workers`), so `threads=8` at 4000 sims is eight
//! 500-sim trees, not one 4000-sim tree. Each tree is shallower and
//! loses the sequential refinement where later simulations exploit what
//! earlier ones learned. Whether that costs anything at our deployed
//! budgets has never been measured: the only strength number on record
//! (`docs/experiments/2026-06-10-combat-fullrun-winrate-search-vs-leaf-findings.md`,
//! 1 vs 16 threads, n=135) predates the `59588deba` worker-seed
//! decorrelation fix, and `docs/perf/2026-06-08-mcts-perf-d1-item1-parallelism.md`
//! filed the A/B as an explicit open action item that was never run.
//!
//! Win rate cannot answer this cheaply — its noise floor is far wider
//! than the effect. So this harness uses the same instrument the CRN
//! study used (`mcts_decision_stability.rs`): hold the position fixed
//! and ask which action the search picks, but vary `threads` instead of
//! the CRN flag.
//!
//! ## The two metrics
//!
//! - **Oracle agreement** — fraction of searches that pick the same
//!   action as a single-threaded `ORACLE_SIMS`-budget search on the same
//!   position. This is the quality metric: it is directional. Judge a
//!   config by how often it reaches the answer a much deeper search
//!   reaches.
//! - **Modal share** — fraction of trials (differing only in search
//!   seed) that pick that arm's own most popular action. This is
//!   self-consistency, NOT quality: a config that always picks the same
//!   wrong action scores 1.0. Reported because it is directly
//!   comparable to the 2026-08-08 CRN baseline (0.784 at 1000 sims,
//!   threads=1).
//!
//! Positions where the oracle itself is unstable across its own seeds
//! carry no signal about quality — nothing to agree WITH. Every metric
//! is therefore also reported on a restricted subset.
//!
//! ## Two oracles, because one would beg the question
//!
//! Scoring every arm against a single-threaded deep search risks a
//! shared-method bias: `threads=1` arms and a `threads=1` oracle both
//! argmax over ONE tree's Q distribution, while a merged arm argmaxes
//! over a visit-weighted pool of N trees. Some of any `threads=1`
//! advantage could be that shared geometry rather than better play.
//!
//! So the oracle is run twice at `ORACLE_SIMS` — once single-threaded,
//! once root-parallel at 8 workers — each over `ORACLE_SEEDS` seeds.
//! The **gold subset** is the positions where both oracles are
//! unanimous across their own seeds AND agree with each other. There
//! the reference action is method-independent, and the `gold_agr`
//! column is the metric to judge on. `seq_agr` over all positions is
//! reported alongside as the unrestricted (and slightly biased) view.
//!
//! ## The grid
//!
//! `threads ∈ {1,2,4,8}` × `sims ∈ {1000, 4000, 8000}`. Reading it:
//!
//! - Down a `sims` column: the pure parallelism cost at a fixed total
//!   budget. This is the question — same work, split more ways.
//! - Across a row at fixed sims-per-tree (e.g. `1×1000` vs `8×8000`,
//!   both 1000 sims/tree): whether N shallow trees recover what one
//!   deep tree gets, i.e. the DEPTH arm. Because `threads=8` runs ~5.6×
//!   faster wall-clock, `8×8000` costs about what `1×1000` costs in
//!   real time — so if it agrees with the oracle more often, the
//!   speedup is convertible into strength instead of latency.
//!
//! A final sequential timing pass measures ms/search per config on this
//! host so the wall-clock-matched claim is grounded rather than assumed.
//!
//! ```sh
//! cargo run --release -p sts2-bench --example mcts_thread_agreement -- \
//!     [positions] [trials] [oracle_sims] [timing_positions] \
//!     [clairvoyant|beliefdraw] [rollout_depth]
//! ```

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::time::Instant;

use rayon::prelude::*;
use sts2_content::cards::spec_index::IRONCLAD_SPECS;
use sts2_content::cards::CardSpec;
use sts2_core::{legal_actions, CombatPhase, CombatState};
use sts2_mcts::{par_search, HiddenInfoMode, MctsConfig};

const DECK_SIZE: usize = 24;
/// Same encounter mix as `mcts_decision_stability`, so modal-share
/// numbers are comparable to the 2026-08-08 CRN baseline.
const ENCOUNTERS: [&str; 4] = [
    "FLYCONID_NORMAL",
    "MAWLER_NORMAL",
    "BYRDONIS_ELITE",
    "VANTOM_BOSS",
];
const THREADS: [u32; 4] = [1, 2, 4, 8];
const SIMS: [u32; 3] = [1000, 4000, 8000];
/// Independent oracle runs per position. Their agreement defines the
/// "oracle-confident" subset; their modal pick is the reference action.
const ORACLE_SEEDS: u32 = 3;

fn random_deck(seed: u64) -> Vec<CardSpec> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let mut rng = sts2_core::dotnet_rng::Rng::new(seed ^ 0xDEC0_DEC0_DEC0_DEC0);
    (0..DECK_SIZE)
        .map(|_| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                clippy::cast_sign_loss
            )]
            let idx = rng.next_int_exclusive(0, IRONCLAD_SPECS.len() as i32) as usize;
            IRONCLAD_SPECS[idx]()
        })
        .collect()
}

/// Search seed for trial `t`. Shared across every config so the arms
/// are paired on seed: `threads=1` and `threads=8` at trial `t` start
/// from the same `config.seed` (the workers then derive their own via
/// `splitmix64`, which is exactly the mechanism under test).
fn trial_seed(t: u32) -> u64 {
    u64::from(t)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(0x51ED_2701)
}

fn oracle_seed(k: u32) -> u64 {
    u64::from(k)
        .wrapping_mul(0xD1B5_4A32_D192_ED03)
        .wrapping_add(0x0C1E_0BAD_5EED_0001)
}

/// Which search dialect to sweep.
///
/// `Clairvoyant` is `MctsConfig::default()` — the engine-world / sweep-lane
/// baseline and the config every earlier MCTS study here used.
///
/// `BeliefDraw` is what the live advisor actually deploys
/// (`realgame_advisor.py::HONEST_SEARCH`: `hidden_info="beliefdraw"`,
/// `c_puct=2.5`, `draw_widening_c=2.0`). It matters for THIS question
/// specifically: BeliefDraw defers draw randomness to chance nodes that
/// widen progressively in the visit count, so splitting one budget into
/// N shallower trees also narrows how many draw outcomes each tree ever
/// expands. If root parallelism costs anything anywhere, this dialect is
/// where it should show up first.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Clairvoyant,
    BeliefDraw,
}

/// `depth` is the rollout ply cap and is passed explicitly rather than
/// inherited, because the two defaults disagree and the disagreement is
/// load-bearing: `MctsConfig::default().max_rollout_depth` is **10**,
/// while every Python caller (`RecommendConfig.depth`, and
/// `sts2_rs.mcts_recommend`'s own pyo3 default) uses **200**. At 10 a
/// rollout essentially never reaches a real terminal, so the leaf eval
/// returns ~0 for every action and the root ranking is largely noise —
/// which would make any threads effect unmeasurable for the wrong
/// reason. 200 is what the advisor deploys.
fn base_config(sims: u32, seed: u64, depth: u32, mode: Mode) -> MctsConfig {
    let d = MctsConfig::default();
    let common = MctsConfig {
        max_simulations: sims,
        seed,
        max_rollout_depth: depth,
        ..d
    };
    match mode {
        Mode::Clairvoyant => common,
        Mode::BeliefDraw => MctsConfig {
            hidden_info: HiddenInfoMode::BeliefDraw,
            c_puct: 2.5,
            draw_widening_c: 2.0,
            ..common
        },
    }
}

/// Most common entry, ties broken by the string order so the pick is
/// deterministic regardless of insertion order.
fn modal(counts: &BTreeMap<String, u32>) -> (&str, u32) {
    counts
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
        .map_or(("", 0), |(k, v)| (k.as_str(), *v))
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

/// Wilson 95% interval for a proportion — the honest error bar on an
/// agreement rate. A raw ± on a ratio near 1.0 would overstate the
/// lower tail.
fn wilson(hits: f64, n: f64) -> (f64, f64) {
    if n <= 0.0 {
        return (0.0, 0.0);
    }
    let z = 1.96_f64;
    let p_hat = hits / n;
    let denom = 1.0 + z * z / n;
    let center = p_hat + z * z / (2.0 * n);
    let half_width = z * ((p_hat * (1.0 - p_hat) / n) + (z * z / (4.0 * n * n))).sqrt();
    (
        ((center - half_width) / denom).max(0.0),
        ((center + half_width) / denom).min(1.0),
    )
}

fn main() {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    let n_positions: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(48);
    let trials: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(8);
    let oracle_sims: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(32_000);
    let timing_positions: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(3);
    let mode = match it.next().map(String::as_str) {
        Some("beliefdraw") => Mode::BeliefDraw,
        Some("clairvoyant") | None => Mode::Clairvoyant,
        Some(other) => panic!("unknown mode {other:?} (want clairvoyant|beliefdraw)"),
    };
    let depth: u32 = it
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(MctsConfig::default().max_rollout_depth);
    let mode_label = if mode == Mode::BeliefDraw {
        "beliefdraw(deployed advisor: c_puct=2.5 draw_widening_c=2.0)"
    } else {
        "clairvoyant(engine default)"
    };

    let configs: Vec<(u32, u32)> = SIMS
        .iter()
        .flat_map(|&s| THREADS.iter().map(move |&t| (t, s)))
        .collect();

    eprintln!(
        "[thread_agreement] positions={n_positions} trials={trials} oracle_sims={oracle_sims} \
         configs={} threads={THREADS:?} sims={SIMS:?}",
        configs.len()
    );

    // ── Build the position corpus ───────────────────────────────────
    let mut states: Vec<CombatState> = Vec::new();
    for p in 0..n_positions {
        let encounter = ENCOUNTERS[p as usize % ENCOUNTERS.len()];
        let deck = random_deck(u64::from(p).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let engine_seed = u64::from(p) ^ 0xA5A5_A5A5_A5A5_A5A5;
        let mut state = common::build_ironclad_state_with_deck(encounter, engine_seed, &deck);
        common::warmup_random(&mut state, u64::from(p) ^ 0xDEAD_BEEF, 1);
        if state.phase != CombatPhase::PlayerPlayPhase {
            continue;
        }
        if legal_actions(&state).len() < 2 {
            continue;
        }
        states.push(state);
    }
    let n = states.len();
    let legal_counts: Vec<usize> = states.iter().map(|s| legal_actions(s).len()).collect();
    #[allow(clippy::cast_precision_loss)]
    let chance_baseline = mean(
        &legal_counts
            .iter()
            .map(|&c| 1.0 / c as f64)
            .collect::<Vec<_>>(),
    );
    #[allow(clippy::cast_precision_loss)]
    let mean_legal = mean(&legal_counts.iter().map(|&c| c as f64).collect::<Vec<_>>());
    eprintln!(
        "[thread_agreement] scored positions={n} mean_legal_actions={mean_legal:.1} \
         chance_agreement={chance_baseline:.3}"
    );

    // ── Phase A: two oracles ────────────────────────────────────────
    // Deep search at `oracle_sims`, several seeds, run BOTH
    // single-threaded and root-parallel — see the "two oracles" note in
    // the module docs. `workers == 1` makes `par_search` a thin wrapper
    // over `Mcts::search`, so one call site covers both.
    let t0 = Instant::now();
    let oracle_tasks: Vec<(usize, u32, u32)> = (0..n)
        .flat_map(|p| {
            [1_u32, 8_u32]
                .into_iter()
                .flat_map(move |w| (0..ORACLE_SEEDS).map(move |k| (p, w, k)))
        })
        .collect();
    let oracle_picks: Vec<(usize, u32, String)> = oracle_tasks
        .par_iter()
        .map(|&(p, w, k)| {
            let cfg = base_config(oracle_sims, oracle_seed(k), depth, mode);
            let r = par_search(states[p].clone(), cfg, w);
            (p, w, format!("{:?}", r.best_action))
        })
        .collect();
    let mut seq_counts: Vec<BTreeMap<String, u32>> = vec![BTreeMap::new(); n];
    let mut par_counts: Vec<BTreeMap<String, u32>> = vec![BTreeMap::new(); n];
    for (p, w, pick) in oracle_picks {
        let slot = if w == 1 {
            &mut seq_counts[p]
        } else {
            &mut par_counts[p]
        };
        *slot.entry(pick).or_default() += 1;
    }
    let oracle_ref: Vec<String> = seq_counts.iter().map(|c| modal(c).0.to_string()).collect();
    // Gold subset: both oracles unanimous across their own seeds AND
    // landing on the same action. There the reference is not an
    // artifact of either search method.
    let gold: Vec<bool> = (0..n)
        .map(|p| {
            let (sp, sc) = modal(&seq_counts[p]);
            let (pp, pc) = modal(&par_counts[p]);
            sc == ORACLE_SEEDS && pc == ORACLE_SEEDS && sp == pp
        })
        .collect();
    let n_gold = gold.iter().filter(|&&b| b).count();
    let seq_unanimous = seq_counts
        .iter()
        .filter(|c| modal(c).1 == ORACLE_SEEDS)
        .count();
    let par_unanimous = par_counts
        .iter()
        .filter(|c| modal(c).1 == ORACLE_SEEDS)
        .count();
    let oracles_concur = (0..n)
        .filter(|&p| modal(&seq_counts[p]).0 == modal(&par_counts[p]).0)
        .count();
    eprintln!(
        "[thread_agreement] oracles done in {:.0}s — seq unanimous {seq_unanimous}/{n}, \
         par unanimous {par_unanimous}/{n}, concur {oracles_concur}/{n}, gold {n_gold}/{n}",
        t0.elapsed().as_secs_f64(),
    );

    // ── Phase B: the grid ───────────────────────────────────────────
    let t1 = Instant::now();
    let tasks: Vec<(usize, usize, u32)> = (0..n)
        .flat_map(|p| (0..configs.len()).flat_map(move |c| (0..trials).map(move |t| (p, c, t))))
        .collect();
    eprintln!("[thread_agreement] grid: {} searches", tasks.len());
    let picks: Vec<(usize, usize, String)> = tasks
        .par_iter()
        .map(|&(p, c, t)| {
            let (threads, sims) = configs[c];
            let cfg = base_config(sims, trial_seed(t), depth, mode);
            let r = par_search(states[p].clone(), cfg, threads);
            (p, c, format!("{:?}", r.best_action))
        })
        .collect();
    eprintln!(
        "[thread_agreement] grid done in {:.0}s",
        t1.elapsed().as_secs_f64()
    );

    // counts[c][p] = pick histogram for that config on that position
    let mut counts: Vec<Vec<BTreeMap<String, u32>>> = vec![vec![BTreeMap::new(); n]; configs.len()];
    for (p, c, pick) in picks {
        *counts[c][p].entry(pick).or_default() += 1;
    }

    // ── Phase C: sequential timing ──────────────────────────────────
    // Run alone (no outer rayon) so par_search's own speedup is real.
    let mut ms_per_search: Vec<f64> = Vec::with_capacity(configs.len());
    let tp = (timing_positions as usize).min(n);
    for &(threads, sims) in &configs {
        let start = Instant::now();
        for state in states.iter().take(tp) {
            let cfg = base_config(sims, trial_seed(0), depth, mode);
            let _ = par_search(state.clone(), cfg, threads);
        }
        ms_per_search.push(start.elapsed().as_secs_f64() * 1000.0 / tp.max(1) as f64);
    }

    // ── Report ──────────────────────────────────────────────────────
    println!("mode={mode_label} rollout_depth={depth}");
    println!("positions={n} trials={trials} oracle_sims={oracle_sims} oracle_seeds={ORACLE_SEEDS}");
    println!(
        "mean_legal_actions={mean_legal:.1}  chance_agreement={chance_baseline:.3}  \
         gold_subset={n_gold}/{n}  (seq_unanimous={seq_unanimous} par_unanimous={par_unanimous} \
         concur={oracles_concur})"
    );
    println!();
    println!(
        "{:>7} {:>6} {:>9} | {:>8} {:>15} | {:>8} {:>15} | {:>7} {:>9}",
        "threads",
        "sims",
        "sims/tree",
        "seq_agr",
        "95% CI",
        "gold_agr",
        "95% CI",
        "modal",
        "ms/search",
    );
    for (c, &(threads, sims)) in configs.iter().enumerate() {
        let mut hits = 0.0;
        let mut tot = 0.0;
        let mut chits = 0.0;
        let mut ctot = 0.0;
        let mut modal_shares = Vec::new();
        for p in 0..n {
            let per_pos: u32 = counts[c][p].values().sum();
            let agree = f64::from(counts[c][p].get(&oracle_ref[p]).copied().unwrap_or(0));
            hits += agree;
            tot += f64::from(per_pos);
            if gold[p] {
                chits += agree;
                ctot += f64::from(per_pos);
            }
            modal_shares.push(f64::from(modal(&counts[c][p]).1) / f64::from(per_pos.max(1)));
        }
        let (lo, hi) = wilson(hits, tot);
        let (clo, chi) = wilson(chits, ctot);
        println!(
            "{threads:>7} {sims:>6} {:>9} | {:>8.3} {:>15} | {:>8.3} {:>15} | {:>7.3} {:>9.1}",
            sims / threads,
            hits / tot.max(1.0),
            format!("[{lo:.3},{hi:.3}]"),
            chits / ctot.max(1.0),
            format!("[{clo:.3},{chi:.3}]"),
            mean(&modal_shares),
            ms_per_search[c],
        );
    }
}
