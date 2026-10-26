//! `vantom_starter_winrate` — minimal A/B for the act1_replay vs uq9y
//! gap (see Y0RJF8MPJ5 / A9FDZMU7P6 0/12 vs uq9y's 44.8 % ceiling).
//!
//! Builds N=`fixtures` Vantom-boss fights from the canonical
//! `build_ironclad_state` pipeline (starter deck + Burning Blood,
//! 1-turn random warmup) and plays each to terminal under MCTS with
//! `sims=10_000`, `max_rollout_depth=10` — matching uq9y's published
//! operating point. Reports winrate to compare against the 44.8 %
//! starter-deck ceiling.
//!
//! If this binary reports ~45 %, the 0 / 12 result from
//! act1_replay's force-add path is a state-construction bug
//! (relics not wiring combat hooks, or similar). If it also
//! reports near 0 %, something deeper changed since uq9y closed
//! (engine drift / regression).

#[path = "common/mod.rs"]
mod common;

use std::time::Instant;

use common::{build_ironclad_state, warmup_random};
use sts2_core::{apply, is_terminal, legal_actions, Action, CombatState};
use sts2_mcts::{Mcts, MctsConfig, TerminalEvalMode};

const ENCOUNTER_ID: &str = "VANTOM_BOSS";
const N_FIXTURES: u32 = 30;
const WARMUP_TURNS: u32 = 1;
const SIMS: u32 = 10_000;
const MAX_ROLLOUT_DEPTH: u32 = 10;
const MAX_ACTIONS_PER_FIGHT: u32 = 500;

fn play_one(template: &CombatState, mcts_seed: u64) -> (bool, i32, u32, u64) {
    let mut s = template.clone();
    let start = Instant::now();
    let mut action_count: u32 = 0;
    let mut turn_count: u32 = 0;
    let mut last_round = s.round;
    while !is_terminal(&s) && action_count < MAX_ACTIONS_PER_FIGHT {
        let acts = legal_actions(&s);
        if acts.is_empty() {
            let _ = apply(&mut s, Action::EndTurn);
            action_count += 1;
            continue;
        }
        let cfg = MctsConfig {
            max_simulations: SIMS,
            max_rollout_depth: MAX_ROLLOUT_DEPTH,
            seed: mcts_seed
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(u64::from(action_count)),
            terminal_eval_mode: TerminalEvalMode::RichBoth,
            ..MctsConfig::default()
        };
        let mut mcts = Mcts::new(s.clone(), cfg);
        let rec = mcts.search();
        let _ = apply(&mut s, rec);
        action_count += 1;
        if s.round != last_round {
            turn_count += 1;
            last_round = s.round;
        }
    }
    let wall_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let final_hp = s.player.current_hp.max(0);
    let won = is_terminal(&s) && final_hp > 0;
    (won, final_hp, turn_count, wall_ms)
}

fn main() {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    eprintln!(
        "[vantom_starter_winrate] encounter={ENCOUNTER_ID} fixtures={N_FIXTURES} sims={SIMS} \
         depth={MAX_ROLLOUT_DEPTH}",
    );

    let mut wins = 0u32;
    let mut total_hp: i64 = 0;
    let mut total_turns = 0u64;
    let t_total = Instant::now();
    println!(
        "{:>4} {:>9} {:>4} {:>9} {:>6} {:>7}",
        "fix", "warm_seed", "won", "final_hp", "turns", "wall_s"
    );
    for fixture_id in 0..N_FIXTURES {
        let engine_seed = u64::from(fixture_id) ^ 0xA5A5_A5A5_A5A5_A5A5;
        let warmup_seed = u64::from(fixture_id) ^ 0xDEAD_BEEF_FEED_FACE;
        let mut s = build_ironclad_state(ENCOUNTER_ID, engine_seed);
        warmup_random(&mut s, warmup_seed, WARMUP_TURNS);
        let mcts_seed = u64::from(fixture_id).wrapping_add(1);
        let (won, hp, turns, wall_ms) = play_one(&s, mcts_seed);
        if won {
            wins += 1;
        }
        total_hp += i64::from(hp);
        total_turns += u64::from(turns);
        println!(
            "{:>4} {:>9} {:>4} {:>9} {:>6} {:>7.1}",
            fixture_id,
            warmup_seed & 0xFFFF,
            if won { "W" } else { "L" },
            hp,
            turns,
            wall_ms as f64 / 1000.0,
        );
    }
    let n = f64::from(N_FIXTURES);
    println!();
    println!(
        "SUMMARY: fixtures={N_FIXTURES} wins={wins} winrate={:.1}% avg_hp={:.1} avg_turns={:.1} wall={:.0}s",
        100.0 * f64::from(wins) / n,
        total_hp as f64 / n,
        total_turns as f64 / n,
        t_total.elapsed().as_secs_f64(),
    );
}
