//! `sts2-replay-audit` — drive the recorder over one or more
//! `.run-source` TOML fixtures and report aggregate red-flag counts.
//!
//! Same fixture-resolution surface as `sts2-fight-replay` (positional
//! fixtures or `--lost-runs` / `--won-runs`) but does NOT render HTML;
//! its sole output is per-fixture flag lines and a final summary.
//! Intended to run after a benchmark / corpus regeneration to catch
//! agent/engine regressions like turn-skipping or self-killing plays.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use std::collections::BTreeMap;

use sts2_cli::replay::{
    audit_fight, format_summary, record_from_fixture, AuditSummary, RecorderConfig, RedFlagKind,
};
use sts2_content::cards::dispatch::ensure_installed as ensure_card_dispatch;
use sts2_content::potions::dispatch::ensure_installed as ensure_potion_dispatch;
use sts2_content::relics::dispatch::ensure_installed as ensure_relic_dispatch;

const DEFAULT_FIXTURE_DIR: &str = "/tmp/run-fixtures-full/the-ironclad";
const DEFAULT_RUN_SOURCE: &str = "/tmp/vbzji-ceiling/texel_s50000_d20.json";

struct Args {
    fixtures: Vec<PathBuf>,
    cfg: RecorderConfig,
    lost_runs: bool,
    won_runs: bool,
    quiet: bool,
    json: bool,
    /// Process at most this many fixtures (0 = no cap).
    limit: usize,
    /// Path to a `[budgets]` TOML; when set, the binary exits 1 iff any
    /// `RedFlagKind` count exceeds its budget (instead of the legacy
    /// "any flag at all → exit 1" behavior).
    ci_gate: Option<PathBuf>,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut fixtures: Vec<PathBuf> = Vec::new();
        let mut cfg = RecorderConfig::default();
        let mut lost_runs = false;
        let mut won_runs = false;
        let mut quiet = false;
        let mut json = false;
        let mut limit: usize = 0;
        let mut ci_gate: Option<PathBuf> = None;
        let mut argv: Vec<OsString> = std::env::args_os().skip(1).collect();
        argv.reverse();
        while let Some(raw) = argv.pop() {
            let s = raw.to_string_lossy().to_string();
            match s.as_str() {
                "--sims" => cfg.sims = parse_next(&mut argv, "--sims")?,
                "--rollout-depth" => cfg.rollout_depth = parse_next(&mut argv, "--rollout-depth")?,
                "--c-puct" => cfg.c_puct = parse_next(&mut argv, "--c-puct")?,
                "--seed" => cfg.seed = parse_next(&mut argv, "--seed")?,
                "--limit" => limit = parse_next(&mut argv, "--limit")?,
                "--lost-runs" => lost_runs = true,
                "--won-runs" => won_runs = true,
                "--tree-reuse" => cfg.tree_reuse = true,
                "--quiet" | "-q" => quiet = true,
                "--json" => json = true,
                "--ci-gate" => ci_gate = Some(PathBuf::from(pop_arg(&mut argv, "--ci-gate")?)),
                arg if arg.starts_with("--ci-gate=") => {
                    ci_gate = Some(PathBuf::from(&arg["--ci-gate=".len()..]));
                }
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
                arg if arg.starts_with("--limit=") => limit = parse_kv(arg, "--limit=")?,
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
            lost_runs,
            won_runs,
            quiet,
            json,
            limit,
            ci_gate,
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
        "sts2-replay-audit — count red-flag play patterns over a fixture corpus\n\n\
USAGE:\n    sts2-replay-audit [OPTIONS] <FIXTURE>...\n\n\
ARGUMENTS:\n    <FIXTURE>...    one or more .run-source TOML paths\n\n\
OPTIONS:\n    --sims N           MCTS sims per decision (default 50000)\n\
    --rollout-depth N  rollout depth cap (default 20)\n\
    --c-puct F         exploration constant (default 1.0)\n\
    --seed N           MCTS seed (default 1)\n\
    --limit N          process at most N fixtures (0 = no cap)\n\
    --lost-runs        audit player_outcome='lost' fixtures from\n\
                       {DEFAULT_RUN_SOURCE}\n\
    --won-runs         audit player_outcome='won' fixtures from the\n\
                       same source\n\
    --tree-reuse       reuse MCTS tree across decisions\n\
    --quiet, -q        suppress per-fixture flag lines\n\
    --json             emit a final JSON summary on stdout\n\
    --ci-gate PATH     read a [budgets] TOML; exit 1 iff any class is\n\
                       over its per-class budget (replaces the legacy\n\
                       'any flag at all → exit 1' behavior)\n\
    -h, --help         print this help\n",
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

    let mut fixtures = match resolve_fixtures(&args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    if fixtures.is_empty() {
        eprintln!("error: no fixtures to audit (pass a path or --lost-runs / --won-runs)");
        return ExitCode::from(2);
    }
    if args.limit > 0 && fixtures.len() > args.limit {
        fixtures.truncate(args.limit);
    }

    let mut summary = AuditSummary::default();
    let mut skipped: u32 = 0;
    for fixture in &fixtures {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            record_from_fixture(fixture, &args.cfg)
        }));
        match result {
            Ok(Ok(rec)) => {
                let flags = audit_fight(&rec);
                if !args.quiet {
                    if flags.is_empty() {
                        eprintln!("[ok ] {}", fixture.display());
                    } else {
                        eprintln!("[FLAG] {} — {} flag(s)", fixture.display(), flags.len());
                        for f in &flags {
                            eprintln!(
                                "       turn {:>3} d{:>3} {} — {}",
                                f.turn, f.decision_index, f.kind, f.detail
                            );
                        }
                    }
                }
                summary.record(&flags);
            }
            Ok(Err(e)) => {
                skipped += 1;
                eprintln!("[skip] {} (load error: {e})", fixture.display());
            }
            Err(panic) => {
                skipped += 1;
                let msg = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
                    .unwrap_or_else(|| "<unknown panic>".to_string());
                eprintln!("[skip] {} (engine panic: {msg})", fixture.display());
            }
        }
    }

