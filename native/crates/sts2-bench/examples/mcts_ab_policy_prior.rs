//! `mcts_ab_policy_prior` — policy-only BC spike acceptance harness.
//!
//! Bead `sts2-rs-vxrz`. Sister of `mcts_ab_value_net.rs` (qctu.6),
//! but the moving piece is the **PUCT prior**, not the leaf scorer.
//! Leaf score is left at the default (`LeafEval::Texel`) so the
//! only thing that differs between the two arms is whether PUCT
//! consults the learned policy head.
//!
//! ## Modes
//!
//! - `uniform` (default): `Mcts::new(...)` style — `UniformPolicy`,
//!   `use_puct_prior = false`. Byte-stable against the pre-qctu.9
//!   MCTS path; this is "Baseline A" from the vxrz acceptance
//!   table.
//! - `learned`: `Mcts::with_policy(..., LearnedPolicyPrior::new(arc))`
//!   with `use_puct_prior = true`. Loads the qctu.4-exported ONNX
//!   graph (must include the `policy` output) and projects its
//!   dense `K=640` softmax onto legal actions per qctu.8's
//!   `project_dense_prior`. This is "Variant C".
//!
//! Baseline B (heuristic `play_ordering` softmax prior) is
//! intentionally **deferred** — wiring it would require a fresh
//! `HeuristicPolicy` Rust impl that the spike doc leaves to a
//! follow-up bead.
//!
//! ## Acceptance protocol
//!
//! ```bash
//! # Baseline A.
//! cargo run --release --example mcts_ab_policy_prior -- \
//!     --policy-prior=uniform --sims=1000 \
//!     --output crates/sts2-bench/data/policy_ab_1000_uniform.json
//!
//! # Variant C (requires vxrz-trained ONNX + the qctu.3 weights JSON).
//! cargo run --release --example mcts_ab_policy_prior -- \
//!     --policy-prior=learned --sims=1000 \
//!     --onnx-path /tmp/vxrz-policy-net.onnx \
//!     --norm-path /tmp/vxrz-policy-net.json \
//!     --output crates/sts2-bench/data/policy_ab_1000_learned.json
//!
//! # Diff.
//! cargo run --release --example mcts_ab_diff -- \
//!     crates/sts2-bench/data/policy_ab_1000_uniform.json \
//!     crates/sts2-bench/data/policy_ab_1000_learned.json
//! ```
//!
//! Acceptance gate (per `docs/perf/2026-05-18-policy-only-bc-spike.md`):
//!   - Δ win-rate at `sims = 1000` ≥ **+2 pp** vs `uniform`.
//!   - p50 hint latency regression `< 10 %` (measured separately
//!     via `mcts_smoke` criterion bench).

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
use sts2_core::{is_terminal, legal_actions, Action, CombatState};
use sts2_mcts::{
    EmbeddingValueNet, LearnedPolicyPrior, LearnedPolicyPriorStats, LearnedValueNet, Mcts,
    MctsConfig, Policy, RootOutcome, UniformPolicy,
};

#[derive(Debug)]
struct Args {
    policy_prior: PolicyPriorMode,
    sims: u32,
    onnx_path: Option<PathBuf>,
    norm_path: Option<PathBuf>,
    /// Path to an `EmbeddingValueNet` weights JSON.  Mutually exclusive
    /// with `--norm-path` / `--onnx-path` (wide net); specifying both is
    /// an error.
    embed_weights: Option<PathBuf>,
    output: PathBuf,
    corpus_kind: CorpusKind,
    corpus_path: Option<PathBuf>,
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
            Self::Realistic => "mcts_corpus_realistic.json",
        }
    }
}

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

