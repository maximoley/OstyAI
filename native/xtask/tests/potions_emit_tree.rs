//! Emit-tree test for split-potions: full synthetic dispatch fixture →
//! expected file set, per-id handler files, helper-decoy + const isolation,
//! regenerated flat `let entries` installer (NO call_once, NO per-id install),
//! two-const `pub(crate) use` re-export, NO specs.rs, and a replay-order check
//! confirming the regenerated `entries` array replays the same (id, handler)
//! registrations in the same array order as the monolith.
use xtask_lib::split_potions::emit_tree_to_string_map;

// Fixture: 3 registered potions (fire, kings_courage, attack) + the
// `drink_character_card_potion` DECOY (drink_-named, NOT registered) + a free
// helper + two consts (one test-referenced via `super::`, one not).
const SRC: &str = r"
use sts2_core::card_effects::EffectResult;
use sts2_core::potion_effects::{self, PotionEffectFn};
use sts2_core::{CombatState, PotionKind};

const KINGS_COURAGE_FORGE_AMOUNT: i32 = 3;
const CARD_GEN_POTION_CANDIDATES: i32 = 7;

/// Shared sub-routine — the `drink_`-named DECOY. NOT in the array.
fn drink_character_card_potion(state: &mut CombatState) -> EffectResult {
    EffectResult::Resolved
}

fn resolve_enemy_target(state: &CombatState) -> u32 {
    let _ = super::helpers_noop();
    0
}

/// Fire Potion: deal damage.
fn drink_fire_potion(state: &mut CombatState) -> EffectResult {
    EffectResult::Resolved
}

fn drink_kings_courage(state: &mut CombatState) -> EffectResult {
    let _ = KINGS_COURAGE_FORGE_AMOUNT;
    EffectResult::Resolved
}

fn drink_attack_potion(state: &mut CombatState) -> EffectResult {
    drink_character_card_potion(state)
}

pub fn install_default_registry() {
    let entries: [(PotionKind, PotionEffectFn); 3] = [
        (PotionKind::FirePotion, drink_fire_potion as PotionEffectFn),
        (
            PotionKind::KingsCourage,
            drink_kings_courage as PotionEffectFn,
        ),
        (
            PotionKind::AttackPotion,
            drink_attack_potion as PotionEffectFn,
        ),
    ];
    potion_effects::register_many(&entries);
}

pub fn ensure_installed() {
    install_default_registry();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smoke() {
        assert_eq!(super::KINGS_COURAGE_FORGE_AMOUNT, 3);
        let _ = drink_fire_potion;
    }
}
";

#[test]
fn emits_expected_file_set() {
    let map = emit_tree_to_string_map(SRC);
    assert!(map.contains_key("prelude.rs"));
    assert!(map.contains_key("helpers.rs"));
    assert!(map.contains_key("mod.rs"));
    assert!(map.contains_key("tests.rs"));
    assert!(map.contains_key("fire_potion.rs"));
    assert!(map.contains_key("kings_courage.rs"));
    assert!(map.contains_key("attack_potion.rs"));
}

#[test]
fn no_specs_module() {
    let map = emit_tree_to_string_map(SRC);
    assert!(
        !map.contains_key("specs.rs"),
        "potions have no specs module"
    );
    assert!(
        !map["prelude.rs"].contains("super::specs::*"),
        "prelude must not glob a specs module"
    );
    assert!(!map["mod.rs"].contains("pub mod specs;"));
}

#[test]
fn handlers_routed_to_their_id_files() {
    let map = emit_tree_to_string_map(SRC);
    let fire = &map["fire_potion.rs"];
    assert!(fire.contains("pub(crate) fn drink_fire_potion"));
    assert!(fire.contains("/// Fire Potion: deal damage."));
    // other handlers are NOT here.
    assert!(!fire.contains("drink_kings_courage"));
    assert!(!fire.contains("drink_attack_potion"));

    let kc = &map["kings_courage.rs"];
    assert!(kc.contains("pub(crate) fn drink_kings_courage"));

    let ap = &map["attack_potion.rs"];
    assert!(ap.contains("pub(crate) fn drink_attack_potion"));
}

#[test]
fn decoy_and_helper_stay_in_helpers() {
    let map = emit_tree_to_string_map(SRC);
    let h = &map["helpers.rs"];
    // The drink_-named DECOY is a helper, NOT a per-id module.
    assert!(h.contains("pub(crate) fn drink_character_card_potion"));
    assert!(!map.contains_key("character_card_potion.rs"));
    // Free helper.
    assert!(h.contains("pub(crate) fn resolve_enemy_target"));
    // Consts relocated + bumped to pub(crate).
    assert!(h.contains("pub(crate) const KINGS_COURAGE_FORGE_AMOUNT"));
    assert!(h.contains("pub(crate) const CARD_GEN_POTION_CANDIDATES"));
}

#[test]
fn super_rewritten_to_crate_potions_in_helpers() {
    let map = emit_tree_to_string_map(SRC);
    assert!(map["helpers.rs"].contains("crate::potions::helpers_noop"));
    assert!(!map["helpers.rs"].contains("super::helpers_noop"));
}

