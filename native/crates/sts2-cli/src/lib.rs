// Bead `sts2-rs-a2fh`: the cli/sim/agent module tree was originally
// binary-private (`mod cli;` in main.rs). Re-exposing it via this lib
// target makes its `pub` types externally visible, which fires the
// workspace-level `missing_debug_implementations` warn lint on every
// `pub struct` that didn't bother. Adding `#[derive(Debug)]` to ~6
// pre-existing types would be a sweeping but mechanical change; this
// allow keeps the lib re-export surgical. The CLI binary's own copy
// still emits the same types in a binary context where the lint
// doesn't fire (no externally-reachable `pub`), so the workspace
// signal is preserved where it can act.
#![allow(missing_debug_implementations)]

//! `sts2-cli` library surface — bead `sts2-rs-a2fh`.
//!
//! The CLI binary at `src/main.rs` still owns the clap precedence
//! machinery (CLI flags > scenario file > defaults) plus the
//! interactive REPL. Everything below the merge layer — TOML
//! deserialization, deck/relic/potion resolution, and the
//! `(ScenarioFile, seed) → CombatState` materialization — lives
//! here so it can be reused from `sts2-corpus-gen`, the bench
//! harnesses, and any future tooling that needs a real
//! `CombatState` from a `.run-source` fixture without going
//! through `argv`.

/// Content hash of THIS crate's build inputs, stamped at build time by
/// `build.rs`. Compiled INTO the crate so a stale `sts2-cli` rlib carries its
/// old hash; one of the parts of `sts2_rs.src_hash()`. See
/// `sts2_core::SRC_HASH`.
pub const SRC_HASH: &str = env!("STS2_CLI_SRC_HASH");

pub mod agent;
pub mod cli;
pub mod replay;
pub mod sim;

use sts2_core::CombatState;

use crate::sim::scenario::deck::{resolve_deck, DeckCardSpec, DeckInput};
use crate::sim::scenario::potions::{resolve_potions, PotionInput};
use crate::sim::scenario::relics::{resolve_relics, seed_relics_with_counters, RelicInput};
use crate::sim::scenario::toml_file::{lower_monsters, parse_player_class, ScenarioFile};
use crate::sim::{build_starter_state_from_monsters_with_ascension, PlayerClass};

/// Why a `(ScenarioFile, seed) → CombatState` materialization failed.
///
/// Mirrors the failure surfaces of the underlying resolvers
/// (`resolve_deck` / `resolve_relics` / `resolve_potions` /
/// `lower_monsters` / `seed_relics_with_counters`) but flattened
/// into one error type so consumers can `?` against a single
/// `Result` rather than juggling five.
#[derive(Debug)]
pub enum ScenarioBuildError {
    /// `[player] class = "Foo"` did not match a known class.
    UnknownPlayerClass(String),
    /// `[player] deck = [...]` contained an unresolvable card token
    /// (e.g. a future schema-2 `Foo+N` shape `kbi1` hasn't landed).
    Deck(String),
    /// `relics = [...]` or `[player] relics = [...]` contained an
    /// unresolvable relic token; or counter seeding failed.
    Relics(String),
    /// `potions = [...]` or `[player] potions = [...]` contained an
    /// unresolvable potion token.
    Potions(String),
    /// `[[monsters]]` table parse failed at the content lookup layer
    /// (e.g. unknown `kind` token).
    Monsters(String),
    /// Scenario had no `[[monsters]]` block at all. The CLI fills
    /// this from `--encounter <preset>`; the library cannot because
    /// it has no preset registry without the CLI flag.
    NoMonsters,
}

impl std::fmt::Display for ScenarioBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownPlayerClass(s) => write!(f, "unknown player class: {s}"),
            Self::Deck(s) => write!(f, "deck resolution failed: {s}"),
            Self::Relics(s) => write!(f, "relic resolution failed: {s}"),
            Self::Potions(s) => write!(f, "potion resolution failed: {s}"),
            Self::Monsters(s) => write!(f, "monster lowering failed: {s}"),
            Self::NoMonsters => write!(f, "scenario has no [[monsters]] block"),
        }
    }
}

impl std::error::Error for ScenarioBuildError {}

