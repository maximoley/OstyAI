//! `drawchance_diagnose` — H1 diagnostic for the F1 `DrawChance` widening
//! investigation (bead `sts2-rs-k1z14`, G2/G3 retune).
//!
//! Leading hypothesis (H1): at typical visit counts the DPW cap
//! `ceil(C·(visits+1)^α)` yields only 1-2 outcomes at draw-chance nodes, so
//! F1 degenerates to a single fake determinization per draw event ≈ F0 with
//! extra overhead. This harness runs `--decisions` independent searches
//! under `HiddenInfoMode::DrawChance`, walks each resulting tree's arena
//! after `search()`, and buckets every chance node by
//! [`sts2_mcts::MctsNode::chance_is_draw_triggered`] (draw-detected vs
//! roll-detected), reporting per-bucket: outcome-count histogram, mean
//! outcomes/node, and the fraction of node visits landing on outcome 0.
//!
//! ```sh
//! cargo build --release --example drawchance_diagnose
//! target/release/examples/drawchance_diagnose --decisions 50 --sims 1000
//! ```
//!
//! This harness is read-only: it only walks `mcts.tree.arena` AFTER
//! `search()` returns, so it cannot perturb search behavior. (The
//! `chance_is_draw_triggered` field it buckets on is itself load-bearing
//! inside the search under `DrawChance` — it selects the draw-widening
//! DPW class and gates the outcome-0 reseed/pile-reshuffle; see its doc
//! in `sts2-mcts` — but that is a property of the searches being
//! measured, not of this harness.)

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;

use sts2_content::cards::spec_index::IRONCLAD_SPECS;
use sts2_content::cards::CardSpec;
use sts2_mcts::{HiddenInfoMode, Mcts, MctsConfig, NodeKind};

const DECK_SIZE: usize = 25;
const ENCOUNTER: &str = "MAWLER_NORMAL";

/// Mirrors `mcts_realistic_profile::random_deck`.
fn random_deck(seed: u64, size: usize) -> Vec<CardSpec> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let mut rng = sts2_core::dotnet_rng::Rng::new(seed ^ 0xDEC0_DEC0_DEC0_DEC0);
    (0..size)
        .map(|_| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            #[allow(clippy::cast_sign_loss)]
            let idx = rng.next_int_exclusive(0, IRONCLAD_SPECS.len() as i32) as usize;
            IRONCLAD_SPECS[idx]()
        })
        .collect()
}

#[derive(Default)]
struct Bucket {
    nodes: u64,
    total_outcomes: u64,
    /// outcome count -> number of chance nodes with exactly that many
    /// realized outcomes.
    outcome_hist: BTreeMap<usize, u64>,
    total_visits: u64,
    outcome0_visits: u64,
    /// visit-band label -> (nodes, total_outcomes, total_visits,
    /// outcome0_visits) — isolates whether low outcome counts are a
    /// tree-shape artifact (most nodes are low-visit leaves) or persist
    /// even at high visit counts where the DPW cap has room to widen.
    by_visit_band: BTreeMap<&'static str, (u64, u64, u64, u64)>,
}

fn visit_band(visits: u32) -> &'static str {
    match visits {
        0..=1 => "1_visits=0-1",
        2..=4 => "2_visits=2-4",
        5..=19 => "3_visits=5-19",
        20..=99 => "4_visits=20-99",
        _ => "5_visits=100+",
    }
}

impl Bucket {
    fn record(&mut self, n_outcomes: usize, visits: u32, outcome0_visits: u32) {
        self.nodes += 1;
        self.total_outcomes += n_outcomes as u64;
        *self.outcome_hist.entry(n_outcomes).or_insert(0) += 1;
        self.total_visits += u64::from(visits);
        self.outcome0_visits += u64::from(outcome0_visits);
        let band = self.by_visit_band.entry(visit_band(visits)).or_default();
        band.0 += 1;
        band.1 += n_outcomes as u64;
        band.2 += u64::from(visits);
        band.3 += u64::from(outcome0_visits);
    }