    eprintln!(
        "[replay-audit] {}; skipped={}",
        format_summary(&summary),
        skipped,
    );

    let ci_gate_result = if let Some(path) = args.ci_gate.as_deref() {
        match read_budgets(path) {
            Ok(budgets) => Some(evaluate_budgets(&summary, &budgets)),
            Err(e) => {
                eprintln!(
                    "error: --ci-gate {} could not be loaded: {e}",
                    path.display()
                );
                return ExitCode::from(2);
            }
        }
    } else {
        None
    };

    if let Some(ref gate) = ci_gate_result {
        if gate.over_budget.is_empty() {
            eprintln!("[replay-audit] ci-gate: PASS (all classes under budget)");
        } else {
            eprintln!(
                "[replay-audit] ci-gate: FAIL ({} class(es) over budget)",
                gate.over_budget.len(),
            );
            for row in &gate.over_budget {
                eprintln!(
                    "  {} count={} budget={} over_by={}",
                    row.kind, row.count, row.budget, row.over_by,
                );
            }
        }
    }

    if args.json {
        print_json_summary(&summary, skipped, ci_gate_result.as_ref());
    }

    if let Some(gate) = ci_gate_result {
        if gate.over_budget.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        }
    } else if summary.fights_with_any_flag > 0 {
        // Legacy behavior preserved when --ci-gate is not specified.
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[derive(Debug, Clone)]
struct GateRow {
    kind: RedFlagKind,
    count: u32,
    budget: u32,
    over_by: i64,
}

#[derive(Debug, Clone)]
struct CiGateResult {
    rows: Vec<GateRow>,
    over_budget: Vec<GateRow>,
}

fn read_budgets(path: &Path) -> Result<BTreeMap<String, u32>, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let doc: toml::Value = toml::from_str(&raw).map_err(|e| format!("parse: {e}"))?;
    let table = doc
        .get("budgets")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "missing top-level [budgets] table".to_string())?;
    let mut out: BTreeMap<String, u32> = BTreeMap::new();
    for (k, v) in table {
        let n = v
            .as_integer()
            .ok_or_else(|| format!("[budgets].{k} is not an integer"))?;
        let n = u32::try_from(n).map_err(|_| format!("[budgets].{k}={n} is out of range"))?;
        out.insert(k.clone(), n);
    }
    let valid: std::collections::HashSet<&str> =
        RedFlagKind::ALL.iter().map(|k| k.as_str()).collect();
    for k in out.keys() {
        if !valid.contains(k.as_str()) {
            return Err(format!(
                "[budgets].{k} is not a known RedFlagKind ({})",
                valid.iter().copied().collect::<Vec<_>>().join(", "),
            ));
        }
    }
    Ok(out)
}

fn evaluate_budgets(summary: &AuditSummary, budgets: &BTreeMap<String, u32>) -> CiGateResult {
    let mut rows: Vec<GateRow> = Vec::new();
    for kind in RedFlagKind::ALL {
        let count = summary.count(kind);
        let budget = budgets.get(kind.as_str()).copied().unwrap_or(u32::MAX);
        let over_by = i64::from(count) - i64::from(budget);
        rows.push(GateRow {
            kind,
            count,
            budget,
            over_by,
        });
    }
    let over_budget: Vec<GateRow> = rows.iter().filter(|r| r.over_by > 0).cloned().collect();
    CiGateResult { rows, over_budget }
}

