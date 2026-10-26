//! Calcify (Necrobinder pool). C#-first port.
use super::prelude::*;

/// Calcify (Uncommon, 1 energy, Power). Apply Calcify (4; +2 upgraded) to self.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Calcify.cs
// OnPlay: PowerCmd.Apply<CalcifyPower>(Owner, CalcifyPower.BaseValue=4, Owner).
// OnUpgrade: CalcifyPower.UpgradeValueBy(2). Self-target, so `target` is unused.
pub(crate) fn play_calcify(state: &mut CombatState, _target: Option<CreatureUid>) -> EffectResult {
    let amount = if current_card_upgraded(state) { 6 } else { 4 };
    apply_power_to_player(state, PowerKind::Calcify, amount);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::CalcifyCard,
        name: "CALCIFY_CARD",
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
    // OnUpgrade bumps the CalcifyPower amount (PowerVar), not a stat field; the
    // +2 is applied in the handler via current_card_upgraded.
    None
}
