//! Unplayable quest; progression is runstate::enter_room. Source: v0.111.0 Models.Cards/Dowsing.cs.
use super::prelude::*;
pub(crate) fn play_dowsing(_state: &mut CombatState, _target: Option<CreatureUid>) -> EffectResult {
    EffectResult::Resolved
}