fn load_realistic(corpus_path: &std::path::Path) -> Vec<RealizedEntry> {
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
        "[mcts_ab_policy_prior] loaded realistic corpus ({} entries, generator={})",
        corpus.entries.len(),
        corpus.generator,
    );
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    corpus
        .entries
        .into_iter()
        .map(|entry| {
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
            let _ = sts2_core::combat_loop::start_combat(&mut template);
            let _ = sts2_core::combat_loop::start_player_turn(&mut template);
            let state_hash = sts2_core::hashing::state_hash(&template);
            RealizedEntry {
                state_id: entry.state_id,
                encounter: fixture
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("<unknown>")
                    .to_string(),
                tier: parse_realistic_tier(&entry.tier),
                state_hash,
                template,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PolicyPriorMode {
    Uniform,
    Learned,
}

impl PolicyPriorMode {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "uniform" | "Uniform" => Some(Self::Uniform),
            "learned" | "Learned" => Some(Self::Learned),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Uniform => "uniform",
            Self::Learned => "learned",
        }
    }
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
    let mut policy_prior = PolicyPriorMode::Uniform;
    let mut sims = 1000u32;
    let mut onnx_path: Option<PathBuf> = None;
    let mut norm_path: Option<PathBuf> = None;
    let mut embed_weights: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    // Default flipped from Starter → Realistic by sts2-rs-a6wt.
    // See `[[feedback_corpus_starter_deck_too_simple]]` for rationale.
    let mut corpus_kind = CorpusKind::Realistic;
    let mut corpus_path: Option<PathBuf> = None;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let (key, value) = match arg.find('=') {
            Some(i) => (arg[..i].to_string(), Some(arg[i + 1..].to_string())),
            None => (arg.clone(), it.next()),
        };
        match (key.as_str(), value) {
            ("--policy-prior", Some(v)) => {
                policy_prior = PolicyPriorMode::parse(&v).ok_or_else(|| {
                    format!("--policy-prior must be 'uniform' or 'learned', got {v}")
                })?;
            }
            ("--sims", Some(v)) => {
                sims = v.parse().map_err(|e| format!("--sims parse: {e}"))?;
            }
            ("--onnx-path", Some(v)) => onnx_path = Some(PathBuf::from(v)),
            ("--norm-path", Some(v)) => norm_path = Some(PathBuf::from(v)),
            ("--embed-weights", Some(v)) => embed_weights = Some(PathBuf::from(v)),
            ("--output", Some(v)) => output = Some(PathBuf::from(v)),
            ("--corpus", Some(v)) => corpus_kind = parse_corpus_kind(&v)?,
            ("--corpus-path", Some(v)) => corpus_path = Some(PathBuf::from(v)),
            ("--help" | "-h", _) => {
                eprintln!("{USAGE}");
                std::process::exit(0);
            }
            (k, _) => return Err(format!("unknown or malformed flag: {k}")),
        }
    }

    // --embed-weights and --norm-path/--onnx-path are mutually exclusive:
    // both specify a net backend for the same prior slot, so the
    // combination is ambiguous.
    if embed_weights.is_some() && (norm_path.is_some() || onnx_path.is_some()) {
        return Err(
            "--embed-weights and --norm-path/--onnx-path are mutually exclusive: \
             both specify a policy-prior net backend; pass only one"
                .to_string(),
        );
    }

    // s6v2t removed the tract/ONNX forward; --norm-path (the weights
    // JSON) is now the canonical input and --onnx-path is legacy. Accept
    // either or --embed-weights, matching `load_learned_net`'s own check
    // (sts2-rs-1bcx: the stale onnx-only guard blocked the post-s6v2t
    // learned-prior A/B).
    if matches!(policy_prior, PolicyPriorMode::Learned)
        && norm_path.is_none()
        && onnx_path.is_none()
        && embed_weights.is_none()
    {
        return Err(
            "--policy-prior=learned requires --norm-path (weights JSON), \
             --onnx-path (legacy), or --embed-weights (embedding net)"
                .to_string(),
        );
    }

    let output = output.unwrap_or_else(|| {
        data_path(&format!(
            "policy_ab_{sims}_{}_{}.json",
            policy_prior.label(),
            git_short_sha(),
        ))
    });

    Ok(Args {
        policy_prior,
        sims,
        onnx_path,
        norm_path,
        embed_weights,
        output,
        corpus_kind,
        corpus_path,
    })
}

