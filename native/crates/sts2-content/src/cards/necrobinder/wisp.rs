//! Hand-authored C#-first port. Keep the prelude import.
use super::prelude::*;

/// Wisp (Common, 0 energy, Skill, Self). Gain 1 energy. Exhaust.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Wisp.cs
//
// CanonicalVars:   EnergyVar(1)            -> energy gained on play (1)
// CanonicalKeywords: Exhaust
// ctor: base(0, CardType.Skill, CardRarity.Common, TargetType.Self)
// OnPlay:   PlayerCmd.GainEnergy(Energy.IntValue, Owner)  -> gain 1 energy
// OnUpgrade: AddKeyword(CardKeyword.Retain)               -> spec keyword delta
//
// The energy gain (1) is NOT upgraded (OnUpgrade only adds the Retain keyword),
// so there is no `current_card_upgraded` branch in the handler; the upgrade is
// purely a spec keyword delta recorded in `upgrade()`.
pub(crate) fn play_wisp(state: &mut CombatState, _target: Option<CreatureUid>) -> EffectResult {
    // PlayerCmd.GainEnergy(Energy.IntValue=1, Owner).
    state.gain_energy(1);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Wisp,
        name: "WISP",
        cost: 0,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Common,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::EXHAUST,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade: AddKeyword(CardKeyword.Retain). The base Exhaust keyword is
    // retained and Retain is OR-ed in.
    Some(CardUpgradeDelta {
        cost: None,
        base_damage: None,
        base_block: None,
        keywords: Some(CardKeywords::EXHAUST.with(CardKeywords::RETAIN)),
    })
}
