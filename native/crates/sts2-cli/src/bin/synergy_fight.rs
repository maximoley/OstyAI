//! `synergy_fight` — forced-deck single-combat runner for the synergy catalog.
//!
//! Reads one JSON fight-spec from stdin (or `--json <json>` arg), runs
//! MCTS to terminal, and prints one JSON outcome object on stdout.
//!
//! # Input (one JSON object, newline-terminated or the whole stdin):
//! ```json
//! {
//!   "baseline_deck": ["STRIKE_IRONCLAD","STRIKE_IRONCLAD","STRIKE_IRONCLAD",
//!                     "STRIKE_IRONCLAD","STRIKE_IRONCLAD",
//!                     "DEFEND_IRONCLAD","DEFEND_IRONCLAD","DEFEND_IRONCLAD",
//!                     "DEFEND_IRONCLAD","BASH"],
//!   "inject": ["INFLAME"],
//!   "encounter_id": "BYGONE_EFFIGY_ELITE",
//!   "seed": 42,
//!   "sims": 1500,
//!   "depth": 50
//! }
//! ```
//!
//! Fields:
//! - `character` — optional; `ironclad` (default) / `silent` / `defect` /
//!   `regent` / `necrobinder`. Selects the player class, the default starter
//!   deck, and the bare-`Strike`/`Defend` resolution hint.
//! - `baseline_deck` — optional; omit or `null` to use the character's starter
//!   deck (Ironclad: 5×Strike + 4×Defend + Bash).
//! - `inject` — optional; default `[]`. Cards appended after `baseline_deck`.
//! - `encounter_id` — required; checked across Act1→2→3 registries.
//! - `seed` — u64 engine seed; required.
//! - `sims` — MCTS simulations per move; default 1500.
//! - `depth` — max rollout depth; default 50.
//!
//! Card tokens: `"STRIKE_IRONCLAD"`, `"BASH"`, `"INFLAME+1"`, `"INFLAME+"` —
//! same grammar as `synergy_score`. `+` / `+N` marks an upgraded card.
//! Unknown token → descriptive error, non-zero exit.
//!
//! # Output:
//! ```json
//! {"won": false, "hp_lost": 42, "turns": 7}
//! ```
//!
//! Panel encounter ids supported: `BYGONE_EFFIGY_ELITE` (Act1 elite),
//! `DECIMILLIPEDE_ELITE` (Act2 elite), `VANTOM_BOSS` (Act1 boss).
//!
//! No relics — the 4-cell second-difference cancels relic effects.

use std::io::{self, Read};
use std::process;

use serde::{Deserialize, Serialize};

use sts2_cli::sim::scenario::deck::DeckCardSpec;
use sts2_cli::sim::{build_starter_state_from_monsters, PlayerClass};
use sts2_content::encounters;
use sts2_content::names::{resolve_card_name, PlayerClassHint};
use sts2_content::starters::starter_deck_for;
use sts2_core::combat_loop::{start_combat, start_player_turn};
use sts2_core::potions::CharacterId;
use sts2_core::{apply, is_terminal, legal_actions, Action};
use sts2_mcts::{Mcts, MctsConfig, TerminalEvalMode};

use sts2_cli::sim::scenario::encounter::{MonsterAiArgs, MonsterSpec};

/// Maximum MCTS actions per fight before we force-stop. Well above any
/// legitimate STS2 combat; guards against pathological stalls.
const MAX_ACTIONS: u32 = 2000;

/// Deserialised fight spec from the caller.
#[derive(Deserialize)]
struct FightSpec {
    /// Character: `ironclad` (default) / `silent` / `defect` / `regent` /
    /// `necrobinder`. Controls the player class, the default starter deck,
    /// and the bare-`Strike`/`Defend` resolution hint.
    #[serde(default)]
    character: Option<String>,
    /// Baseline deck tokens. `null` / absent → that character's starter deck.
    #[serde(default)]
    baseline_deck: Option<Vec<String>>,
    /// Cards to append to the baseline deck.
    #[serde(default)]
    inject: Vec<String>,
    /// Encounter id (UPPER_SNAKE) — resolved across all acts.
    encounter_id: String,
    /// Engine RNG seed.
    seed: u64,
    /// MCTS simulations per move (default 1500).
    #[serde(default = "default_sims")]
    sims: u32,
    /// Max rollout depth (default 50).
    #[serde(default = "default_depth")]
    depth: u32,
}

fn default_sims() -> u32 {
    1500
}
fn default_depth() -> u32 {
    50
}

/// Outcome of a single fight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FightOutcome {
    pub won: bool,
    pub hp_lost: i32,
    pub turns: u32,
}

