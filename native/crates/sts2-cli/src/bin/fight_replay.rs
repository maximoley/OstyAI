//! DEPRECATED (2026-06-09): superseded by the sts2-pycli workbench MCTS
//! recorder. Prefer `python -m workbench.record --fixture <toml>` →
//! `python -m workbench.report <log.jsonl>` (Markdown) and
//! `python -m workbench.aggregate <logs_dir>`. Kept for reference; not deleted.
//!
//! `sts2-fight-replay` — drive MCTS through one or more `.run-source`
//! TOML fixtures and emit a self-contained HTML page per fight.
//!
//! See `crates/sts2-cli/src/replay/` for the recorder/renderer
//! module. Companion to bead `sts2-rs-ls5xj`'s conditional-outcome
//! slice (`docs/perf/2026-05-20-ls5xj-conditional-slice.md`).

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sts2_cli::replay::{
    audit_fight, format_summary, record_from_fixture, render_fight_html, render_index_html,
    AuditSummary, FightRecord, RecorderConfig,
};
use sts2_content::cards::dispatch::ensure_installed as ensure_card_dispatch;
use sts2_content::potions::dispatch::ensure_installed as ensure_potion_dispatch;
use sts2_content::relics::dispatch::ensure_installed as ensure_relic_dispatch;

const DEFAULT_OUT: &str = "target/fight-replays";
const DEFAULT_FIXTURE_DIR: &str = "/tmp/run-fixtures-full/the-ironclad";
/// vbzji per-state artifact at sims=50k / depth=20 — the same
/// 200-fixture corpus the ls5xj conditional slice was computed
/// against. Each entry's `encounter` field is the fixture stem,
/// which we resolve back to a TOML and filter by
/// `[provenance].player_outcome == "lost"`.
const DEFAULT_LOST_RUN_SOURCE: &str = "/tmp/vbzji-ceiling/texel_s50000_d20.json";

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutcomeFilter {
    All,
    OnlyMctsWon,
    OnlyMctsLost,
}

struct Args {
    fixtures: Vec<PathBuf>,
    cfg: RecorderConfig,
    out_dir: PathBuf,
    lost_runs: bool,
    won_runs: bool,
    outcome_filter: OutcomeFilter,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut fixtures: Vec<PathBuf> = Vec::new();
        let mut cfg = RecorderConfig::default();
        let mut out_dir = PathBuf::from(DEFAULT_OUT);
        let mut lost_runs = false;
        let mut won_runs = false;
        let mut outcome_filter = OutcomeFilter::All;
        let mut argv: Vec<OsString> = std::env::args_os().skip(1).collect();
        argv.reverse(); // pop from back for cheap iteration
        while let Some(raw) = argv.pop() {
            let s = raw.to_string_lossy().to_string();
            match s.as_str() {
                "--sims" => cfg.sims = parse_next(&mut argv, "--sims")?,
                "--rollout-depth" => cfg.rollout_depth = parse_next(&mut argv, "--rollout-depth")?,
                "--c-puct" => cfg.c_puct = parse_next(&mut argv, "--c-puct")?,
                "--seed" => cfg.seed = parse_next(&mut argv, "--seed")?,
                "--out" => out_dir = PathBuf::from(pop_arg(&mut argv, "--out")?),
                "--lost-runs" => lost_runs = true,
                "--won-runs" => won_runs = true,
                "--only-mcts-lost" => outcome_filter = OutcomeFilter::OnlyMctsLost,
                "--only-mcts-won" => outcome_filter = OutcomeFilter::OnlyMctsWon,
                "--mcts-mistakes" => {
                    won_runs = true;
                    outcome_filter = OutcomeFilter::OnlyMctsLost;
                }
                "--tree-reuse" => cfg.tree_reuse = true,
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                arg if arg.starts_with("--sims=") => cfg.sims = parse_kv(arg, "--sims=")?,
                arg if arg.starts_with("--rollout-depth=") => {
                    cfg.rollout_depth = parse_kv(arg, "--rollout-depth=")?;
                }
                arg if arg.starts_with("--c-puct=") => cfg.c_puct = parse_kv(arg, "--c-puct=")?,
                arg if arg.starts_with("--seed=") => cfg.seed = parse_kv(arg, "--seed=")?,
                arg if arg.starts_with("--out=") => {
                    out_dir = PathBuf::from(&arg["--out=".len()..]);
                }
                arg if arg.starts_with("--") => return Err(format!("unknown flag {arg}")),
                _ => fixtures.push(PathBuf::from(&s)),
            }
        }
        if lost_runs && won_runs {
            return Err("--lost-runs and --won-runs are mutually exclusive".to_string());
        }
        Ok(Self {
            fixtures,
            cfg,
            out_dir,
            lost_runs,
            won_runs,
            outcome_filter,
        })
    }
}

