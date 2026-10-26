//! `eval_rescale` — bead `sts2-rs-zzpb` empirical rescaling probe.
//!
//! Drives ~10 000 Ironclad-vs-act1 mid-combat positions via
//! `build_ironclad_state` + `warmup_random` (the same realization
//! path the MCTS A/B corpus uses), evaluates each with
//! [`sts2_core::evaluate_in_progress_raw`], and reports the empirical
//! distribution of the pre-clamp raw score. The `p99 / 0.45` ratio is
//! the new `IN_PROGRESS_DIVISOR` per plan §4.1.3 + §10 Wave-3.
//!
//! ```sh
//! cargo run --release --example eval_rescale
//! cargo run --release --example eval_rescale -- --realistic
//! ```
//!
//! ## `--realistic` (bead `sts2-rs-le5x`)
//!
//! When invoked with `--realistic`, the sampler swaps the starter-deck
//! `build_ironclad_state` path for the realistic-corpus path: it loads
//! `crates/sts2-bench/data/mcts_corpus_realistic.json` (50 fixtures
//! across common/elite/boss buckets), re-realizes each fixture via
//! `sts2_cli::scenario_to_combat_state`, applies the same `warmup_random`
//! distribution over `turns ∈ 0..=4`, and samples ~200 positions per
//! fixture for a 10K total. The starter and realistic regimes report
//! the same p1/p5/p50/p95/p99 + recommended-divisor numbers so the two
//! can be compared by re-running with and without the flag.

#[path = "common/mod.rs"]
mod common;

use std::path::Path;

use serde::Deserialize;
use sts2_cli::scenario_to_combat_state;
use sts2_cli::sim::scenario::toml_file::load_scenario_file;
use sts2_content::encounters::act1_overgrowth::ACT1_ENCOUNTERS;
use sts2_content::encounters::EncounterTier;
use sts2_core::{evaluate_in_progress_raw, is_terminal};

use common::{build_ironclad_state, warmup_random};

const SAMPLES: u32 = 10_000;
const REALISTIC_CORPUS_PATH: &str = "crates/sts2-bench/data/mcts_corpus_realistic.json";

/// Mirrors the `entries` row of `mcts_corpus_realistic.json` — only the
/// fields this probe needs are captured; the writer's bucket-glob /
/// version metadata is ignored at read time.
#[derive(Debug, Deserialize)]
struct RealisticCorpusFile {
    entries: Vec<RealisticEntry>,
}

#[derive(Debug, Deserialize)]
struct RealisticEntry {
    fixture_path: String,
    seed: u64,
    /// Floor lifted from the source TOML filename at corpus-gen
    /// time (see `mcts_corpus_realistic.rs::RealisticCorpusEntry`).
    /// Used by `--realistic-floor-min` (bead `sts2-rs-tidt`) to
    /// re-stratify the loaded corpus toward the
    /// `sts2-rs-t4tn`-balanced floor-40+ regime without
    /// regenerating the corpus file.
    #[serde(default)]
    floor: u16,
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return f32::NAN;
    }
    #[allow(clippy::cast_precision_loss)]
    let idx_f = (p / 100.0) * (sorted.len() - 1) as f32;
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let idx = idx_f.round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Optional realistic-corpus floor filter for the
/// `--realistic-floor-min N` flag (bead `sts2-rs-tidt`). `0`
/// disables (default — preserves prior behavior).
fn parse_realistic_floor_min() -> u16 {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--realistic-floor-min" {
            return args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        } else if let Some(eq) = a.strip_prefix("--realistic-floor-min=") {
            return eq.parse().unwrap_or(0);
        }
    }
    0
}

