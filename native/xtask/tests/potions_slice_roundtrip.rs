//! Slice round-trip + super-rewrite tests for split-potions.
use xtask_lib::split_cards::item_span_bytes;
use xtask_lib::split_potions::POTION_SUPER_TARGET;
use xtask_lib::split_relics::rewrite_super_to;

const SRC: &str = "\
/// Drink the King's Courage potion: gain Strength.
fn drink_kings_courage(state: &mut CombatState) -> EffectResult {
    let amount = super::helpers::forge_amount();
    EffectResult::Resolved
}
";

#[test]
fn slice_captures_doc_comment_and_full_body() {
    let file = syn::parse_file(SRC).unwrap();
    let item = &file.items[0];
    let (start, end) = item_span_bytes(item);
    let slice = &SRC[start..end];
    assert!(slice.contains("/// Drink the King's Courage potion: gain Strength."));
    assert!(slice.contains("fn drink_kings_courage"));
    assert!(slice.trim_end().ends_with('}'));
    assert!(slice.contains("EffectResult::Resolved"));
}

#[test]
fn rewrite_super_targets_crate_potions() {
    let file = syn::parse_file(SRC).unwrap();
    let item = &file.items[0];
    let (start, end) = item_span_bytes(item);
    let slice = &SRC[start..end];
    let out = rewrite_super_to(slice, POTION_SUPER_TARGET);
    // dispatch lives under crate::potions → super:: => crate::potions::
    assert!(out.contains("crate::potions::helpers::forge_amount"));
    assert!(!out.contains("super::helpers"));
}

#[test]
fn potion_super_target_is_crate_potions() {
    assert_eq!(POTION_SUPER_TARGET, "crate::potions::");
}
