//! Source: v0.111.0 MegaCrit.Sts2.Core.Models.Potions/Ambergris.cs.
use super::prelude::*;
pub(crate) fn drink_ambergris(state: &mut CombatState, _target: Option<u8>) -> EffectResult {
    if let Some(uid) = state.player.creature_uid {
        let max_hp = state
            .creatures
            .iter()
            .find(|c| c.id == uid.0)
            .map_or(0, |c| c.max_hp);
        state.creature_heal(uid, max_hp / 2);
        sts2_core::card_effects::apply_power_to_creature(
            state,
            uid,
            sts2_core::PowerKind::Ambergris,
            1,
        );
    }
    EffectResult::Resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beta_ambergris_heals_half_max_hp_and_banks_extra_turns() {
        let mut state = CombatState::default();
        state.player.max_hp = 80;
        state.player.current_hp = 10;
        state.init_player_creature();
        drink_ambergris(&mut state, None);
        assert_eq!(state.player.current_hp, 50);
        assert!(sts2_core::hooks::dispatch_should_take_extra_turn(
            &state,
            &sts2_core::hooks::CombatLifecycleCtx::default()
        ));
        drink_ambergris(&mut state, None);
        assert_eq!(state.player.current_hp, 80);
        assert!(state
            .powers
            .iter()
            .any(|p| p.kind == sts2_core::PowerKind::Ambergris && p.amount == 2));
    }
}
