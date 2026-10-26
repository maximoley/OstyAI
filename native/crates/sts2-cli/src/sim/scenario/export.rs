//! Shared `ScenarioFile` TOML writer for a [`RunManager`] combat-entry
//! snapshot.
//!
//! Bead `sts2-rs-cws0i`: extracted from `corpus_gen`'s private
//! `god_entry_to_toml` so `run_arena`'s champion-run fixture harvest can
//! reuse the exact serialization corpus_gen's god-mode sampler already
//! validated (deck/relic/monster token shapes, the ascension-gated
//! `[player].ascension` field). `corpus_gen`'s call site passes an empty
//! potion slice, so its emitted bytes are unchanged by this refactor —
//! the harvest path is the only caller that passes a real belt.

use sts2_core::potions::PotionKind;
use sts2_core::run_manager::RunManager;

use super::potions::pascal_name;

/// Serialize a positioned run's combat ENTRY into a `schema = 1`
/// `ScenarioFile` TOML string. Deck card ids → `to_python_name()` (+ `"+"`
/// for upgraded), monster roster → `python_name()` + the spawned HP.
///
/// `snapshot_hp` is the HP the fixture should start at — the caller's
/// responsibility to pass a realistic value (a god-mode sampler must
/// reset off its unkillable progression HP first; a real champion run's
/// `rm.run_state.current_hp` is already realistic and needs no reset).
///
/// Relics carry `rs.relics` verbatim, filtered to ids with a runtime
/// `RelicKind` so the loader's `seed_relics_with_counters` never
/// hard-errors on an unported relic. `potions` is the player's belt at
/// the snapshot point (engine slot order); pass `&[]` to reproduce the
/// pre-potion-export byte shape (`potions = []`).
///
/// `class_name` is the real character's `[player].class` string — a
/// Regent fixture must not mislabel as Ironclad, or the loader's class
/// fallback silently drops the Regent identity.
///
/// `ascension` is written as `[player].ascension` ONLY when nonzero — the
/// pre-ascension-flag export never emitted this key, so omitting it at
/// ascension 0 keeps unset-ascension callers byte-identical to the old
/// output instead of adding a new `ascension = 0` line everywhere.
// Only `corpus_gen` and `run_arena` call this (as `sts2_cli::...`, an
// external-crate call into the library target); the `main.rs` CLI binary
// never does, so its private copy of this dual-compiled module (bead
// `sts2-rs-a2fh`) sees no in-tree caller and flags dead_code.
#[allow(dead_code)]
#[must_use]
pub fn render_run_entry_toml(
    rm: &RunManager,
    snapshot_hp: i32,
    class_name: &str,
    ascension: u8,
    potions: &[PotionKind],
) -> String {
    let rs = &rm.run_state;
    let deck_tokens: Vec<String> = rs
        .master_deck
        .iter()
        .map(|c| {
            let mut s = c.card_id.to_python_name().to_string();
            if c.upgraded {
                s.push('+');
            }
            format!("{s:?}")
        })
        .collect();

    let relic_tokens: Vec<String> = rs
        .relics
        .iter()
        .filter(|id| id.to_relic_kind().is_some())
        .map(|id| format!("{:?}", id.to_python_name()))
        .collect();

    let potion_tokens: Vec<String> = potions
        .iter()
        .map(|k| format!("{:?}", pascal_name(*k)))
        .collect();

    let mut out = String::new();
    out.push_str(&format!(
        "schema = 1\n\n[player]\nclass = \"{class_name}\"\n"
    ));
    if ascension > 0 {
        out.push_str(&format!("ascension = {ascension}\n"));
    }
    out.push_str(&format!("hp = {snapshot_hp}\nmax_hp = {snapshot_hp}\n"));
    out.push_str(&format!("deck = [{}]\n", deck_tokens.join(", ")));
    out.push_str(&format!("relics = [{}]\n", relic_tokens.join(", ")));
    out.push_str(&format!("potions = [{}]\n", potion_tokens.join(", ")));

    if let Some(cs) = rm.combat.as_ref() {
        for uid in &cs.enemy_uids {
            if let Some(c) = cs.creatures.iter().find(|cr| cr.id == uid.0) {
                if let Some(kind) = c.species {
                    out.push_str(&format!(
                        "\n[[monsters]]\nkind = {:?}\nhp = {}\n",
                        kind.python_name(),
                        c.max_hp.max(1),
                    ));
                }
            }
        }
    }
    out
}

// Bead `sts2-rs-cws0i` test 1 (shared writer round-trips potions +
// ascension + monster roster through `load_scenario_file` +
// `scenario_to_combat_state_with_ascension`) lives in
// `crates/sts2-cli/tests/harvest_export_roundtrip.rs`, NOT here.
// `src/sim/**` is compiled twice — once under the `sts2-cli` library
// crate root (`src/lib.rs: pub mod sim;`), once under the `sts2-cli`
// binary crate root (`src/main.rs: mod sim;`, bead `sts2-rs-a2fh`) — and
// only the library root exports `scenario_to_combat_state_with_ascension`.
// A `#[cfg(test)]` block here that called it would compile under the lib
// target and fail under the bin target (`cannot find function ... in the
// crate root`), exactly the trap `scenario_lib_roundtrip.rs`'s doc
// comment already routes around: cross-crate-root assertions belong in
// `tests/`, where `use sts2_cli::...` is an ordinary external-crate
// import in every context.
#[cfg(test)]
mod tests {
    use super::*;
    use sts2_core::potions::CharacterId;
    use sts2_core::run_manager::RunManager;

    fn hand_built_rm(ascension: u8) -> RunManager {
        let mut rm = sts2_content::run_callbacks::new_run_manager_for_character_with_ascension(
            42,
            CharacterId::Ironclad,
            ascension,
        );
        rm.run_state.current_hp = 55;
        rm.run_state.potions = vec![PotionKind::FirePotion, PotionKind::BlockPotion];
        rm
    }

    #[test]
    fn empty_potions_matches_legacy_empty_line() {
        let rm = hand_built_rm(0);
        let toml = render_run_entry_toml(&rm, 55, "Ironclad", 0, &[]);
        assert!(toml.contains("potions = []\n"));
    }

    #[test]
    fn ascension_zero_omits_field() {
        let rm = hand_built_rm(0);
        let toml = render_run_entry_toml(&rm, 55, "Ironclad", 0, &[]);
        assert!(!toml.contains("ascension"));
    }

    #[test]
    fn nonzero_ascension_and_real_potions_render() {
        let rm = hand_built_rm(8);
        let potions = rm.run_state.potions.clone();
        let toml = render_run_entry_toml(&rm, 55, "Ironclad", 8, &potions);
        assert!(toml.contains("ascension = 8"));
        assert!(toml.contains("FirePotion"));
        assert!(toml.contains("BlockPotion"));
        // No [[monsters]] block: `rm.combat` is unset in this hand-built
        // fixture (the monster-roster assertion needs a real CombatState,
        // which the `tests/` round-trip covers with the full loader).
        assert!(!toml.contains("[[monsters]]"));
    }
}
