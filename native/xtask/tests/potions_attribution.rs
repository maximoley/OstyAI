//! Registration-ATTRIBUTION test for split-potions: a synthetic
//! `install_default_registry` with a `let entries` array → correct per-id
//! grouping (id_snake derived from the registration's first arg, NOT the fn
//! name) + preserved array order. Exercises the `drink_character_card_potion`
//! decoy: the 3 card-potion wrappers ARE in the array (so they classify as
//! handlers), but a `drink_character_card_potion` absent from the array is a
//! helper — verified in the emit-tree test.
use xtask_lib::split_potions::parse_install_entries;

const SRC: &str = r"
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
fn entries_parsed_in_array_order() {
    let regs = parse_install_entries(&install_fn());
    assert_eq!(regs.len(), 3);

    assert_eq!(regs[0].array_index, 0);
    assert_eq!(regs[0].id, "PotionKind::FirePotion");
    assert_eq!(regs[0].id_snake, "fire_potion");
    assert_eq!(regs[0].handler, "drink_fire_potion");

    assert_eq!(regs[1].array_index, 1);
    assert_eq!(regs[1].id, "PotionKind::KingsCourage");
    assert_eq!(regs[1].id_snake, "kings_courage");
    assert_eq!(regs[1].handler, "drink_kings_courage");

    assert_eq!(regs[2].array_index, 2);
    assert_eq!(regs[2].id, "PotionKind::AttackPotion");
    assert_eq!(regs[2].id_snake, "attack_potion");
    assert_eq!(regs[2].handler, "drink_attack_potion");
}

#[test]
fn id_snake_from_first_arg_not_fn_name() {
    // KingsCourage's id_snake is `kings_courage` (from PotionKind::KingsCourage),
    // independent of the handler fn name. This is the attribution guarantee that
    // defeats the `drink_character_card_potion` decoy.
    let regs = parse_install_entries(&install_fn());
    assert_eq!(regs[1].id_snake, "kings_courage");
    // The cast `as PotionEffectFn` is stripped from the handler.
    assert!(!regs[1].handler.contains("as"));
    assert!(!regs[1].handler.contains("PotionEffectFn"));
}
