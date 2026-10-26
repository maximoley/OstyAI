//! `mcts_red_flags` — benchmark-tier wrapper around the replay audit.
//!
//! Drives the `FightRecord` recorder (same path as
//! `sts2-fight-replay`) over a fixture set and emits a byte-stable
//! JSON report at `crates/sts2-bench/data/mcts_red_flags_<sha>.json`
//! plus a human-readable summary on stderr. Slots into the same
//! `data/<harness>_<git-short-sha>.json` workflow as `mcts_ab` /
//! `mcts_oracle` so two runs at different SHAs can be diffed
//! mechanically.
//!
//! See `docs/explainers/replay-audit.md` for what each red-flag kind
//! means and `crates/sts2-cli/src/replay/audit.rs` for the detector
//! source.
//!
//! Default fixture set is the first 50 Ironclad `.run-source` TOMLs
//! under `/tmp/run-fixtures-full/the-ironclad/`. Override with
//! `--glob <pattern>` or pass explicit fixture paths.
//!
//! Usage:
//! ```sh
//! cargo run --release --example mcts_red_flags
//! cargo run --release --example mcts_red_flags -- --sims 5000 --rollout-depth 20 --limit 100
//! cargo run --release --example mcts_red_flags -- --glob '/tmp/run-fixtures-full/the-ironclad/*floor00[1-3]*.toml'
//! ```

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use common::{data_path, git_short_sha, write_json_pretty};
use serde::{Deserialize, Serialize};
use sts2_cli::replay::{audit_fight, record_from_fixture, RecorderConfig, RedFlagKind};
use sts2_content::cards::dispatch::ensure_installed as ensure_card_dispatch;
use sts2_content::potions::dispatch::ensure_installed as ensure_potion_dispatch;
use sts2_content::relics::dispatch::ensure_installed as ensure_relic_dispatch;

const DEFAULT_GLOB: &str = "/tmp/run-fixtures-full/the-ironclad/*.toml";
const DEFAULT_LIMIT: usize = 50;
const DEFAULT_SIMS: u32 = 1000;
const DEFAULT_ROLLOUT_DEPTH: u32 = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RedFlagReport {
    git_sha: String,
    generator: String,
    fixture_glob: String,
    fixtures_requested: usize,
    sims: u32,
    rollout_depth: u32,
    fights_audited: u32,
    fights_with_any_flag: u32,
    fights_skipped: u32,
    wall_ms: u64,
    /// Per-kind counts keyed by `RedFlagKind::as_str()`. `BTreeMap`
    /// keeps the JSON byte-stable across runs.
    counts: BTreeMap<String, u32>,
    /// Lightweight per-fixture rollup. Sorted by fixture stem for
    /// determinism.
    per_fixture: Vec<PerFixtureRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PerFixtureRow {
    fixture: String,
    won: bool,
    decisions: u32,
    total_flags: u32,
    counts: BTreeMap<String, u32>,
}

struct Args {
    glob: String,
    limit: usize,
    explicit_fixtures: Vec<PathBuf>,
    cfg: RecorderConfig,
    quiet: bool,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut glob = DEFAULT_GLOB.to_string();
        let mut limit = DEFAULT_LIMIT;
        let mut explicit_fixtures: Vec<PathBuf> = Vec::new();
        let mut cfg = RecorderConfig {
            sims: DEFAULT_SIMS,
            rollout_depth: DEFAULT_ROLLOUT_DEPTH,
            ..RecorderConfig::default()
        };
        let mut quiet = false;
        let mut argv: Vec<String> = std::env::args().skip(1).collect();
        argv.reverse();
        while let Some(arg) = argv.pop() {
            match arg.as_str() {
                "--glob" => glob = argv.pop().ok_or("--glob expects a value")?,
                "--limit" => limit = parse_pop(&mut argv, "--limit")?,
                "--sims" => cfg.sims = parse_pop(&mut argv, "--sims")?,
                "--rollout-depth" => cfg.rollout_depth = parse_pop(&mut argv, "--rollout-depth")?,
                "--c-puct" => cfg.c_puct = parse_pop(&mut argv, "--c-puct")?,
                "--seed" => cfg.seed = parse_pop(&mut argv, "--seed")?,
                "--tree-reuse" => cfg.tree_reuse = true,
                "--quiet" | "-q" => quiet = true,
                "--help" | "-h" => {
                    eprintln!(
                        "mcts_red_flags — drive recorder over fixtures, write data/mcts_red_flags_<sha>.json\n\n\
USAGE: cargo run --release --example mcts_red_flags -- [OPTIONS] [FIXTURE]...\n\n\
OPTIONS:\n  --glob PATTERN     glob for fixtures (default {DEFAULT_GLOB})\n\
  --limit N          cap fixtures (default {DEFAULT_LIMIT}, 0 = no cap)\n\
  --sims N           MCTS sims per decision (default {DEFAULT_SIMS})\n\
  --rollout-depth N  rollout depth cap (default {DEFAULT_ROLLOUT_DEPTH})\n\
  --c-puct F         exploration constant\n\
  --seed N           MCTS seed\n\
  --tree-reuse       reuse MCTS tree across decisions\n\
  --quiet, -q        suppress per-fixture FLAG lines on stderr\n",
                    );
                    std::process::exit(0);
                }
                other if other.starts_with("--") => return Err(format!("unknown flag {other}")),
                _ => explicit_fixtures.push(PathBuf::from(&arg)),
            }
        }
        Ok(Self {
            glob,
            limit,
            explicit_fixtures,
            cfg,
            quiet,
        })
    }
}

