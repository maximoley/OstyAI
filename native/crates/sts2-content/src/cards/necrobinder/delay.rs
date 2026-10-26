//! Hand-authored (C#-first port). Keep the prelude import.
use super::prelude::*;

/// Delay (Uncommon, 2 energy, Skill, Self). Gain 11 Block and 1 energy next turn.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Delay.cs
//
// C# `OnPlay`: `CreatureCmd.TriggerAnim("Cast")` (cosmetic, ignored),
// `CreatureCmd.GainBlock(Owner.Creature, DynamicVars.Block)`, then
// `PowerCmd.Apply<EnergyNextTurnPower>(Owner.Creature, DynamicVars.Energy,
// Owner.Creature, this)` — self-applied EnergyNextTurn.
// `CanonicalVars` = `BlockVar(11m)` + `EnergyVar(1)`, so base Block is 11 and
// base Energy is 1. `GainsBlock => true`.
// `OnUpgrade`: `Block.UpgradeValueBy(2m)` → 13 Block, and
// `Energy.UpgradeValueBy(1m)` → 2 EnergyNextTurn upgraded.
// Block is a stat field, so its upgrade bump lives in `upgrade()`
// (`base_block: Some(13)`); the EnergyNextTurn amount is a magnitude, so its
// upgrade bump is gated here on `current_card_upgraded`.
// No `CanonicalKeywords` (no Exhaust/Ethereal).
pub(crate) fn play_delay(state: &mut CombatState, _target: Option<CreatureUid>) -> EffectResult {
    gain_block_player(state, current_block(state));
    let energy = if current_card_upgraded(state) { 2 } else { 1 };
    apply_power_to_player(state, PowerKind::EnergyNextTurn, energy);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Delay,
        name: "DELAY",
        cost: 2,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Uncommon,
        base_damage: 0,
        base_block: 11,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade: Block.UpgradeValueBy(2) → 13. The EnergyNextTurn bump
    // (1 → 2) is a magnitude, applied in the handler's current_card_upgraded
    // branch, so it has no stat-field delta here.
    Some(CardUpgradeDelta {
        cost: None,
        base_damage: None,
        base_block: Some(13),
        keywords: None,
    })
}