/// Starter-deck sampler — original `sts2-rs-zzpb` path. Drives one
/// `build_ironclad_state` call per sample with a varying warmup depth.
fn collect_samples_starter() -> (Vec<f32>, u32, u32) {
    let encounters: Vec<&str> = ACT1_ENCOUNTERS
        .iter()
        .filter(|e| {
            matches!(
                e.tier,
                EncounterTier::Weak | EncounterTier::Normal | EncounterTier::Elite
            )
        })
        .map(|e| e.id)
        .collect();
    assert!(!encounters.is_empty(), "no act1 encounters in registry");

    let mut samples: Vec<f32> = Vec::with_capacity(SAMPLES as usize);
    let mut sampled_terminal: u32 = 0;
    let mut sampled_ok: u32 = 0;

    for i in 0..SAMPLES {
        let enc_id = encounters[(i as usize) % encounters.len()];
        let engine_seed = u64::from(i).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let warmup_seed = 0xC0FFEE_u64.wrapping_add(u64::from(i));
        let warmup_turns = i % 5;
        let mut state = build_ironclad_state(enc_id, engine_seed);
        warmup_random(&mut state, warmup_seed, warmup_turns);
        if is_terminal(&state) {
            sampled_terminal += 1;
            continue;
        }
        samples.push(evaluate_in_progress_raw(&state));
        sampled_ok += 1;
    }

    (samples, sampled_ok, sampled_terminal)
}

