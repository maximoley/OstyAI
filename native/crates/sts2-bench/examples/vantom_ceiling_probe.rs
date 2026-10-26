//! `vantom_ceiling_probe` — diagnostic harness for bead `sts2-rs-uq9y`
//! (zlhm.2: boss_vantom strong-search-ceiling disambiguation), extended
//! by `sts2-rs-5a53` to cover all three act-1 bosses for typology.
//!
//! Per `docs/perf/2026-05-17-rsp5-budget-sweep-rerun.md`, `boss_vantom`
//! plateaus at 43–45% winrate across `sims ∈ {100, 1K, 3K, 10K, 30K}`
//! and never breaks 50%. `boss_kin` sits at 0% (g0ai moves it to
//! ceilings preset — deck-ceiling hypothesis). `boss_ceremonial` sits
//! at 53-68%, between vantom and the elites. Running the same probe
//! across all three produces a per-boss typology table.
//!
//! Two candidate explanations per boss:
//!
//! 1. **Policy / eval gap** — the leaf evaluator + UCT prior push the
//!    search toward the *wrong action class* at the root. More sims
//!    don't fix this because the additional rollouts settle deeper
//!    into the same wrong subtree.
//! 2. **PIMC determinization variance** — single-determinization MCTS
//!    re-rolls the draw pile per simulation against the cloned root
//!    state. At high enemy-randomness scenarios (Vantom's Phantom
//!    summons + STATUS_CARD intent) the variance across rollouts
//!    swamps the Q signal regardless of budget.
//!
//! This probe pins the determinization seed (`MctsConfig::seed`)
//! constant across both budgets so any Q-divergence is attributable
//! purely to *more search compute*, not draw-pile permutation noise.
//!
//! For each of N fixtures (N=20 Vantom-boss states with varied warmup
//! seeds) we run MCTS at `sims ∈ {10_000, 30_000}` with the same
//! `MctsConfig.seed` and record:
//!
//! - The top-3 actions by mean Q (with visits + mean_value).
//! - Whether the recommended action changes between the two budgets.
//! - The Q-gap between the top action and the second-best.
//!
//! If 10K and 30K agree on the recommended action AND their top-3 sets
//! overlap heavily, the ceiling lives outside the search — the eval is
//! pushing both budgets toward the same suboptimal action. If they
//! disagree, more sims are still moving the policy; the ceiling is at
//! least partly a search-budget problem.
//!
//! Output JSON: `data/vantom_ceiling_probe_<git-short-sha>.json`.
//! Markdown summary: `docs/perf/2026-05-17-boss-vantom-ceiling.md`
//! (hand-authored — this harness produces the raw numbers).
//!
//! ```sh
//! cargo run --release --example vantom_ceiling_probe
//! ```

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Parser, ValueEnum};
use common::{
    action_to_string, build_ironclad_state_with_deck, data_path, git_short_sha, warmup_random,
    write_json_pretty,
};
use serde::{Deserialize, Serialize};
use sts2_content::cards::ironclad_basic::ironclad_starter_deck;
use sts2_content::cards::spec_index::IRONCLAD_SPECS;
use sts2_content::cards::CardSpec;
use sts2_content::names::{resolve_card_name, PlayerClassHint};
use sts2_core::{is_terminal, legal_actions};
use sts2_mcts::{Mcts, MctsConfig, RootOutcome};

const ENCOUNTER_IDS: &[&str] = &["THE_KIN_BOSS", "CEREMONIAL_BEAST_BOSS", "VANTOM_BOSS"];
const N_FIXTURES: u32 = 20;
const WARMUP_TURNS: u32 = 1;
const FIXED_DETERMINIZATION_SEED: u64 = 42;
const BUDGETS: &[u32] = &[10_000, 30_000];

/// Deck pool source. Mirrors `corpus_gen.rs::DeckSource` (bead 2vwf)
/// so the DUAL probe extension (bead `sts2-rs-yg5kz`, aze8.5) can
/// re-run against realistic decks instead of only the 10-card starter.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum DeckSource {
    /// `ironclad_starter_deck()` — the 10-card pre-curse pile. Default
    /// for back-compat: matches the pre-yg5kz output byte-for-byte
    /// (modulo the `_starter` filename suffix).
    Starter,
    /// Pull human-played decks from `--sts2log-fixtures-dir`. Filter
    /// to Ironclad fixtures whose `[player].deck` resolves through
    /// `resolve_card_name`. Pool is sorted by filename; per-fixture
    /// deck is `pool[fixture_id % pool.len()]`.
    Sts2logPerfect,
    /// Sample `--random-deck-size` cards uniformly from
    /// `IRONCLAD_SPECS` (86 entries at pin 01c094d). Per-fixture deck
    /// is keyed on `(fixture_id, FIXED_DETERMINIZATION_SEED)` so the
    /// pool is reproducible across hosts.
    RandomFullPool,
}

