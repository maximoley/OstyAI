use syn::parse_quote;
use xtask_lib::classify::{classify_item, Bucket};

#[test]
fn play_fn_becomes_card_with_stem() {
    let item: syn::Item = parse_quote! {
        fn play_anger(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult { EffectResult::Resolved }
    };
    assert_eq!(
        classify_item(&item),
        Bucket::Card {
            stem: "anger".into()
        }
    );
}

#[test]
fn helper_fn_becomes_helper() {
    let item: syn::Item = parse_quote! { fn current_damage(state: &CombatState) -> i32 { 0 } };
    assert_eq!(classify_item(&item), Bucket::Helper);
}

#[test]
fn spec_fn_becomes_specs() {
    let item: syn::Item = parse_quote! { fn ironclad_attack_specs() -> [CardSpec; 36] { todo!() } };
    assert_eq!(classify_item(&item), Bucket::Specs);
}

#[test]
fn pool_const_and_installer_become_pool() {
    let c: syn::Item = parse_quote! { const POOL: &[(CardId, PlayCardFn)] = &[]; };
    let f: syn::Item = parse_quote! { pub fn install_pool() {} };
    assert_eq!(classify_item(&c), Bucket::Pool);
    assert_eq!(classify_item(&f), Bucket::Pool);
}

#[test]
fn test_mod_becomes_tests() {
    let item: syn::Item = parse_quote! { #[cfg(test)] mod tests {} };
    assert_eq!(classify_item(&item), Bucket::Tests);
}
