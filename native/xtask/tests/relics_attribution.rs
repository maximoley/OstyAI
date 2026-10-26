//! Registration-ATTRIBUTION test: a synthetic `install_default_registry` with
//! 2 ids across 3 channels (incl. a `register_many` array) → correct per-id
//! grouping + preserved global order.
use xtask_lib::split_relics::{id_to_snake, parse_install_statements};

// Two ids — CrackedCore and BurningBlood — across three channels:
//   1. legacy register_many array (both ids, CrackedCore first)
//   2. register_before_side_turn_start (CrackedCore)
//   3. register_after_combat_victory (BurningBlood)
// Expected GLOBAL order:
//   [0] register_many CrackedCore  (array elem 0)
//   [1] register_many BurningBlood (array elem 1)
//   [2] before_side_turn_start CrackedCore
//   [3] after_combat_victory BurningBlood
const SRC: &str = r"
pub fn install_default_registry() {
    INSTALL_ONCE.call_once(|| {
        register_many(&[
            (RelicId::CrackedCore, on_combat_start_cracked_core as RelicEffectFn),
            (RelicId::BurningBlood, on_combat_start_burning_blood as RelicEffectFn),
        ]);
        register_before_side_turn_start(
            RelicId::CrackedCore,
            on_before_side_turn_start_cracked_core as BeforeSideTurnStartFn,
        );
        register_after_combat_victory(
            RelicId::BurningBlood,
            on_after_combat_victory_burning_blood as AfterCombatVictoryFn,
        );
    });
}
";

fn install_fn() -> syn::ItemFn {
    let file = syn::parse_file(SRC).unwrap();
    file.items
        .into_iter()
        .find_map(|it| match it {
            syn::Item::Fn(f) if f.sig.ident == "install_default_registry" => Some(f),
            _ => None,
        })
        .unwrap()
}

#[test]
fn id_to_snake_converts_camel_to_snake() {
    assert_eq!(id_to_snake("RelicId::CrackedCore"), "cracked_core");
    assert_eq!(id_to_snake("RelicId::RingOfTheSnake"), "ring_of_the_snake");
    assert_eq!(id_to_snake("BurningBlood"), "burning_blood");
}

#[test]
fn attribution_global_order_and_split() {
    let f = install_fn();
    let stmts = parse_install_statements(&f, SRC);
    assert_eq!(stmts.len(), 4, "2 array elems + 2 per-channel calls");

    // Global order + ids.
    assert_eq!(stmts[0].global_index, 0);
    assert_eq!(stmts[0].id_snake, "cracked_core");
    assert_eq!(stmts[0].handler, "on_combat_start_cracked_core");

    assert_eq!(stmts[1].global_index, 1);
    assert_eq!(stmts[1].id_snake, "burning_blood");
    assert_eq!(stmts[1].handler, "on_combat_start_burning_blood");

    assert_eq!(stmts[2].global_index, 2);
    assert_eq!(stmts[2].id_snake, "cracked_core");
    assert_eq!(stmts[2].handler, "on_before_side_turn_start_cracked_core");

    assert_eq!(stmts[3].global_index, 3);
    assert_eq!(stmts[3].id_snake, "burning_blood");
    assert_eq!(stmts[3].handler, "on_after_combat_victory_burning_blood");
}

#[test]
fn register_many_array_is_split_per_element() {
    let f = install_fn();
    let stmts = parse_install_statements(&f, SRC);
    // Each array element becomes a single-element register_many(&[...]).
    assert_eq!(
        stmts[0].text,
        "register_many(&[(RelicId::CrackedCore, on_combat_start_cracked_core as RelicEffectFn)]);"
    );
    assert_eq!(
        stmts[1].text,
        "register_many(&[(RelicId::BurningBlood, on_combat_start_burning_blood as RelicEffectFn)]);"
    );
}

#[test]
fn per_id_grouping_preserves_within_id_order() {
    let f = install_fn();
    let stmts = parse_install_statements(&f, SRC);

    let cracked: Vec<usize> = stmts
        .iter()
        .filter(|s| s.id_snake == "cracked_core")
        .map(|s| s.global_index)
        .collect();
    let burning: Vec<usize> = stmts
        .iter()
        .filter(|s| s.id_snake == "burning_blood")
        .map(|s| s.global_index)
        .collect();

    // CrackedCore: array elem 0, then before_side_turn_start at 2.
    assert_eq!(cracked, vec![0, 2]);
    // BurningBlood: array elem 1, then after_combat_victory at 3.
    assert_eq!(burning, vec![1, 3]);
}

#[test]
fn per_channel_statement_is_verbatim_byte_slice() {
    let f = install_fn();
    let stmts = parse_install_statements(&f, SRC);
    // The per-channel register call keeps the `as <Fn>` cast verbatim.
    assert!(stmts[2]
        .text
        .contains("on_before_side_turn_start_cracked_core as BeforeSideTurnStartFn"));
    assert!(stmts[3]
        .text
        .contains("on_after_combat_victory_burning_blood as AfterCombatVictoryFn"));
}
