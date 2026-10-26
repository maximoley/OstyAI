//! Necrobinder — Danse Macabre.
use super::prelude::*;

/// Danse Macabre (Uncommon, 1 energy, Power, Self). Applies DanseMacabrePower
/// (Counter) to self with amount = the PowerVar base value (4, upgraded 6).
/// DanseMacabrePower then grants `Amount` Block (Unpowered) before the owner
/// plays any card whose resolved energy cost is >= 2 (the power's EnergyVar).
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/DanseMacabre.cs
//
// CanonicalVars:
//   PowerVar<DanseMacabrePower>(4m)   -> stacks applied (OnUpgrade: +2 -> 6)
//   EnergyVar(2)                      -> hover-tip only on the card; the real
//                                        threshold lives on DanseMacabrePower.
// OnPlay: PowerCmd.Apply<DanseMacabrePower>(Owner, DynamicVars["DanseMacabrePower"]
//   .BaseValue) — the "Cast" anim is cosmetic and ignored. The applied amount is
//   a PowerVar magnitude (not damage/block/cost), so the OnUpgrade +2 bump is
//   gated here on current_card_upgraded and upgrade() returns None.
pub(crate) fn play_danse_macabre(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    let amount = if current_card_upgraded(state) { 6 } else { 4 };
    apply_power_to_player(state, PowerKind::DanseMacabre, amount);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::DanseMacabre,
        name: "DANSE_MACABRE",
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
    // C# OnUpgrade only bumps the DanseMacabrePower PowerVar amount (4→6); that
    // is applied in the handler via current_card_upgraded, not as a stat field.
    None
}