fn print_json_summary(summary: &AuditSummary, skipped: u32, ci_gate: Option<&CiGateResult>) {
    let counts: Vec<String> = RedFlagKind::ALL
        .iter()
        .map(|k| format!("\"{}\":{}", k, summary.count(*k)))
        .collect();
    let gate_json = ci_gate.map_or_else(String::new, |gate| {
        let rows: Vec<String> = gate
            .rows
            .iter()
            .map(|r| {
                format!(
                    "\"{}\":{{\"count\":{},\"budget\":{},\"over_by\":{}}}",
                    r.kind, r.count, r.budget, r.over_by,
                )
            })
            .collect();
        format!(
            ",\"ci_gate\":{{\"pass\":{},\"rows\":{{{}}}}}",
            gate.over_budget.is_empty(),
            rows.join(","),
        )
    });
    println!(
        "{{\"fights_audited\":{},\"fights_with_any_flag\":{},\"skipped\":{},\"counts\":{{{}}}{}}}",
        summary.fights_audited,
        summary.fights_with_any_flag,
        skipped,
        counts.join(","),
        gate_json,
    );
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
    let src = Path::new(DEFAULT_RUN_SOURCE);
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

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    struct TmpFile {
        path: PathBuf,
    }
    impl Drop for TmpFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    fn write_tmp(contents: &str) -> TmpFile {
        // Use process id + nanosecond timestamp for uniqueness — the
        // test never runs concurrently against the same path within
        // a single process, but cargo test parallelism across procs
        // is fine.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sts2_blunder_budget_{}_{}.toml",
            std::process::id(),
            n,
        ));
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f.flush().unwrap();
        TmpFile { path }
    }

    impl TmpFile {
        fn path(&self) -> &Path {
            &self.path
        }
    }

    #[test]
    fn budget_parser_accepts_known_kinds() {
        let toml = "[budgets]\nsuicide_play = 3\nlethal_intent_ignored = 7\n";
        let f = write_tmp(toml);
        let budgets = read_budgets(f.path()).expect("parse should succeed");
        assert_eq!(budgets.get("suicide_play"), Some(&3));
        assert_eq!(budgets.get("lethal_intent_ignored"), Some(&7));
    }

    #[test]
    fn budget_parser_rejects_unknown_kind() {
        let toml = "[budgets]\nnot_a_real_kind = 1\n";
        let f = write_tmp(toml);
        let err = read_budgets(f.path()).unwrap_err();
        assert!(err.contains("not_a_real_kind"), "got: {err}");
    }

    #[test]
    fn evaluate_budgets_passes_when_under() {
        let mut summary = AuditSummary {
            fights_audited: 1,
            ..AuditSummary::default()
        };
        // Synthetic 2 suicide_play flags under a budget of 3 → pass.
        for _ in 0..2 {
            summary.record(&[sts2_cli::replay::RedFlag {
                kind: RedFlagKind::SuicidePlay,
                turn: 1,
                decision_index: 0,
                detail: String::new(),
            }]);
        }
        let mut budgets = BTreeMap::new();
        budgets.insert("suicide_play".to_string(), 3);
        let gate = evaluate_budgets(&summary, &budgets);
        assert!(
            gate.over_budget.is_empty(),
            "expected pass: {:?}",
            gate.rows
        );
    }

    #[test]
    fn evaluate_budgets_fails_when_over() {
        let mut summary = AuditSummary {
            fights_audited: 1,
            ..AuditSummary::default()
        };
        for _ in 0..5 {
            summary.record(&[sts2_cli::replay::RedFlag {
                kind: RedFlagKind::SuicidePlay,
                turn: 1,
                decision_index: 0,
                detail: String::new(),
            }]);
        }
        let mut budgets = BTreeMap::new();
        budgets.insert("suicide_play".to_string(), 3);
        let gate = evaluate_budgets(&summary, &budgets);
        assert_eq!(gate.over_budget.len(), 1);
        let row = &gate.over_budget[0];
        assert_eq!(row.kind, RedFlagKind::SuicidePlay);
        assert_eq!(row.count, 5);
        assert_eq!(row.budget, 3);
        assert_eq!(row.over_by, 2);
    }

    #[test]
    fn classes_without_explicit_budget_are_unbounded() {
        let mut summary = AuditSummary::default();
        // Lots of EndTurnWithPlayable flags but no row in the TOML.
        for _ in 0..1000 {
            summary.record(&[sts2_cli::replay::RedFlag {
                kind: RedFlagKind::EndTurnWithPlayable,
                turn: 1,
                decision_index: 0,
                detail: String::new(),
            }]);
        }
        let budgets = BTreeMap::new();
        let gate = evaluate_budgets(&summary, &budgets);
        assert!(
            gate.over_budget.is_empty(),
            "omitted classes should treat budget as ∞: {:?}",
            gate.over_budget,
        );
    }
}
