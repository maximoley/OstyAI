//! `mcts_ab_value_net` — Wave-6 acceptance gate harness.
//!
//! Bead `sts2-rs-5n5y` (qctu.6). Runs the yuyc 50-state corpus at
//! `sims=300` under one of two leaf-score modes and persists the
//! per-state outcomes in the standard [`AbReport`] schema so the
//! existing `mcts_ab_diff` example can compare two runs.
//!
//! ## Modes
//!
//! - `texel` (default): the engine's `LeafEval::Texel` —
//!   `sts2_core::evaluate_in_progress`. Byte-stable against the
//!   pre-qctu MCTS path.
//! - `learned`: `LeafEval::Learned` backed by the ONNX model at
//!   `--onnx-path`. The model must be a qctu.4-exported graph
//!   (`features [N, 80]` input + `value` head). Optional
//!   `--norm-path` points at the qctu.3 weights JSON so the
//!   `feature_mean` / `feature_std` arrays from the trainer get
//!   applied inline before each forward pass.
//!
//! ## Acceptance protocol
//!
//! ```bash
//! # Baseline at HEAD (Texel HCE).
//! cargo run --release --example mcts_ab_value_net -- \
//!     --leaf-eval=texel --sims=300 \
//!     --output crates/sts2-bench/data/mcts_ab_300_texel.json
//!
//! # Learned-net challenger (requires a qctu.4 .onnx and qctu.3 JSON).
//! cargo run --release --example mcts_ab_value_net -- \
//!     --leaf-eval=learned --sims=300 \
//!     --onnx-path /tmp/value-policy-net.onnx \
//!     --norm-path /tmp/value-policy-net.json \
//!     --output crates/sts2-bench/data/mcts_ab_300_learned.json
//!
//! # Diff.
//! cargo run --release --example mcts_ab_diff -- \
//!     crates/sts2-bench/data/mcts_ab_300_texel.json \
//!     crates/sts2-bench/data/mcts_ab_300_learned.json
//! ```
//!
//! Acceptance gate (mirrors `sts2-rs-p8gq` for the Texel chain):
//!   - Δ win-rate at sims=300 ≥ **+3 pp** vs the Texel baseline.
//!   - No per-state regression > 2 wins per seed-bucket.

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use common::{
    action_to_string, data_path, git_short_sha, realize_state, write_json_pretty, AbReport,
    BudgetResult, Corpus, PerStateRow,
};
use serde::{Deserialize, Serialize};
use sts2_cli::scenario_to_combat_state;
use sts2_cli::sim::scenario::toml_file::load_scenario_file;
use sts2_core::{apply, is_terminal, legal_actions, Action, CombatPhase, CombatState};
use sts2_mcts::{
    EmbeddingValueNet, LeafEval, LearnedPolicyPrior, LearnedValueNet, Mcts, MctsConfig,
    TerminalEvalMode,
};

/// Advance `state` by `n` player turns of uniform-random legal play.
///
/// Each "turn" is: while phase==PlayerPlayPhase, pick a random legal
/// action and apply it; whenever EndTurn is picked (or phase exits
/// PlayerPlayPhase), drive Action::EndTurn until phase returns to
/// PlayerPlayPhase. Returns true if the state is alive (not terminal)
/// after advancing; false if the combat resolved mid-advance and the
/// caller should drop this entry.
///
/// Seeded deterministically from `seed` so repeated calls produce
/// the same mid-combat snapshot — critical for A/B comparability
/// across leaf-eval modes.
fn advance_n_turns(state: &mut CombatState, n: u32, seed: u64) -> bool {
    if n == 0 {
        return !is_terminal(state);
    }
    let mut rng = sts2_core::dotnet_rng::Rng::new(seed);
    let mut turns_advanced = 0u32;
    let hard_step_cap = 10_000u64;
    let mut step_index = 0u64;

    while turns_advanced < n && !is_terminal(state) {
        if step_index >= hard_step_cap {
            return false;
        }
        step_index += 1;

        if matches!(state.phase, CombatPhase::PlayerPlayPhase) {
            let actions = legal_actions(state);
            if actions.is_empty() {
                let _ = apply(state, Action::EndTurn);
                turns_advanced += 1;
                continue;
            }
            let idx = rng.next_int_exclusive(0, actions.len() as i32) as usize;
            let action = actions[idx].clone();
            let was_end_turn = matches!(action, Action::EndTurn);
            let _ = apply(state, action);
            if was_end_turn {
                turns_advanced += 1;
            }
        } else {
            let _ = apply(state, Action::EndTurn);
        }
    }
    !is_terminal(state)
}

