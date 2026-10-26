//! Source: beta `Models.Relics/UnceasingTop.cs`: AutoPrePlay, Play, AutoPostPlay.
use super::prelude::*;

pub(crate) fn on_after_hand_emptied_unceasing_top(
    state: &mut CombatState,
    _ctx: &crate::hooks::CombatLifecycleCtx,
) -> EffectResult {
    if !state.player_turn_phase().allows_empty_hand_draw() {
        return EffectResult::NotImplemented;
    }
    if !player_owns_relic(state, crate::RelicKind::UnceasingTop) {
        return EffectResult::NotImplemented;
    }
    // C# `CardPileCmd.Draw(fromHandDraw = false)` — vetoed by NoDraw/Fiddle.
    state.draw_into_hand_card_effect(UNCEASING_TOP_DRAW);
    EffectResult::Resolved
}

pub(crate) fn install() {
    register_owned_after_hand_emptied(
        RelicId::UnceasingTop,
        crate::RelicKind::UnceasingTop,
        on_after_hand_emptied_unceasing_top as AfterHandEmptiedFn,
    );
}

#[cfg(test)]
mod beta_tests {
    use super::*;
    use std::num::NonZeroU32;

    #[test]
    fn beta_top_draws_only_in_the_three_source_play_phases() {
        for phase in [
            crate::PlayerTurnPhase::None,
            crate::PlayerTurnPhase::Start,
            crate::PlayerTurnPhase::AutoPrePlay,
            crate::PlayerTurnPhase::Play,
            crate::PlayerTurnPhase::AutoPostPlay,
            crate::PlayerTurnPhase::End,
        ] {
            let mut state = CombatState {
                phase: crate::CombatPhase::PlayerPlayPhase,
                player_turn_phase_override: Some(phase),
                ..CombatState::default()
            };
            state.relics.push(crate::Relic {
                id: NonZeroU32::new(1).unwrap(),
                kind: crate::RelicKind::UnceasingTop,
                counter: 0,
                is_wax: false,
                is_melted: false,
            });
            let uid = crate::CardUid::new(1).unwrap();
            state.cards.push(crate::cards::Card {
                uid,
                card_id: crate::cards::CardId::StrikeIronclad,
                ..crate::cards::Card::pod_defaults()
            });
            state.player.draw.push(uid);
            on_after_hand_emptied_unceasing_top(
                &mut state,
                &crate::hooks::CombatLifecycleCtx::default(),
            );
            assert_eq!(
                state.player.hand.len(),
                usize::from(phase.allows_empty_hand_draw()),
                "{phase:?}"
            );
        }
    }
}
