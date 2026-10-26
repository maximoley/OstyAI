//! `mcts_realistic_profile` — realistic-deck MCTS profiling harness
//! (perf-investigation, not a bead).
//!
//! Complements `mcts_profile` (starter deck vs Nibbit, PIMC-tunable)
//! with the fixture the 2026-07-04 callgrind profile was taken on:
//! a 25-card random-full-pool Ironclad deck vs `MAWLER_NORMAL`,
//! **chance-node mode** (the `MctsConfig` default), rollout depth 200.
//!
//! ```sh
//! cargo build --release --example mcts_realistic_profile
//! target/release/examples/mcts_realistic_profile <decks> <searches_per_deck> <sims> [--hidden-info MODE]
//! # e.g. the perf-iteration shape:
//! target/release/examples/mcts_realistic_profile 8 5 1000
//! # determinism check (run twice, compare `sink=`):
//! target/release/examples/mcts_realistic_profile 4 2 500
//! # G3 perf gate (bead sts2-rs-k1z14, part 2): compare the three
//! # HiddenInfoMode arms at fixed work.
//! target/release/examples/mcts_realistic_profile 8 5 1000 --hidden-info clairvoyant
//! target/release/examples/mcts_realistic_profile 8 5 1000 --hidden-info rootscramble
//! target/release/examples/mcts_realistic_profile 8 5 1000 --hidden-info drawchance
//! ```
//!
//! The reported `sink=` value is an FNV-1a hash over the Debug rendering
//! of every chosen root action — byte-identical across runs of the same
//! build, and across builds ONLY if search behavior (chosen actions) is
//! unchanged. Use it as the bit-identity gate for perf work. NOTE: the
//! sink is only expected to match across `--hidden-info` arms when the
//! mode is `clairvoyant` (the historical default) — `rootscramble` and
//! `drawchance` deliberately choose different actions (root blindfold /
//! draws-as-chance), so cross-mode sink comparison is not meaningful;
//! only same-mode, same-args reruns are a determinism check.
//!
//! `--hidden-info` defaults to `clairvoyant` (byte-identical to every
//! pre-`sts2-rs-k1z14` invocation of this bench with no flag at all).

#[path = "common/mod.rs"]
mod common;

use std::time::Instant;

use sts2_content::cards::spec_index::IRONCLAD_SPECS;
use sts2_content::cards::CardSpec;
use sts2_mcts::{HiddenInfoMode, Mcts, MctsConfig};

const DECK_SIZE: usize = 25;
const ENCOUNTER: &str = "MAWLER_NORMAL";

/// Sample `size` cards uniformly from `IRONCLAD_SPECS`, keyed on `seed`
/// (deterministic, host-independent). Mirrors `vantom_ceiling_probe`'s
/// `random_full_pool_deck`.
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

/// FNV-1a over a byte slice — stable across runs/builds.
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

    // `--hidden-info MODE` may appear anywhere in the arg list (bead
    // sts2-rs-k1z14 G3 perf gate); every other arg is positional
    // (decks, searches_per_deck, sims), same as before this flag existed.
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let mut hidden_info = HiddenInfoMode::Clairvoyant;
    let mut positional = Vec::with_capacity(raw_args.len());
    let mut it = raw_args.into_iter();
    while let Some(a) = it.next() {
        if a == "--hidden-info" {
            let v = it.next().unwrap_or_else(|| {
                eprintln!(
                    "--hidden-info needs a value (clairvoyant|rootscramble|drawchance|beliefdraw)"
                );
                std::process::exit(1);
            });
            hidden_info = match v.as_str() {
                "clairvoyant" => HiddenInfoMode::Clairvoyant,
                "rootscramble" => HiddenInfoMode::RootScramble,
                "drawchance" => HiddenInfoMode::DrawChance,
                "beliefdraw" => HiddenInfoMode::BeliefDraw,
                other => {
                    eprintln!("unknown --hidden-info mode {other}");
                    std::process::exit(1);
                }
            };
        } else {
            positional.push(a);
        }
    }
    let mut pos = positional.into_iter();
    let n_decks: u32 = pos.next().and_then(|s| s.parse().ok()).unwrap_or(8);
    let searches_per_deck: u32 = pos.next().and_then(|s| s.parse().ok()).unwrap_or(5);
    let sims: u32 = pos.next().and_then(|s| s.parse().ok()).unwrap_or(1000);

    let cfg_for = |seed: u64| MctsConfig {
        max_simulations: sims,
        max_rollout_depth: 200,
        seed,
        hidden_info,
        ..MctsConfig::default()
    };

    // Warmup (primes allocator / lazy tables; untimed).
    {
        let deck = random_deck(0xA5A5, DECK_SIZE);
        let state = common::build_ironclad_state_with_deck(ENCOUNTER, 1, &deck);
        let mut mcts = Mcts::new(state, cfg_for(0xFEED));
        let _ = mcts.search();
    }

    let n_searches = n_decks * searches_per_deck;
    let mut sink = 0xCBF2_9CE4_8422_2325_u64; // FNV-1a offset basis
    let start = Instant::now();
    for deck_idx in 0..n_decks {
        let deck = random_deck(
            u64::from(deck_idx).wrapping_mul(0x9E37_79B9_7F4A_7C15),
            DECK_SIZE,
        );
        for search_idx in 0..searches_per_deck {
            // Distinct engine seed per (deck, search) so HP rolls / draw
            // shuffles differ; distinct search seed so the per-sim RNG
            // streams differ.
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
        "hidden_info={hidden_info:?} decks={n_decks} searches/deck={searches_per_deck} \
         sims={sims} total={:.3}s per_search={per_search_ms:.2}ms searches/sec={:.2} \
         sink={sink:#018x}",
        elapsed.as_secs_f64(),
        1000.0 / per_search_ms,
    );
}
