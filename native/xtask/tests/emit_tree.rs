use xtask_lib::split_cards::emit_tree_to_string_map;

const SRC: &str = "\
use std::num::NonZeroU32;

fn current_damage(s: &S) -> i32 { 0 }

/// Anger doc.
fn play_anger(s: &mut S, t: Option<U>) -> EffectResult { EffectResult::Resolved }

pub fn install_pool() { card_effects::register_many(POOL); }

const POOL: &[(CardId, PlayCardFn)] = &[
    (CardId::Anger, play_anger as PlayCardFn),
];
";

#[test]
fn emits_expected_files() {
    let map = emit_tree_to_string_map(SRC, "ironclad");
    assert!(map.contains_key("anger.rs"));
    assert!(map.contains_key("helpers.rs"));
    assert!(map.contains_key("prelude.rs"));
    assert!(map.contains_key("mod.rs"));

    assert!(map["anger.rs"].contains("use super::prelude::*;"));
    assert!(map["anger.rs"].contains("pub(crate) fn play_anger"));
    assert!(map["anger.rs"].contains("/// Anger doc."));

    assert!(map["helpers.rs"].contains("pub(crate) fn current_damage"));

    // mod.rs declares modules and rebuilds POOL referencing the moved fn.
    assert!(map["mod.rs"].contains("pub mod anger;"));
    assert!(map["mod.rs"].contains("anger::play_anger as PlayCardFn"));
    assert!(map["mod.rs"].contains("pub fn install_pool"));
}
