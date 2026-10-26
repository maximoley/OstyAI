//! Slice round-trip + super-rewrite tests for split-relics.
use xtask_lib::split_cards::item_span_bytes;
use xtask_lib::split_relics::{rewrite_super_to, RELIC_SUPER_TARGET};

const SRC: &str = "\
/// Burning Blood doc.
fn on_after_combat_victory_burning_blood(state: &mut CombatState) -> EffectResult {
    let _ = super::card_effects::heal_player(state, 6);
    EffectResult::Resolved
}
";

#[test]
fn slice_captures_doc_comment_and_full_body() {
    let file = syn::parse_file(SRC).unwrap();
    let item = &file.items[0];
    let (start, end) = item_span_bytes(item);
    let slice = &SRC[start..end];
    assert!(slice.contains("/// Burning Blood doc."));
    assert!(slice.contains("on_after_combat_victory_burning_blood"));
    assert!(slice.trim_end().ends_with('}'));
    assert!(slice.contains("EffectResult::Resolved"));
}

#[test]
fn rewrite_super_targets_crate_root_for_relics() {
    let file = syn::parse_file(SRC).unwrap();
    let item = &file.items[0];
    let (start, end) = item_span_bytes(item);
    let slice = &SRC[start..end];
    let out = rewrite_super_to(slice, RELIC_SUPER_TARGET);
    // relic_effects lives directly under the crate root → super:: => crate::
    assert!(out.contains("crate::card_effects::heal_player"));
    assert!(!out.contains("super::card_effects"));
    // Not the cards path.
    assert!(!out.contains("crate::cards::"));
}

#[test]
fn relic_super_target_is_crate() {
    assert_eq!(RELIC_SUPER_TARGET, "crate::");
}
