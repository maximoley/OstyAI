//! Regression tests for the extractor defects surfaced by the Ironclad pilot:
//! keyword stems, `super::` body paths, spec-fn re-export, `pub(crate)`
//! re-export form, and test-module wrapper stripping.

use xtask_lib::split_cards::emit_tree_to_string_map;

const SRC: &str = "\
use std::num::NonZeroU32;
use super::types::CardSpec;

fn ironclad_attack_specs() -> [CardSpec; 1] { todo!() }

fn play_break(s: &mut S, t: Option<U>) -> EffectResult {
    super::spec_index::apply(s);
    EffectResult::Resolved
}

fn play_infernal_blade(s: &mut S, t: Option<U>) -> EffectResult {
    let _ = ironclad_attack_specs();
    EffectResult::Resolved
}

pub fn install_pool() { card_effects::register_many(POOL); }

const POOL: &[(CardId, PlayCardFn)] = &[
    (CardId::Break, play_break as PlayCardFn),
    (CardId::InfernalBlade, play_infernal_blade as PlayCardFn),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t() { let _ = NonZeroU32::new(1); install_pool(); }
}
";

#[test]
fn keyword_stem_is_raw_escaped_in_mod_and_pool() {
    let map = emit_tree_to_string_map(SRC, "ironclad");
    assert!(
        map["mod.rs"].contains("pub mod r#break;"),
        "mod decl: {}",
        map["mod.rs"]
    );
    assert!(map["mod.rs"].contains("r#break::play_break as PlayCardFn"));
    assert!(!map["mod.rs"].contains("pub mod break;"));
    // file is still named with the bare stem
    assert!(map.contains_key("break.rs"));
}

#[test]
fn super_paths_in_bodies_become_crate_cards() {
    let map = emit_tree_to_string_map(SRC, "ironclad");
    assert!(map["break.rs"].contains("crate::cards::spec_index::apply"));
    assert!(!map["break.rs"].contains("super::spec_index"));
    // but the emitter's own prelude import is untouched
    assert!(map["break.rs"].contains("use super::prelude::*;"));
}

#[test]
fn prelude_reexports_are_pub_crate_and_glob_specs() {
    let map = emit_tree_to_string_map(SRC, "ironclad");
    let prelude = &map["prelude.rs"];
    assert!(prelude.contains("pub(crate) use super::helpers::*;"));
    assert!(prelude.contains("pub(crate) use super::specs::*;"));
    // monolith `use super::types::CardSpec;` -> pub(crate) use crate::cards::types
    assert!(prelude.contains("pub(crate) use crate::cards::types::CardSpec;"));
    assert!(!prelude.contains("\npub use "));
}

#[test]
fn pub_module_root_helper_is_reexported_from_mod() {
    // A module-root `pub fn` that is not a card/spec/pool/test (e.g. a
    // starter-deck builder called from other crates) lands in helpers.rs but
    // MUST be re-exported from mod.rs so `<char>::<name>` keeps resolving.
    const SRC: &str = "\
pub fn silent_starter_deck() -> Vec<CardId> { vec![] }
fn current_damage(s: &S) -> i32 { 0 }
fn play_anger(s: &mut S, t: Option<U>) -> EffectResult { EffectResult::Resolved }
pub fn install_pool() { card_effects::register_many(POOL); }
const POOL: &[(CardId, PlayCardFn)] = &[ (CardId::Anger, play_anger as PlayCardFn) ];
";
    let map = emit_tree_to_string_map(SRC, "silent");
    assert!(
        map["mod.rs"].contains("pub use helpers::silent_starter_deck;"),
        "mod.rs: {}",
        map["mod.rs"]
    );
    assert!(map["helpers.rs"].contains("pub fn silent_starter_deck"));
    // a private helper is NOT re-exported
    assert!(!map["mod.rs"].contains("pub use helpers::current_damage;"));
}

#[test]
fn test_module_wrapper_is_stripped() {
    let map = emit_tree_to_string_map(SRC, "ironclad");
    let t = &map["tests.rs"];
    // inner contents present, but NOT re-wrapped in an extra `mod tests`
    assert!(t.contains("use super::*;"));
    assert!(t.contains("install_pool();"));
    assert!(!t.contains("mod tests {"));
}

use xtask_lib::split_cards::to_pub_crate_use;

