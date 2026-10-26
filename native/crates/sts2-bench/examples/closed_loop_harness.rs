//! `closed_loop_harness` — terminal-fight win-rate evaluator (bead
//! `sts2-rs-n5yex`, GH #20).
//!
//! Reports actual closed-loop fight outcomes, NOT root-rollout win
//! fractions. The existing `mcts_ab_value_net` / `vbzji_outcome_slice.py`
//! report `wins / visits` from the recommended root action — that's
//! the per-rollout average inside MCTS, not the agent's win rate when
//! the recommended action is actually applied and the fight is played
//! out to terminal.
//!
//! Loop shape (per state in the corpus):
//!   1. Load the realistic-corpus state at floor / encounter.
//!   2. While not terminal:
//!        a. Run MCTS for `--sims` from the current root.
//!        b. Pick the recommended (argmax-Q tiebreak visits) action.
//!        c. Apply it to the real state.
//!   3. Record the terminal `(player_won, final_hp, dmg_taken, turns,
//!      action_count)`.
//!
//! Report shape (additive — does NOT collide with `AbReport`):
//!
//! ```jsonc
//! {
//!   "version": 1,
//!   "git_sha": "...",
//!   "engine_version": "0.0.1",
//!   "config": { "leaf_eval": "texel", "sims": 300, "max_rollout_depth": 10 },
//!   "summary": {
//!     "states_total": 50,
//!     "states_won": 21,
//!     "win_rate": 0.42,
//!     "avg_final_hp": 22.1,
//!     "avg_turns_to_terminal": 7.2
//!   },
//!   "per_state": [
//!     { "state_id": 0, "encounter": "...", "tier": "Common",
//!       "player_won": true, "final_hp": 41, "dmg_taken": 39,
//!       "turns": 5, "action_count": 14, "wall_ms": 320 }
//!   ]
//! }
//! ```
//!
//! Comparison with `AbReport.per_state[i].results["300"].wins / visits`:
//! - `wins / visits` averages over many simulated rollouts that may
//!   never actually be played; it overstates agent strength when the
//!   recommended action is bad in expectation but the rollout policy
//!   gets lucky.
//! - `closed_loop.summary.win_rate` is the unconditional terminal
//!   fight win rate from the SAME initial state under the SAME
//!   recommended-action policy as MCTS in deployment.
//!
//! Bead acceptance: "Bench output distinguishes root-rollout win
//! fraction from closed-loop win rate. Harness compares 2 MCTS
//! configs by terminal fight results from same initial states.
//! Existing docs stop using root rollout % as agent win rates."
//!
//! Scaffold scope (this commit): single-config closed-loop runner +
//! summary. Multi-config side-by-side comparison is a clean follow-up
//! once the scaffold has produced its first calibration baseline.

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use common::{data_path, git_short_sha, write_json_pretty, CorpusTier};
use serde::{Deserialize, Serialize};
use sts2_cli::scenario_to_combat_state;
use sts2_cli::sim::scenario::toml_file::load_scenario_file;
use sts2_core::{apply, is_terminal, legal_actions, CombatState};
use sts2_mcts::{LeafEval, LearnedValueNet, Mcts, MctsConfig, TerminalEvalMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeafEvalMode {
    Texel,
    Learned,
}

impl LeafEvalMode {
    fn label(self) -> &'static str {
        match self {
            Self::Texel => "texel",
            Self::Learned => "learned",
        }
    }
}

