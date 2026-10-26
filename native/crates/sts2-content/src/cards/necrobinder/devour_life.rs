//! Devour Life (Necrobinder). Hand-authored C#-first port via the character-port skill.
//! C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/DevourLife.cs
use super::prelude::*;

/// Devour Life (Rare, 1 energy, Power, Self). Apply `DevourLifePower(1)`
/// (2 upgraded) to the player. While active, whenever the player plays a Soul
/// card the Osty is summoned/grown by the power's amount (the reactive lives in
/// `dispatch_after_card_played`).
//
// C# OnPlay:
//   CreatureCmd.TriggerAnim(Owner.Creature, "Cast", ...);  // cosmetic
//   PowerCmd.Apply<DevourLifePower>(Owner.Creature, DevourLifePower.BaseValue, Owner.Creature, this);
//
// CanonicalVars:
//   PowerVar<DevourLifePower>(1m)  → 1 stack. OnUpgrade +1 → 2.
pub(crate) fn play_devour_life(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    // DevourLifePower: 1 base, +1 on upgrade. The power amount is not a spec
    // field, so the upgrade bump is gated on current_card_upgraded.
    let amount = if current_card_upgraded(state) { 2 } else { 1 };
    apply_power_to_player(state, PowerKind::DevourLifePower, amount);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::DevourLifeCard,
        name: "DEVOUR_LIFE_CARD",
        cost: 1,
        card_type: CardType::Power,
        target_type: TargetType::Self_,
        rarity: CardRarity::Rare,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: DevourLifePower.UpgradeValueBy(1) (1 → 2). The power amount
    // is computed in the handler via current_card_upgraded, not a spec field, so
    // this returns None to keep one source of truth.
    None
}
