//! `mcts_decision_stability` — how often does the search actually have an
//! opinion, and does CRN sharpen it?
//!
//! Two questions, one harness, both on realistic mid-turn positions:
//!
//! 1. **Near-tie rate.** The 2026-06-25 blunder analysis measured 55.6% of
//!    combat decisions as near-ties (best-vs-2nd root Q margin < 0.01) with
//!    the chosen action holding only ~34% of root visits. That number is a
//!    one-off from a since-changed engine; this re-measures it live.
//!
//! 2. **Common random numbers.** `MctsConfig::common_random_numbers` gives
//!    every stochastic edge off one decision node the same
//!    `randomness_base`, so siblings are compared under matched luck
//!    instead of each being scored on its own nature draws. It ships OFF
//!    and its own doc-comment prescribes the A/B: judge on per-decision
//!    stability (modal share / flip rate), not win rate, because a
//!    variance-reduction technique moves the *noise floor* and a win-rate
//!    panel's own noise floor is far wider than the effect.
//!
//!    `sts2-pycli/python/diag_decision_stability.py` implements that A/B on
//!    the Python side but no result is recorded in any doc, so the lever has
//!    never been concluded. This is the Rust-side equivalent: hold the
//!    position fixed, vary only the search seed, and ask how often the
//!    search changes its mind.
//!
//! **Modal share** is the fraction of trials that picked the most popular
//! action — 1.0 means certain, 0.5 on a two-way choice means a coin flip.
//! CRN should RAISE it. It is not obliged to change which action wins.
//!
//! ```sh
//! cargo build --release --example mcts_decision_stability
//! target/release/examples/mcts_decision_stability [positions] [trials] [sims]
//! ```

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::time::Instant;

use sts2_content::cards::spec_index::IRONCLAD_SPECS;
use sts2_content::cards::CardSpec;
use sts2_core::{legal_actions, CombatPhase, CombatState};
use sts2_mcts::{Mcts, MctsConfig};

const DECK_SIZE: usize = 24;
const NEAR_TIE_EPS: f32 = 0.01;
const ENCOUNTERS: [&str; 4] = [
    "FLYCONID_NORMAL",
    "MAWLER_NORMAL",
    "BYRDONIS_ELITE",
    "VANTOM_BOSS",
];

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

/// One measurement of a single fixed position: which action each of `trials`
/// differently-seeded searches picked, plus the Q margin and the chosen
/// action's visit share from the first trial.
struct Measured {
    modal_share: f64,
    q_margin: f32,
    chosen_visit_share: f64,
}

