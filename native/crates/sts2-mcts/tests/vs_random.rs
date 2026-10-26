//! MCTS vs uniform-random win-rate comparison on the P4-p84 vertical-slice
//! Ironclad-vs-Cultist scenario (bead sts2-rs-69p acceptance).
//!
//! Runs 100 combats per arm. MCTS uses 200 sims per move with a 50-move
//! per-combat cap. Asserts `mcts_win_rate >= random_win_rate + 0.05`.

mod common;

use common::build_vertical_slice_hp;

// Harder than the default 80/48 slice — at default HP random play wins
// ~100% because the Phase-3 Cultist intent stub plus a 1-energy 6-damage
// Strike is enough to grind down a 48-HP enemy before the player dies.
// Knock the player down to 20 HP and bump cultist HP to surface the
// MCTS vs random gap.
const PLAYER_HP: i32 = 12;
const CULTIST_HP: i32 = 80;
use sts2_core::{apply, is_terminal, legal_actions};
use sts2_mcts::{Mcts, MctsConfig, RolloutPolicyKind};

const TRIALS: u64 = 100;
const MCTS_SIMS: u32 = 200;
const MAX_MOVES_PER_COMBAT: u32 = 50;

fn run_random(seed: u64) -> bool {
    let mut state = build_vertical_slice_hp(seed, PLAYER_HP, CULTIST_HP);
    let mut rng = sts2_core::dotnet_rng::Rng::new(seed ^ 0xDEAD_BEEF);
    let mut moves = 0u32;
    while !is_terminal(&state) && moves < MAX_MOVES_PER_COMBAT {
        let acts = legal_actions(&state);
        if acts.is_empty() {
            break;
        }
        let idx = rng.next_int_exclusive(0, acts.len() as i32) as usize;
        let action = acts[idx].clone();
        let _ = apply(&mut state, action);
        moves += 1;
    }
    state.player_won
}

fn run_mcts(seed: u64) -> bool {
    let mut state = build_vertical_slice_hp(seed, PLAYER_HP, CULTIST_HP);
    let mut moves = 0u32;
    while !is_terminal(&state) && moves < MAX_MOVES_PER_COMBAT {
        let acts = legal_actions(&state);
        if acts.is_empty() {
            break;
        }
        // Pinned to the pre-`sts2-rs-dfv1` Uniform rollout policy so the
        // win-rate expectations in this test stay calibrated against the
        // baseline that originally set them.
        let cfg = MctsConfig {
            max_simulations: MCTS_SIMS,
            seed: seed ^ u64::from(moves).wrapping_mul(0xA5A5_A5A5),
            max_rollout_depth: 200,
            rollout_policy: RolloutPolicyKind::Uniform,
            ..MctsConfig::default()
        };
        let mut mcts = Mcts::new(state.clone(), cfg);
        let action = mcts.search();
        let _ = apply(&mut state, action);
        moves += 1;
    }
    state.player_won
}

// Heavy quality-gate (~56s in dev profile): 100 trials × 200 MCTS sims/move.
// Moved out of the per-commit suite to keep `ci-fast` under a minute; the
// `parity-nightly.yml` workflow runs it via `--include-ignored`.
#[test]
#[ignore]
fn mcts_outperforms_random_on_vertical_slice() {
    let mut random_wins = 0u32;
    let mut mcts_wins = 0u32;
    for seed in 0..TRIALS {
        if run_random(seed) {
            random_wins += 1;
        }
        if run_mcts(seed) {
            mcts_wins += 1;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let random_rate = f64::from(random_wins) / TRIALS as f64;
    #[allow(clippy::cast_precision_loss)]
    let mcts_rate = f64::from(mcts_wins) / TRIALS as f64;

    eprintln!(
        "vs_random: random_wins={random_wins}/{TRIALS} ({random_rate:.2}) \
         mcts_wins={mcts_wins}/{TRIALS} ({mcts_rate:.2})"
    );

    assert!(
        mcts_rate >= random_rate + 0.05,
        "mcts_rate ({mcts_rate:.3}) must beat random_rate ({random_rate:.3}) by ≥0.05"
    );
}