const USAGE: &str = "Usage: mcts_ab_policy_prior [OPTIONS]\n\n\
Options:\n  \
--policy-prior=uniform|learned   Prior used by PUCT selection (default: uniform)\n  \
--sims=N                         Sim budget per state (default: 1000)\n  \
--norm-path=PATH                 Wide-net weights JSON for --policy-prior=learned (canonical since s6v2t)\n  \
--onnx-path=PATH                 Legacy alias for wide net; derives <onnx-path>.json weights\n  \
--embed-weights=PATH             EmbeddingValueNet weights JSON for --policy-prior=learned\n                                   (mutually exclusive with --norm-path / --onnx-path)\n  \
--corpus=starter|realistic       Corpus schema (default: realistic)\n  \
--corpus-path=PATH               Override corpus JSON file location\n  \
--output=PATH                    JSON output (default: data/policy_ab_<sims>_<mode>_<sha>.json)";

fn load_learned_net(args: &Args) -> Result<Arc<LearnedValueNet>, String> {
    // s6v2t: prefer --norm-path (now the weights JSON itself); fall
    // back to deriving <onnx_path>.json for legacy invocations.
    let weights_path = if let Some(p) = &args.norm_path {
        p.clone()
    } else if let Some(o) = &args.onnx_path {
        let mut p = o.clone();
        p.set_extension("json");
        p
    } else {
        return Err(
            "--policy-prior=learned requires --norm-path (weights JSON) or --onnx-path (legacy)"
                .to_string(),
        );
    };
    let net = LearnedValueNet::from_weights_path(&weights_path).map_err(|e| {
        format!(
            "weights JSON load failed at {}: {e}",
            weights_path.display()
        )
    })?;
    Ok(Arc::new(net))
}

fn load_embed_net(path: &std::path::Path) -> Result<Arc<EmbeddingValueNet>, String> {
    let net = EmbeddingValueNet::from_weights_path(path)
        .map_err(|e| format!("embed weights JSON load failed at {}: {e}", path.display()))?;
    Ok(Arc::new(net))
}

/// Run a single MCTS search and return (recommended action, root outcomes, PV).
///
/// Factored out so the three prior arms (uniform / wide-learned / embed-learned)
/// can share the post-search capture without duplicating it.
fn run_search<P: Policy>(
    state: CombatState,
    config: MctsConfig,
    policy: P,
) -> (Action, Vec<RootOutcome>, Vec<Action>) {
    let mut mcts = Mcts::with_policy(state, config, policy);
    let recommended = mcts.search();
    let outcomes = mcts.root_outcomes();
    let pv = mcts.get_pv();
    (recommended, outcomes, pv)
}

