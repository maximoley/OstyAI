//! v0.111.0 SetupPlayerTurn: reset energy, before-draw hooks, draw-count
//! modifiers, natural draw, then Bellows.AfterPlayerTurnStart.
use std::num::NonZeroU32;
use sts2_core::cards::CardId;
use sts2_core::{CardUid, CombatState, PowerKind, Relic, RelicKind, StepResult};
fn state() -> CombatState {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    let mut state = CombatState::default();
    state.player.character = Some(sts2_core::CharacterId::Necrobinder);
    state.player.current_hp = 66;
    state.player.max_hp = 66;
    state.init_player_creature();
    state.round = 1;
    state.phase = sts2_core::CombatPhase::PlayerTurnStart;
    state.side = sts2_core::Side::Player;
    for raw in 1..=12 {
        let mut card =
            sts2_content::cards::spec_index::card_pod_from_id(CardId::DefendNecrobinder, false)
                .unwrap();
        card.uid = CardUid::new(raw).unwrap();
        state.player.draw.push(card.uid);
        state.cards.push(card);
    }
    state
}
fn relic(state: &mut CombatState, kind: RelicKind, counter: i32) {
    state.relics.push(Relic {
        id: NonZeroU32::new(state.relics.len() as u32 + 1).unwrap(),
        kind,
        counter,
        is_wax: false,
        is_melted: false,
    });
}
#[test]
fn pendulum_extra_opening_draw_is_upgraded_by_bellows_in_either_acquisition_order() {
    for reverse in [false, true] {
        let mut state = state();
        if reverse {
            relic(&mut state, RelicKind::Bellows, 0);
            relic(&mut state, RelicKind::Pendulum, 2);
        } else {
            relic(&mut state, RelicKind::Pendulum, 2);
            relic(&mut state, RelicKind::Bellows, 0);
        }
        let _ = sts2_core::combat_loop::start_player_turn(&mut state);
        assert_eq!(state.player.hand.len(), 6);
        assert_eq!(state.player.draw.len(), 6);
        for uid in &state.player.hand {
            let card = &state.cards[uid.0.get() as usize - 1];
            assert!(card.upgraded);
            assert_eq!(card.base_block, 8);
        }
        assert_eq!(
            state
                .relics
                .iter()
                .find(|r| r.kind == RelicKind::Pendulum)
                .unwrap()
                .counter,
            0
        );
    }
}
#[test]
fn pendulum_advances_before_draw_fold_and_only_each_third_turn_adds_one() {
    let mut state = state();
    relic(&mut state, RelicKind::Pendulum, 0);
    for expected in [5, 5, 6, 5, 5, 6] {
        let _ = sts2_core::relic_effects::dispatch_before_hand_draw(&mut state);
        assert_eq!(
            sts2_core::relic_effects::dispatch_modify_hand_draw(&state, 5),
            expected
        );
    }
}
#[test]
fn energy_reset_effects_finish_before_toolbox_suspends_before_draw() {
    let mut state = state();
    relic(&mut state, RelicKind::Toolbox, 0);
    let owner = state.player.creature_uid.unwrap();
    sts2_core::card_effects::apply_power_to_creature(&mut state, owner, PowerKind::StarNextTurn, 3);
    assert_eq!(
        sts2_core::combat_loop::start_player_turn(&mut state),
        StepResult::PendingChoice
    );
    assert!(state.player.hand.is_empty());
    assert_eq!(state.player.stars, 3);
    assert_eq!(state.player_power_amount(PowerKind::StarNextTurn), 0);
    assert!(state.pending_relic_hand_draw_resume.is_some());
}