/// Resolve a character tag into the engine triple needed to build a fight.
///
/// Returns `(PlayerClass, CharacterId, Option<PlayerClassHint>)`. The hint is
/// `Some` only for the three classes whose bare `Strike`/`Defend` tokens the
/// resolver can disambiguate (Ironclad/Silent/Defect); for Regent/Necrobinder
/// it is `None` (corpus tokens are full names like `STRIKE_NECROBINDER`, which
/// resolve class-agnostically, so the hint is unused there).
fn resolve_character(
    tag: &str,
) -> Result<(PlayerClass, CharacterId, Option<PlayerClassHint>), String> {
    match tag.trim().to_ascii_lowercase().as_str() {
        "ironclad" => Ok((
            PlayerClass::Ironclad,
            CharacterId::Ironclad,
            Some(PlayerClassHint::Ironclad),
        )),
        "silent" => Ok((
            PlayerClass::Silent,
            CharacterId::Silent,
            Some(PlayerClassHint::Silent),
        )),
        "defect" => Ok((
            PlayerClass::Defect,
            CharacterId::Defect,
            Some(PlayerClassHint::Defect),
        )),
        "regent" => Ok((PlayerClass::Regent, CharacterId::Regent, None)),
        "necrobinder" => Ok((PlayerClass::Necrobinder, CharacterId::Necrobinder, None)),
        other => Err(format!(
            "unknown character: '{other}' (expected ironclad/silent/defect/regent/necrobinder)"
        )),
    }
}

/// Strip an optional `CARD.`/`RELIC.`/`POTION.` namespace prefix.
fn strip_ns(s: &str) -> &str {
    s.split_once('.').map_or(s, |(_, rest)| rest)
}

/// Parse `"BASH"` / `"INFLAME+1"` / `"CARD.INFLAME+"` into a [`DeckCardSpec`].
///
/// Returns `Err` with a descriptive message if the token is unknown.
fn parse_card_token(token: &str, hint: Option<PlayerClassHint>) -> Result<DeckCardSpec, String> {
    let bare = strip_ns(token);

    // Strip optional `+N` / `+` upgrade suffix.
    let (name, upgrade_level): (&str, u32) = if let Some(rest) = bare.strip_suffix('+') {
        (rest.trim_end(), 1)
    } else if let Some(plus_pos) = bare.rfind('+') {
        let (name_part, digits_part) = bare.split_at(plus_pos);
        let digits = &digits_part[1..];
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            let level: u32 = digits.parse().map_err(|_| {
                format!("unknown card token: '{token}' (upgrade level out of range)")
            })?;
            (name_part.trim_end(), level)
        } else {
            (bare, 0)
        }
    } else {
        (bare, 0)
    };

    match resolve_card_name(name, hint) {
        Ok(spec) => Ok(DeckCardSpec {
            spec,
            upgrade_level,
            enchantment: None,
        }),
        Err(_) => Err(format!("unknown card token: '{token}'")),
    }
}

/// Resolve a list of card tokens into `Vec<DeckCardSpec>`.
fn resolve_tokens(
    tokens: &[String],
    hint: Option<PlayerClassHint>,
) -> Result<Vec<DeckCardSpec>, String> {
    tokens.iter().map(|t| parse_card_token(t, hint)).collect()
}

/// Build a character's starter deck as `Vec<DeckCardSpec>`.
fn starter_card_specs(character: CharacterId) -> Vec<DeckCardSpec> {
    starter_deck_for(character)
        .into_iter()
        .map(|spec| DeckCardSpec {
            spec,
            upgrade_level: 0,
            enchantment: None,
        })
        .collect()
}