/// Realistic-corpus sampler — bead `sts2-rs-le5x`. Walks the 50
/// `mcts_corpus_realistic.json` entries and draws ~200 positions per
/// fixture (entry.seed + i for the realize step, plus a varied warmup
/// depth) for ~10K total samples. Fixtures that fail to realize
/// (unresolved relic / unmapped card) are skipped — the post-`mr7g`
/// corpus is content-curated for the Ironclad blocklist so this is
/// expected to be near-zero in practice.
fn collect_samples_realistic(floor_min: u16) -> (Vec<f32>, u32, u32, u32, u32) {
    let path = Path::new(REALISTIC_CORPUS_PATH);
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut corpus: RealisticCorpusFile =
        serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    assert!(
        !corpus.entries.is_empty(),
        "{} has zero entries",
        path.display()
    );
    if floor_min > 0 {
        let before = corpus.entries.len();
        corpus.entries.retain(|e| e.floor >= floor_min);
        eprintln!(
            "[eval_rescale] tidt: --realistic-floor-min={floor_min} kept {}/{before} fixtures",
            corpus.entries.len(),
        );
        assert!(
            !corpus.entries.is_empty(),
            "no realistic-corpus entries survive --realistic-floor-min={floor_min}; \
             either lower the threshold or regenerate the corpus with a wider floor pool",
        );
    }

    let fixtures = corpus.entries.len();
    let per_fixture = (SAMPLES as usize).div_ceil(fixtures);

    let mut samples: Vec<f32> = Vec::with_capacity(SAMPLES as usize);
    let mut sampled_terminal: u32 = 0;
    let mut sampled_ok: u32 = 0;
    let mut sampled_unresolvable: u32 = 0;
    let mut fixtures_used: u32 = 0;

    'fixtures: for entry in &corpus.entries {
        let Ok(fixture) = load_scenario_file(Path::new(&entry.fixture_path)) else {
            sampled_unresolvable += 1;
            continue;
        };
        let mut fixture_emitted = false;
        for i in 0..per_fixture {
            if samples.len() >= SAMPLES as usize {
                break 'fixtures;
            }
            let engine_seed = entry.seed.wrapping_add(i as u64);
            let warmup_seed = entry.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ u64::from(u32::try_from(i).unwrap_or(0));
            #[allow(clippy::cast_possible_truncation)]
            let warmup_turns = (i as u32) % 5;
            let Ok(mut state) = scenario_to_combat_state(&fixture, engine_seed) else {
                sampled_unresolvable += 1;
                continue;
            };
            warmup_random(&mut state, warmup_seed, warmup_turns);
            if is_terminal(&state) {
                sampled_terminal += 1;
                continue;
            }
            samples.push(evaluate_in_progress_raw(&state));
            sampled_ok += 1;
            fixture_emitted = true;
        }
        if fixture_emitted {
            fixtures_used += 1;
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    let fixtures_total = fixtures as u32;
    (
        samples,
        sampled_ok,
        sampled_terminal,
        sampled_unresolvable,
        fixtures_total.min(fixtures_used + (fixtures_total - fixtures_used)),
    )
}

fn report(label: &str, samples: &mut Vec<f32>, sampled_ok: u32, sampled_terminal: u32) {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p1 = percentile(samples, 1.0);
    let p5 = percentile(samples, 5.0);
    let p50 = percentile(samples, 50.0);
    let p95 = percentile(samples, 95.0);
    let p99 = percentile(samples, 99.0);
    let min = *samples.first().unwrap_or(&f32::NAN);
    let max = *samples.last().unwrap_or(&f32::NAN);
    let recommended_divisor = p99.abs().max(p1.abs()) / 0.45;

    println!("# eval_rescale — {label}");
    println!("# samples requested: {SAMPLES}");
    println!("# samples ok: {sampled_ok}");
    println!("# samples skipped (terminal): {sampled_terminal}");
    println!();
    println!("min     = {min:>10.4}");
    println!("p1      = {p1:>10.4}");
    println!("p5      = {p5:>10.4}");
    println!("p50     = {p50:>10.4}");
    println!("p95     = {p95:>10.4}");
    println!("p99     = {p99:>10.4}");
    println!("max     = {max:>10.4}");
    println!();
    println!("recommended IN_PROGRESS_DIVISOR (max(|p1|,|p99|)/0.45) = {recommended_divisor:.4}");
}

fn main() {
    sts2_content::cards::dispatch::ensure_installed();
    let realistic = std::env::args().any(|a| a == "--realistic");
    // Bead `sts2-rs-tidt`: `--joint` runs both the starter sampler
    // and the realistic sampler (under the same
    // `--realistic-floor-min`) and reports per-regime + joint
    // percentiles + a final recommended divisor pooled across both.
    let joint = std::env::args().any(|a| a == "--joint");
    let floor_min = parse_realistic_floor_min();

    if joint {
        let (mut starter_samples, ss_ok, ss_term) = collect_samples_starter();
        let (mut realistic_samples, rs_ok, rs_term, rs_unres, fixtures) =
            collect_samples_realistic(floor_min);
        let label_r = if floor_min > 0 {
            format!("realistic floor-min={floor_min} (sts2-rs-tidt)")
        } else {
            "realistic corpus (sts2-rs-le5x)".to_string()
        };
        report(
            "starter (sts2-rs-zzpb)",
            &mut starter_samples,
            ss_ok,
            ss_term,
        );
        println!();
        report(&label_r, &mut realistic_samples, rs_ok, rs_term);
        println!();
        println!("# realistic-extra:");
        println!("# fixtures considered: {fixtures}");
        println!("# samples unresolvable (skipped fixtures): {rs_unres}");

        // Joint pool: equal-weight concatenation of both lists.
        // Each list already targets ~SAMPLES=10K, so the joint is
        // ~20K, with equal contribution from each regime — the
        // weighting choice matches "the value-net training step
        // sees both regimes equally" intent of tidt's acceptance.
        let mut joint_samples: Vec<f32> =
            Vec::with_capacity(starter_samples.len() + realistic_samples.len());
        joint_samples.extend_from_slice(&starter_samples);
        joint_samples.extend_from_slice(&realistic_samples);
        let joint_ok = ss_ok + rs_ok;
        let joint_term = ss_term + rs_term;
        println!();
        report(
            "JOINT — starter + realistic (sts2-rs-tidt)",
            &mut joint_samples,
            joint_ok,
            joint_term,
        );
        return;
    }

    if realistic {
        let (mut samples, sampled_ok, sampled_terminal, sampled_unresolvable, fixtures) =
            collect_samples_realistic(floor_min);
        let label = if floor_min > 0 {
            format!("realistic floor-min={floor_min} (sts2-rs-tidt)")
        } else {
            "realistic corpus (sts2-rs-le5x)".to_string()
        };
        report(&label, &mut samples, sampled_ok, sampled_terminal);
        println!();
        println!("# realistic-extra:");
        println!("# fixtures considered: {fixtures}");
        println!("# samples unresolvable (skipped fixtures): {sampled_unresolvable}");
    } else {
        let (mut samples, sampled_ok, sampled_terminal) = collect_samples_starter();
        report(
            "starter baseline (sts2-rs-zzpb)",
            &mut samples,
            sampled_ok,
            sampled_terminal,
        );
    }
}
