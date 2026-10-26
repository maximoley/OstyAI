//! Hand-authored (C#-first port). Necrobinder Sow.
use super::prelude::*;

/// `Sow` (Common, 1 energy, Attack, Retain). Deal 8 damage to ALL enemies.
//
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Sow.cs
//   CanonicalVars     = [ DamageVar(8, Move) ]
//   CanonicalKeywords = [ Retain ]
//   ctor base(1, CardType.Attack, CardRarity.Common, TargetType.AllEnemies)
//   OnPlay    -> DamageCmd.Attack(Damage.BaseValue).TargetingAllOpponents
//   OnUpgrade -> Damage.UpgradeValueBy(3)  (8 -> 11; via spec CardUpgradeDelta)
pub(crate) fn play_sow(state: &mut CombatState, _target: Option<CreatureUid>) -> EffectResult {
    // `current_damage` already reflects the OnUpgrade 8->11 bump via the
    // spec's CardUpgradeDelta (base_damage). AllEnemies → AoE.
    deal_damage_all_enemies(state, current_damage(state));
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Sow,
        name: "SOW",
        cost: 1,
        card_type: CardType::Attack,
        target_type: TargetType::AllEnemies,
        rarity: CardRarity::Common,
        base_damage: 8, // DamageVar(8)
        base_block: 0,
        keywords: CardKeywords::RETAIN, // CanonicalKeywords = [ Retain ]
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade -> Damage.UpgradeValueBy(3): 8 -> 11.
    Some(CardUpgradeDelta {
        cost: None,
        base_damage: Some(11),
        base_block: None,
        keywords: None,
    })
}