#[derive(Debug)]
struct Args {
    leaf_eval: LeafEvalMode,
    sims: u32,
    onnx_path: Option<PathBuf>,
    norm_path: Option<PathBuf>,
    output: PathBuf,
    corpus_path: PathBuf,
    max_rollout_depth: u32,
    /// Hard cap on actions per fight; safety net for pathological
    /// loops. Default 1000 is far above any realistic STS2 combat
    /// length (typical 5-30 actions per fight).
    max_actions_per_fight: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct RealisticCorpus {
    entries: Vec<RealisticCorpusEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct RealisticCorpusEntry {
    state_id: u32,
    tier: String,
    fixture_path: String,
    seed: u64,
}

#[derive(Debug, Clone, Serialize)]
struct PerStateOutcome {
    state_id: u32,
    encounter: String,
    tier: CorpusTier,
    player_won: bool,
    final_hp: i32,
    dmg_taken: i32,
    turns: u32,
    action_count: u32,
    wall_ms: u64,
    /// Set when the fight hit the action-cap safety net. A "timeout"
    /// is recorded as `player_won = false` for win-rate accounting.
    timeout: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ClosedLoopSummary {
    states_total: u32,
    states_won: u32,
    states_timeout: u32,
    win_rate: f64,
    avg_final_hp: f64,
    avg_turns: f64,
}

#[derive(Debug, Clone, Serialize)]
struct ClosedLoopReport {
    version: u32,
    git_sha: String,
    engine_version: String,
    config: BTreeMap<String, serde_json::Value>,
    summary: ClosedLoopSummary,
    per_state: Vec<PerStateOutcome>,
}

fn parse_tier(s: &str) -> CorpusTier {
    match s {
        "elite" => CorpusTier::Elite,
        "boss" => CorpusTier::Boss,
        _ => CorpusTier::Common,
    }
}

fn parse_args() -> Result<Args, String> {
    let mut leaf_eval = LeafEvalMode::Texel;
    let mut sims = 300u32;
    let mut onnx_path: Option<PathBuf> = None;
    let mut norm_path: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut corpus_path: Option<PathBuf> = None;
    let mut max_rollout_depth = 10u32;
    let mut max_actions_per_fight = 1000u32;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let (key, value) = match arg.find('=') {
            Some(i) => (arg[..i].to_string(), Some(arg[i + 1..].to_string())),
            None => (arg.clone(), it.next()),
        };
        match (key.as_str(), value) {
            ("--leaf-eval", Some(v)) => {
                leaf_eval = match v.as_str() {
                    "texel" => LeafEvalMode::Texel,
                    "learned" => LeafEvalMode::Learned,
                    _ => return Err(format!("--leaf-eval must be 'texel' or 'learned', got {v}")),
                };
            }
            ("--sims", Some(v)) => {
                sims = v.parse().map_err(|e| format!("--sims parse: {e}"))?;
            }
            ("--onnx-path", Some(v)) => onnx_path = Some(PathBuf::from(v)),
            ("--norm-path", Some(v)) => norm_path = Some(PathBuf::from(v)),
            ("--output", Some(v)) => output = Some(PathBuf::from(v)),
            ("--corpus-path", Some(v)) => corpus_path = Some(PathBuf::from(v)),
            ("--max-rollout-depth", Some(v)) => {
                max_rollout_depth = v
                    .parse()
                    .map_err(|e| format!("--max-rollout-depth parse: {e}"))?;
            }
            ("--max-actions-per-fight", Some(v)) => {
                max_actions_per_fight = v
                    .parse()
                    .map_err(|e| format!("--max-actions-per-fight parse: {e}"))?;
            }
            ("--help" | "-h", _) => {
                eprintln!("{USAGE}");
                std::process::exit(0);
            }
            (k, _) => return Err(format!("unknown or malformed flag: {k}")),
        }
    }

    let corpus_path = corpus_path.unwrap_or_else(|| data_path("mcts_corpus_realistic.json"));
    let output = output.unwrap_or_else(|| {
        data_path(&format!(
            "closed_loop_{sims}_{}_{}.json",
            leaf_eval.label(),
            git_short_sha(),
        ))
    });

    Ok(Args {
        leaf_eval,
        sims,
        onnx_path,
        norm_path,
        output,
        corpus_path,
        max_rollout_depth,
        max_actions_per_fight,
    })
}

const USAGE: &str = "Usage: closed_loop_harness [OPTIONS]\n\n\
Options:\n  \
--leaf-eval=texel|learned     Leaf-score path (default: texel)\n  \
--sims=N                      MCTS sims per decision (default: 300)\n  \
--onnx-path=PATH              Required when --leaf-eval=learned\n  \
--norm-path=PATH              Optional norm-stats JSON for Learned\n  \
--corpus-path=PATH            Corpus JSON (default: data/mcts_corpus_realistic.json)\n  \
--max-rollout-depth=N         MCTS rollout depth-cut (default: 10)\n  \
--max-actions-per-fight=N     Safety cap on actions per fight (default: 1000)\n  \
--output=PATH                 JSON output";

fn build_leaf_eval(args: &Args) -> Result<LeafEval, String> {
    match args.leaf_eval {
        LeafEvalMode::Texel => Ok(LeafEval::Texel),
        LeafEvalMode::Learned => {
            // s6v2t: prefer --norm-path (weights JSON); fall back to
            // <onnx-path>.json for legacy callers.
            let weights_path = if let Some(p) = args.norm_path.as_ref() {
                p.clone()
            } else if let Some(o) = args.onnx_path.as_ref() {
                let mut p = o.clone();
                p.set_extension("json");
                p
            } else {
                return Err("--leaf-eval=learned requires --norm-path or --onnx-path".to_string());
            };
            let net = LearnedValueNet::from_weights_path(&weights_path).map_err(|e| {
                format!(
                    "weights JSON load failed at {}: {e}",
                    weights_path.display()
                )
            })?;
            Ok(LeafEval::Learned(Arc::new(net)))
        }
    }
}

/// Drive one fight from `template` to terminal under MCTS-recommended
/// actions. Returns the per-state outcome row.
fn play_one_fight(
    state_id: u32,
    encounter: String,
    tier: CorpusTier,
    seed: u64,
    template: &CombatState,
    leaf_eval: &LeafEval,
    args: &Args,
) -> PerStateOutcome {
    let starting_hp = template.player.current_hp;
    let mut state = template.clone();
    let _ = sts2_core::combat_loop::start_combat(&mut state);
    let _ = sts2_core::combat_loop::start_player_turn(&mut state);

    let mut action_count = 0u32;
    let mut turn_count = 0u32;
    let mut last_round = state.round;
    let start = Instant::now();
    let mut timeout = false;

    while !is_terminal(&state) && action_count < args.max_actions_per_fight {
        let actions = legal_actions(&state);
        if actions.is_empty() {
            let _ = apply(&mut state, sts2_core::Action::EndTurn);
            action_count += 1;
            continue;
        }

        let cfg = MctsConfig {
            max_simulations: args.sims,
            max_rollout_depth: args.max_rollout_depth,
            seed: seed
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(u64::from(action_count)),
            terminal_eval_mode: TerminalEvalMode::RichBoth,
            ..MctsConfig::default()
        };
        let mut mcts = Mcts::new(state.clone(), cfg).with_leaf_eval(leaf_eval.clone());
        let recommended = mcts.search();
        let _ = apply(&mut state, recommended);
        action_count += 1;
        if state.round != last_round {
            turn_count += 1;
            last_round = state.round;
        }
    }

    if !is_terminal(&state) {
        timeout = true;
    }

    let wall_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let final_hp = state.player.current_hp.max(0);
    let dmg_taken = (starting_hp - final_hp).max(0);
    let player_won = is_terminal(&state) && final_hp > 0;

    PerStateOutcome {
        state_id,
        encounter,
        tier,
        player_won,
        final_hp,
        dmg_taken,
        turns: turn_count,
        action_count,
        wall_ms,
        timeout,
    }
}

fn main() -> std::io::Result<()> {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[closed_loop_harness] error: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    let leaf_eval = match build_leaf_eval(&args) {
        Ok(le) => le,
        Err(e) => {
            eprintln!("[closed_loop_harness] error: {e}");
            std::process::exit(2);
        }
    };

    eprintln!(
        "[closed_loop_harness] mode={} sims={} max_rollout_depth={} corpus={}",
        args.leaf_eval.label(),
        args.sims,
        args.max_rollout_depth,
        args.corpus_path.display(),
    );

    let bytes = std::fs::read(&args.corpus_path)
        .unwrap_or_else(|e| panic!("failed to read corpus {}: {e}", args.corpus_path.display()));
    let corpus: RealisticCorpus = serde_json::from_slice(&bytes).expect("corpus JSON parses");

    std::panic::set_hook(Box::new(|_| {}));

    let mut outcomes: Vec<PerStateOutcome> = Vec::with_capacity(corpus.entries.len());
    for entry in &corpus.entries {
        let fixture = PathBuf::from(&entry.fixture_path);
        let Ok(scenario) = load_scenario_file(&fixture) else {
            eprintln!(
                "[closed_loop_harness] skip state_id={} (fixture load failed)",
                entry.state_id,
            );
            continue;
        };
        let Ok(template) = scenario_to_combat_state(&scenario, entry.seed) else {
            eprintln!(
                "[closed_loop_harness] skip state_id={} (scenario_to_combat_state failed)",
                entry.state_id,
            );
            continue;
        };
        let tier = parse_tier(&entry.tier);
        let encounter = fixture
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>")
            .to_string();
        let seed = entry.seed;
        let state_id = entry.state_id;
        let leaf = leaf_eval.clone();
        let args_ref = &args;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            play_one_fight(
                state_id,
                encounter.clone(),
                tier,
                seed,
                &template,
                &leaf,
                args_ref,
            )
        }));
        let outcome = result.unwrap_or_else(|_| {
            eprintln!(
                "[closed_loop_harness] state_id={} PANICKED (content gap); recording loss",
                entry.state_id,
            );
            PerStateOutcome {
                state_id,
                encounter,
                tier,
                player_won: false,
                final_hp: 0,
                dmg_taken: template.player.current_hp,
                turns: 0,
                action_count: 0,
                wall_ms: 0,
                timeout: false,
            }
        });
        eprintln!(
            "[closed_loop_harness] state_id={} won={} hp={}/{} dmg={} turns={} actions={} wall_ms={}{}",
            outcome.state_id,
            outcome.player_won,
            outcome.final_hp,
            template.player.max_hp,
            outcome.dmg_taken,
            outcome.turns,
            outcome.action_count,
            outcome.wall_ms,
            if outcome.timeout { " TIMEOUT" } else { "" },
        );
        outcomes.push(outcome);
    }

    let states_total = u32::try_from(outcomes.len()).unwrap_or(u32::MAX);
    let states_won =
        u32::try_from(outcomes.iter().filter(|o| o.player_won).count()).unwrap_or(u32::MAX);
    let states_timeout =
        u32::try_from(outcomes.iter().filter(|o| o.timeout).count()).unwrap_or(u32::MAX);
    let win_rate = if states_total == 0 {
        0.0
    } else {
        f64::from(states_won) / f64::from(states_total)
    };
    let avg_final_hp = if states_total == 0 {
        0.0
    } else {
        outcomes.iter().map(|o| f64::from(o.final_hp)).sum::<f64>() / f64::from(states_total)
    };
    let avg_turns = if states_total == 0 {
        0.0
    } else {
        outcomes.iter().map(|o| f64::from(o.turns)).sum::<f64>() / f64::from(states_total)
    };

    let mut config = BTreeMap::new();
    config.insert(
        "leaf_eval".into(),
        serde_json::Value::from(args.leaf_eval.label()),
    );
    config.insert("sims".into(), serde_json::Value::from(args.sims));
    config.insert(
        "max_rollout_depth".into(),
        serde_json::Value::from(args.max_rollout_depth),
    );
    config.insert(
        "max_actions_per_fight".into(),
        serde_json::Value::from(args.max_actions_per_fight),
    );

    let report = ClosedLoopReport {
        version: 1,
        git_sha: git_short_sha(),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        config,
        summary: ClosedLoopSummary {
            states_total,
            states_won,
            states_timeout,
            win_rate,
            avg_final_hp,
            avg_turns,
        },
        per_state: outcomes,
    };

    write_json_pretty(&args.output, &report)?;
    eprintln!(
        "[closed_loop_harness] DONE - states_total={} won={} timeout={} win_rate={:.3} -> {}",
        states_total,
        states_won,
        states_timeout,
        win_rate,
        args.output.display(),
    );
    Ok(())
}
