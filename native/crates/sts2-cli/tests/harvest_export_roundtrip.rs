//! Bead `sts2-rs-cws0i` test 1: the shared writer
//! (`sts2_cli::sim::scenario::export::render_run_entry_toml`) emits
//! potions + ascension + the real character for a hand-built
//! `RunManager`, and the resulting TOML round-trips through
//! `load_scenario_file` + `scenario_to_combat_state_with_ascension`:
//! potion belt non-empty, ascension stamped, monster roster matches.
//!
//! Lives in `tests/` (an ordinary external-crate consumer of
//! `sts2_cli::`), not in `src/sim/scenario/export.rs`'s own
//! `#[cfg(test)]` block — see that file's test-module doc comment for
//! why (the `sim` tree is compiled twice, under two different crate
//! roots, and only the library root exports
//! `scenario_to_combat_state_with_ascension`; `scenario_lib_roundtrip.rs`
//! is the existing precedent for keeping cross-crate-root assertions
//! here).

use sts2_content::run_callbacks::new_run_manager_for_character_with_ascension;
use sts2_core::potions::{CharacterId, PotionKind};
use sts2_core::MonsterKind;

use sts2_cli::scenario_to_combat_state_with_ascension;
use sts2_cli::sim::scenario::export::render_run_entry_toml;
use sts2_cli::sim::scenario::toml_file::load_scenario_file;
use sts2_cli::sim::scenario::MonsterSpec;
use sts2_cli::sim::{build_starter_state_from_monsters_with_ascension, PlayerClass};

#[test]
fn round_trips_potions_ascension_and_monsters() {
    let ascension = 8u8;
    let seed = 42i64 as i32;

    let mut rm =
        new_run_manager_for_character_with_ascension(seed, CharacterId::Ironclad, ascension);
    rm.run_state.current_hp = 55;
    rm.run_state.potions = vec![PotionKind::FirePotion, PotionKind::BlockPotion];
    rm.combat = Some(build_starter_state_from_monsters_with_ascension(
        u64::from(seed as u32),
        vec![MonsterSpec::new(MonsterKind::Nibbit)],
        PlayerClass::Ironclad,
        None,
        None,
        None,
        ascension,
    ));

    let potions = rm.run_state.potions.clone();
    let toml = render_run_entry_toml(&rm, 55, "Ironclad", ascension, &potions);
    assert!(toml.contains("ascension = 8"), "toml:\n{toml}");
    assert!(toml.contains("FirePotion"), "toml:\n{toml}");
    assert!(toml.contains("BlockPotion"), "toml:\n{toml}");
    assert!(toml.contains("Nibbit"), "toml:\n{toml}");

    let path = std::env::temp_dir().join(format!(
        "sts2-cws0i-harvest-export-roundtrip-{}.toml",
        std::process::id()
    ));
    std::fs::write(&path, &toml).expect("write scratch fixture");
    let parsed = load_scenario_file(&path).expect("fixture parses");
    std::fs::remove_file(&path).ok();

    let state = scenario_to_combat_state_with_ascension(&parsed, u64::from(seed as u32), ascension)
        .expect("fixture realizes");

    assert!(
        !state.player.potions.is_empty(),
        "potion belt must round-trip non-empty"
    );
    assert_eq!(state.enemy_uids.len(), 1, "monster roster must round-trip");
    assert_eq!(
        parsed
            .player
            .as_ref()
            .and_then(|p| p.ascension)
            .unwrap_or(0),
        u32::from(ascension),
        "ascension must round-trip through [player].ascension"
    );
}