#[derive(Parser, Debug)]
#[command(about = "Vantom-style boss strong-search-ceiling probe (sts2-rs-uq9y / 5a53 / yg5kz)")]
struct Args {
    /// Deck pool to use for the player. `starter` reproduces the
    /// pre-yg5kz behavior exactly.
    #[arg(long, value_enum, default_value_t = DeckSource::Starter)]
    deck_source: DeckSource,
    /// Directory of sts2log `.toml` fixtures (only used when
    /// `--deck-source sts2log-perfect`). Defaults to the `eh83`
    /// scrape path used by `corpus_gen`.
    #[arg(long, default_value = "data/sts2log/perfect-runs")]
    sts2log_fixtures_dir: PathBuf,
    /// Deck size when `--deck-source random-full-pool`. 15 matches
    /// the `corpus_gen` default.
    #[arg(long, default_value_t = 15)]
    random_deck_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PerActionRow {
    action: String,
    visits: u32,
    mean_value: f32,
    wins: u32,
    losses: u32,
    ties: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PerBudgetSnapshot {
    sims: u32,
    recommended: String,
    wall_ms: u64,
    top_actions: Vec<PerActionRow>,
    q_gap_top1_top2: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FixtureRow {
    fixture_id: u32,
    engine_seed: u64,
    warmup_seed: u64,
    warmup_turns: u32,
    legal_action_count: usize,
    snapshots: BTreeMap<String, PerBudgetSnapshot>,
    recommended_agrees_across_budgets: bool,
    top3_jaccard_overlap: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BossReport {
    encounter_id: String,
    fixtures: Vec<FixtureRow>,
    aggregate: AggregateStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProbeReport {
    version: u32,
    git_sha: String,
    encounter_ids: Vec<String>,
    fixed_determinization_seed: u64,
    budgets: Vec<u32>,
    warmup_turns: u32,
    n_fixtures: u32,
    bosses: Vec<BossReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AggregateStats {
    /// Fraction of fixtures where 10K and 30K return the same action.
    recommended_agreement_rate: f32,
    /// Mean Jaccard overlap of the top-3 action sets across budgets.
    mean_top3_jaccard: f32,
    /// Mean Q-gap (top1 - top2) at each budget — saturated gaps suggest
    /// policy lock-in; near-zero gaps suggest the search hasn't
    /// separated lines yet.
    mean_q_gap_per_budget: BTreeMap<String, f32>,
}

fn capture_snapshot(mcts: &mut Mcts, sims: u32, start: Instant) -> PerBudgetSnapshot {
    let recommended = mcts.search_n(sims);
    let wall_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let outcomes: Vec<RootOutcome> = mcts.root_outcomes();
    let mut sorted = outcomes;
    sorted.sort_by(|a, b| {
        b.mean_value
            .partial_cmp(&a.mean_value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let top: Vec<PerActionRow> = sorted
        .iter()
        .take(3)
        .map(|o| PerActionRow {
            action: action_to_string(&o.action),
            visits: o.visits,
            mean_value: o.mean_value,
            wins: o.wins,
            losses: o.losses,
            ties: o.ties,
        })
        .collect();
    let q_gap = if sorted.len() >= 2 {
        sorted[0].mean_value - sorted[1].mean_value
    } else {
        0.0
    };
    PerBudgetSnapshot {
        sims,
        recommended: action_to_string(&recommended),
        wall_ms,
        top_actions: top,
        q_gap_top1_top2: q_gap,
    }
}

fn jaccard(a: &[PerActionRow], b: &[PerActionRow]) -> f32 {
    use std::collections::BTreeSet;
    let sa: BTreeSet<&str> = a.iter().map(|r| r.action.as_str()).collect();
    let sb: BTreeSet<&str> = b.iter().map(|r| r.action.as_str()).collect();
    let intersection = sa.intersection(&sb).count() as f32;
    let union = sa.union(&sb).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

/// Resolve the deck pool once per process. Empty `Vec` for
/// `RandomFullPool` (each fixture realizes its own deck from
/// `(fixture_id, FIXED_DETERMINIZATION_SEED)`). For `Starter` the pool
/// is `[ironclad_starter_deck()]`; for `Sts2logPerfect` we eagerly load
/// every Ironclad fixture under `--sts2log-fixtures-dir`. Mirrors
/// `corpus_gen.rs::load_sts2log_perfect_pool`.
fn load_deck_pool(args: &Args) -> Vec<Vec<CardSpec>> {
    match args.deck_source {
        DeckSource::Starter => vec![ironclad_starter_deck()],
        DeckSource::RandomFullPool => Vec::new(),
        DeckSource::Sts2logPerfect => match load_sts2log_perfect_pool(&args.sts2log_fixtures_dir) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[vantom_ceiling_probe] {e}");
                std::process::exit(2);
            }
        },
    }
}

#[derive(Deserialize)]
struct FixturePlayer {
    class: String,
    deck: Vec<String>,
}

#[derive(Deserialize)]
struct FixtureRoot {
    player: FixturePlayer,
}

fn load_sts2log_perfect_pool(dir: &Path) -> Result<Vec<Vec<CardSpec>>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("toml"))
        .collect();
    paths.sort();

    let mut pool: Vec<Vec<CardSpec>> = Vec::new();
    let mut skipped_class = 0usize;
    let mut skipped_card = 0usize;
    let mut skipped_parse = 0usize;
    for p in &paths {
        let Ok(text) = std::fs::read_to_string(p) else {
            skipped_parse += 1;
            continue;
        };
        let Ok(parsed): Result<FixtureRoot, _> = toml::from_str(&text) else {
            skipped_parse += 1;
            continue;
        };
        if !parsed.player.class.eq_ignore_ascii_case("Ironclad") {
            skipped_class += 1;
            continue;
        }
        let mut deck = Vec::with_capacity(parsed.player.deck.len());
        let mut any_unknown = false;
        for name in &parsed.player.deck {
            if let Ok(spec) = resolve_card_name(name, Some(PlayerClassHint::Ironclad)) {
                deck.push(spec);
            } else {
                any_unknown = true;
                break;
            }
        }
        if any_unknown {
            skipped_card += 1;
            continue;
        }
        if !deck.is_empty() {
            pool.push(deck);
        }
    }
    eprintln!(
        "[vantom_ceiling_probe] sts2log-perfect: loaded {} decks from {} (skipped {} non-Ironclad, \
         {} unknown-card, {} parse-error; pool order = sorted filename)",
        pool.len(),
        dir.display(),
        skipped_class,
        skipped_card,
        skipped_parse,
    );
    if pool.is_empty() {
        return Err(format!(
            "no usable sts2log-perfect Ironclad decks under {}",
            dir.display()
        ));
    }
    Ok(pool)
}

fn random_full_pool_deck(seed: u64, size: usize) -> Vec<CardSpec> {
    let mut rng = sts2_core::dotnet_rng::Rng::new(seed ^ 0xDEC0_DEC0_DEC0_DEC0);
    (0..size)
        .map(|_| {
            let idx = rng.next_int_exclusive(0, IRONCLAD_SPECS.len() as i32) as usize;
            IRONCLAD_SPECS[idx]()
        })
        .collect()
}

fn deck_for_fixture(args: &Args, pool: &[Vec<CardSpec>], fixture_id: u32) -> Vec<CardSpec> {
    match args.deck_source {
        DeckSource::RandomFullPool => {
            // Key off (fixture_id, FIXED_DETERMINIZATION_SEED) so the
            // realized deck is host-independent.
            let seed = u64::from(fixture_id).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ FIXED_DETERMINIZATION_SEED;
            random_full_pool_deck(seed, args.random_deck_size)
        }
        _ => pool[(fixture_id as usize) % pool.len()].clone(),
    }
}

fn deck_source_tag(s: DeckSource) -> &'static str {
    match s {
        DeckSource::Starter => "starter",
        DeckSource::Sts2logPerfect => "sts2log-perfect",
        DeckSource::RandomFullPool => "random-full-pool",
    }
}

fn run_boss(encounter_id: &str, args: &Args, pool: &[Vec<CardSpec>]) -> BossReport {
    let mut fixtures: Vec<FixtureRow> = Vec::with_capacity(N_FIXTURES as usize);

    for fixture_id in 0..N_FIXTURES {
        let engine_seed = u64::from(fixture_id) ^ 0xA5A5_A5A5_A5A5_A5A5;
        let warmup_seed = u64::from(fixture_id) ^ 0xDEAD_BEEF_FEED_FACE;
        let deck = deck_for_fixture(args, pool, fixture_id);
        let mut state = build_ironclad_state_with_deck(encounter_id, engine_seed, &deck);
        warmup_random(&mut state, warmup_seed, WARMUP_TURNS);
        let legal = legal_actions(&state);
        if is_terminal(&state) || legal.is_empty() {
            eprintln!(
                "[vantom_ceiling_probe] {encounter_id} fixture {fixture_id} degenerate (terminal or no legal actions); skipping"
            );
            continue;
        }
        let mut snapshots: BTreeMap<String, PerBudgetSnapshot> = BTreeMap::new();
        for &sims in BUDGETS {
            // Depth=10 matches the memory entry `project_mcts_budget_knee`
            // recommendation and the rsp5 sweep's operating point. The
            // MctsConfig default is 200, which lets rollouts reach
            // terminal and saturate Q at -1.0 on Vantom (a fast-kill
            // boss), masking the rollout-vs-real-play gap. Pinning at 10
            // forces leaf evaluation through `evaluate_in_progress`
            // instead of the rollout terminal value — same surface that
            // rsp5 measured.
            let config = MctsConfig {
                max_simulations: sims,
                max_rollout_depth: 10,
                seed: FIXED_DETERMINIZATION_SEED,
                ..MctsConfig::default()
            };
            let mut mcts = Mcts::new(state.clone(), config);
            let start = Instant::now();
            let snap = capture_snapshot(&mut mcts, sims, start);
            eprintln!(
                "[vantom_ceiling_probe] {encounter_id} fixture {fixture_id} sims={sims} rec={} q_gap={:.4} wall_ms={}",
                snap.recommended, snap.q_gap_top1_top2, snap.wall_ms
            );
            snapshots.insert(sims.to_string(), snap);
        }
        let rec_10k = snapshots[&BUDGETS[0].to_string()].recommended.clone();
        let rec_30k = snapshots[&BUDGETS[1].to_string()].recommended.clone();
        let top3_jac = jaccard(
            &snapshots[&BUDGETS[0].to_string()].top_actions,
            &snapshots[&BUDGETS[1].to_string()].top_actions,
        );
        fixtures.push(FixtureRow {
            fixture_id,
            engine_seed,
            warmup_seed,
            warmup_turns: WARMUP_TURNS,
            legal_action_count: legal.len(),
            snapshots,
            recommended_agrees_across_budgets: rec_10k == rec_30k,
            top3_jaccard_overlap: top3_jac,
        });
    }

    let n = fixtures.len() as f32;
    let agreement = if n > 0.0 {
        fixtures
            .iter()
            .filter(|f| f.recommended_agrees_across_budgets)
            .count() as f32
            / n
    } else {
        0.0
    };
    let mean_jac = if n > 0.0 {
        fixtures.iter().map(|f| f.top3_jaccard_overlap).sum::<f32>() / n
    } else {
        0.0
    };
    let mut mean_q_gap: BTreeMap<String, f32> = BTreeMap::new();
    for &sims in BUDGETS {
        let k = sims.to_string();
        let mean = if n > 0.0 {
            fixtures
                .iter()
                .map(|f| f.snapshots[&k].q_gap_top1_top2)
                .sum::<f32>()
                / n
        } else {
            0.0
        };
        mean_q_gap.insert(k, mean);
    }

    let aggregate = AggregateStats {
        recommended_agreement_rate: agreement,
        mean_top3_jaccard: mean_jac,
        mean_q_gap_per_budget: mean_q_gap,
    };

    BossReport {
        encounter_id: encounter_id.to_string(),
        fixtures,
        aggregate,
    }
}

/// Classify a boss into the four-bucket typology based on its
/// agreement/Jaccard/Q-gap fingerprint. Heuristic — drives the
/// markdown typology row; raw numbers are still the load-bearing
/// artifact for review.
fn classify(b: &BossReport) -> &'static str {
    let agg = &b.aggregate;
    let n_completed = b.fixtures.len();
    if n_completed == 0 {
        return "degenerate (no completed fixtures — deck-ceiling)";
    }
    let agreement = agg.recommended_agreement_rate;
    let jac = agg.mean_top3_jaccard;
    let q10 = agg
        .mean_q_gap_per_budget
        .get(&BUDGETS[0].to_string())
        .copied()
        .unwrap_or(0.0);
    let q30 = agg
        .mean_q_gap_per_budget
        .get(&BUDGETS[1].to_string())
        .copied()
        .unwrap_or(0.0);
    // Q-gap collapses to ~0 across BOTH budgets → search has no
    // gradient signal: leaf evaluator returns ~the same value for
    // every root action. Either every line wins (saturated +1) or
    // every line loses (saturated −1). Deck doesn't carry enough
    // firepower for the encounter at this loadout. This check has
    // to come BEFORE the policy-eval check because a Q-flat search
    // can also exhibit 100% agreement-by-tiebreak.
    if q10.abs() < 0.005 && q30.abs() < 0.005 {
        return "deck-ceiling";
    }
    // Real (non-flat) Q-gradient AND both budgets converge to the
    // same action class → policy/eval ceiling: search is decisive
    // about the *wrong* line. Vantom's classic fingerprint.
    if agreement >= 0.9 && jac >= 0.66 {
        return "policy-eval ceiling";
    }
    // High disagreement but tight Q-gaps → variance ceiling (search
    // is unstable; more sims rearrange the top picks without
    // resolving them).
    if agreement < 0.6 {
        return "variance ceiling";
    }
    "mixed"
}

fn main() -> std::io::Result<()> {
    sts2_content::cards::dispatch::ensure_installed();
    let args = Args::parse();
    let pool = load_deck_pool(&args);
    let tag = deck_source_tag(args.deck_source);
    eprintln!(
        "[vantom_ceiling_probe] deck-source={tag} (pool size: {})",
        if pool.is_empty() {
            "random-per-fixture".to_string()
        } else {
            pool.len().to_string()
        }
    );

    let mut bosses: Vec<BossReport> = Vec::with_capacity(ENCOUNTER_IDS.len());
    for &enc in ENCOUNTER_IDS {
        eprintln!("\n=== {enc} ===");
        bosses.push(run_boss(enc, &args, &pool));
    }

    let sha = git_short_sha();
    let report = ProbeReport {
        version: 2,
        git_sha: sha.clone(),
        encounter_ids: ENCOUNTER_IDS.iter().map(|s| (*s).to_string()).collect(),
        fixed_determinization_seed: FIXED_DETERMINIZATION_SEED,
        budgets: BUDGETS.to_vec(),
        warmup_turns: WARMUP_TURNS,
        n_fixtures: N_FIXTURES,
        bosses: bosses.clone(),
    };

    // Bead `sts2-rs-yg5kz`: output filename now carries the deck-source
    // tag so starter / sts2log-perfect / random-full-pool runs at the
    // same git SHA don't clobber each other.
    let path = data_path(&format!("vantom_ceiling_probe_{sha}_{tag}.json"));
    write_json_pretty(&path, &report)?;

    eprintln!("\n=== TYPOLOGY (sts2-rs-5a53 / 3-boss extension) ===");
    eprintln!(
        "{:<22} {:>8} {:>12} {:>14} {:>14}  class",
        "encounter", "agree%", "top3_jaccard", "mean_q_gap_10k", "mean_q_gap_30k",
    );
    for b in &bosses {
        let q10 = b
            .aggregate
            .mean_q_gap_per_budget
            .get(&BUDGETS[0].to_string())
            .copied()
            .unwrap_or(0.0);
        let q30 = b
            .aggregate
            .mean_q_gap_per_budget
            .get(&BUDGETS[1].to_string())
            .copied()
            .unwrap_or(0.0);
        eprintln!(
            "{:<22} {:>7.1}% {:>12.2} {:>14.4} {:>14.4}  {}",
            b.encounter_id,
            b.aggregate.recommended_agreement_rate * 100.0,
            b.aggregate.mean_top3_jaccard,
            q10,
            q30,
            classify(b),
        );
    }
    eprintln!(
        "\nwrote {} bosses to {}",
        report.bosses.len(),
        path.display()
    );
    Ok(())
}
