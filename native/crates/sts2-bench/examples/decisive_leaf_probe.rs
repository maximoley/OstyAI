//! `decisive_leaf_probe` — how much of the search budget lands on leaves
//! whose value is already known?
//!
//! Motivated by an evaluation of AmEx-MCTS (arXiv:2402.08511), whose whole
//! premise is that vanilla MCTS "expends its limited resources on
//! reevaluating previously explored regions". In our engine the only nodes
//! that are *fully* explored in that paper's sense are decisive leaves:
//! terminal states and solver-proven wins/losses (`MctsNode::proven_win` /
//! `proven_loss`). `Mcts::rollouts_skipped()` counts exactly the simulations
//! that bottomed out on one, so `rollouts_skipped / sims` is an upper bound
//! on the budget AmEx-style subtree exclusion could reclaim.
//!
//! Measured on the deployed configuration (BeliefDraw, `c_puct` 2.5,
//! `draw_widening_c` 2.0, 1000 sims) over full MCTS-vs-engine combats, so
//! the sample covers late-combat positions where decisive leaves are dense
//! — not just turn 1, where they are absent by construction.
//!
//! ```sh
//! cargo run --release --example decisive_leaf_probe -- [combats] [sims]
//! ```

#[path = "common/mod.rs"]
mod common;

use rayon::prelude::*;
use sts2_content::cards::spec_index::IRONCLAD_SPECS;
use sts2_content::cards::CardSpec;
use sts2_core::{apply, is_terminal, legal_actions, Action};
use sts2_mcts::{HiddenInfoMode, Mcts, MctsConfig};

const DECK_SIZE: usize = 25;
const ENCOUNTERS: [&str; 3] = ["MAWLER_NORMAL", "NIBBITS_NORMAL", "BYRDONIS_ELITE"];
const MAX_ACTIONS_PER_FIGHT: u32 = 400;

/// Sample `size` cards uniformly from `IRONCLAD_SPECS`, keyed on `seed`
/// (deterministic, host-independent). Mirrors `mcts_realistic_profile`.
fn random_deck(seed: u64, size: usize) -> Vec<CardSpec> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let mut rng = sts2_core::dotnet_rng::Rng::new(seed ^ 0xDEC0_DEC0_DEC0_DEC0);
    (0..size)
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

/// One decision's worth of probe telemetry.
struct Decision {
    round: u16,
    sims: u32,
    skipped: u64,
    root_proven_win: bool,
    root_proven_loss: bool,
    /// Simulation index at which the root first became solver-proven, if
    /// ever. Everything after it is budget spent on a settled question:
    /// `pick_root_action` rule 2 returns the proven child unconditionally.
    sims_to_proof: Option<u32>,
    tt_hits: u64,
    nodes: u64,
}

fn play_one(encounter: &str, deck_seed: u64, engine_seed: u64, sims: u32) -> Vec<Decision> {
    let deck = random_deck(deck_seed, DECK_SIZE);
    let mut s = common::build_ironclad_state_with_deck(encounter, engine_seed, &deck);
    let mut out = Vec::new();
    let mut action_count: u32 = 0;
    while !is_terminal(&s) && action_count < MAX_ACTIONS_PER_FIGHT {
        let acts = legal_actions(&s);
        if acts.is_empty() {
            let _ = apply(&mut s, Action::EndTurn);
            action_count += 1;
            continue;
        }
        let cfg = MctsConfig {
            max_simulations: sims,
            max_rollout_depth: 200,
            seed: engine_seed
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(u64::from(action_count)),
            hidden_info: HiddenInfoMode::BeliefDraw,
            c_puct: 2.5,
            draw_widening_c: 2.0,
            ..MctsConfig::default()
        };
        let mut mcts = Mcts::new(s.clone(), cfg);
        // Run the budget one simulation at a time so the sim index at which
        // the root first becomes proven is observable. The full budget is
        // still spent, so `chosen` matches the deployed `search()` result.
        let mut sims_to_proof = None;
        let mut chosen = Action::EndTurn;
        for i in 0..sims {
            chosen = mcts
                .try_search_n(1)
                .expect("root has legal actions (checked above)");
            if sims_to_proof.is_none() && (mcts.root_proven_win() || mcts.root_proven_loss()) {
                sims_to_proof = Some(i + 1);
            }
        }
        out.push(Decision {
            round: s.round,
            sims,
            sims_to_proof,
            skipped: mcts.rollouts_skipped(),
            root_proven_win: mcts.root_proven_win(),
            root_proven_loss: mcts.root_proven_loss(),
            tt_hits: mcts.tt_hits(),
            nodes: mcts.nodes_allocated(),
        });
        let _ = apply(&mut s, chosen);
        action_count += 1;
    }
    out
}