#[test]
fn no_per_id_install_fn() {
    let map = emit_tree_to_string_map(SRC);
    // Delta from relics: per-id files carry ONLY the handler, no install().
    for f in ["fire_potion.rs", "kings_courage.rs", "attack_potion.rs"] {
        assert!(
            !map[f].contains("fn install()"),
            "{f} must not have a per-id install()"
        );
    }
}

#[test]
fn mod_rs_regenerates_flat_installer_in_array_order() {
    let map = emit_tree_to_string_map(SRC);
    let m = &map["mod.rs"];
    assert!(m.contains("pub fn install_default_registry()"));
    assert!(m.contains("pub fn ensure_installed()"));
    // Flat shape — NO call_once guard (delta from relics).
    assert!(!m.contains("call_once"));
    assert!(m.contains("let entries: [(PotionKind, PotionEffectFn); 3] = ["));
    assert!(m.contains("potion_effects::register_many(&entries);"));

    // Each element qualified to <id_snake>::<handler>, in ARRAY order.
    let fire = m
        .find("fire_potion::drink_fire_potion as PotionEffectFn")
        .unwrap();
    let kc = m
        .find("kings_courage::drink_kings_courage as PotionEffectFn")
        .unwrap();
    let ap = m
        .find("attack_potion::drink_attack_potion as PotionEffectFn")
        .unwrap();
    assert!(fire < kc, "fire before kings_courage (array order)");
    assert!(kc < ap, "kings_courage before attack (array order)");

    // Module decls (sorted) + handler glob root.
    assert!(m.contains("pub mod fire_potion;"));
    assert!(m.contains("pub mod kings_courage;"));
    assert!(m.contains("pub mod attack_potion;"));
    assert!(m.contains("pub mod helpers;"));
}

#[test]
fn test_referenced_const_is_reexported_at_root() {
    let map = emit_tree_to_string_map(SRC);
    let m = &map["mod.rs"];
    // KINGS_COURAGE_FORGE_AMOUNT is reached by the test mod via `super::` →
    // pub(crate) use re-export. CARD_GEN_POTION_CANDIDATES is NOT reached → not
    // re-exported.
    assert!(
        m.contains("pub(crate) use helpers::{KINGS_COURAGE_FORGE_AMOUNT};")
            || m.contains("KINGS_COURAGE_FORGE_AMOUNT"),
        "test-referenced const must be re-exported at root"
    );
    assert!(m.contains("pub(crate) use helpers::{"));
    // The non-referenced const must NOT be in the re-export list.
    let reexport_line = m
        .lines()
        .find(|l| l.contains("pub(crate) use helpers::{"))
        .unwrap();
    assert!(reexport_line.contains("KINGS_COURAGE_FORGE_AMOUNT"));
    assert!(!reexport_line.contains("CARD_GEN_POTION_CANDIDATES"));
}

#[test]
fn tests_module_relocated_and_unwrapped() {
    let map = emit_tree_to_string_map(SRC);
    let t = &map["tests.rs"];
    assert!(t.contains("fn smoke()"));
    assert!(!t.contains("mod tests {"));
    // per-id globs so tests reaching handlers directly resolve.
    assert!(t.contains("fire_potion::*"));
    assert!(t.contains("kings_courage::*"));
    assert!(t.contains("attack_potion::*"));
    // The test's `super::KINGS_COURAGE_FORGE_AMOUNT` is preserved verbatim.
    assert!(t.contains("super::KINGS_COURAGE_FORGE_AMOUNT"));
}

/// THE load-bearing replay check: the regenerated `entries` array must replay
/// the same (id, handler) pairs in the same array order as the monolith.
#[test]
fn regenerated_entries_replays_same_order() {
    let map = emit_tree_to_string_map(SRC);
    let m = &map["mod.rs"];
    let body = &m[m.find("let entries").unwrap()..m.find("register_many(&entries)").unwrap()];

    let expected: Vec<(&str, &str)> = vec![
        ("PotionKind::FirePotion", "drink_fire_potion"),
        ("PotionKind::KingsCourage", "drink_kings_courage"),
        ("PotionKind::AttackPotion", "drink_attack_potion"),
    ];

    // Extract (id, handler) pairs in source order from the regenerated array.
    let mut got: Vec<(String, String)> = Vec::new();
    for line in body.lines() {
        let l = line.trim();
        if let Some(id_start) = l.find("PotionKind::") {
            let id_rest = &l[id_start..];
            let id_end = id_rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                .unwrap_or(id_rest.len());
            let id = &id_rest[..id_end];
            // handler is the ident right before ` as PotionEffectFn`.
            let handler = l
                .find(" as PotionEffectFn")
                .map(|i| {
                    l[..i]
                        .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .next()
                        .unwrap_or("")
                        .to_string()
                })
                .unwrap_or_default();
            if !handler.is_empty() {
                got.push((id.to_string(), handler));
            }
        }
    }

    assert_eq!(
        got.len(),
        expected.len(),
        "registration count preserved (=3)"
    );
    for (g, e) in got.iter().zip(expected.iter()) {
        assert_eq!(g.0, e.0, "id");
        assert_eq!(g.1, e.1, "handler");
    }
}