/// Materialize a [`CombatState`] from a parsed [`ScenarioFile`] +
/// seed without touching clap. Mirrors the lower half of
/// `sts2-cli combat --scenario <toml>` — every input that the CLI's
/// arg-merge layer takes from the file (player class / HP / max-HP /
/// deck / relics / potions / monsters / relic counters) flows
/// through here. Anything the CLI takes from explicit flags
/// (encounter preset / random-all / agent / output formatting)
/// stays in `main.rs`.
///
/// `[player]` defaults follow `main.rs`'s "file value or built-in"
/// rule:
/// - `class` ⇒ falls back to [`PlayerClass::Ironclad`] (matches
///   the CLI's `--player ironclad` default). Defect/Regent/
///   Necrobinder also fall back to Ironclad until sts2-core wires
///   those classes (see `merge_scenario_into_args`).
/// - `hp` / `max_hp` ⇒ unset ⇒ engine's per-class starter HP
///   (see `build_state` in `scenario::starter`).
/// - `deck` ⇒ unset ⇒ class starter deck.
/// - `relics` / `potions` ⇒ unset ⇒ empty.
///
/// `seed` drives the RNG arms inside
/// `build_starter_state_from_monsters` (combat RNG + per-spawn HP
/// rolls) and `resolve_potions` (when the input is a random
/// preset — for inline-only fixtures the rng is consumed but
/// produces no spread). Use the same seed across fixtures to keep
/// the per-spawn HP rolls reproducible.
///
/// # Errors
///
/// Returns a [`ScenarioBuildError`] when any of the resolver layers
/// reject the file (unknown player class, unresolvable card token,
/// unknown relic, missing `[[monsters]]` block, etc.).
pub fn scenario_to_combat_state(
    scenario: &ScenarioFile,
    seed: u64,
) -> Result<CombatState, ScenarioBuildError> {
    scenario_to_combat_state_with_ascension(scenario, seed, 0)
}

