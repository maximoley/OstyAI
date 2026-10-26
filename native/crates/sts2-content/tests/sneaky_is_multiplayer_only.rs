//! `SneakyPower` grants no Block in single-player — and that is faithful,
//! not a gap (GH #638).
//!
//! GH #638 asked for `SneakyPower.AfterCardPlayed` to be ported, describing it
//! as "in single-player it is the monsters' attacks that feed the player's
//! Block". That reading is wrong, and this file exists so nobody acts on it.
//!
//! C# `SneakyPower.cs:24`:
//!
//! [decompiled C# omitted — see the cited path]
//!
//! `CardModel.Owner` is a **`Player`** (`CardModel.cs:334`), not a `Creature`.
//! Monsters own no cards, so `cardPlay.Card.Owner.Creature` is always *some
//! player's* creature and the `!=` means "a DIFFERENT PLAYER played an
//! Attack" — never "a monster attacked". Three things confirm the power is
//! multiplayer-only by construction:
//!
//!   * `Sneaky.cs:19` — `MultiplayerConstraint => MultiplayerOnly`. C#
//!     `CardFactory.FilterForPlayerCount` drops the card from every
//!     single-player pool, and Rust mirrors that in
//!     `spec_index::is_multiplayer_only`.
//!   * `Sneaky.cs:30` is the only site in the whole decomp that applies
//!     `SneakyPower`.
//!   * Rust has no allies in production: every `state.allies.push` is inside a
//!     `#[cfg(test)]` block, so there is no second card owner to compare
//!     against.
//!
//! So the guard is `false` for every reachable single-player state. Porting it
//! would add a branch no test could reach and a `Flash()` site that could never
//! fire; the `SNEAKY` row in `power_flash_sites.json` stays `ported: false`
//! with that reason recorded, alongside `GRAVITY` and `MAGIC_BOMB`.
//!
//! The C# oracle agrees: the `card:SNEAKY_CARD` coverage trial is clean
//! (`parity_audit.py --coverage --coverage-subjects 'card:(SNEAKY_CARD)$'
//! --coverage-character Silent`, recorded fresh 2026-09-02).
//!
//! The test below is the guard against the plausible mis-port — "any Attack
//! grants the owner Block" — which would fire on the player's own Strike.

use sts2_content::cards::dispatch as content_dispatch;
use sts2_core::cards::CardId;
use sts2_core::{CardUid, CombatState, MonsterKind, PowerKind};

/// A play-ready Silent state with `cards` in hand, in order.
fn state_with_cards_in_hand(cards: &[CardId]) -> CombatState {
    content_dispatch::install_default_registry();
    // `install_default_registry` wires the effect dispatch but not the card POD
    // factory, which lives on its own OnceLock. Idempotent.
    sts2_core::register_card_factory(sts2_content::cards::spec_index::card_pod_from_id);
    let mut state = CombatState {
        run_rng: sts2_core::run_rng_set::RunRngSet::new(4242_i32),
        ..Default::default()
    };
    state.player.character = Some(sts2_core::potions::CharacterId::Silent);
    state.player.current_hp = 80;
    state.player.max_hp = 80;
    state.player.energy = 3;
    state.phase = sts2_core::CombatPhase::PlayerPlayPhase;
    // `play_sneaky` applies the power through `state.player.creature_uid`, so
    // the mirror Creature must exist or the card's own effect is a no-op.
    state.init_player_creature();

    let enemy = state.spawn_enemy(MonsterKind::Nibbit);
    if let Some(c) = state.creatures.iter_mut().find(|c| c.id == enemy.0) {
        c.max_hp = 200;
        c.current_hp = 200;
    }

    let factory = sts2_core::lookup_card_factory().expect("card factory installed");
    for &id in cards {
        let mut card = factory(id, false).expect("card factory has the card");
        let uid = CardUid::new(u32::try_from(state.cards.len() + 1).expect("fits")).expect("nz");
        card.uid = uid;
        state.cards.push(card);
        state.player.hand.push(uid);
    }
    state
}

#[test]
fn sneaky_grants_no_block_when_the_player_attacks() {
    let mut state = state_with_cards_in_hand(&[CardId::SneakyCard, CardId::StrikeSilent]);

    // Play Sneaky. Cost 2, Power, Self.
    let _ = sts2_core::combat_loop::play_card(&mut state, 0, Some(0));
    assert_eq!(
        state.player_power_amount(PowerKind::Sneaky),
        1,
        "the card really applied the power — without this the test below is \
         vacuous"
    );
    assert_eq!(state.player.block, 0, "applying the power grants no Block");

    // Play Strike. The player owns it, so C#'s `!=` guard rejects it.
    let _ = sts2_core::combat_loop::play_card(&mut state, 0, Some(0));

    assert_eq!(
        state.player.block, 0,
        "the player's OWN Attack must not feed Sneaky: C# compares the card's \
         owning PLAYER against the power's owner, and in single-player they \
         are always the same creature"
    );
    assert_eq!(
        state.player_power_amount(PowerKind::Sneaky),
        1,
        "and the stack is untouched — SneakyPower is a Counter that never \
         decrements here"
    );
}
