//! Source: v0.111.0/MegaCrit.Sts2.Core.Models.Relics/BrilliantScarf.cs.
use std::num::NonZeroU32;
use sts2_core::cards::CardId;
use sts2_core::relic_effects::{self, CardEventCtx};
use sts2_core::{CardUid, CombatState, Relic, RelicKind, Side};

fn fixture(id: CardId) -> (CombatState, CardUid) {
    sts2_content::relics::dispatch::ensure_installed();
    let mut state = CombatState::default();
    state.player.current_hp = 66;
    state.player.max_hp = 66;
    state.init_player_creature();
    state.relics.push(Relic {
        id: NonZeroU32::new(1).unwrap(),
        kind: RelicKind::BrilliantScarf,
        counter: 4,
        is_wax: false,
        is_melted: false,
    });
    let mut card = sts2_content::cards::spec_index::card_pod_from_id(id, false).unwrap();
    card.uid = CardUid::new(1).unwrap();
    state.cards.push(card);
    state.player.hand.push(card.uid);
    (state, card.uid)
}

#[test]
fn fifth_manual_play_zeroes_energy_after_surcharges_and_zeroes_stars() {
    let (mut state, uid) = fixture(CardId::DyingStar);
    assert_eq!(sts2_core::effective_star_cost(&state.cards[0]), 3);
    assert_eq!(
        sts2_core::effective_star_cost_in_combat(&state, &state.cards[0]),
        0
    );
    assert_eq!(sts2_core::hooks::effective_card_cost(&state, uid), 0);
    let player = state.player.creature_uid.unwrap();
    sts2_core::card_effects::apply_power_to_creature(
        &mut state,
        player,
        sts2_core::PowerKind::Tangled,
        2,
    );
    assert_eq!(sts2_core::hooks::effective_card_cost(&state, uid), 0);
    state.relics[0].counter = 5;
    assert_eq!(
        sts2_core::effective_star_cost_in_combat(&state, &state.cards[0]),
        3
    );
    assert!(sts2_core::hooks::effective_card_cost(&state, uid) >= 2);
}

#[test]
fn fifth_play_discount_obeys_pile_and_melted_gates() {
    let (mut state, uid) = fixture(CardId::DefendNecrobinder);
    assert_eq!(sts2_core::hooks::effective_card_cost(&state, uid), 0);
    state.player.hand.clear();
    state.player.draw.push(uid);
    assert_eq!(sts2_core::hooks::effective_card_cost(&state, uid), 1);
    state.player.draw.clear();
    state.player.play.push(uid);
    assert_eq!(sts2_core::hooks::effective_card_cost(&state, uid), 0);
    state.relics[0].is_melted = true;
    assert_eq!(sts2_core::hooks::effective_card_cost(&state, uid), 1);
    state.relics[0].is_melted = false;
    state.is_over = true;
    assert_eq!(sts2_core::hooks::effective_card_cost(&state, uid), 1);
}

#[test]
fn automatic_plays_do_not_consume_scarf_and_combat_end_resets_counter() {
    let (mut state, uid) = fixture(CardId::DefendNecrobinder);
    let mut ctx = CardEventCtx {
        card_id: CardId::DefendNecrobinder,
        card_uid: Some(uid),
        target: None,
        card_tags: sts2_core::cards::CardTags::NONE,
        energy_spent: 0,
        is_autoplay: true,
    };
    let _ = relic_effects::dispatch_after_card_played(&mut state, &ctx);
    assert_eq!(state.relics[0].counter, 4);
    ctx.is_autoplay = false;
    let _ = relic_effects::dispatch_after_card_played(&mut state, &ctx);
    assert_eq!(state.relics[0].counter, 5);
    let _ = relic_effects::dispatch_before_side_turn_start(&mut state, Side::Enemy);
    assert_eq!(state.relics[0].counter, 5);
    let _ = relic_effects::dispatch_after_combat_end(&mut state);
    assert_eq!(state.relics[0].counter, 0);
}