#[derive(Debug)]
struct Args {
    leaf_eval: LeafEvalMode,
    sims: u32,
    onnx_path: Option<PathBuf>,
    norm_path: Option<PathBuf>,
    output: PathBuf,
    /// Bead `sts2-rs-d892` (aze8.2). Mirrors `mcts_ab.rs::65q2`'s
    /// `(--corpus kind, --corpus-path filename)` pair: `kind`
    /// picks which schema to deserialize, `filename` (optional)
    /// overrides the default file. Defaults: `starter` +
    /// `mcts_corpus.json` — bit-identical back-compat with the
    /// 5n5y/qctu.6 baseline. Pointed at `realistic` +
    /// `mcts_corpus_realistic.json` for the Wave-6 retry that
    /// `os46` saturation analysis blocked on.
    corpus_kind: CorpusKind,
    corpus_path: Option<PathBuf>,
    /// Mid-combat A/B test: advance the loaded state by N player
    /// turns of uniform-random legal play before running MCTS. The
    /// v2 corpus fixtures are start-of-combat snapshots (hand=0,
    /// pre-draw); at sims=10+ the MCTS root action is fully
    /// determined regardless of leaf eval, so the harness cannot
    /// differentiate Texel from Learned. Advancing N turns
    /// surfaces deeper combat states where the leaf eval may
    /// surface differently. Default 0 = back-compat (start-of-
    /// combat). Deterministic per state_id.
    advance_turns: u32,
    /// MCTS depth at which a rollout depth-cuts and consults the
    /// leaf eval (Texel or Learned). Default 200 = effectively
    /// never hit — rollouts terminate naturally before depth 200,
    /// so the leaf eval is a no-op. Set to 10 (corpus_gen default,
    /// per `project_mcts_budget_knee` memory) for the A/B to
    /// actually exercise the leaf eval.
    max_rollout_depth: u32,
    terminal_eval_mode: TerminalEvalMode,
    /// Bead `sts2-rs-fgw1x` (s6umt follow-up). When set to a path to
    /// an existing `AbReport` JSON, the per-state loop classifies each
    /// new (this-run = branch) vs baseline outcome as W/D/L from the
    /// branch's perspective and accumulates LLR. When LLR crosses the
    /// Wald acceptance bounds defined by `sprt_*` flags, the run
    /// terminates early. Min-sample guard at N=10. Default `None`
    /// preserves byte-stable behavior — no SPRT, runs the full corpus.
    sprt_baseline: Option<PathBuf>,
    sprt_cfg: common::SprtConfig,
    /// Bead `sts2-rs-l38nd`. When `true`, run MCTS selection in
    /// PUCT mode with a `LearnedPolicyPrior` reading from
    /// `policy_onnx_path` instead of UCB + UniformPolicy. The
    /// leaf-eval mode is independent — typical usage pairs PUCT
    /// (better selection) with `--leaf-eval=texel` (better leaf
    /// scoring, post-vbzji) to isolate the policy-head lift.
    use_puct_prior: bool,
    policy_onnx_path: Option<PathBuf>,
    policy_norm_path: Option<PathBuf>,
    /// Caissawary Texel composition coefficient `k` for the learned /
    /// learned-embed leaf eval: emit `tanh(nn_logit + k·texel_norm)`
    /// instead of pure NN. `None` ⇒ pure NN. Bead `sts2-rs-ae14u`.
    texel_compose_k: Option<f32>,
    /// Phase 2 ExpI. When set, build the PUCT prior from an
    /// `EmbeddingValueNet` (DeepSets, `value_policy_net_generalist.py`
    /// output) instead of the legacy `LearnedValueNet`. Implies PUCT
    /// (no separate `--use-puct-prior` needed). The corpus-gen
    /// `--embed-prior-weights` path consumes the same net, so a prior
    /// that shaped self-play scores identically here in the gate.
    policy_embed_path: Option<PathBuf>,
    /// Phase 2 ExpI. When set, the leaf eval is a `LeafEval::TexelWideRuntime`
    /// reading these `texel_tune.py --target value` weights
    /// (`fitted_weights_ordered`) — overrides `--leaf-eval`. Lets a round's
    /// freshly-fit wide leaf be gated against the const champion in one binary.
    leaf_weights_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CorpusKind {
    Starter,
    Realistic,
}

impl CorpusKind {
    fn default_filename(self) -> &'static str {
        match self {
            Self::Starter => "mcts_corpus.json",
            // sts2-rs-5wqir: point at the 200-state v2 corpus
            // (acbj-balanced, group-split provenance) instead of the
            // legacy 50-state `mcts_corpus_realistic.json`. l38nd's
            // +4.30pp PUCT-lift number was measured on v2; the 50-state
            // file was the default and silently halved every qmncd /
            // 5wqir follow-up A/B (n=50 → wider CIs, baseline drifted
            // to +2.67pp). Keep `mcts_corpus_realistic.json` reachable
            // via explicit `--corpus-path` for historical-comparison
            // runs.
            Self::Realistic => "mcts_corpus_realistic_v2.json",
        }
    }
}