fn main() -> std::io::Result<()> {
    sts2_content::cards::dispatch::ensure_installed();

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[mcts_ab_policy_prior] error: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    // Exactly one of these will be Some when policy_prior == Learned.
    // When policy_prior == Uniform, both are None.
    let wide_net: Option<Arc<LearnedValueNet>>;
    let embed_net: Option<Arc<EmbeddingValueNet>>;
    match args.policy_prior {
        PolicyPriorMode::Uniform => {
            wide_net = None;
            embed_net = None;
        }
        PolicyPriorMode::Learned => {
            if let Some(ref path) = args.embed_weights {
                embed_net = match load_embed_net(path) {
                    Ok(arc) => Some(arc),
                    Err(e) => {
                        eprintln!("[mcts_ab_policy_prior] error: {e}");
                        std::process::exit(2);
                    }
                };
                wide_net = None;
            } else {
                wide_net = match load_learned_net(&args) {
                    Ok(arc) => Some(arc),
                    Err(e) => {
                        eprintln!("[mcts_ab_policy_prior] error: {e}");
                        std::process::exit(2);
                    }
                };
                embed_net = None;
            }
        }
    }

    eprintln!(
        "[mcts_ab_policy_prior] mode={} sims={} output={}",
        args.policy_prior.label(),
        args.sims,
        args.output.display(),
    );

    let corpus_path = args
        .corpus_path
        .clone()
        .unwrap_or_else(|| data_path(args.corpus_kind.default_filename()));
    eprintln!(
        "[mcts_ab_policy_prior] corpus={:?} path={}",
        args.corpus_kind,
        corpus_path.display()
    );
    let realized = match args.corpus_kind {
        CorpusKind::Starter => load_starter(&corpus_path),
        CorpusKind::Realistic => {
            std::panic::set_hook(Box::new(|_| {}));
            load_realistic(&corpus_path)
        }
    };

    let mut per_state: Vec<PerStateRow> = Vec::with_capacity(realized.len());
    let sims_key = args.sims.to_string();
    let mut wall_total: u64 = 0;

    for entry in &realized {
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
                // sts2-rs-bhet3: depth=20 (was 200). Policy-prior A/B
                // measures play strength — at depth=200 rollouts
                // terminate naturally before the cap and the leaf
                // evaluator is a no-op (vbzji finding).
                max_rollout_depth: 20,
                seed: u64::from(entry.state_id)
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    .wrapping_add(u64::from(args.sims)),
                use_puct_prior: matches!(args.policy_prior, PolicyPriorMode::Learned),
                ..MctsConfig::default()
            };
            let template_clone = template.clone();
            let wide_clone = wide_net.clone();
            let embed_clone = embed_net.clone();
            let start = Instant::now();
            // Capture LearnedPolicyPrior stats Arc BEFORE moving the prior
            // into Mcts so we can read uniform-fallback / mean-legal-mass /
            // target-collision counters after the search returns (sts2-rs-yxpe1,
            // GH #13).
            let mut prior_stats: Option<Arc<LearnedPolicyPriorStats>> = None;
            let search_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                match (wide_clone, embed_clone) {
                    (None, None) => run_search(template_clone, config, UniformPolicy),
                    (Some(arc), None) => {
                        let prior = LearnedPolicyPrior::new(arc);
                        prior_stats = Some(prior.stats());
                        run_search(template_clone, config, prior)
                    }
                    (None, Some(arc)) => {
                        let prior = LearnedPolicyPrior::<EmbeddingValueNet>::new(arc);
                        prior_stats = Some(prior.stats());
                        run_search(template_clone, config, prior)
                    }
                    (Some(_), Some(_)) => {
                        // Rejected in parse_args; unreachable at runtime.
                        unreachable!("both wide_net and embed_net are Some — parse_args should have rejected this")
                    }
                }
            }));
            let wall_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            wall_total = wall_total.saturating_add(wall_ms);
            let Ok((recommended, outcomes, pv)) = search_result else {
                eprintln!(
                    "[mcts_ab_policy_prior] state_id={} sims={} PANICKED (content gap); \
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
                "[mcts_ab_policy_prior] state_id={} sims={} wall_ms={wall_ms} wins={wins} losses={losses}",
                entry.state_id, args.sims
            );
            // sts2-rs-yxpe1 (GH #13): when running with the learned policy
            // prior, surface the uniform-fallback rate + mean legal mass so
            // we can tell what fraction of evaluations degenerated to
            // uniform-on-legal (i.e., the trained policy contributed zero
            // signal). Stays an eprintln rather than a JSON-schema column
            // so downstream consumers keep working — aggregate the line
            // with grep / awk if you need it post-hoc.
            if let Some(stats) = &prior_stats {
                eprintln!(
                    "[mcts_ab_policy_prior] state_id={} prior_stats: evaluations={} uniform_fallbacks={} (rate={:.3}) mean_legal_mass={:.3} target_collisions={} collision_rate={:.3}",
                    entry.state_id,
                    stats.evaluations(),
                    stats.uniform_fallbacks(),
                    stats.uniform_fallback_rate(),
                    stats.mean_legal_mass(),
                    stats.target_collisions_total(),
                    stats.collision_rate(),
                );
            }
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
            results,
        });
    }

    let report = AbReport {
        version: 1,
        git_sha: git_short_sha(),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        per_state,
    };
    write_json_pretty(&args.output, &report)?;
    eprintln!(
        "[mcts_ab_policy_prior] DONE — {} states × 1 budget ({} sims, {} prior) → {} (wall_total={}ms)",
        report.per_state.len(),
        args.sims,
        args.policy_prior.label(),
        args.output.display(),
        wall_total,
    );
    Ok(())
}
