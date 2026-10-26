//! Classifier tests for `split-relics`: handler vs helper vs channels vs
//! install vs test-mod vs imports.
use std::collections::BTreeMap;
use syn::parse_quote;
use xtask_lib::split_relics::{classify_relic_item, RelicBucket};

fn handler_map() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert(
        "on_after_combat_victory_burning_blood".into(),
        "burning_blood".into(),
    );
    m
}

#[test]
fn registered_handler_becomes_handler_grouped_by_id() {
    let item: syn::Item = parse_quote! {
        fn on_after_combat_victory_burning_blood(state: &mut CombatState) -> EffectResult {
            EffectResult::Resolved
        }
    };
    assert_eq!(
        classify_relic_item(&item, &handler_map()),
        RelicBucket::Handler {
            id_snake: "burning_blood".into()
        }
    );
}

#[test]
fn unregistered_free_fn_becomes_helper() {
    let item: syn::Item = parse_quote! {
        fn player_owns_relic(state: &CombatState, id: RelicId) -> bool { false }
    };
    assert_eq!(
        classify_relic_item(&item, &handler_map()),
        RelicBucket::Helper
    );
}

#[test]
fn dispatch_wrapper_becomes_channels() {
    let item: syn::Item = parse_quote! {
        pub fn dispatch_after_combat_victory(state: &mut CombatState) -> EffectResult {
            EffectResult::Resolved
        }
    };
    assert_eq!(
        classify_relic_item(&item, &handler_map()),
        RelicBucket::Channels
    );
}

#[test]
fn channel_payload_type_becomes_channels() {
    let s: syn::Item = parse_quote! { pub struct CardEventCtx { pub id: u32 } };
    let e: syn::Item = parse_quote! { pub enum AddSource { Draw, Power } };
    assert_eq!(
        classify_relic_item(&s, &handler_map()),
        RelicBucket::Channels
    );
    // `AddSource` ends in `Source`.
    assert_eq!(
        classify_relic_item(&e, &handler_map()),
        RelicBucket::Channels
    );
}

#[test]
fn relic_hook_channel_invocation_becomes_channels() {
    let item: syn::Item = parse_quote! {
        relic_hook_channel!(AfterCombatVictoryFn, AFTER_COMBAT_VICTORY, registry_x, register_x, register_many_x, lookup_x, listeners_x, is_empty_x, fn(&mut CombatState) -> EffectResult);
    };
    assert_eq!(
        classify_relic_item(&item, &handler_map()),
        RelicBucket::Channels
    );
}

#[test]
fn macro_rules_def_becomes_channels() {
    let item: syn::Item = parse_quote! {
        macro_rules! relic_hook_channel { () => {}; }
    };
    assert_eq!(
        classify_relic_item(&item, &handler_map()),
        RelicBucket::Channels
    );
}

#[test]
fn install_and_alias_become_install() {
    let f: syn::Item = parse_quote! { pub fn install_default_registry() {} };
    let a: syn::Item = parse_quote! { pub fn ensure_installed() {} };
    assert_eq!(
        classify_relic_item(&f, &handler_map()),
        RelicBucket::Install
    );
    assert_eq!(
        classify_relic_item(&a, &handler_map()),
        RelicBucket::Install
    );
}

#[test]
fn registry_plumbing_fn_becomes_helper() {
    let item: syn::Item = parse_quote! {
        pub fn registered_count() -> usize { 0 }
    };
    assert_eq!(
        classify_relic_item(&item, &handler_map()),
        RelicBucket::Helper
    );
}

#[test]
fn test_mod_becomes_tests() {
    let item: syn::Item = parse_quote! { #[cfg(test)] mod tests {} };
    assert_eq!(
        classify_relic_item(&item, &handler_map()),
        RelicBucket::Tests
    );
}

#[test]
fn use_import_becomes_imports() {
    let item: syn::Item = parse_quote! { use crate::relics::RelicId; };
    assert_eq!(
        classify_relic_item(&item, &handler_map()),
        RelicBucket::Imports
    );
}