/// Reader-only mirror of `mcts_corpus_realistic.rs::RealisticCorpus`.
/// Same shape as the sibling declaration in `mcts_ab.rs` (bead
/// `sts2-rs-ch1h`).
#[derive(Debug, Clone, Deserialize, Serialize)]
struct RealisticCorpus {
    version: u32,
    git_sha: String,
    generator: String,
    bucket_globs: std::collections::BTreeMap<String, String>,
    entries: Vec<RealisticCorpusEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RealisticCorpusEntry {
    state_id: u32,
    tier: String,
    fixture_path: String,
    floor: u16,
    seed: u64,
    // gh #14: v3 corpora stratify on player_outcome and persist the
    // label here. Optional + default so v1/v2 JSON still loads.
    #[serde(default)]
    player_outcome: Option<String>,
}

/// One realized state for the MCTS loop. Mirrors the
/// `RealizedEntry` shape in `mcts_ab.rs::ch1h` so future work can
/// hoist the load helper into `common/`.
struct RealizedEntry {
    state_id: u32,
    encounter: String,
    tier: common::CorpusTier,
    state_hash: u64,
    template: CombatState,
}

fn parse_realistic_tier(s: &str) -> common::CorpusTier {
    match s {
        "elite" => common::CorpusTier::Elite,
        "boss" => common::CorpusTier::Boss,
        _ => common::CorpusTier::Common,
    }
}

fn load_starter(corpus_path: &std::path::Path) -> Vec<RealizedEntry> {
    let bytes = std::fs::read(corpus_path).unwrap_or_else(|e| {
        panic!(
            "failed to read {} ({}). Run `cargo run --release --example mcts_corpus` first.",
            corpus_path.display(),
            e
        )
    });
    let corpus: Corpus = serde_json::from_slice(&bytes).expect("corpus JSON parses");
    corpus
        .entries
        .into_iter()
        .map(|entry| {
            let mut entry_mut = entry.clone();
            let template = realize_state(&mut entry_mut);
            RealizedEntry {
                state_id: entry.state_id,
                encounter: entry.encounter,
                tier: entry.tier,
                state_hash: entry_mut.state_hash,
                template,
            }
        })
        .collect()
}

fn load_realistic(corpus_path: &std::path::Path, advance_turns: u32) -> Vec<RealizedEntry> {
    let bytes = std::fs::read(corpus_path).unwrap_or_else(|e| {
        panic!(
            "failed to read {} ({}). Run `cargo run --release --example mcts_corpus_realistic` first.",
            corpus_path.display(),
            e
        )
    });
    let corpus: RealisticCorpus =
        serde_json::from_slice(&bytes).expect("realistic corpus JSON parses");
    eprintln!(
        "[mcts_ab_value_net] loaded realistic corpus ({} entries, generator={})",
        corpus.entries.len(),
        corpus.generator,
    );
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    corpus
        .entries
        .into_iter()
        .filter_map(|entry| {
            let fixture = PathBuf::from(&entry.fixture_path);
            let scenario = load_scenario_file(&fixture).unwrap_or_else(|e| {
                panic!(
                    "failed to load realistic fixture {} ({}). Regenerate via \
                     `cargo run --release --example mcts_corpus_realistic`.",
                    fixture.display(),
                    e
                )
            });
            let mut template =
                scenario_to_combat_state(&scenario, entry.seed).unwrap_or_else(|e| {
                    panic!(
                        "scenario_to_combat_state failed for {} ({}). The \
                         corpus generator should have filtered this entry.",
                        fixture.display(),
                        e
                    )
                });
            // Library returns state pre-`start_combat`; drive to
            // PlayerPlayPhase so MCTS sees a real decision point.
            let _ = sts2_core::combat_loop::start_combat(&mut template);
            let _ = sts2_core::combat_loop::start_player_turn(&mut template);
            // sts2-rs-vbzji: optional mid-combat snapshot. Advance
            // `advance_turns` player turns of uniform-random play
            // before MCTS sees the state. Skips entries that resolve
            // (terminal) during the advance phase.
            let alive = advance_n_turns(
                &mut template,
                advance_turns,
                entry.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15),
            );
            if !alive {
                return None;
            }
            let state_hash = sts2_core::hashing::state_hash(&template);
            Some(RealizedEntry {
                state_id: entry.state_id,
                encounter: fixture
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("<unknown>")
                    .to_string(),
                tier: parse_realistic_tier(&entry.tier),
                state_hash,
                template,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeafEvalMode {
    Texel,
    /// Wide 172-feature Texel leaf (`LeafEval::TexelWide`).
    TexelWide,
    Learned,
    /// Embedding-aware generalist net (`value_policy_net_generalist.py`).
    LearnedEmbed,
}

impl LeafEvalMode {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "texel" | "Texel" => Some(Self::Texel),
            "texel-wide" | "TexelWide" => Some(Self::TexelWide),
            "learned" | "Learned" => Some(Self::Learned),
            "learned-embed" | "embed" => Some(Self::LearnedEmbed),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Texel => "texel",
            Self::TexelWide => "texel-wide",
            Self::Learned => "learned",
            Self::LearnedEmbed => "learned-embed",
        }
    }
}

/// Phase 2: the PUCT prior is absent, a legacy `LearnedValueNet`
/// (`--use-puct-prior` + `--policy-norm-path`/`--policy-onnx-path`), or a
/// DeepSets `EmbeddingValueNet` (`--policy-embed-path`,
/// `value_policy_net_generalist.py` output). The two net types monomorphize
/// `Mcts<P>` differently, so the per-state search branches on this.
#[derive(Clone)]
enum PriorKind {
    None,
    Legacy(Arc<LearnedValueNet>),
    Embed(Arc<EmbeddingValueNet>),
}

fn parse_corpus_kind(s: &str) -> Result<CorpusKind, String> {
    match s {
        "starter" => Ok(CorpusKind::Starter),
        "realistic" => Ok(CorpusKind::Realistic),
        _ => Err(format!(
            "--corpus expects 'starter' or 'realistic', got {s:?}"
        )),
    }
}

fn parse_args() -> Result<Args, String> {
    let mut leaf_eval = LeafEvalMode::Texel;
    let mut sims = 300u32;
    let mut onnx_path: Option<PathBuf> = None;
    let mut norm_path: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    // Default flipped from Starter → Realistic by sts2-rs-a6wt.
    // See `[[feedback_corpus_starter_deck_too_simple]]` for rationale.
    let mut corpus_kind = CorpusKind::Realistic;
    let mut corpus_path: Option<PathBuf> = None;
    let mut advance_turns = 0u32;
    // sts2-rs-vbzji: defaults changed from the previous (depth=200,
    // terminal=binary) configuration that disabled the leaf eval
    // entirely. At depth=200 rollouts almost always terminate before
    // hitting the cap, so Texel/Learned were never consulted — every
    // prior A/B verdict (dnhe/lomw/acbj/ibfg.{a,b}) reported Δ≈0pp
    // and PV[0]=100% because the harness was comparing two
    // configurations that both ignored their leaf evals. Depth=10
    // matches `sts2-corpus-gen --max-rollout-depth 10` (the value
    // that actually trains the corpus); RichBoth fixes the flat
    // `-1.0` loss-branch in `terminal_value` that throws away the
    // rich state signal.
    let mut max_rollout_depth = 10u32;
    let mut terminal_eval_mode = TerminalEvalMode::RichBoth;
    let mut sprt_baseline: Option<PathBuf> = None;
    let mut sprt_cfg = common::SprtConfig::default();
    let mut use_puct_prior = false;
    let mut policy_onnx_path: Option<PathBuf> = None;
    let mut policy_norm_path: Option<PathBuf> = None;
    let mut policy_embed_path: Option<PathBuf> = None;
    let mut leaf_weights_path: Option<PathBuf> = None;
    let mut texel_compose_k: Option<f32> = None;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let (key, value) = match arg.find('=') {
            Some(i) => (arg[..i].to_string(), Some(arg[i + 1..].to_string())),
            None => (arg.clone(), it.next()),
        };
        match (key.as_str(), value) {
            ("--leaf-eval", Some(v)) => {
                leaf_eval = LeafEvalMode::parse(&v).ok_or_else(|| {
                    format!("--leaf-eval must be 'texel', 'learned', or 'learned-embed', got {v}")
                })?;
            }
            ("--sims", Some(v)) => {
                sims = v.parse().map_err(|e| format!("--sims parse: {e}"))?;
            }
            ("--onnx-path", Some(v)) => onnx_path = Some(PathBuf::from(v)),
            ("--norm-path", Some(v)) => norm_path = Some(PathBuf::from(v)),
            ("--output", Some(v)) => output = Some(PathBuf::from(v)),
            ("--corpus", Some(v)) => corpus_kind = parse_corpus_kind(&v)?,
            ("--corpus-path", Some(v)) => corpus_path = Some(PathBuf::from(v)),
            ("--advance-turns", Some(v)) => {
                advance_turns = v
                    .parse()
                    .map_err(|e| format!("--advance-turns parse: {e}"))?;
            }
            ("--max-rollout-depth", Some(v)) => {
                max_rollout_depth = v
                    .parse()
                    .map_err(|e| format!("--max-rollout-depth parse: {e}"))?;
            }
            ("--terminal-eval-mode", Some(v)) => {
                terminal_eval_mode = match v.as_str() {
                    "binary" => TerminalEvalMode::Binary,
                    "rich-loss" | "richloss" => TerminalEvalMode::RichLoss,
                    "rich-both" | "richboth" => TerminalEvalMode::RichBoth,
                    _ => return Err(format!(
                        "--terminal-eval-mode must be 'binary', 'rich-loss', or 'rich-both', got {v}"
                    )),
                };
            }
            ("--sprt-baseline", Some(v)) => sprt_baseline = Some(PathBuf::from(v)),
            ("--sprt-elo0", Some(v)) => {
                sprt_cfg.elo0 = v.parse().map_err(|e| format!("--sprt-elo0 parse: {e}"))?;
            }
            ("--sprt-elo1", Some(v)) => {
                sprt_cfg.elo1 = v.parse().map_err(|e| format!("--sprt-elo1 parse: {e}"))?;
            }
            ("--sprt-alpha", Some(v)) => {
                sprt_cfg.alpha = v.parse().map_err(|e| format!("--sprt-alpha parse: {e}"))?;
            }
            ("--sprt-beta", Some(v)) => {
                sprt_cfg.beta = v.parse().map_err(|e| format!("--sprt-beta parse: {e}"))?;
            }
            ("--use-puct-prior", v) => {
                use_puct_prior = match v.as_deref() {
                    None | Some("true" | "1") => true,
                    Some("false" | "0") => false,
                    Some(other) => {
                        return Err(format!(
                            "--use-puct-prior expects true/false/1/0, got {other}"
                        ))
                    }
                };
            }
            ("--policy-onnx-path", Some(v)) => policy_onnx_path = Some(PathBuf::from(v)),
            ("--policy-norm-path", Some(v)) => policy_norm_path = Some(PathBuf::from(v)),
            ("--policy-embed-path", Some(v)) => policy_embed_path = Some(PathBuf::from(v)),
            ("--leaf-weights-path", Some(v)) => leaf_weights_path = Some(PathBuf::from(v)),
            ("--texel-compose-k", Some(v)) => {
                texel_compose_k = Some(
                    v.parse()
                        .map_err(|e| format!("--texel-compose-k parse: {e}"))?,
                );
            }
            ("--help" | "-h", _) => {
                eprintln!("{USAGE}");
                std::process::exit(0);
            }
            (k, _) => return Err(format!("unknown or malformed flag: {k}")),
        }
    }

    let output = output.unwrap_or_else(|| {
        data_path(&format!(
            "mcts_ab_{sims}_{}_{}.json",
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
        corpus_kind,
        corpus_path,
        advance_turns,
        max_rollout_depth,
        terminal_eval_mode,
        sprt_baseline,
        sprt_cfg,
        use_puct_prior,
        policy_onnx_path,
        policy_norm_path,
        texel_compose_k,
        policy_embed_path,
        leaf_weights_path,
    })
}

const USAGE: &str = "Usage: mcts_ab_value_net [OPTIONS]\n\n\
Options:\n  \
--leaf-eval=texel|learned   Leaf-score path (default: texel)\n  \
--sims=N                    Sim budget per state (default: 300)\n  \
--onnx-path=PATH            Required when --leaf-eval=learned\n  \
--norm-path=PATH            Optional qctu.3 weights JSON for normalization\n  \
--corpus=starter|realistic  Corpus schema (default: starter)\n  \
--corpus-path=PATH          Corpus JSON file override (default: data/mcts_corpus.json\n                              \
                            for starter, data/mcts_corpus_realistic.json for realistic)\n  \
--advance-turns=N           Advance state N player turns of random play before MCTS\n                              \
                            (default: 0 = start-of-combat snapshot)\n  \
--max-rollout-depth=N       Rollout depth-cut where leaf eval is consulted\n                              \
                            (default: 10 — matches corpus_gen. Values >= 50 make\n                              \
                            the leaf eval a no-op since rollouts terminate first)\n  \
--terminal-eval-mode=MODE   Terminal-state value computation: binary | rich-loss | rich-both\n                              \
                            (default: rich-both. binary returns flat -1.0 on losses,\n                              \
                            discarding the rich state signal)\n  \
--output=PATH               JSON output (default: data/mcts_ab_<sims>_<mode>_<sha>.json)\n  \
--sprt-baseline=PATH        AbReport JSON to use as the SPRT baseline. When set, per-state\n                              \
                            outcomes are paired against the baseline's same-state row and\n                              \
                            an LLR accumulator drives early-termination on Wald-bound crossing\n                              \
                            (bead sts2-rs-fgw1x, parent s6umt).\n  \
--sprt-elo0=N               Lower Elo bound (default: 0.0, null hypothesis).\n  \
--sprt-elo1=N               Upper Elo bound (default: 10.0, alt hypothesis).\n  \
--sprt-alpha=N              Type-I error rate (default: 0.05).\n  \
--sprt-beta=N               Type-II error rate (default: 0.05).\n  \
--policy-embed-path=PATH    Phase 2: DeepSets EmbeddingValueNet JSON as PUCT prior\n                              \
                            (value_policy_net_generalist.py output; implies PUCT).\n  \
--leaf-weights-path=PATH    Phase 2: texel_tune.py --target value JSON as runtime wide\n                              \
                            leaf (fitted_weights_ordered); overrides --leaf-eval.";

/// Resolve the weights JSON path from a harness's flag pair. Post-s6v2t
/// the JSON carries both the trained weights AND the per-column norm
/// stats. Prefer `--norm-path` (the JSON itself); fall back to deriving
/// `<onnx_path with .json>` so legacy invocations keep working.
fn resolve_weights_path(
    norm_path: Option<&PathBuf>,
    onnx_path: Option<&PathBuf>,
) -> Result<PathBuf, String> {
    if let Some(p) = norm_path {
        return Ok(p.clone());
    }
    if let Some(o) = onnx_path {
        let mut p = o.clone();
        p.set_extension("json");
        return Ok(p);
    }
    Err("requires --norm-path (weights JSON) or --onnx-path (legacy)".to_string())
}

fn build_leaf_eval(args: &Args) -> Result<LeafEval, String> {
    // Phase 2: a runtime wide leaf overrides --leaf-eval entirely.
    if let Some(p) = args.leaf_weights_path.as_ref() {
        let w = sts2_mcts::value_net::load_wide_leaf_weights(p)
            .map_err(|e| format!("--leaf-weights-path {} load failed: {e}", p.display()))?;
        return Ok(LeafEval::TexelWideRuntime(Arc::new(w)));
    }
    match args.leaf_eval {
        LeafEvalMode::Texel => Ok(LeafEval::Texel),
        LeafEvalMode::TexelWide => Ok(LeafEval::TexelWide),
        LeafEvalMode::Learned => {
            let weights_path =
                resolve_weights_path(args.norm_path.as_ref(), args.onnx_path.as_ref())
                    .map_err(|e| format!("--leaf-eval=learned {e}"))?;
            let net = LearnedValueNet::from_weights_path(&weights_path).map_err(|e| {
                format!(
                    "weights JSON load failed at {}: {e}",
                    weights_path.display()
                )
            })?;
            Ok(LeafEval::Learned(Arc::new(net)))
        }
        LeafEvalMode::LearnedEmbed => {
            let weights_path =
                resolve_weights_path(args.norm_path.as_ref(), args.onnx_path.as_ref())
                    .map_err(|e| format!("--leaf-eval=learned-embed {e}"))?;
            let net = EmbeddingValueNet::from_weights_path(&weights_path).map_err(|e| {
                format!(
                    "embedding weights JSON load failed at {}: {e}",
                    weights_path.display()
                )
            })?;
            let net = net.with_texel_compose(args.texel_compose_k);
            Ok(LeafEval::LearnedEmbed(Arc::new(net)))
        }
    }
}

fn main() -> std::io::Result<()> {
    sts2_content::cards::dispatch::ensure_installed();

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[mcts_ab_value_net] error: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    let leaf_eval = match build_leaf_eval(&args) {
        Ok(le) => le,
        Err(e) => {
            eprintln!("[mcts_ab_value_net] error: {e}");
            std::process::exit(2);
        }
    };

    eprintln!(
        "[mcts_ab_value_net] mode={} sims={} output={} use_puct_prior={}",
        args.leaf_eval.label(),
        args.sims,
        args.output.display(),
        args.use_puct_prior,
    );

    // Bead `sts2-rs-l38nd`. When `--use-puct-prior` is set, load the
    // policy-head ONNX once and share an `Arc<LearnedValueNet>` across
    // per-state `LearnedPolicyPrior` instances. Mode-independent of
    // `--leaf-eval`: the typical pairing is texel-leaf + learned-policy
    // (vbzji established texel as the better leaf scorer; we want to
    // see if the policy head still provides independent lift via PUCT).
    let prior_kind: PriorKind = if let Some(p) = args.policy_embed_path.as_ref() {
        // Phase 2: DeepSets prior from value_policy_net_generalist.py. Same
        // net the corpus-gen --embed-prior-weights path loads, so a prior that
        // shaped self-play scores identically here in the gate.
        match EmbeddingValueNet::from_weights_path(p) {
            Ok(n) => {
                eprintln!(
                    "[mcts_ab_value_net] embed policy prior loaded from {}",
                    p.display()
                );
                PriorKind::Embed(Arc::new(n))
            }
            Err(e) => {
                eprintln!(
                    "[mcts_ab_value_net] --policy-embed-path load failed at {}: {e}",
                    p.display()
                );
                std::process::exit(2);
            }
        }
    } else if args.use_puct_prior {
        let weights_path = match resolve_weights_path(
            args.policy_norm_path.as_ref(),
            args.policy_onnx_path.as_ref(),
        ) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[mcts_ab_value_net] --use-puct-prior {e}");
                std::process::exit(2);
            }
        };
        match LearnedValueNet::from_weights_path(&weights_path) {
            Ok(n) => {
                eprintln!(
                    "[mcts_ab_value_net] policy weights path={} loaded",
                    weights_path.display()
                );
                PriorKind::Legacy(Arc::new(n))
            }
            Err(e) => {
                eprintln!(
                    "[mcts_ab_value_net] policy weights JSON load failed at {}: {e}",
                    weights_path.display()
                );
                std::process::exit(2);
            }
        }
    } else {
        PriorKind::None
    };
    // An embed prior implies PUCT even without --use-puct-prior.
    let want_prior = !matches!(prior_kind, PriorKind::None);

    let corpus_path = args
        .corpus_path
        .clone()
        .unwrap_or_else(|| data_path(args.corpus_kind.default_filename()));
    eprintln!(
        "[mcts_ab_value_net] corpus={:?} path={}",
        args.corpus_kind,
        corpus_path.display()
    );
    let realized = match args.corpus_kind {
        CorpusKind::Starter => load_starter(&corpus_path),
        CorpusKind::Realistic => {
            // Realistic mode exercises card-dispatch on cards the
            // starter pile dodges; per-state catch_unwind below
            // recovers, this hook just silences default-handler spew.
            std::panic::set_hook(Box::new(|_| {}));
            load_realistic(&corpus_path, args.advance_turns)
        }
    };

    let mut per_state: Vec<PerStateRow> = Vec::with_capacity(realized.len());
    let sims_key = args.sims.to_string();
    let mut wall_total: u64 = 0;

    // SPRT setup (sts2-rs-fgw1x). When --sprt-baseline is set, load the
    // baseline AbReport into a state_id → BudgetResult lookup and
    // accumulate paired W/D/L outcomes after each of our per-state
    // results. Early-exit on Wald-bound crossing (min N=10).
    let sprt_baseline: Option<BTreeMap<u32, BudgetResult>> =
        args.sprt_baseline.as_ref().map(|path| {
            let bytes = std::fs::read(path)
                .unwrap_or_else(|e| panic!("--sprt-baseline read {} failed: {e}", path.display()));
            let report: AbReport = serde_json::from_slice(&bytes)
                .unwrap_or_else(|e| panic!("--sprt-baseline parse {} failed: {e}", path.display()));
            report
                .per_state
                .into_iter()
                .filter_map(|row| {
                    row.results
                        .get(&sims_key)
                        .cloned()
                        .map(|r| (row.state_id, r))
                })
                .collect()
        });
    let mut sprt_w = 0u32;
    let mut sprt_d = 0u32;
    let mut sprt_l = 0u32;
    let mut sprt_verdict_at: Option<(usize, common::SprtVerdict)> = None;

    for (loop_idx, entry) in realized.iter().enumerate() {
        let template = &entry.template;
        let degenerate = is_terminal(template) || legal_actions(template).is_empty();

        let mut results: BTreeMap<String, BudgetResult> = BTreeMap::new();
        if degenerate {
            results.insert(
                sims_key.clone(),
                BudgetResult {
                    wins: 0,
                    losses: 0,
                    ties: 0,
                    visits: 0,
                    pv_depth: 0,
                    pv: Vec::new(),
                    recommended: "<degenerate>".to_string(),
                    wall_ms: 0,
                },
            );
        } else {
            let config = MctsConfig {
                c_puct: std::f32::consts::SQRT_2,
                max_simulations: args.sims,
                max_rollout_depth: args.max_rollout_depth,
                terminal_eval_mode: args.terminal_eval_mode,
                seed: u64::from(entry.state_id)
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    .wrapping_add(u64::from(args.sims)),
                use_puct_prior: want_prior,
                ..MctsConfig::default()
            };
            // Wrap the MCTS pass so a content-gap panic on one
            // realistic-corpus state doesn't kill the run. The
            // workspace-wide panic=unwind setting (post-2vwf) makes
            // catch_unwind effective; per-state recovery mirrors
            // mcts_ab.rs::ch1h.
            let template_clone = template.clone();
            let scorer = leaf_eval.clone();
            let prior_kind_clone = prior_kind.clone();
            let start = Instant::now();
            let search_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match prior_kind_clone {
                    PriorKind::Embed(net) => {
                        let prior = LearnedPolicyPrior::new(net);
                        let mut mcts =
                            Mcts::with_policy(template_clone, config, prior).with_leaf_eval(scorer);
                        let recommended = mcts.search();
                        let outcomes = mcts.root_outcomes();
                        let pv = mcts.get_pv();
                        (recommended, outcomes, pv)
                    }
                    PriorKind::Legacy(net) => {
                        let prior = LearnedPolicyPrior::new(net);
                        let mut mcts =
                            Mcts::with_policy(template_clone, config, prior).with_leaf_eval(scorer);
                        let recommended = mcts.search();
                        let outcomes = mcts.root_outcomes();
                        let pv = mcts.get_pv();
                        (recommended, outcomes, pv)
                    }
                    PriorKind::None => {
                        let mut mcts = Mcts::new(template_clone, config).with_leaf_eval(scorer);
                        let recommended = mcts.search();
                        let outcomes = mcts.root_outcomes();
                        let pv = mcts.get_pv();
                        (recommended, outcomes, pv)
                    }
                }));
            let wall_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            wall_total = wall_total.saturating_add(wall_ms);
            let Ok((recommended, outcomes, pv)) = search_result else {
                eprintln!(
                    "[mcts_ab_value_net] state_id={} sims={} PANICKED (content gap); \
                     emitting <panic> row",
                    entry.state_id, args.sims,
                );
                results.insert(
                    sims_key.clone(),
                    BudgetResult {
                        wins: 0,
                        losses: 0,
                        ties: 0,
                        visits: 0,
                        pv_depth: 0,
                        pv: Vec::new(),
                        recommended: "<panic>".to_string(),
                        wall_ms: 0,
                    },
                );
                per_state.push(PerStateRow {
                    state_id: entry.state_id,
                    encounter: entry.encounter.clone(),
                    tier: entry.tier,
                    state_hash: entry.state_hash,
                    results,
                });
                continue;
            };
            let pv_depth = u32::try_from(pv.len()).unwrap_or(u32::MAX);

            let wins: u32 = outcomes.iter().map(|o| o.wins).sum();
            let losses: u32 = outcomes.iter().map(|o| o.losses).sum();
            let ties: u32 = outcomes.iter().map(|o| o.ties).sum();
            let visits: u32 = outcomes.iter().map(|o| o.visits).sum();

            eprintln!(
                "[mcts_ab_value_net] state_id={} sims={} wall_ms={wall_ms} wins={wins} losses={losses}",
                entry.state_id, args.sims
            );
            results.insert(
                sims_key.clone(),
                BudgetResult {
                    wins,
                    losses,
                    ties,
                    visits,
                    pv_depth,
                    pv: pv.iter().map(action_to_string).collect(),
                    recommended: action_to_string(&recommended),
                    wall_ms: 0,
                },
            );
        }

        per_state.push(PerStateRow {
            state_id: entry.state_id,
            encounter: entry.encounter.clone(),
            tier: entry.tier,
            state_hash: entry.state_hash,
            results: results.clone(),
        });

        // SPRT classification + early-exit (sts2-rs-fgw1x). Only when
        // --sprt-baseline is set AND this state has a corresponding
        // baseline row for the same sims budget. Skip degenerate/panic
        // rows (their results contain a sentinel recommended like
        // "<degenerate>" / "<panic>").
        if let (Some(baseline), Some(branch_r)) = (sprt_baseline.as_ref(), results.get(&sims_key)) {
            if !matches!(branch_r.recommended.as_str(), "<degenerate>" | "<panic>") {
                if let Some(base_r) = baseline.get(&entry.state_id) {
                    match common::sprt_classify(base_r, branch_r) {
                        common::SprtOutcome::BranchWin => sprt_w += 1,
                        common::SprtOutcome::BranchLoss => sprt_l += 1,
                        common::SprtOutcome::Draw => sprt_d += 1,
                    }
                    let verdict = common::sprt_check(sprt_w, sprt_d, sprt_l, &args.sprt_cfg, 10);
                    if !matches!(verdict, common::SprtVerdict::Continue) {
                        sprt_verdict_at = Some((loop_idx + 1, verdict));
                        let llr = common::sprt_llr(sprt_w, sprt_d, sprt_l, &args.sprt_cfg);
                        eprintln!(
                            "[mcts_ab_value_net] SPRT verdict={:?} at state {} of {} \
                             (W/D/L={}/{}/{}, LLR={:.3}); terminating early.",
                            verdict,
                            loop_idx + 1,
                            realized.len(),
                            sprt_w,
                            sprt_d,
                            sprt_l,
                            llr,
                        );
                        break;
                    }
                }
            }
        }
    }

    if let Some(baseline) = sprt_baseline.as_ref() {
        let total = realized.len();
        let processed = per_state.len();
        let llr = common::sprt_llr(sprt_w, sprt_d, sprt_l, &args.sprt_cfg);
        let final_verdict = sprt_verdict_at.map_or(common::SprtVerdict::Continue, |(_, v)| v);
        eprintln!(
            "[mcts_ab_value_net] SPRT summary: baseline_path={:?} baseline_states={} \
             processed={}/{} W/D/L={}/{}/{} LLR={:.3} verdict={:?}",
            args.sprt_baseline.as_ref().map(|p| p.display().to_string()),
            baseline.len(),
            processed,
            total,
            sprt_w,
            sprt_d,
            sprt_l,
            llr,
            final_verdict,
        );
    }

    let report = AbReport {
        version: 1,
        git_sha: git_short_sha(),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        per_state,
    };
    write_json_pretty(&args.output, &report)?;
    eprintln!(
        "[mcts_ab_value_net] DONE — {} states × 1 budget ({} sims, {} mode) → {} (wall_total={}ms)",
        report.per_state.len(),
        args.sims,
        args.leaf_eval.label(),
        args.output.display(),
        wall_total,
    );
    Ok(())
}
