//! Necrobinder — Haunt.
use super::prelude::*;

/// Haunt (Uncommon, 1 energy, Power). Applies HauntPower (Counter) to self
/// with amount = HpLoss base value (7, upgraded 9). HauntPower then deals that
/// much unblockable+unpowered damage to a random hittable enemy each time the
/// owner plays a Soul.
// C# source: ../sts2-wiki/decompiled/v0.111.0/MegaCrit.Sts2.Core.Models.Cards/Haunt.cs
pub(crate) fn play_haunt(state: &mut CombatState, _target: Option<CreatureUid>) -> EffectResult {
    // C# Haunt.OnPlay: PowerCmd.Apply<HauntPower>(Owner, DynamicVars.HpLoss.BaseValue).
    // HpLossVar(7m); OnUpgrade → UpgradeValueBy(2m) ⇒ 9 (gated, not a stat field).
    // TargetType::Self_ — no enemy target.
    let amount = if current_card_upgraded(state) { 9 } else { 7 };
    apply_power_to_player(state, PowerKind::Haunt, amount);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Haunt,
        name: "HAUNT",
        cost: 1,
        card_type: CardType::Power,
        target_type: TargetType::Self_,
        rarity: CardRarity::Uncommon,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade only bumps the HpLoss PowerVar amount (7→9); that is applied
    // in the handler via current_card_upgraded, not as a damage/block/cost stat.
    None
}