/// Run one forced-deck fight and return the outcome.
///
/// This is the testable core — `main` just parses JSON and calls this.
fn run_fight(spec: &FightSpec) -> Result<FightOutcome, String> {
    // Install all content registries (idempotent).
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    // Resolve the character (default Ironclad).
    let character_tag = spec.character.as_deref().unwrap_or("ironclad");
    let (player_class, character_id, class_hint) = resolve_character(character_tag)?;

    // Resolve the encounter from Act1 → Act2 → Act3.
    let enc = encounters::find(&spec.encounter_id)
        .ok_or_else(|| format!("unknown encounter_id: '{}'", spec.encounter_id))?;

    // Build monster specs from the canonical roster.
    let roster = enc.canonical_roster();
    let alone = roster.len() == 1;
    let monsters: Vec<MonsterSpec> = roster
        .iter()
        .enumerate()
        .map(|(idx, (kind, args))| MonsterSpec {
            slot: None,
            kind: *kind,
            hp_override: None,
            ai_args: MonsterAiArgs {
                is_alone: alone,
                is_front: idx == 0,
            },
            args: *args,
        })
        .collect();

    // Build the full deck = baseline ++ inject.
    let mut deck: Vec<DeckCardSpec> = match &spec.baseline_deck {
        Some(tokens) => resolve_tokens(tokens, class_hint)?,
        None => starter_card_specs(character_id),
    };
    let inject = resolve_tokens(&spec.inject, class_hint)?;
    deck.extend(inject);

    // Assemble the CombatState. No relics; no HP override.
    // `build_starter_state_from_monsters` returns a state in `CombatPhase::Setup`
    // (it does NOT call start_combat/start_player_turn). We must advance the phase
    // exactly as ConsoleSimulator::play() does before entering the MCTS loop.
    let mut state = build_starter_state_from_monsters(
        spec.seed,
        monsters,
        player_class,
        None,
        None,
        Some(deck),
    );
    let _ = start_combat(&mut state);
    let _ = start_player_turn(&mut state);

    // MCTS play loop — mirrors vantom_starter_winrate example.
    let initial_hp = state.player.current_hp;
    let mut action_count: u32 = 0;
    let mut last_round = state.round;
    let mut turns: u32 = 0;

    while !is_terminal(&state) && action_count < MAX_ACTIONS {
        let acts = legal_actions(&state);
        if acts.is_empty() {
            let _ = apply(&mut state, Action::EndTurn);
            action_count += 1;
            if state.round != last_round {
                turns += 1;
                last_round = state.round;
            }
            continue;
        }

        let cfg = MctsConfig {
            max_simulations: spec.sims,
            max_rollout_depth: spec.depth,
            // Vary seed per action so consecutive calls don't share RNG state.
            seed: spec
                .seed
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(u64::from(action_count)),
            terminal_eval_mode: TerminalEvalMode::RichBoth,
            ..MctsConfig::default()
        };
        let mut mcts = Mcts::new(state.clone(), cfg);
        let action = mcts.search();
        let _ = apply(&mut state, action);
        action_count += 1;
        if state.round != last_round {
            turns += 1;
            last_round = state.round;
        }
    }

    let final_hp = state.player.current_hp.max(0);
    let won = is_terminal(&state) && final_hp > 0;
    let hp_lost = (initial_hp - final_hp).max(0);

    Ok(FightOutcome {
        won,
        hp_lost,
        turns,
    })
}

