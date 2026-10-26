//! Cross-simulator random-playout benchmark (2026-08-17).
//!
//! Companion to the sts_lightspeed_fork `lsbench` harness in
//! `experiments/sim-benchmark-2026-08-17/` — both drive the SAME
//! workload shape so throughput is comparable across simulators:
//!
//! * `fullrun`: uniform-random policy over `get_available_actions()`
//!   from run start until the run ends (death / win / cap). Livelock
//!   hashing is DISABLED so the timed loop is enumerate→pick→apply
//!   only, matching the lightspeed loop.
//! * `combat`: uniform-random policy over `legal_actions()` from a
//!   starter-deck Ironclad combat vs a random Act-1 pool encounter,
//!   re-cloned from a per-seed template for `--reps` playouts.
//!
//! Output: one JSON summary line on stdout.

use std::time::Instant;

use clap::Parser;
use rand::{Rng as _, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;
use rayon::prelude::*;

use sts2_cli::sim::scenario::resolve_encounter;
use sts2_cli::sim::{build_starter_state_from_monsters, PlayerClass};
use sts2_content::run_callbacks::new_run_manager_for_character;
use sts2_core::combat_loop::{start_combat, start_player_turn};
use sts2_core::full_run::{drive_full_run, FullRunConfig, RandomPolicy, RunOutcome};
use sts2_core::potions::CharacterId;
use sts2_core::{apply, legal_actions};

#[derive(Parser, Debug)]
struct Args {
    /// `fullrun` or `combat`.
    #[arg(long)]
    mode: String,
    /// Number of seeds (fullrun: one run per seed; combat: one
    /// encounter template per seed).
    #[arg(long, default_value_t = 1000)]
    seeds: u64,
    #[arg(long, default_value_t = 0)]
    seed_start: u64,
    /// Random playouts per combat template (combat mode only).
    #[arg(long, default_value_t = 10)]
    reps: u64,
    /// Worker threads (1 = single-thread timing loop).
    #[arg(long, default_value_t = 1)]
    threads: usize,
}

#[derive(Default)]
struct Tally {
    runs: u64,
    wins: u64,
    steps: u64,
    anomalies: u64,
}

impl Tally {
    fn merge(mut self, o: Tally) -> Tally {
        self.runs += o.runs;
        self.wins += o.wins;
        self.steps += o.steps;
        self.anomalies += o.anomalies;
        self
    }
}

fn fullrun_one(seed: u64) -> Tally {
    let cfg = FullRunConfig {
        livelock_stall_limit: 0, // no per-step hashing in the timed loop
        ..FullRunConfig::default()
    };
    let seed_u64 = seed;
    let mut rm = new_run_manager_for_character(seed_u64, CharacterId::Ironclad);
    let mut policy = RandomPolicy::from_run_seed(seed_u64);
    let report = drive_full_run(&mut rm, &mut policy, &cfg);
    // Anything that isn't a clean win/loss (livelock, wedge,
    // NotImplemented, cap) is flagged so a non-zero count invalidates
    // the throughput number.
    let anomalous = !matches!(report.outcome, RunOutcome::Won | RunOutcome::Lost);
    Tally {
        runs: 1,
        wins: u64::from(report.player_won),
        steps: report.steps as u64,
        anomalies: u64::from(anomalous),
    }
}

fn combat_template(seed: u64) -> sts2_core::CombatState {
    let mut enc_rng = sts2_core::dotnet_rng::Rng::new(seed);
    let monsters =
        resolve_encounter("random-act1", &mut enc_rng).expect("random-act1 preset resolves");
    let mut state =
        build_starter_state_from_monsters(seed, monsters, PlayerClass::Ironclad, None, None, None);
    let _ = start_combat(&mut state);
    let _ = start_player_turn(&mut state);
    state
}

fn combat_one(template: &sts2_core::CombatState, rep_seed: u64) -> Tally {
    let mut state = template.clone();
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(rep_seed);
    let mut steps = 0u64;
    while !state.is_over && steps < 5_000 {
        let actions = legal_actions(&state);
        if actions.is_empty() {
            break;
        }
        let pick = rng.gen_range(0..actions.len());
        let _ = apply(&mut state, actions[pick].clone());
        steps += 1;
    }
    Tally {
        runs: 1,
        wins: u64::from(state.player_won),
        steps,
        anomalies: u64::from(!state.is_over),
    }
}

fn main() {
    let args = Args::parse();
    sts2_content::cards::dispatch::ensure_installed();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.threads)
        .build()
        .expect("rayon pool");

    let seeds: Vec<u64> = (args.seed_start..args.seed_start + args.seeds).collect();

    // Warm-up + template construction OUTSIDE the timed region.
    let (tally, elapsed) = match args.mode.as_str() {
        "fullrun" => {
            let _ = fullrun_one(args.seed_start); // warm registries
            let t0 = Instant::now();
            let tally = pool.install(|| {
                seeds
                    .par_iter()
                    .map(|&s| fullrun_one(s))
                    .reduce(Tally::default, Tally::merge)
            });
            (tally, t0.elapsed())
        }
        "combat" => {
            let templates: Vec<_> = seeds.iter().map(|&s| combat_template(s)).collect();
            let _ = combat_one(&templates[0], 1); // warm-up
            let t0 = Instant::now();
            let tally = pool.install(|| {
                templates
                    .par_iter()
                    .enumerate()
                    .map(|(i, tpl)| {
                        (0..args.reps)
                            .map(|r| combat_one(tpl, (i as u64) * 1_000_003 + r))
                            .fold(Tally::default(), Tally::merge)
                    })
                    .reduce(Tally::default, Tally::merge)
            });
            (tally, t0.elapsed())
        }
        // Diagnostic: RunManager construction cost alone (no driving).
        "setup" => {
            let t0 = Instant::now();
            let mut n = 0u64;
            for &s in &seeds {
                let rm = new_run_manager_for_character(s % i32::MAX as u64, CharacterId::Ironclad);
                n += rm.get_available_actions().len() as u64;
            }
            (
                Tally {
                    runs: seeds.len() as u64,
                    steps: n,
                    ..Tally::default()
                },
                t0.elapsed(),
            )
        }
        other => panic!("unknown mode {other:?}"),
    };

    let secs = elapsed.as_secs_f64();
    println!(
        "{{\"sim\":\"sts2-rs\",\"mode\":\"{}\",\"threads\":{},\"seeds\":{},\"reps\":{},\"playouts\":{},\"wins\":{},\"decisions\":{},\"anomalies\":{},\"elapsed_s\":{:.4},\"playouts_per_s\":{:.1},\"decisions_per_s\":{:.0}}}",
        args.mode,
        args.threads,
        args.seeds,
        args.reps,
        tally.runs,
        tally.wins,
        tally.steps,
        tally.anomalies,
        secs,
        tally.runs as f64 / secs,
        tally.steps as f64 / secs,
    );
}
