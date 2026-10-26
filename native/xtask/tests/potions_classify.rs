//! Classifier tests for split-potions: handler (registered drink_*) vs helper
//! (unregistered drink_* decoy + free fns + consts) vs install vs test-mod vs
//! imports — driven by the handler→id map built from the install array.
use std::collections::BTreeMap;
use xtask_lib::split_potions::{classify_potion_item, PotionBucket};

fn handler_map() -> BTreeMap<String, String> {
    // `drink_fire_potion` and `drink_attack_potion` ARE registered (so they are
    // handlers). `drink_character_card_potion` is the DECOY — NOT in the map.
    let mut m = BTreeMap::new();
    m.insert("drink_fire_potion".to_string(), "fire_potion".to_string());
    m.insert(
        "drink_attack_potion".to_string(),
        "attack_potion".to_string(),
    );
    m
}

fn items(src: &str) -> Vec<syn::Item> {
    syn::parse_file(src).unwrap().items
}

#[test]
fn registered_drink_fn_is_handler_under_array_id() {
    let m = handler_map();
    let it = &items("fn drink_fire_potion(s: &mut S) -> E { E::R }")[0];
    assert_eq!(
        classify_potion_item(it, &m),
        PotionBucket::Handler {
            id_snake: "fire_potion".to_string()
        }
    );
}

#[test]
fn unregistered_drink_decoy_is_helper_not_handler() {
    let m = handler_map();
    // The `drink_character_card_potion` decoy: drink_-prefixed but ABSENT from
    // the array → Helper, NOT a per-id handler.
    let it = &items("fn drink_character_card_potion(s: &mut S) -> E { E::R }")[0];
    assert_eq!(classify_potion_item(it, &m), PotionBucket::Helper);
}

#[test]
fn free_helper_fns_are_helpers() {
    let m = handler_map();
    for src in [
        "fn apply_power_signed_to_creature(s: &mut S) {}",
        "fn resolve_enemy_target(s: &S) -> u32 { 0 }",
        "fn open_generated_free_choice(s: &mut S) {}",
        "fn route_generated_into_hand(s: &mut S) {}",
    ] {
        let it = &items(src)[0];
        assert_eq!(classify_potion_item(it, &m), PotionBucket::Helper, "{src}");
    }
}

#[test]
fn consts_are_helpers() {
    let m = handler_map();
    for src in [
        "const KINGS_COURAGE_FORGE_AMOUNT: i32 = 3;",
        "pub const BONE_BREW_SUMMON_AMOUNT: i32 = 2;",
        "const CARD_GEN_POTION_CANDIDATES: &[u8] = &[];",
        "const DISTILLED_CHAOS_PLAYS: i32 = 3;",
    ] {
        let it = &items(src)[0];
        assert_eq!(classify_potion_item(it, &m), PotionBucket::Helper, "{src}");
    }
}

#[test]
fn install_and_ensure_are_install() {
    let m = handler_map();
    let install = &items("pub fn install_default_registry() {}")[0];
    let ensure = &items("pub fn ensure_installed() {}")[0];
    assert_eq!(classify_potion_item(install, &m), PotionBucket::Install);
    assert_eq!(classify_potion_item(ensure, &m), PotionBucket::Install);
}

#[test]
fn cfg_test_mod_is_tests() {
    let m = handler_map();
    let it = &items("#[cfg(test)]\nmod tests { fn x() {} }")[0];
    assert_eq!(classify_potion_item(it, &m), PotionBucket::Tests);
}

#[test]
fn use_is_imports() {
    let m = handler_map();
    let it = &items("use sts2_core::CombatState;")[0];
    assert_eq!(classify_potion_item(it, &m), PotionBucket::Imports);
}