fn pop_arg(argv: &mut Vec<OsString>, name: &str) -> Result<String, String> {
    argv.pop()
        .map(|os| os.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{name} expects a value"))
}

fn parse_next<T: std::str::FromStr>(argv: &mut Vec<OsString>, name: &str) -> Result<T, String> {
    let v = pop_arg(argv, name)?;
    v.parse()
        .map_err(|_| format!("{name}={v} could not be parsed"))
}

fn parse_kv<T: std::str::FromStr>(arg: &str, prefix: &str) -> Result<T, String> {
    let v = &arg[prefix.len()..];
    v.parse()
        .map_err(|_| format!("{prefix}{v} could not be parsed"))
}

fn print_help() {
    println!(
        "sts2-fight-replay — HTML replay viewer for sts2-rs MCTS fights\n\n\
USAGE:\n    sts2-fight-replay [OPTIONS] <FIXTURE>...\n\
ARGUMENTS:\n    <FIXTURE>...    one or more .run-source TOML paths\n\n\
OPTIONS:\n    --sims N           MCTS sims per decision (default 50000)\n\
    --rollout-depth N  rollout depth cap (default 20)\n\
    --c-puct F         exploration constant (default 1.0)\n\
    --seed N           MCTS seed (default 1)\n\
    --out DIR          output directory (default {DEFAULT_OUT})\n\
    --lost-runs        replay the player_outcome='lost' fixtures from\n\
                       the vbzji 200-state corpus (joined against\n\
                       {DEFAULT_LOST_RUN_SOURCE}); skips the FIXTURE\n\
                       positional arguments\n\
    --won-runs         mirror of --lost-runs: replay the\n\
                       player_outcome='won' fixtures (~188 of 200)\n\
    --mcts-mistakes    shortcut for --won-runs --only-mcts-lost: the\n\
                       interesting failure set (positions a human\n\
                       solved that MCTS doesn't)\n\
    --only-mcts-won    suppress HTML emission for fights MCTS lost\n\
    --only-mcts-lost   suppress HTML emission for fights MCTS won\n\
    --tree-reuse       reuse the MCTS tree across decisions via\n\
                       Mcts::reroot (default: fresh root per decision)\n\
    -h, --help         print this help\n\n\
Wiki image root: set STS2_WIKI env var (default\n\
    ../sts2-wiki, the monorepo sibling).",
    );
}

fn main() -> ExitCode {
    let args = match Args::parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    ensure_card_dispatch();
    ensure_potion_dispatch();
    ensure_relic_dispatch();

    let fixtures = match resolve_fixtures(&args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    if fixtures.is_empty() {
        eprintln!("error: no fixtures to replay (pass a path or --lost-runs)");
        return ExitCode::from(2);
    }

    if let Err(e) = fs::create_dir_all(&args.out_dir) {
        eprintln!("error: cannot create {}: {e}", args.out_dir.display());
        return ExitCode::from(1);
    }

    let mut records: Vec<FightRecord> = Vec::with_capacity(fixtures.len());
    let mut mcts_won_count: u32 = 0;
    let mut mcts_lost_count: u32 = 0;
    let mut filtered_out_count: u32 = 0;
    let mut audit_summary = AuditSummary::default();
    for fixture in &fixtures {
        eprintln!(
            "[fight-replay] recording {} (sims={}, depth={})",
            fixture.display(),
            args.cfg.sims,
            args.cfg.rollout_depth
        );
        // Engine content gaps occasionally panic (`bash: enemy-
        // targeted, engine validated`, unported relic, etc). Mirror
        // the `mcts_oracle` policy: catch the panic, log it, and
        // continue with the next fixture so a single bad state
        // doesn't kill the whole batch.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            record_from_fixture(fixture, &args.cfg)
        }));
        match result {
            Ok(Ok(rec)) => {
                if rec.final_won {
                    mcts_won_count += 1;
                } else {
                    mcts_lost_count += 1;
                }
                let keep = match args.outcome_filter {
                    OutcomeFilter::All => true,
                    OutcomeFilter::OnlyMctsWon => rec.final_won,
                    OutcomeFilter::OnlyMctsLost => !rec.final_won,
                };
                if !keep {
                    filtered_out_count += 1;
                    eprintln!(
                        "  skipped (filter) — mcts {}, {} decisions",
                        if rec.final_won { "WON" } else { "LOST" },
                        rec.decisions.len(),
                    );
                    continue;
                }
                let html_path = args.out_dir.join(format!("{}.html", rec.fixture_stem));
                if let Err(e) = fs::write(&html_path, render_fight_html(&rec)) {
                    eprintln!("  error writing {}: {e}", html_path.display());
                    continue;
                }
                let flags = audit_fight(&rec);
                audit_summary.record(&flags);
                eprintln!(
                    "  → {} ({} decisions, mcts {}, {} red flag{})",
                    html_path.display(),
                    rec.decisions.len(),
                    if rec.final_won { "WON" } else { "LOST" },
                    flags.len(),
                    if flags.len() == 1 { "" } else { "s" },
                );
                records.push(rec);
            }
            Ok(Err(e)) => {
                eprintln!("  skipped (load error): {e}");
            }
            Err(panic) => {
                let msg = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
                    .unwrap_or_else(|| "<unknown panic>".to_string());
                eprintln!("  skipped (engine panic): {msg}");
            }
        }
    }

    eprintln!(
        "[fight-replay] tally: {mcts_won_count} mcts wins / {mcts_lost_count} mcts losses, {filtered_out_count} filtered out",
    );
    eprintln!("[fight-replay] audit: {}", format_summary(&audit_summary));
    if records.is_empty() {
        eprintln!("no replays matched (try removing --only-mcts-* filter)");
        return ExitCode::from(1);
    }

    let index_html = render_index_html(&records);
    let index_path = args.out_dir.join("index.html");
    if let Err(e) = fs::write(&index_path, index_html) {
        eprintln!("error writing index {}: {e}", index_path.display());
        return ExitCode::from(1);
    }
    eprintln!("[fight-replay] index → {}", index_path.display());
    ExitCode::SUCCESS
}