fn measure(state: &CombatState, trials: u32, sims: u32, crn: bool) -> Measured {
    let mut picks: BTreeMap<String, u32> = BTreeMap::new();
    let mut q_margin = 0.0_f32;
    let mut chosen_visit_share = 0.0_f64;
    for trial in 0..trials {
        let cfg = MctsConfig {
            max_simulations: sims,
            // Vary ONLY the search seed across trials — the position is fixed.
            seed: u64::from(trial).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x51ED_2701,
            common_random_numbers: crn,
            ..MctsConfig::default()
        };
        let mut mcts = Mcts::new(state.clone(), cfg);
        let action = mcts.search();
        *picks.entry(format!("{action:?}")).or_default() += 1;

        if trial == 0 {
            let outcomes = mcts.root_outcomes();
            let mut qs: Vec<f32> = outcomes.iter().map(|o| o.mean_value).collect();
            qs.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
            q_margin = match qs.as_slice() {
                [best, second, ..] => best - second,
                _ => f32::INFINITY,
            };
            let total: u32 = outcomes.iter().map(|o| o.visits).sum();
            let chosen = outcomes
                .iter()
                .max_by(|a, b| {
                    a.mean_value
                        .partial_cmp(&b.mean_value)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map_or(0, |o| o.visits);
            chosen_visit_share = if total == 0 {
                0.0
            } else {
                f64::from(chosen) / f64::from(total)
            };
        }
    }
    let top = picks.values().copied().max().unwrap_or(0);
    Measured {
        modal_share: f64::from(top) / f64::from(trials),
        q_margin,
        chosen_visit_share,
    }
}

fn main() {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut pos_it = args.iter();
    let n_positions: u32 = pos_it.next().and_then(|s| s.parse().ok()).unwrap_or(40);
    let trials: u32 = pos_it.next().and_then(|s| s.parse().ok()).unwrap_or(9);
    let sims: u32 = pos_it.next().and_then(|s| s.parse().ok()).unwrap_or(1000);

    eprintln!(
        "[mcts_decision_stability] positions={n_positions} trials/position={trials} sims={sims}"
    );

    let start = Instant::now();
    let mut off_shares = Vec::new();
    let mut on_shares = Vec::new();
    let mut near_ties = 0_u32;
    let mut margins = Vec::new();
    let mut visit_shares = Vec::new();
    let mut tie_off = Vec::new();
    let mut tie_on = Vec::new();
    let mut scored = 0_u32;

    for p in 0..n_positions {
        let encounter = ENCOUNTERS[p as usize % ENCOUNTERS.len()];
        let deck = random_deck(u64::from(p).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let engine_seed = u64::from(p) ^ 0xA5A5_A5A5_A5A5_A5A5;
        let mut state = common::build_ironclad_state_with_deck(encounter, engine_seed, &deck);
        // Warm a turn so the position is mid-combat rather than turn-1
        // opening, which is unrepresentatively shallow.
        common::warmup_random(&mut state, u64::from(p) ^ 0xDEAD_BEEF, 1);
        if state.phase != CombatPhase::PlayerPlayPhase {
            continue;
        }
        // A position with <2 legal actions has nothing to be unstable about.
        if legal_actions(&state).len() < 2 {
            continue;
        }
        scored += 1;

        let off = measure(&state, trials, sims, /*crn=*/ false);
        let on = measure(&state, trials, sims, /*crn=*/ true);
        off_shares.push(off.modal_share);
        on_shares.push(on.modal_share);
        margins.push(off.q_margin);
        visit_shares.push(off.chosen_visit_share);
        if off.q_margin < NEAR_TIE_EPS {
            near_ties += 1;
            tie_off.push(off.modal_share);
            tie_on.push(on.modal_share);
        }
    }

    let mean = |v: &[f64]| -> f64 {
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<f64>() / v.len() as f64
        }
    };
    let mean_f32 = |v: &[f32]| -> f64 {
        if v.is_empty() {
            0.0
        } else {
            f64::from(v.iter().sum::<f32>()) / v.len() as f64
        }
    };

    println!("positions_scored={scored} trials={trials} sims={sims}");
    println!(
        "near_tie_rate(margin<{NEAR_TIE_EPS})={}/{scored} ({:.1}%)  mean_q_margin={:.5}  \
         mean_chosen_visit_share={:.3}",
        near_ties,
        100.0 * f64::from(near_ties) / f64::from(scored.max(1)),
        mean_f32(&margins),
        mean(&visit_shares),
    );
    println!(
        "modal_share  crn=off {:.3}   crn=on {:.3}   delta {:+.3}",
        mean(&off_shares),
        mean(&on_shares),
        mean(&on_shares) - mean(&off_shares),
    );
    println!(
        "flip_rate    crn=off {:.3}   crn=on {:.3}   delta {:+.3}",
        1.0 - mean(&off_shares),
        1.0 - mean(&on_shares),
        (1.0 - mean(&on_shares)) - (1.0 - mean(&off_shares)),
    );
    // The near-tie subset is where a variance-reduction technique can
    // actually change anything; positions the search already has an opinion
    // about dilute the aggregate.
    println!(
        "near-ties only (n={}):  modal_share crn=off {:.3}  crn=on {:.3}  delta {:+.3}  wall={:.1}s",
        tie_off.len(),
        mean(&tie_off),
        mean(&tie_on),
        mean(&tie_on) - mean(&tie_off),
        start.elapsed().as_secs_f64(),
    );
}