fn parse_pop<T: std::str::FromStr>(argv: &mut Vec<String>, name: &str) -> Result<T, String> {
    let raw = argv
        .pop()
        .ok_or_else(|| format!("{name} expects a value"))?;
    raw.parse()
        .map_err(|_| format!("{name}={raw} could not be parsed"))
}

fn main() -> std::io::Result<()> {
    let args = match Args::parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };

    ensure_card_dispatch();
    ensure_potion_dispatch();
    ensure_relic_dispatch();

    let mut fixtures: Vec<PathBuf> = if args.explicit_fixtures.is_empty() {
        let mut hits: Vec<PathBuf> = glob::glob(&args.glob)
            .map_err(std::io::Error::other)?
            .filter_map(Result::ok)
            .collect();
        hits.sort();
        hits
    } else {
        args.explicit_fixtures.clone()
    };
    if args.limit > 0 && fixtures.len() > args.limit {
        fixtures.truncate(args.limit);
    }
    if fixtures.is_empty() {
        eprintln!("error: no fixtures matched glob {:?}", args.glob);
        std::process::exit(1);
    }

    eprintln!(
        "[red-flags] auditing {} fixture(s), sims={}, depth={}",
        fixtures.len(),
        args.cfg.sims,
        args.cfg.rollout_depth,
    );

    let started = Instant::now();
    let mut total_counts: BTreeMap<String, u32> = empty_counts();
    let mut per_fixture: Vec<PerFixtureRow> = Vec::with_capacity(fixtures.len());
    let mut fights_audited: u32 = 0;
    let mut fights_with_any_flag: u32 = 0;
    let mut fights_skipped: u32 = 0;

    for fixture in &fixtures {
        let recorded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            record_from_fixture(fixture, &args.cfg)
        }));
        match recorded {
            Ok(Ok(rec)) => {
                let flags = audit_fight(&rec);
                fights_audited += 1;
                if !flags.is_empty() {
                    fights_with_any_flag += 1;
                }
                let mut row_counts: BTreeMap<String, u32> = empty_counts();
                for f in &flags {
                    let key = f.kind.to_string();
                    *row_counts.entry(key.clone()).or_default() += 1;
                    *total_counts.entry(key).or_default() += 1;
                }
                if !args.quiet && !flags.is_empty() {
                    eprintln!("  [FLAG] {} — {} flag(s)", rec.fixture_stem, flags.len());
                }
                per_fixture.push(PerFixtureRow {
                    fixture: rec.fixture_stem.clone(),
                    won: rec.final_won,
                    decisions: u32::try_from(rec.decisions.len()).unwrap_or(u32::MAX),
                    total_flags: u32::try_from(flags.len()).unwrap_or(u32::MAX),
                    counts: row_counts,
                });
            }
            Ok(Err(e)) => {
                fights_skipped += 1;
                eprintln!("  [skip] {} (load error: {e})", fixture.display());
            }
            Err(panic) => {
                fights_skipped += 1;
                let msg = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
                    .unwrap_or_else(|| "<unknown panic>".to_string());
                eprintln!("  [skip] {} (engine panic: {msg})", fixture.display());
            }
        }
    }

    per_fixture.sort_by(|a, b| a.fixture.cmp(&b.fixture));
    let wall_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let git_sha = git_short_sha();

    let report = RedFlagReport {
        generator: "mcts_red_flags".to_string(),
        fixture_glob: if args.explicit_fixtures.is_empty() {
            args.glob.clone()
        } else {
            "<explicit-fixtures>".to_string()
        },
        fixtures_requested: fixtures.len(),
        sims: args.cfg.sims,
        rollout_depth: args.cfg.rollout_depth,
        fights_audited,
        fights_with_any_flag,
        fights_skipped,
        wall_ms,
        counts: total_counts.clone(),
        per_fixture,
        git_sha: git_sha.clone(),
    };

    let out_path = data_path(&format!("mcts_red_flags_{git_sha}.json"));
    write_json_pretty(&out_path, &report)?;

    eprintln!("[red-flags] summary:");
    eprintln!(
        "  audited={fights_audited}, flagged_fights={fights_with_any_flag}, skipped={fights_skipped}, wall_ms={wall_ms}",
    );
    for kind in RedFlagKind::ALL {
        let n = total_counts.get(kind.as_str()).copied().unwrap_or(0);
        eprintln!("  {} = {n}", kind.as_str());
    }
    eprintln!("[red-flags] wrote {}", out_path.display());

    Ok(())
}

fn empty_counts() -> BTreeMap<String, u32> {
    let mut m = BTreeMap::new();
    for kind in RedFlagKind::ALL {
        m.insert(kind.as_str().to_string(), 0);
    }
    m
}