/// Ascension-aware variant of [`scenario_to_combat_state`]. `ascension`
/// is passed EXPLICITLY by the caller — the loader does NOT read
/// `[player].ascension` from the fixture itself (see
/// `sim::scenario::toml_file` for why that field is informational-only
/// here); Task 4's evaluator reads the field from the fixture and
/// passes it through this parameter.
///
/// # Errors
///
/// Same failure surfaces as [`scenario_to_combat_state`].
pub fn scenario_to_combat_state_with_ascension(
    scenario: &ScenarioFile,
    seed: u64,
    ascension: u8,
) -> Result<CombatState, ScenarioBuildError> {
    // ── Player class + HP ──────────────────────────────────────
    let player_class =
        if let Some(class_str) = scenario.player.as_ref().and_then(|p| p.class.as_deref()) {
            let parsed = parse_player_class(class_str)
                .map_err(|e| ScenarioBuildError::UnknownPlayerClass(e.to_string()))?;
            // `main.rs` falls back to Ironclad for Defect/Regent/Necrobinder
            // until sts2-core wires those classes (see
            // `merge_scenario_into_args`). Replicate that here so
            // library-loaded states match CLI-loaded states for the same
            // fixture.
            match parsed {
                PlayerClass::Ironclad | PlayerClass::Silent => parsed,
                PlayerClass::Defect | PlayerClass::Regent | PlayerClass::Necrobinder => {
                    PlayerClass::Ironclad
                }
            }
        } else {
            PlayerClass::Ironclad
        };

    let hp_override = scenario
        .player
        .as_ref()
        .and_then(|p| p.hp)
        .map(|h| u32::try_from(h.max(0)).unwrap_or(0));
    let max_hp_override = scenario.player.as_ref().and_then(|p| p.max_hp);

    // ── Deck ───────────────────────────────────────────────────
    let resolved_deck: Option<Vec<DeckCardSpec>> = if let Some(player) = scenario.player.as_ref() {
        if player.deck.is_empty() {
            None
        } else {
            let inline = player.deck.join(",");
            let input = DeckInput::Inline(inline);
            Some(
                resolve_deck(&input, player_class)
                    .map_err(|e| ScenarioBuildError::Deck(e.to_string()))?,
            )
        }
    } else {
        None
    };

    // ── Relics (top-level OR nested under [player]) ────────────
    let file_relics: &[String] = if !scenario.relics.is_empty() {
        &scenario.relics
    } else if let Some(p) = scenario.player.as_ref() {
        &p.relics
    } else {
        &[]
    };
    let resolved_relics = if file_relics.is_empty() {
        Vec::new()
    } else {
        resolve_relics(&RelicInput::Inline(file_relics.to_vec()))
            .map_err(|e| ScenarioBuildError::Relics(e.to_string()))?
    };

    // ── Potions (top-level OR nested under [player]) ───────────
    let file_potions: &[String] = if !scenario.potions.is_empty() {
        &scenario.potions
    } else if let Some(p) = scenario.player.as_ref() {
        &p.potions
    } else {
        &[]
    };
    let resolved_potions = if file_potions.is_empty() {
        Vec::new()
    } else {
        let mut potion_rng = sts2_core::dotnet_rng::Rng::new(seed);
        resolve_potions(&PotionInput::Inline(file_potions.to_vec()), &mut potion_rng)
            .map_err(|e| ScenarioBuildError::Potions(e.to_string()))?
    };

    // ── Monsters ───────────────────────────────────────────────
    if scenario.monsters.is_empty() {
        return Err(ScenarioBuildError::NoMonsters);
    }
    let monsters = lower_monsters(&scenario.monsters)
        .map_err(|e| ScenarioBuildError::Monsters(e.to_string()))?;

    // ── Materialize ────────────────────────────────────────────
    let mut state = build_starter_state_from_monsters_with_ascension(
        seed,
        monsters,
        player_class,
        hp_override,
        max_hp_override,
        resolved_deck,
        ascension,
    );

    // Mirror `main.rs`: potions push directly onto the player belt
    // (the legacy state.potions arena stays empty until P7-8's
    // h9u-pair work lands).
    for kind in &resolved_potions {
        state.player.potions.push(*kind);
    }

    // Content-side dispatch tables must be installed before
    // start_combat (or any subsequent action) so listeners can
    // resolve via `relic_effects::dispatch` / `potions::dispatch`.
    // `ensure_installed` is idempotent.
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    let relic_counters = scenario
        .player
        .as_ref()
        .map(|p| p.relic_counters.clone())
        .unwrap_or_default();
    seed_relics_with_counters(&mut state, &resolved_relics, &relic_counters)
        .map_err(|e| ScenarioBuildError::Relics(e.to_string()))?;

    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sts2_core::monsters::Intent;

    /// A9 `DeadlyEnemies` attack scaling reaches the library entry
    /// point: a lone Nibbit's opening move is deterministically BUTT
    /// (`ascension_scaling::ASCENSION_ATTACK_SCALING` A0=12/A9=13 —
    /// `crates/sts2-content/src/ascension_scaling.rs`), so realizing
    /// the same fixture at ascension 0 vs 9 must produce different
    /// telegraphed intent damage after `start_combat`.
    #[test]
    fn a9_fixture_gets_deadly_attack_scaling() {
        let toml = r#"
schema = 1

[[monsters]]
kind = "Nibbit"
"#;
        let scenario: ScenarioFile = toml::from_str(toml).expect("minimal Nibbit scenario parses");

        let mut a0 =
            scenario_to_combat_state_with_ascension(&scenario, 7, 0).expect("A0 scenario realizes");
        let mut a9 =
            scenario_to_combat_state_with_ascension(&scenario, 7, 9).expect("A9 scenario realizes");
        assert_eq!(a0.ascension, 0);
        assert_eq!(a9.ascension, 9);

        let _ = sts2_core::combat_loop::start_combat(&mut a0);
        let _ = sts2_core::combat_loop::start_combat(&mut a9);

        let dmg = |state: &CombatState| match state.creatures[0].intent {
            Some(Intent::Attack { damage }) => damage,
            other => {
                panic!("expected Nibbit's opening BUTT to be a lone Attack intent, got {other:?}")
            }
        };
        let a0_damage = dmg(&a0);
        let a9_damage = dmg(&a9);
        assert_eq!(
            a0_damage, 12,
            "A0 Nibbit BUTT damage should be the table base"
        );
        assert_eq!(
            a9_damage, 13,
            "A9 Nibbit BUTT damage should be the table DeadlyEnemies value"
        );
        assert_ne!(
            a0_damage, a9_damage,
            "ascension 0 vs 9 must produce different telegraphed attack damage"
        );
    }
}