    fn report(&self, label: &str) {
        if self.nodes == 0 {
            println!("  {label}: 0 chance nodes");
            return;
        }
        let mean_outcomes = self.total_outcomes as f64 / self.nodes as f64;
        let outcome0_frac = if self.total_visits == 0 {
            0.0
        } else {
            100.0 * self.outcome0_visits as f64 / self.total_visits as f64
        };
        let at_most_2 = self
            .outcome_hist
            .iter()
            .filter(|(&k, _)| k <= 2)
            .map(|(_, &v)| v)
            .sum::<u64>();
        let at_most_2_pct = 100.0 * at_most_2 as f64 / self.nodes as f64;
        println!(
            "  {label}: nodes={} mean_outcomes={mean_outcomes:.3} \
             outcomes<=2={at_most_2_pct:.1}% outcome0_visit_frac={outcome0_frac:.1}% \
             (total_visits={})",
            self.nodes, self.total_visits,
        );
        print!("    outcome-count histogram: ");
        for (k, v) in &self.outcome_hist {
            print!("{k}:{v} ");
        }
        println!();
        println!("    by visit-band (nodes, mean_outcomes, outcome0_visit_frac%):");
        for (band, (n, out, vis, o0)) in &self.by_visit_band {
            let mo = *out as f64 / *n as f64;
            let of = if *vis == 0 {
                0.0
            } else {
                100.0 * *o0 as f64 / *vis as f64
            };
            println!("      {band}: nodes={n} mean_outcomes={mo:.3} outcome0_visit_frac={of:.1}%");
        }
    }
}

fn main() {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    let mut decisions: u32 = 50;
    let mut sims: u32 = 1000;
    let mut chance_widening_c: Option<f32> = None;
    let mut chance_widening_alpha: Option<f32> = None;

    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = || -> f32 {
            it.next()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| panic!("{flag} needs a numeric value"))
        };
        match flag.as_str() {
            "--decisions" => decisions = val() as u32,
            "--sims" => sims = val() as u32,
            "--chance-widening-c" => chance_widening_c = Some(val()),
            "--chance-widening-alpha" => chance_widening_alpha = Some(val()),
            other => panic!("unknown flag {other}"),
        }
    }

    let mut draw_bucket = Bucket::default();
    let mut roll_bucket = Bucket::default();

    for i in 0..decisions {
        let deck = random_deck(u64::from(i).wrapping_mul(0x9E37_79B9_7F4A_7C15), DECK_SIZE);
        let engine_seed = u64::from(i) + 1;
        let state = common::build_ironclad_state_with_deck(ENCOUNTER, engine_seed, &deck);
        let mut cfg = MctsConfig {
            max_simulations: sims,
            max_rollout_depth: 200,
            seed: engine_seed ^ 0x5EED_5EED,
            hidden_info: HiddenInfoMode::DrawChance,
            ..MctsConfig::default()
        };
        if let Some(c) = chance_widening_c {
            cfg.chance_widening_c = c;
        }
        if let Some(a) = chance_widening_alpha {
            cfg.chance_widening_alpha = a;
        }
        let mut mcts = Mcts::new(state, cfg);
        let _ = mcts.search();

        for node in &mcts.tree.arena {
            if node.kind != NodeKind::Chance {
                continue;
            }
            let n_outcomes = node.chance_outcomes.len();
            let outcome0_visits = node
                .chance_outcomes
                .first()
                .map_or(0, |o| mcts.tree.arena[o.child.0 as usize].visits);
            let bucket = if node.chance_is_draw_triggered {
                &mut draw_bucket
            } else {
                &mut roll_bucket
            };
            bucket.record(n_outcomes, node.visits, outcome0_visits);
        }
    }

    println!(
        "================ DRAWCHANCE H1 DIAGNOSTIC (decisions={decisions} sims={sims}) ================"
    );
    draw_bucket.report("draw-detected");
    roll_bucket.report("roll-detected");
    let total_nodes = draw_bucket.nodes + roll_bucket.nodes;
    println!(
        "  total chance nodes={total_nodes} (draw={} roll={})",
        draw_bucket.nodes, roll_bucket.nodes
    );
}