fn resolve_fixtures(args: &Args) -> Result<Vec<PathBuf>, String> {
    if !args.lost_runs && !args.won_runs {
        return Ok(args.fixtures.clone());
    }
    let needle = if args.lost_runs {
        "player_outcome = \"lost\""
    } else {
        "player_outcome = \"won\""
    };
    let src = Path::new(DEFAULT_LOST_RUN_SOURCE);
    let raw =
        fs::read_to_string(src).map_err(|e| format!("could not read {}: {e}", src.display()))?;
    let doc: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("invalid JSON in {}: {e}", src.display()))?;
    let per_state = doc
        .get("per_state")
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("{} has no per_state array", src.display()))?;
    let fixture_dir = Path::new(DEFAULT_FIXTURE_DIR);
    let mut out: Vec<PathBuf> = Vec::new();
    for entry in per_state {
        let Some(encounter) = entry.get("encounter").and_then(|v| v.as_str()) else {
            continue;
        };
        // Security (sts2-rs-ywvnk): `encounter` comes from an untrusted corpus
        // JSON and is interpolated into a filesystem path. Skip any value that
        // is not a single plain filename component so a crafted `../../..` (or
        // absolute) stem cannot escape `fixture_dir` to read arbitrary *.toml.
        if !is_safe_fixture_stem(encounter) {
            continue;
        }
        let path = fixture_dir.join(format!("{encounter}.toml"));
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if content.contains(needle) {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// Returns `true` only when `stem` is a single plain filename component — i.e.
/// safe to interpolate into `fixture_dir.join(format!("{stem}.toml"))` without
/// escaping `fixture_dir`. Rejects path separators, `..`/`.` components,
/// absolute paths, drive prefixes, and the empty string. Used to harden
/// `resolve_fixtures` against path traversal from untrusted corpus JSON
/// (bead `sts2-rs-ywvnk`).
fn is_safe_fixture_stem(stem: &str) -> bool {
    use std::path::Component;
    let mut comps = Path::new(stem).components();
    matches!(
        (comps.next(), comps.next()),
        (Some(Component::Normal(_)), None)
    )
}

#[cfg(test)]
mod tests {
    use super::is_safe_fixture_stem;

    #[test]
    fn accepts_plain_stems() {
        for ok in [
            "fight_001",
            "the_ironclad_vs_nibbit",
            "encounter-42",
            "a.b", // a dotted stem is still one Normal component
        ] {
            assert!(is_safe_fixture_stem(ok), "should accept {ok:?}");
        }
    }

    #[test]
    fn rejects_traversal_and_separators() {
        for bad in [
            "",
            "..",
            ".",
            "../secret",
            "../../etc/passwd",
            "a/b",
            "a/../b",
            "/etc/passwd",
            "./fight_001",
            "sub/fight_001",
        ] {
            assert!(!is_safe_fixture_stem(bad), "should reject {bad:?}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn rejects_windows_separators_and_prefixes() {
        for bad in ["a\\b", "..\\secret", "C:\\windows", "\\\\server\\share"] {
            assert!(!is_safe_fixture_stem(bad), "should reject {bad:?}");
        }
    }
}