fn main() {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    let mut args = std::env::args().skip(1);
    let combats: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(9);
    let sims: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1000);

    eprintln!(
        "[decisive_leaf_probe] combats={combats} sims={sims} deployed-cfg=BeliefDraw/2.5/2.0"
    );

    let jobs: Vec<(usize, u32)> = (0..combats)
        .map(|i| (i as usize % ENCOUNTERS.len(), i))
        .collect();

    let per_combat: Vec<Vec<Decision>> = jobs
        .par_iter()
        .map(|&(enc_idx, i)| {
            let d = play_one(
                ENCOUNTERS[enc_idx],
                u64::from(i).wrapping_mul(0x9E37_79B9_7F4A_7C15),
                u64::from(i) + 1,
                sims,
            );
            eprintln!(
                "[decisive_leaf_probe] combat {i} ({}) done: {} decisions",
                ENCOUNTERS[enc_idx],
                d.len()
            );
            d
        })
        .collect();

    let all: Vec<&Decision> = per_combat.iter().flatten().collect();
    if all.is_empty() {
        eprintln!("no decisions recorded");
        return;
    }
    let total_sims: u64 = all.iter().map(|d| u64::from(d.sims)).sum();
    let total_skipped: u64 = all.iter().map(|d| d.skipped).sum();
    let proven_win = all.iter().filter(|d| d.root_proven_win).count();
    let proven_loss = all.iter().filter(|d| d.root_proven_loss).count();
    let tt_hits: u64 = all.iter().map(|d| d.tt_hits).sum();
    let nodes: u64 = all.iter().map(|d| d.nodes).sum();

    #[allow(clippy::cast_precision_loss)]
    let pct = |num: u64, den: u64| 100.0 * num as f64 / den as f64;
    #[allow(clippy::cast_precision_loss)]
    let tt_per_sim = tt_hits as f64 / total_sims as f64;

    println!("decisions               = {}", all.len());
    println!("total sims              = {total_sims}");
    println!(
        "sims on decisive leaves = {total_skipped} ({:.2}%)",
        pct(total_skipped, total_sims)
    );
    println!(
        "roots proven win/loss   = {proven_win} / {proven_loss} of {} ({:.1}% / {:.1}%)",
        all.len(),
        pct(proven_win as u64, all.len() as u64),
        pct(proven_loss as u64, all.len() as u64),
    );
    println!("tt hits                 = {tt_hits} ({tt_per_sim:.2} per sim), nodes = {nodes}");
    #[allow(clippy::cast_precision_loss)]
    let nodes_per_sim = nodes as f64 / total_sims as f64;
    println!("distinct nodes per sim  = {nodes_per_sim:.3}");

    // Budget spent after the root was already solver-proven: `pick_root_action`
    // rule 2 returns the proven child unconditionally, so these sims cannot
    // change the answer. This is what a `break` on `root_proven_*` reclaims.
    let post_proof: u64 = all
        .iter()
        .filter_map(|d| d.sims_to_proof.map(|k| u64::from(d.sims - k)))
        .sum();
    println!(
        "sims after root proven  = {post_proof} ({:.2}% of total budget)",
        pct(post_proof, total_sims)
    );

    // Per-round breakdown: decisive leaves should be rare early, dense late.
    println!("\nround  decisions   sims   decisive%   proven-root%   post-proof%   nodes/sim");
    let max_round = all.iter().map(|d| d.round).max().unwrap_or(0);
    for r in 0..=max_round {
        let bucket: Vec<&&Decision> = all.iter().filter(|d| d.round == r).collect();
        if bucket.is_empty() {
            continue;
        }
        let s: u64 = bucket.iter().map(|d| u64::from(d.sims)).sum();
        let k: u64 = bucket.iter().map(|d| d.skipped).sum();
        let p = bucket
            .iter()
            .filter(|d| d.root_proven_win || d.root_proven_loss)
            .count();
        let pp: u64 = bucket
            .iter()
            .filter_map(|d| d.sims_to_proof.map(|k| u64::from(d.sims - k)))
            .sum();
        let n: u64 = bucket.iter().map(|d| d.nodes).sum();
        #[allow(clippy::cast_precision_loss)]
        let npsim = n as f64 / s as f64;
        println!(
            "{r:>5}  {:>9}  {s:>6}   {:>8.2}%   {:>11.1}%   {:>10.2}%   {npsim:>9.3}",
            bucket.len(),
            pct(k, s),
            pct(p as u64, bucket.len() as u64),
            pct(pp, s),
        );
    }
}
