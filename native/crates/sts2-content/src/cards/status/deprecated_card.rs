//! Source: v0.111.0 Models.Cards/DeprecatedCard.cs.
use super::prelude::*;
pub(crate) fn play_deprecated_card(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    state.draw_with_followup(
        1,
        sts2_core::DrawFollowup::DeprecatedRemoveDeckVersion,
        remove_deck_version,
    );
    EffectResult::Resolved
}
fn remove_deck_version(state: &mut CombatState, source: Option<CardUid>, _cards: &[CardUid]) {
    if let Some(uid) = source {
        let index = uid.0.get() as usize - 1;
        let deck_index = state.cards[index].combat_var(sts2_core::cards::VarKey::MasterDeckIndex);
        if deck_index > 0 {
            state.cards[index].set_combat_var(
                sts2_core::cards::VarKey::RemoveMasterDeckInstance,
                deck_index,
            );
            state.cards[index].set_combat_var(sts2_core::cards::VarKey::MasterDeckIndex, 0);
        }
    }
}
