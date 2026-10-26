//! Final beta v0.111.0 Pendulum: natural hand-draw modifier.
use super::prelude::*;

pub(crate) fn before_hand_draw_pendulum(state: &mut CombatState) -> EffectResult {
    if !player_owns_relic(state, crate::RelicKind::Pendulum) {
        return EffectResult::NotImplemented;
    }
    if let Some(relic) = state
        .relics
        .iter_mut()
        .find(|r| !r.is_melted && r.kind == crate::RelicKind::Pendulum)
    {
        relic.counter = (relic.counter + 1) % 3;
        return EffectResult::Resolved;
    }
    EffectResult::NotImplemented
}
pub(crate) fn modify_hand_draw_pendulum(state: &CombatState, current: i32) -> i32 {
    if state
        .relics
        .iter()
        .any(|r| !r.is_melted && r.kind == crate::RelicKind::Pendulum && r.counter == 0)
    {
        current + 1
    } else {
        current
    }
}
pub(crate) fn install() {
    register_owned_before_hand_draw(
        RelicId::Pendulum,
        crate::RelicKind::Pendulum,
        before_hand_draw_pendulum as BeforeHandDrawFn,
    );
    register_owned_modify_hand_draw(
        RelicId::Pendulum,
        crate::RelicKind::Pendulum,
        modify_hand_draw_pendulum as ModifyHandDrawFn,
    );
}