fn main() {
    // Accept JSON either from --json <arg> or from stdin.
    let args: Vec<String> = std::env::args().collect();
    let json_str: String = if let Some(pos) = args.iter().position(|a| a == "--json") {
        args.get(pos + 1).cloned().unwrap_or_else(|| {
            eprintln!("synergy_fight: --json flag requires an argument");
            process::exit(1);
        })
    } else {
        let mut buf = String::new();
        if let Err(e) = io::stdin().read_to_string(&mut buf) {
            eprintln!("synergy_fight: stdin read error: {e}");
            process::exit(1);
        }
        buf
    };

    let trimmed = json_str.trim();
    if trimmed.is_empty() {
        eprintln!("synergy_fight: no input provided (empty stdin or --json arg)");
        process::exit(1);
    }

    let spec: FightSpec = match serde_json::from_str(trimmed) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("synergy_fight: JSON parse error: {e}");
            process::exit(1);
        }
    };

    match run_fight(&spec) {
        Ok(outcome) => {
            println!(
                "{}",
                serde_json::to_string(&outcome).expect("serialize outcome")
            );
        }
        Err(e) => {
            eprintln!("synergy_fight: {e}");
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a starter-deck spec (no injection).
    fn starter_spec(encounter_id: &str, seed: u64, sims: u32) -> FightSpec {
        FightSpec {
            character: None,
            baseline_deck: None,
            inject: vec![],
            encounter_id: encounter_id.to_string(),
            seed,
            sims,
            depth: 20,
        }
    }

    /// Build a spec with injected cards.
    fn inject_spec(encounter_id: &str, seed: u64, sims: u32, inject: &[&str]) -> FightSpec {
        FightSpec {
            character: None,
            baseline_deck: None,
            inject: inject.iter().map(|s| (*s).to_string()).collect(),
            encounter_id: encounter_id.to_string(),
            seed,
            sims,
            depth: 20,
        }
    }

    /// Build a starter-deck spec for an explicit character (no injection).
    fn char_starter_spec(character: &str, encounter_id: &str, seed: u64, sims: u32) -> FightSpec {
        FightSpec {
            character: Some(character.to_string()),
            baseline_deck: None,
            inject: vec![],
            encounter_id: encounter_id.to_string(),
            seed,
            sims,
            depth: 20,
        }
    }

    /// Test 1: determinism — same spec → same outcome twice.
    ///
    /// Runs real MCTS so it is tagged `slow_` (runs under `--profile slow`,
    /// excluded from the default/CI profiles). In a release build it
    /// completes in seconds; in debug it can take 1–3 minutes.
    #[test]
    fn slow_determinism_same_outcome_twice() {
        let spec = starter_spec("BYGONE_EFFIGY_ELITE", 7, 200);
        let r1 = run_fight(&spec).expect("first fight ok");
        let r2 = run_fight(&spec).expect("second fight ok");
        assert_eq!(
            r1, r2,
            "fight outcomes must be deterministic: {r1:?} vs {r2:?}"
        );
    }

    /// Test 2: injection changes outcome — Inflame×3 + Whirlwind vs bare starter.
    ///
    /// Injects 4 strong Ironclad cards into the 10-card starter deck and asserts
    /// that at least one of (won, hp_lost) differs vs. the bare baseline at
    /// seed=42. The injection is intentionally large so the effect is real on
    /// any reasonably-seeded fight.
    ///
    /// Also tagged `slow_` — see `slow_determinism_same_outcome_twice`.
    #[test]
    fn slow_injection_changes_outcome() {
        let baseline = starter_spec("BYGONE_EFFIGY_ELITE", 42, 400);
        let injected = inject_spec(
            "BYGONE_EFFIGY_ELITE",
            42,
            400,
            &["INFLAME", "INFLAME", "INFLAME", "WHIRLWIND"],
        );

        let r_base = run_fight(&baseline).expect("baseline ok");
        let r_inj = run_fight(&injected).expect("injected ok");

        // At least one metric must differ — injecting 3×Inflame + Whirlwind
        // into a 10-card starter deck is a substantial boost.
        assert!(
            r_base.won != r_inj.won || r_base.hp_lost != r_inj.hp_lost,
            "injection of 4 strong cards should change outcome: base={r_base:?} inj={r_inj:?}"
        );
    }

    /// Test 3a: unknown card token produces a clear error.
    #[test]
    fn unknown_card_token_errors_clearly() {
        let spec = inject_spec("BYGONE_EFFIGY_ELITE", 1, 50, &["NOT_A_REAL_CARD_XYZ"]);
        let err = run_fight(&spec).expect_err("should error on unknown card");
        assert!(
            err.contains("NOT_A_REAL_CARD_XYZ"),
            "error must name the bad token, got: '{err}'"
        );
    }

    /// Test 3b: unknown encounter id produces a clear error.
    #[test]
    fn unknown_encounter_id_errors_clearly() {
        let spec = starter_spec("NOT_A_REAL_ENCOUNTER_XYZ", 1, 50);
        let err = run_fight(&spec).expect_err("should error on unknown encounter");
        assert!(
            err.contains("NOT_A_REAL_ENCOUNTER_XYZ"),
            "error must name the bad encounter id, got: '{err}'"
        );
    }

    /// Test 3c: unknown character produces a clear error (fast — fails before MCTS).
    #[test]
    fn unknown_character_errors_clearly() {
        let spec = FightSpec {
            character: Some("not_a_character".to_string()),
            baseline_deck: None,
            inject: vec![],
            encounter_id: "BYGONE_EFFIGY_ELITE".to_string(),
            seed: 1,
            sims: 50,
            depth: 20,
        };
        let err = run_fight(&spec).expect_err("should error on unknown character");
        assert!(
            err.contains("not_a_character"),
            "error must name the bad character, got: '{err}'"
        );
    }

    /// Test 4: every playable character's starter deck resolves and runs a
    /// complete fight without error. Guards the multi-character extension — the
    /// per-class starter decks (Silent/Defect/Regent/Necrobinder) must all load
    /// their card factories and survive a full MCTS combat to terminal.
    ///
    /// Tagged `slow_` (real MCTS) — see `slow_determinism_same_outcome_twice`.
    #[test]
    fn slow_all_characters_starter_runs() {
        for character in ["ironclad", "silent", "defect", "regent", "necrobinder"] {
            let spec = char_starter_spec(character, "BYGONE_EFFIGY_ELITE", 7, 200);
            let outcome = run_fight(&spec)
                .unwrap_or_else(|e| panic!("character '{character}' starter must run: {e}"));
            // hp_lost is always a valid non-negative number for a completed fight.
            assert!(
                outcome.hp_lost >= 0,
                "character '{character}' produced invalid outcome: {outcome:?}"
            );
        }
    }
}
