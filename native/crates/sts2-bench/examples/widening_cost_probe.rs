//! `widening_cost_probe` — where does the PR #467 EndTurn-widening 2.4×
//! wall-clock cost actually go? (perf-investigation, not a bead)
//!
//! Runs the deployed 1000-sim BeliefDraw dialect (c_puct 2.5,
//! draw_widening_c 2.0) on the `mcts_realistic_profile` fixture
//! (25-card random-pool Ironclad deck vs MAWLER_NORMAL, rollout depth
//! 200) with the EndTurn DPW coefficients taken from the CLI, so the
//! pre-retune (0.5/0.4) and deployed (5.787/0.436) arms are directly
//! comparable on identical seeds.
//!
//! ```text
//! cargo build --release --example widening_cost_probe
//! target/release/examples/widening_cost_probe --endturn-c 0.5   --endturn-alpha 0.4
//! target/release/examples/widening_cost_probe --endturn-c 5.787 --endturn-alpha 0.436
//! ```

#[path = "common/mod.rs"]
mod common;

use std::time::Instant;

use sts2_content::cards::spec_index::IRONCLAD_SPECS;
use sts2_content::cards::CardSpec;
use sts2_mcts::{HiddenInfoMode, Mcts, MctsConfig};

const DECK_SIZE: usize = 25;
const ENCOUNTER: &str = "MAWLER_NORMAL";

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

fn fnv1a(hash: u64, bytes: &[u8]) -> u64 {
    let mut h = hash;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

fn main() {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let mut endturn_c = 5.787_f32;
    let mut endturn_alpha = 0.436_f32;
    let mut n_decks = 8_u32;
    let mut searches_per_deck = 3_u32;
    let mut sims = 1000_u32;
    let mut closed_gate = false;
    let mut it = raw_args.into_iter();
    while let Some(a) = it.next() {
        if a == "--closed-gate" {
            closed_gate = true;
            continue;
        }
        let mut val = || it.next().expect("flag needs a value");
        match a.as_str() {
            "--endturn-c" => endturn_c = val().parse().expect("f32"),
            "--endturn-alpha" => endturn_alpha = val().parse().expect("f32"),
            "--decks" => n_decks = val().parse().expect("u32"),
            "--searches" => searches_per_deck = val().parse().expect("u32"),
            "--sims" => sims = val().parse().expect("u32"),
            other => panic!("unknown flag {other}"),
        }
    }

    // Deployed 1000-sim BeliefDraw dialect (policy/mod.rs::mcts_config_for)
    // with the EndTurn DPW pair parameterized.
    let cfg_for = |seed: u64| {
        let mut c = MctsConfig {
            max_simulations: sims,
            max_rollout_depth: 200,
            seed,
            hidden_info: HiddenInfoMode::BeliefDraw,
            ..MctsConfig::default()
        };
        c.c_puct = 2.5;
        c.draw_widening_c = 2.0;
        c.endturn_widening_c = endturn_c;
        c.endturn_widening_alpha = endturn_alpha;
        c.chance_closed_stops_widening = closed_gate;
        c
    };

    // Warmup (primes allocator / lazy tables; untimed).
    {
        let deck = random_deck(0xA5A5, DECK_SIZE);
        let state = common::build_ironclad_state_with_deck(ENCOUNTER, 1, &deck);
        let mut mcts = Mcts::new(state, cfg_for(0xFEED));
        let _ = mcts.search();
    }

    let n_searches = n_decks * searches_per_deck;
    let mut sink = 0xCBF2_9CE4_8422_2325_u64;
    let start = Instant::now();
    for deck_idx in 0..n_decks {
        let deck = random_deck(
            u64::from(deck_idx).wrapping_mul(0x9E37_79B9_7F4A_7C15),
            DECK_SIZE,
        );
        for search_idx in 0..searches_per_deck {
            let engine_seed = u64::from(deck_idx) * 1000 + u64::from(search_idx) + 1;
            let state = common::build_ironclad_state_with_deck(ENCOUNTER, engine_seed, &deck);
            let mut mcts = Mcts::new(state, cfg_for(engine_seed ^ 0x5EED_5EED));
            let action = mcts.search();
            sink = fnv1a(sink, format!("{action:?}").as_bytes());
        }
    }
    let elapsed = start.elapsed();

    let per_search_ms = elapsed.as_secs_f64() * 1000.0 / f64::from(n_searches);
    println!(
        "endturn_c={endturn_c} endturn_alpha={endturn_alpha} closed_gate={closed_gate} decks={n_decks} \
         searches/deck={searches_per_deck} sims={sims} total={:.3}s \
         per_search={per_search_ms:.2}ms searches/sec={:.2} sink={sink:#018x}",
        elapsed.as_secs_f64(),
        1000.0 / per_search_ms,
    );
}
