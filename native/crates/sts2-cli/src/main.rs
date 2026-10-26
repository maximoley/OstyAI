//! sts2-cli — interactive REPL + scripted-replay driver for `sts2-core`.

// Swap the global allocator to mimalloc. The hot MCTS path
// (`one_simulation` clones `CombatState` once per sim) hammers the
// allocator across rayon workers — glibc's heap serializes past ~8
// threads on this workload. mimalloc is lock-free per-thread and
// scales linearly out to physical-core count. Verified 4–6× wall-time
// improvement at 24+ threads on the AMD EPYC 7443P test box.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// Bead `sts2-rs-a2fh`: this crate now exposes a `[lib]` target at
// `src/lib.rs` whose `pub mod {agent, cli, sim}` mirror these binary-
// local declarations. The lib and bin targets compile independently
// (each owns its own module tree); we re-declare them here so the
// binary continues to use `crate::*` paths without churn. Downstream
// callers (corpus_gen, bench harnesses) consume the library copy via
// `sts2_cli::*`.
mod agent;
mod cli;
mod sim;

use std::io::{self, BufReader, Write};
use std::process::ExitCode;

use clap::{parser::ValueSource, ArgMatches, CommandFactory, FromArgMatches};

use crate::agent::human::HumanAgent;
use crate::agent::mcts::{MctsAgent, MctsAgentConfig};
use crate::agent::random::RandomAgent;
use crate::cli::combat::{AgentKind, CombatArgs};
use crate::cli::{Cli, Command};
use crate::sim::jsonl::JsonlWriter;
use crate::sim::scenario::deck::{format_summary as format_deck_summary, resolve_deck, DeckInput};
use crate::sim::scenario::encounter::parse_inline_monsters;
use crate::sim::scenario::potions::{
    render_banner as format_potions_banner, resolve_potions, PotionInput,
};
use crate::sim::scenario::random as random_mod;
use crate::sim::scenario::relics::{
    format_relics_for_banner, resolve_relics, seed_relics_with_counters, RelicInput,
};
use crate::sim::scenario::toml_file::{
    load_scenario_file, lower_monsters, parse_player_class, render_scenario_toml, ScenarioFile,
};
use crate::sim::scenario::{resolve_encounter, summarize_monsters};
use crate::sim::{build_starter_state_from_monsters, ConsoleSimulator, SimulatorContext};

