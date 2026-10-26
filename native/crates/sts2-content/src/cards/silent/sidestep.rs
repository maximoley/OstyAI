//! Source: v0.111.0 Models.Cards/Sidestep.cs.
use super::prelude::*;
pub(crate) fn play_sidestep(state: &mut CombatState, _target: Option<CreatureUid>) -> EffectResult {
    apply_power_to_player(
        state,
        PowerKind::EnergyNextTurn,
        if current_card_upgraded(state) { 2 } else { 1 },
    );
    EffectResult::Resolved
}
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Sidestep,
        name: "SIDESTEP",
        cost: 0,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Uncommon,
        ..CardSpec::base()
    }
}