#[test]
fn to_pub_crate_use_bumps_use_under_cfg_test_attr() {
    // colorless regression: a `#[cfg(test)]`-attributed alias must become a
    // `pub(crate) use` (preserving the attr) so `super::prelude::<alias>`
    // references from tests.rs are not E0603-inaccessible.
    let out = to_pub_crate_use("#[cfg(test)]\nuse crate::cards::status_specs_generated as sspecs;");
    assert_eq!(
        out,
        "#[cfg(test)]\npub(crate) use crate::cards::status_specs_generated as sspecs;"
    );
    // already-pub(crate) is untouched; plain `use` still bumps.
    assert_eq!(to_pub_crate_use("use a::b;"), "pub(crate) use a::b;");
    assert_eq!(
        to_pub_crate_use("pub(crate) use a::b;"),
        "pub(crate) use a::b;"
    );
}

#[test]
fn impl_block_relocated_verbatim_not_visibility_bumped() {
    // colorless regression (E0449): `pub(crate) impl …` is illegal — `impl`
    // blocks must move into helpers.rs verbatim.
    const SRC: &str = "\
pub enum Rider { A, B }
impl Rider { fn x(self) -> i32 { 0 } }
fn play_anger(s: &mut S, t: Option<U>) -> EffectResult { EffectResult::Resolved }
pub fn install_pool() { card_effects::register_many(POOL); }
const POOL: &[(CardId, PlayCardFn)] = &[ (CardId::Anger, play_anger as PlayCardFn) ];
";
    let map = emit_tree_to_string_map(SRC, "colorless");
    let helpers = &map["helpers.rs"];
    assert!(helpers.contains("impl Rider {"), "helpers: {helpers}");
    assert!(!helpers.contains("pub(crate) impl"), "helpers: {helpers}");
}

#[test]
fn pub_crate_module_root_fn_is_reexported_with_pub_crate() {
    // colorless regression: a module-root `pub(crate) fn` reachable as
    // `crate::cards::<char>::<name>` from OTHER pools must be re-exported from
    // mod.rs with `pub(crate) use` (not dropped, not widened to `pub use`).
    const SRC: &str = "\
pub(crate) fn create_character_cards_into_arena(s: &mut S) {}
fn play_anger(s: &mut S, t: Option<U>) -> EffectResult { EffectResult::Resolved }
pub fn install_pool() { card_effects::register_many(POOL); }
const POOL: &[(CardId, PlayCardFn)] = &[ (CardId::Anger, play_anger as PlayCardFn) ];
";
    let map = emit_tree_to_string_map(SRC, "colorless");
    assert!(
        map["mod.rs"].contains("pub(crate) use helpers::create_character_cards_into_arena;"),
        "mod.rs: {}",
        map["mod.rs"]
    );
    assert!(!map["mod.rs"].contains("pub use helpers::create_character_cards_into_arena;"));
}

#[test]
fn install_pool_extra_registrations_are_preserved_and_qualified() {
    // regent regression: the generated install_pool must carry over ANY
    // statements beyond `register_many(POOL)` (e.g. register_many_late_effects),
    // with relocated helper-fn refs qualified to `helpers::`. Dropping them
    // silently de-registered a bus consumer + left the helper dead (E -D warn).
    const SRC: &str = "\
fn make_it_so_late_effect(s: &mut S) {}
fn play_make_it_so(s: &mut S, t: Option<U>) -> EffectResult { EffectResult::Resolved }
pub fn install_pool() {
    card_effects::register_many(POOL);
    // late-effect bus consumers.
    card_effects::register_many_late_effects(&[(
        CardId::MakeItSo,
        make_it_so_late_effect as card_effects::LateEffectFn,
    )]);
}
const POOL: &[(CardId, PlayCardFn)] = &[ (CardId::MakeItSo, play_make_it_so as PlayCardFn) ];
";
    let map = emit_tree_to_string_map(SRC, "regent");
    let modrs = &map["mod.rs"];
    assert!(
        modrs.contains("card_effects::register_many(POOL);"),
        "mod.rs: {modrs}"
    );
    assert!(
        modrs.contains("register_many_late_effects"),
        "extra reg dropped: {modrs}"
    );
    // helper fn ref qualified to helpers::, exactly once (not double-prefixed).
    assert!(
        modrs.contains("helpers::make_it_so_late_effect as card_effects::LateEffectFn"),
        "mod.rs: {modrs}"
    );
    assert!(
        !modrs.contains("helpers::helpers::"),
        "double-qualified: {modrs}"
    );
    // the inline comment is preserved verbatim.
    assert!(modrs.contains("// late-effect bus consumers."));
}

#[test]
fn install_pool_with_no_extra_statements_stays_minimal() {
    let map = emit_tree_to_string_map(SRC, "ironclad");
    let modrs = &map["mod.rs"];
    // SRC's install_pool is just register_many(POOL) -> no spurious extra body.
    assert!(
        modrs.contains("    card_effects::register_many(POOL);\n}"),
        "mod.rs: {modrs}"
    );
    assert!(!modrs.contains("register_many_late_effects"));
}