fn main() -> ExitCode {
    // Capture the raw `ArgMatches` alongside the derived `Cli` so the
    // scenario-file merge (bead `sts2-rs-g2ex`) can ask clap which
    // `combat` flags were user-supplied vs. default-filled. clap derive
    // doesn't surface `ValueSource` through `Cli::parse()` directly, so
    // we walk the matches by hand.
    let matches = Cli::command().get_matches();
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(c) => c,
        Err(e) => {
            e.exit();
        }
    };

    match cli.command {
        Command::Combat(args) => {
            // The `combat` subcommand's `ArgMatches` lives under the
            // matched subcommand name. If clap routed us here the
            // submatches must exist.
            let combat_matches = matches
                .subcommand_matches("combat")
                .expect("combat subcommand matched");
            run_combat(args, combat_matches)
        }
        Command::List(args) => {
            let mut stdout = io::stdout().lock();
            match cli::list::run(args, &mut stdout) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("write error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Fuzz(args) => {
            let mut stdout = io::stdout().lock();
            match cli::fuzz::run(args, &mut stdout) {
                // Non-zero exit when the sweep found crashes/anomalies so
                // CI fails on a finding.
                Ok(0) => ExitCode::SUCCESS,
                Ok(_) => ExitCode::FAILURE,
                Err(e) => {
                    eprintln!("fuzz error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Seedfind(args) => {
            let mut stdout = io::stdout().lock();
            match cli::seedfind::run(args, &mut stdout) {
                Ok(_) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("seedfind error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

/// Was the named CLI flag explicitly set by the user (i.e. not
/// defaulted-in and not absent)? Used by the scenario-file merge so
/// `--player Ironclad` overrides a `[player] class = "Silent"` line
/// in the file while a defaulted `--player` does not.
fn was_supplied(matches: &ArgMatches, name: &str) -> bool {
    matches!(matches.value_source(name), Some(ValueSource::CommandLine))
}

/// Fold a parsed scenario file's player/deck/relics/potions sections
/// into the CLI args struct, leaving any user-supplied CLI flags
/// untouched. Returns `Err(ExitCode)` on unrecoverable input (e.g.
/// unknown `player.class`).
fn merge_scenario_into_args(
    args: &mut CombatArgs,
    file: &ScenarioFile,
    matches: &ArgMatches,
) -> Result<(), ExitCode> {
    use crate::cli::combat::CliPlayerClass;
    use crate::sim::PlayerClass;

    if let Some(player) = file.player.as_ref() {
        if let Some(class_str) = player.class.as_deref() {
            if !was_supplied(matches, "player") {
                match parse_player_class(class_str) {
                    Ok(PlayerClass::Ironclad) => args.player = CliPlayerClass::Ironclad,
                    Ok(PlayerClass::Silent) => args.player = CliPlayerClass::Silent,
                    // Defect/Regent/Necrobinder accepted at the scenario
                    // layer (bead `sts2-rs-5m6k`) to unblock .run-corpus
                    // fixtures. Engine layer still uses the Ironclad
                    // starter pool (see `starter::build_state`); surface
                    // the fallback so the divergence isn't silent.
                    Ok(
                        cls
                        @ (PlayerClass::Defect | PlayerClass::Regent | PlayerClass::Necrobinder),
                    ) => {
                        args.player = CliPlayerClass::Ironclad;
                        eprintln!(
                            "note: scenario player.class '{class_str}' ({cls:?}) is not \
                             yet wired in sts2-core; falling back to Ironclad starter \
                             HP / deck (sts2-rs-5m6k)"
                        );
                    }
                    Err(e) => {
                        eprintln!("scenario error: {e}");
                        return Err(ExitCode::FAILURE);
                    }
                }
            }
        }
        if let Some(hp) = player.hp {
            if args.hp.is_none() {
                // Bead `sts2-rs-ppuk`: scenario `[player] hp` is `i32`
                // to round-trip overkill-death dumps; clamp negatives
                // to 0 for the unsigned `--hp` arg.
                args.hp = Some(u32::try_from(hp.max(0)).unwrap_or(0));
            }
        }
        if let Some(max_hp) = player.max_hp {
            if args.max_hp.is_none() {
                // Bead `sts2-rs-f3g0`: thread `max_hp` independently
                // so wounded `sts2log` fixtures start at hp/max_hp
                // rather than collapsing both into the same value.
                args.max_hp = Some(max_hp);
            }
        }
        if !player.deck.is_empty() {
            // CLI deck flags (inline / file / preset) override the
            // scenario file's `[player] deck = [...]` only when one of
            // them is set; otherwise the file's inline list seeds the
            // `--deck` flag in comma-joined form.
            let cli_deck_set =
                args.deck.is_some() || args.deck_file.is_some() || args.deck_preset.is_some();
            if !cli_deck_set {
                args.deck = Some(player.deck.join(","));
            }
        }
    }

    // Accept relics/potions declared at the top level OR nested under
    // `[player]`. TOML's section-scoping rules make any plain `relics
    // = [...]` line that follows the `[player]` header parse as
    // `player.relics`; we want both shapes to work transparently.
    let file_relics: &[String] = if !file.relics.is_empty() {
        &file.relics
    } else if let Some(p) = file.player.as_ref() {
        &p.relics
    } else {
        &[]
    };
    if !file_relics.is_empty() {
        let cli_relic_set =
            !args.relic.is_empty() || args.relic_preset.is_some() || args.relic_file.is_some();
        if !cli_relic_set {
            args.relic = file_relics.to_vec();
        }
    }

    let file_potions: &[String] = if !file.potions.is_empty() {
        &file.potions
    } else if let Some(p) = file.player.as_ref() {
        &p.potions
    } else {
        &[]
    };
    if !file_potions.is_empty() {
        let cli_potion_set =
            !args.potion.is_empty() || args.potion_preset.is_some() || args.potion_file.is_some();
        if !cli_potion_set {
            args.potion = file_potions.to_vec();
        }
    }

    Ok(())
}

/// Apply `--random-all` and per-input `random` substitutions to
/// `args` in place. Bead `sts2-rs-c9s5` (cliext F3).
///
/// Precedence (highest → lowest): explicit CLI per-flag values >
/// `--random-all` (only fills inputs the user did not supply) >
/// scenario-file values > defaults. After this returns, each random
/// preset string has been replaced by concrete inline values so the
/// downstream `resolve_*` paths see a normal explicit-input shape.
fn apply_random_flags(args: &mut CombatArgs, matches: &ArgMatches) -> Result<(), String> {
    // INVARIANT: `encounter` uses clap's `was_supplied` (i.e.
    // `ValueSource::CommandLine`) because it has `default_value =
    // "nibbit"` — field-value inspection cannot distinguish defaulted
    // from explicit. Deck/relic/potion have no `default_value`, so
    // `None`/empty-vec inspection is reliable. If a `default_value`
    // is ever added to `--deck-preset` / `--relic-preset` /
    // `--potion-preset`, this asymmetry will silently break and you
    // must switch the affected input to `was_supplied(matches, "<flag>")`.
    let cli_deck_set =
        args.deck.is_some() || args.deck_file.is_some() || args.deck_preset.is_some();
    let cli_relic_set =
        !args.relic.is_empty() || args.relic_preset.is_some() || args.relic_file.is_some();
    let cli_potion_set =
        !args.potion.is_empty() || args.potion_preset.is_some() || args.potion_file.is_some();
    let cli_encounter_set = was_supplied(matches, "encounter");

    if args.random_all {
        if !cli_deck_set {
            args.deck_preset = Some("random".to_string());
        }
        if !cli_relic_set {
            args.relic_preset = Some("random".to_string());
        }
        if !cli_potion_set {
            args.potion_preset = Some("random".to_string());
        }
        if !cli_encounter_set {
            args.encounter = "random".to_string();
        }
    }

    if args.deck_preset.as_deref() == Some("random") {
        // Resolve the random-deck character: explicit `--random-deck-character`
        // wins; otherwise pull a class from a SALT_CLASS fork so `--seed`
        // alone reproduces the scenario. Cliext F4 (bead sts2-rs-9yoh).
        let class = match args.random_deck_character.as_deref() {
            Some(tag) => sts2_content::names::PlayerClassHint::from_tag(tag).ok_or_else(|| {
                format!(
                    "--random-deck-character: unknown class '{tag}' \
                     (supported: ironclad, silent)"
                )
            })?,
            None => random_mod::random_class(args.seed),
        };
        // Resolve the size: explicit `--random-deck-size` wins, default
        // 10. Bounded 1..=200 to keep CLI memory + JSONL output sane.
        let size = match args.random_deck_size {
            Some(n) if (1..=200).contains(&n) => n,
            Some(n) => {
                return Err(format!(
                    "--random-deck-size: {n} out of range (must be 1..=200)"
                ))
            }
            None => random_mod::DEFAULT_RANDOM_DECK_SIZE,
        };
        let deck = random_mod::random_deck(args.seed, size, class);
        let tokens: Vec<String> = deck.iter().map(|d| format!("{:?}", d.spec.id)).collect();
        args.deck = Some(tokens.join(","));
        args.deck_preset = None;
        // Keep `--player` aligned with the random-deck class unless the
        // user pinned `--player` explicitly. Otherwise the scenario
        // banner (and per-class HP/starter-relic seeding) would diverge
        // from the deck the player actually drew. Cliext F4.
        if !was_supplied(matches, "player") {
            // PlayerClass currently only supports Ironclad/Silent in
            // sts2-core; Defect cards can populate a deck but the
            // player class stays whatever --player was set to. Skip
            // realignment when class falls outside the wired set.
            match class {
                sts2_content::names::PlayerClassHint::Ironclad => {
                    args.player = crate::cli::combat::CliPlayerClass::Ironclad;
                }
                sts2_content::names::PlayerClassHint::Silent => {
                    args.player = crate::cli::combat::CliPlayerClass::Silent;
                }
                sts2_content::names::PlayerClassHint::Defect => {
                    // Defect player class not wired in sts2-core yet —
                    // user gets a Defect deck on an Ironclad/Silent HP
                    // pool. Surface this once so it's not silent.
                    eprintln!(
                        "note: --random-deck-character defect picks Defect cards but \
                         leaves --player at the default; PlayerClass::Defect is not \
                         yet wired in sts2-core",
                    );
                }
            }
        }
    }

    if args.relic_preset.as_deref() == Some("random") {
        let relics = random_mod::random_relics(args.seed);
        args.relic = relics.iter().map(|r| format!("{r:?}")).collect();
        args.relic_preset = None;
    }

    if args.potion_preset.as_deref() == Some("random") {
        let potions = random_mod::random_potions(args.seed);
        args.potion = potions.iter().map(|p| format!("{p:?}")).collect();
        args.potion_preset = None;
    }

    if args.encounter == "random" {
        args.encounter = random_mod::random_encounter(args.seed);
    }
    Ok(())
}

fn run_combat(mut args: CombatArgs, matches: &ArgMatches) -> ExitCode {
    // ── Scenario-file merge (bead sts2-rs-g2ex / cliext D1) ────────
    // Precedence: CLI > scenario file > defaults. We materialize the
    // file (if any), then for each scalar flag check the matches'
    // `value_source` — if it's `CommandLine` the CLI value wins,
    // otherwise the file's value seeds `args` in place.
    let scenario_file = match args.scenario.as_deref() {
        Some(path) => match load_scenario_file(path) {
            Ok(f) => Some(f),
            Err(e) => {
                eprintln!("scenario error: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let scenario_monsters: Option<Vec<crate::sim::scenario::encounter::MonsterSpec>> =
        if let Some(f) = scenario_file.as_ref() {
            match lower_monsters(&f.monsters) {
                Ok(v) => Some(v),
                Err(e) => {
                    eprintln!("scenario error: {e}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            None
        };
    if let Some(f) = scenario_file.as_ref() {
        if let Err(code) = merge_scenario_into_args(&mut args, f, matches) {
            return code;
        }
    }

    // ── --random-all + per-input `random` (cliext F3, sts2-rs-c9s5) ──
    // Precedence: explicit CLI per-flag values > --random-all > scenario
    // file > defaults. Each random draw consumes an independent fork of
    // the scenario RNG so `--seed` reproduces the same composite scenario.
    if let Err(e) = apply_random_flags(&mut args, matches) {
        eprintln!("error: {e}");
        return ExitCode::from(2);
    }

    let jsonl = match args.jsonl.as_deref() {
        Some(path) => match JsonlWriter::open(path) {
            Ok(w) => Some(w),
            Err(e) => {
                eprintln!("--jsonl open '{}' failed: {e}", path.display());
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };

    // Resolve starter potions (cliext C4, sts2-rs-bzgs). Done before
    // the banner so the resolved belt prints in the same line. A
    // dedicated RNG (seeded off args.seed) drives the random-3 preset
    // so --seed reproduces the same draw; the main combat RNG is built
    // independently inside build_starter_state to keep monster-HP
    // rolls untouched.
    let potion_input = if !args.potion.is_empty() {
        PotionInput::Inline(args.potion.clone())
    } else if let Some(name) = args.potion_preset.as_deref() {
        PotionInput::Preset(name.to_string())
    } else if let Some(path) = args.potion_file.as_deref() {
        PotionInput::File(path.to_path_buf())
    } else {
        PotionInput::None
    };
    let mut potion_rng = sts2_core::dotnet_rng::Rng::new(args.seed);
    let resolved_potions = match resolve_potions(&potion_input, &mut potion_rng) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    // Monster list source: a scenario file's `[[monsters]]` (D1) takes
    // precedence; otherwise resolve the encounter alias (B2). The
    // encounter RNG is seeded off args.seed so future randomized
    // aliases reproduce. The main combat RNG used inside
    // build_starter_state_from_monsters is independent (also seeded
    // off args.seed), so monster-HP rolls keep their pre-B2 behavior
    // for the 9 deterministic v1 aliases.
    let resolved_monsters = if let Some(specs) = scenario_monsters {
        specs
    } else if let Some(inline) = args.monsters.as_deref() {
        // Cliext H2: --monsters wins over --encounter. If both were
        // explicitly supplied on the command line, error rather than
        // silently dropping one — the precedence is clear but the
        // user almost certainly meant only one.
        if was_supplied(matches, "encounter") {
            eprintln!(
                "error: --monsters and --encounter are mutually exclusive when both \
                 are passed explicitly (got --encounter '{}' and --monsters '{}')",
                args.encounter, inline,
            );
            return ExitCode::FAILURE;
        }
        match parse_inline_monsters(inline) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        let mut enc_rng = sts2_core::dotnet_rng::Rng::new(args.seed);
        match resolve_encounter(&args.encounter, &mut enc_rng) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    };

    // Resolve deck input (bead sts2-rs-63bi / C2). Hoisted before the
    // banner so `--print-scenario` (E1) can emit the resolved deck
    // tokens in its TOML dump.
    let deck_input: Option<DeckInput> = match (&args.deck, &args.deck_file, &args.deck_preset) {
        (Some(s), None, None) => Some(DeckInput::Inline(s.clone())),
        (None, Some(p), None) => Some(DeckInput::File(p.clone())),
        (None, None, Some(p)) => Some(DeckInput::Preset(p.clone())),
        (None, None, None) => None,
        _ => unreachable!("clap deck_input group enforces mutual exclusion"),
    };
    let resolved_deck = if let Some(input) = deck_input {
        match resolve_deck(&input, args.player.into()) {
            Ok(d) => Some(d),
            Err(e) => {
                eprintln!("deck error: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };

    // Resolve relic input (bead sts2-rs-q3tv / C3) — also hoisted for
    // the print-scenario dump.
    let relic_input = if !args.relic.is_empty() {
        RelicInput::Inline(args.relic.clone())
    } else if let Some(preset) = args.relic_preset.as_ref() {
        RelicInput::Preset(preset.clone())
    } else if let Some(path) = args.relic_file.as_ref() {
        RelicInput::File(path.clone())
    } else {
        RelicInput::None
    };
    let resolved_relics = match resolve_relics(&relic_input) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    // ── --print-scenario (cliext E1, bead sts2-rs-92sq) ────────────
    // Dump the fully-resolved scenario (monsters + deck + relics +
    // potions + class/hp) as a TOML block that round-trips through
    // `--scenario <file>`, then exit 0 without running combat. Done
    // BEFORE the banner so the dump is the only thing on stdout.
    if args.print_scenario {
        let mut stdout = io::stdout().lock();
        let deck_tokens: Vec<String> = resolved_deck
            .as_ref()
            .map(|deck| {
                deck.iter()
                    .map(|d| {
                        let pascal = format!("{:?}", d.spec.id);
                        if d.is_upgraded() {
                            format!("{pascal}+")
                        } else {
                            pascal
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let relic_names: Vec<String> = resolved_relics.iter().map(|r| format!("{r:?}")).collect();
        let potion_names: Vec<String> = resolved_potions.iter().map(|p| format!("{p:?}")).collect();
        // `offset_phase` was a Sentry-only field removed in
        // bead `sts2-rs-v1u5`. The TOML renderer still emits the key
        // for back-compat with existing scenario files; we always
        // write `false` here.
        let monster_triples: Vec<(String, bool, Option<u32>)> = resolved_monsters
            .iter()
            .map(|m| (format!("{:?}", m.kind), false, m.hp_override))
            .collect();
        let toml = render_scenario_toml(
            args.player.into(),
            args.hp,
            &deck_tokens,
            &relic_names,
            &potion_names,
            &monster_triples,
        );
        if let Err(e) = stdout.write_all(toml.as_bytes()) {
            eprintln!("write error: {e}");
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }

    let stdout = io::stdout().lock();
    let mut out = stdout;
    // When a scenario file owns the monster list, surface it in the
    // banner instead of the (now-misleading) defaulted `--encounter`.
    let encounter_field = if let Some(path) = args.scenario.as_ref() {
        format!("scenario={}", path.display())
    } else {
        format!("encounter={}", args.encounter)
    };
    if let Err(e) = writeln!(
        out,
        "sts2-cli v{} — seed={} {} ({}) player={:?} agent={:?} {}",
        env!("CARGO_PKG_VERSION"),
        args.seed,
        encounter_field,
        summarize_monsters(&resolved_monsters),
        args.player,
        args.agent,
        format_potions_banner(&resolved_potions),
    ) {
        eprintln!("write error: {e}");
        return ExitCode::FAILURE;
    }

    // Banner: print the resolved deck summary now that deck resolution
    // is hoisted before --print-scenario.
    if let Some(resolved) = resolved_deck.as_ref() {
        if let Err(e) = writeln!(out, "{}", format_deck_summary(resolved)) {
            eprintln!("write error: {e}");
            return ExitCode::FAILURE;
        }
    }

    if !resolved_relics.is_empty() {
        if let Err(e) = writeln!(
            out,
            "  relics: {}",
            format_relics_for_banner(&resolved_relics)
        ) {
            eprintln!("write error: {e}");
            return ExitCode::FAILURE;
        }
    }

    let mut state = build_starter_state_from_monsters(
        args.seed,
        resolved_monsters,
        args.player.into(),
        args.hp,
        args.max_hp,
        resolved_deck,
    );
    // Seed the player's potion belt (cliext C4). The legacy
    // state.potions arena stays empty (P7-8 sts2-rs-h9u will pair
    // slot + arena); legal_actions keys off state.player.potions for
    // the DrinkPotion emission so this is sufficient for the REPL.
    for kind in &resolved_potions {
        state.player.potions.push(*kind);
    }
    // Content-side relic dispatch must be installed before start_combat
    // so listeners can resolve via `relic_effects::dispatch`.
    sts2_content::relics::dispatch::ensure_installed();
    // Content-side potion dispatch must be installed for DrinkPotion
    // to resolve real potion bodies (sts2-rs-mvsu) outside the test
    // contexts that already call ensure_installed in fixtures.
    sts2_content::potions::dispatch::ensure_installed();
    // Schema=2 `[player.relic_counters]` injects non-zero starting
    // counters (e.g. JossPaper's CardsExhausted). Empty map under
    // schema=1 or when the section is omitted.
    let relic_counters = scenario_file
        .as_ref()
        .and_then(|f| f.player.as_ref())
        .map(|p| p.relic_counters.clone())
        .unwrap_or_default();
    if let Err(e) = seed_relics_with_counters(&mut state, &resolved_relics, &relic_counters) {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }

    match args.agent {
        AgentKind::Human => {
            let threads = if args.mcts_threads == 0 {
                u32::try_from(rayon::current_num_threads()).unwrap_or(1)
            } else {
                args.mcts_threads
            };
            let hint = crate::agent::human::HintConfig {
                auto_sims: args.mcts_hint_default,
                c_puct: args
                    .mcts_c_puct
                    .unwrap_or(sts2_mcts::MctsConfig::default().c_puct),
                rollout_depth: args
                    .mcts_rollout_depth
                    .unwrap_or(sts2_mcts::MctsConfig::default().max_rollout_depth),
                threads,
            };
            run_human(state, out, jsonl, args.echo_input, !args.no_prompt, hint)
        }
        AgentKind::Random => run_random(state, out, jsonl, args.seed, args.steps),
        AgentKind::Mcts => {
            let cfg = MctsAgentConfig {
                sims: args.mcts_hint_default.unwrap_or(1000),
                c_puct: args
                    .mcts_c_puct
                    .unwrap_or(sts2_mcts::MctsConfig::default().c_puct),
                rollout_depth: args
                    .mcts_rollout_depth
                    .unwrap_or(sts2_mcts::MctsConfig::default().max_rollout_depth),
                seed: args.seed,
                verbose: jsonl.is_none(),
                tree_reuse: args.mcts_tree_reuse,
                uniform_rollout: args.mcts_uniform_rollout,
            };
            run_mcts(state, out, jsonl, cfg, args.seed)
        }
    }
}

fn run_mcts<W: Write>(
    state: sts2_core::CombatState,
    out: W,
    jsonl: Option<JsonlWriter>,
    cfg: MctsAgentConfig,
    seed: u64,
) -> ExitCode {
    let agent = MctsAgent::new(cfg);
    let ctx = SimulatorContext {
        print_prompts: false,
        print_input: false,
        show_action_menu: false,
        ..SimulatorContext::default()
    };
    let mut sim = ConsoleSimulator::new(state, out, agent, ctx);
    if let Some(w) = jsonl {
        sim = sim.with_jsonl(w);
    }
    let t0 = std::time::Instant::now();
    let play_result = sim.play();
    let wall_ms = t0.elapsed().as_millis();
    match play_result {
        Ok(()) => {
            let outcome = if sim.state.is_over {
                if sim.state.player_won {
                    "win"
                } else {
                    "loss"
                }
            } else {
                "incomplete"
            };
            // Single-line JSON summary on the final stdout line so outer
            // harnesses can `json.loads` it without text-scraping. The
            // tag `"_": "mcts_summary"` makes the line identifiable when
            // mixed with other stdout chatter (banner, REPL lines, etc.).
            // `final_hp` clamps negatives to 0 defensively.
            let final_hp = sim.state.player.current_hp.max(0);
            // Sum enemy HP across all alive enemy creatures at sim end.
            // On `win` these will be 0; on `loss` they record how much
            // damage the player had managed to deal before dying — a
            // proxy for "how close" the loss was. enemy_max_hp is the
            // sum of starting max HP for all enemies in the encounter,
            // captured once at the start of combat (read from the
            // current creature arena since max_hp is immutable once
            // monsters are spawned).
            let mut enemy_hp_remaining: i32 = 0;
            let mut enemy_max_hp: i32 = 0;
            for uid in &sim.state.enemy_uids {
                if let Some(c) = sim.state.creatures.iter().find(|c| c.id == uid.0) {
                    enemy_hp_remaining += c.current_hp.max(0);
                    enemy_max_hp += c.max_hp;
                }
            }
            let summary = serde_json::json!({
                "_": "mcts_summary",
                "seed": seed,
                "outcome": outcome,
                "final_hp": final_hp,
                "max_hp": sim.state.player.max_hp,
                "enemy_hp_remaining": enemy_hp_remaining,
                "enemy_max_hp": enemy_max_hp,
                "turns": sim.state.round,
                "sims": cfg.sims,
                "depth": cfg.rollout_depth,
                "tree_reuse": cfg.tree_reuse,
                "wall_ms": wall_ms as u64,
            });
            let _ = writeln!(sim.out, "{summary}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("simulator error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_random<W: Write>(
    state: sts2_core::CombatState,
    out: W,
    jsonl: Option<JsonlWriter>,
    seed: u64,
    steps: u32,
) -> ExitCode {
    let agent = RandomAgent::with_step_cap(seed, steps);
    let ctx = SimulatorContext {
        print_prompts: false,
        print_input: false,
        show_action_menu: false,
        ..SimulatorContext::default()
    };
    let mut sim = ConsoleSimulator::new(state, out, agent, ctx);
    if let Some(w) = jsonl {
        sim = sim.with_jsonl(w);
    }
    match sim.play() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("simulator error: {e}");
            ExitCode::FAILURE
        }
    }
}

// `agent` owns the stdin lock + line buffer and must live for the entire
// simulation; early-drop would defeat the purpose. False-positive lint.
#[allow(clippy::significant_drop_tightening)]
fn run_human<W: Write>(
    state: sts2_core::CombatState,
    out: W,
    jsonl: Option<JsonlWriter>,
    echo_input: bool,
    print_prompts: bool,
    hint: crate::agent::human::HintConfig,
) -> ExitCode {
    let stdin = io::stdin().lock();
    let mut agent = HumanAgent::new(BufReader::new(stdin));
    agent.echo_input = echo_input;
    agent.print_prompts = print_prompts;
    agent.hint = hint;
    // --jsonl machine-readable output suppresses the auto-prompt block so
    // the jsonl file is the only structured surface and stdout stays terse.
    let show_action_menu = jsonl.is_none();
    agent.show_action_menu = show_action_menu;
    let ctx = SimulatorContext {
        print_prompts,
        print_input: echo_input,
        show_action_menu,
        ..SimulatorContext::default()
    };

    let mut sim = ConsoleSimulator::new(state, out, agent, ctx);
    if let Some(w) = jsonl {
        sim = sim.with_jsonl(w);
    }
    match sim.play() {
        Ok(()) => {
            if sim.ctx.failed_test {
                ExitCode::from(2) // distinct exit code for assertion failure
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("simulator error: {e}");
            ExitCode::FAILURE
        }
    }
}
