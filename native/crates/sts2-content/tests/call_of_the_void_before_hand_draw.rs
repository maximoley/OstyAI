//! Necrobinder `CallOfTheVoidPower.BeforeHandDraw` integration test.
//!
//! With the content registry installed (so the `GetDistinctForCombat`
//! character-pool sampler trampoline is registered), a player carrying
//! `CallOfTheVoidPower(N)` should, at the power-side `before_hand_draw`
//! channel, generate `N` Ethereal cards from the Necrobinder combat pool into
//! hand. This is the only path that exercises the new
//! `hooks::dispatch_before_hand_draw_powers` end to end with a real pool —
//! sts2-core unit tests can't, since the sampler lives in sts2-content.
//!
//! RNG-parity to C# is NOT asserted (the Necrobinder pool is incomplete at this
//! pin); the test pins the observable contract: N cards added, each Ethereal,
//! each a real Necrobinder pool card.

use sts2_core::card_effects::card_ethereal_inst;
use sts2_core::potions::CharacterId;
use sts2_core::{CombatState, Power, PowerKind, PowerUid};

use sts2_content::cards::dispatch as content_dispatch;

/// Build a Necrobinder combat state with `CallOfTheVoidPower(amount)` on the
/// player and a seeded combat-card-generation stream.
fn state_with_call_of_the_void(amount: i32) -> CombatState {
    let mut state = CombatState {
        run_rng: sts2_core::run_rng_set::RunRngSet::new(12345_i32),
        ..Default::default()
    };
    state.player.character = Some(CharacterId::Necrobinder);
    let pid = PowerUid::new(1).expect("nz");
    state.powers.push(Power {
        id: pid.0,
        kind: PowerKind::CallOfTheVoid,
        amount,
        counter: 0,
        applier: None,
        skip_next_tick: false,
    });
    state.player.powers.push(pid);
    state
}

#[test]
fn call_of_the_void_adds_ethereal_pool_cards_to_hand() {
    content_dispatch::install_default_registry();
    let mut state = state_with_call_of_the_void(2);
    assert!(state.player.hand.is_empty(), "hand starts empty");

    sts2_core::hooks::dispatch_before_hand_draw_powers(&mut state);

    assert_eq!(
        state.player.hand.len(),
        2,
        "CallOfTheVoid(2) adds 2 cards to hand before the draw"
    );
    for uid in &state.player.hand {
        let card = state
            .cards
            .iter()
            .find(|c| c.uid == *uid)
            .expect("generated card present in arena");
        assert!(
            card_ethereal_inst(card),
            "each generated card is made Ethereal ({:?})",
            card.card_id
        );
        // Sanity: it is a real card drawn from the Necrobinder combat pool —
        // its id appears in NECROBINDER_SPECS (the generation source). We check
        // pool membership directly rather than `character_of`, whose hand-keyed
        // map still has gaps for several Necrobinder cards (e.g. CountdownCard).
        let in_pool = sts2_content::cards::spec_index::NECROBINDER_SPECS
            .iter()
            .any(|f| f().id == card.card_id);
        assert!(
            in_pool,
            "generated card belongs to the Necrobinder pool ({:?})",
            card.card_id
        );
    }
}

#[test]
fn no_call_of_the_void_is_noop() {
    content_dispatch::install_default_registry();
    let mut state = CombatState::default();
    state.player.character = Some(CharacterId::Necrobinder);
    sts2_core::hooks::dispatch_before_hand_draw_powers(&mut state);
    assert!(
        state.player.hand.is_empty(),
        "no CallOfTheVoid power → no cards generated"
    );
}
