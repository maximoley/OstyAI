//! `HelloWorldPower.BeforeHandDraw` integration test (GH #636).
//!
//! C# `HelloWorldPower.cs:21-29`:
//!
//! [decompiled C# omitted — see the cited path]
//!
//! The rarity pre-filter is the whole point of this test file. A port that
//! samples the character's *whole* combat pool adds the right NUMBER of cards
//! to hand and passes any count-only assertion — while picking the wrong cards
//! AND reading a different number of ints off `combat_card_generation` (the
//! shuffle length is the filtered list's length), which desyncs every later
//! roll in the combat. So every test here asserts the RARITY of what landed,
//! and `the_pool_has_non_common_cards_to_exclude` proves the filter has
//! something to do.
//!
//! Sibling of `call_of_the_void_before_hand_draw.rs`; same reason it lives in
//! sts2-content — the sampler is content-side, so an sts2-core unit test would
//! see an unregistered trampoline and an empty hand.

use sts2_content::cards::dispatch as content_dispatch;
use sts2_content::cards::types::CardRarity;
use sts2_core::potions::CharacterId;
use sts2_core::{CombatState, Power, PowerKind, PowerUid};

/// Build a Defect combat state with `HelloWorldPower(amount)` on the player
/// and a seeded combat-card-generation stream.
fn state_with_hello_world(amount: i32) -> CombatState {
    let mut state = CombatState {
        run_rng: sts2_core::run_rng_set::RunRngSet::new(12345_i32),
        ..Default::default()
    };
    state.player.character = Some(CharacterId::Defect);
    let pid = PowerUid::new(1).expect("nz");
    state.powers.push(Power {
        id: pid.0,
        kind: PowerKind::HelloWorld,
        amount,
        counter: 0,
        applier: None,
        skip_next_tick: false,
    });
    state.player.powers.push(pid);
    state
}

/// The rarity of `id` in the Defect spec slice, or `None` if the id is not a
/// Defect card at all.
fn defect_rarity(id: sts2_core::cards::CardId) -> Option<CardRarity> {
    sts2_content::cards::spec_index::DEFECT_SPECS
        .iter()
        .map(|f| f())
        .find(|s| s.id == id)
        .map(|s| s.rarity)
}

#[test]
fn hello_world_adds_common_pool_cards_to_hand() {
    content_dispatch::install_default_registry();
    let mut state = state_with_hello_world(2);
    assert!(state.player.hand.is_empty(), "hand starts empty");

    sts2_core::hooks::dispatch_before_hand_draw_powers(&mut state);

    assert_eq!(
        state.player.hand.len(),
        2,
        "HelloWorld(2) adds 2 cards to hand before the draw"
    );
    for uid in &state.player.hand {
        let card = state
            .cards
            .iter()
            .find(|c| c.uid == *uid)
            .expect("generated card present in arena");
        assert_eq!(
            defect_rarity(card.card_id),
            Some(CardRarity::Common),
            "C# filters the pool to `Rarity == Common` BEFORE the shuffle, so \
             every generated card must be Common ({:?})",
            card.card_id
        );
    }
}

#[test]
fn the_pool_has_non_common_cards_to_exclude() {
    // Without this, `hello_world_adds_common_pool_cards_to_hand` would pass on
    // a pool that happened to be all-Common and the rarity filter could be
    // deleted unnoticed. The Defect combat pool is overwhelmingly NOT Common.
    let non_common = sts2_content::cards::spec_index::DEFECT_SPECS
        .iter()
        .map(|f| f())
        .filter(|s| s.rarity != CardRarity::Common)
        .count();
    assert!(
        non_common > 10,
        "the Defect pool must carry plenty of non-Common cards for the rarity \
         filter to exclude, else the Common assertions above are vacuous \
         (found {non_common})"
    );
}

#[test]
fn hello_world_generates_distinct_cards() {
    // C# uses `GetDistinctForCombat` (one whole-pool shuffle, then take
    // `count`) rather than CallOfTheVoid's N independent count-1 picks, so the
    // batch never repeats a card id. Three cards is enough to catch a
    // with-replacement port on a pool this size within a few seeds.
    content_dispatch::install_default_registry();
    let mut state = state_with_hello_world(3);

    sts2_core::hooks::dispatch_before_hand_draw_powers(&mut state);

    let mut ids: Vec<_> = state
        .player
        .hand
        .iter()
        .filter_map(|uid| state.cards.iter().find(|c| c.uid == *uid))
        .map(|c| c.card_id)
        .collect();
    assert_eq!(ids.len(), 3);
    ids.sort_by_key(|id| id.to_python_name());
    ids.dedup();
    assert_eq!(ids.len(), 3, "GetDistinctForCombat never repeats a card");
}

#[test]
fn zero_stacks_generate_nothing() {
    // C# guard is `AmountOnTurnStart >= 1`. A power at 0 stacks is not merely
    // a no-op batch: it must not touch `combat_card_generation` at all, since
    // a stray shuffle would desync every later roll.
    content_dispatch::install_default_registry();
    let mut state = state_with_hello_world(0);
    let before = state.run_rng.combat_card_generation.clone();

    sts2_core::hooks::dispatch_before_hand_draw_powers(&mut state);

    assert!(state.player.hand.is_empty(), "0 stacks generates no cards");
    assert_eq!(
        format!("{:?}", state.run_rng.combat_card_generation),
        format!("{before:?}"),
        "0 stacks must not read the combat_card_generation stream"
    );
}

#[test]
fn no_hello_world_is_noop() {
    content_dispatch::install_default_registry();
    let mut state = CombatState::default();
    state.player.character = Some(CharacterId::Defect);
    sts2_core::hooks::dispatch_before_hand_draw_powers(&mut state);
    assert!(
        state.player.hand.is_empty(),
        "no HelloWorld power → no cards generated"
    );
}
