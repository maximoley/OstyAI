//! Eradicate (Necrobinder). Hand-authored C#-first port via character-port skill.
//! C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Eradicate.cs
use super::prelude::*;

/// Eradicate (Rare, X energy, Attack — Retain). Deal 11 damage to a single
/// enemy X times (X = energy spent). Upgraded: 14 damage per hit.
//
// C# `Eradicate.cs`:
//   HasEnergyCostX => true                      → spec cost: -1 (X-cost)
//   CanonicalKeywords: { Retain }               → keywords: RETAIN
//   CanonicalVars: DamageVar(11m, Move)         → base_damage: 11
//   ctor base(0, Attack, Rare, AnyEnemy)        → Attack / Rare / Enemy (AnyEnemy collapses)
//   OnPlay:
//     DamageCmd.Attack(Damage.BaseValue)        → deal_damage_player_to_enemy
//       .WithHitCount(ResolveEnergyXValue())    → loop X (energy spent) times
//       .Targeting(cardPlay.Target)
//   OnUpgrade: Damage.UpgradeValueBy(3m) (11→14) — a damage STAT, so the delta
//     lives in the central `cards/card_upgrades_generated.rs` table
//     (CardId::Eradicate => base_damage: Some(14)). The engine stamps the
//     upgraded base_damage, so `current_damage(state)` already returns 11/14
//     and `upgrade()` returns None here (no per-card delta, no upgraded branch).
pub(crate) fn play_eradicate(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult {
    let t = target.expect("Eradicate: Enemy-targeted");
    let dmg = current_damage(state);
    // C# `.WithHitCount(ResolveEnergyXValue())` — X = energy spent on this card.
    // X-cost reads the stamped `energy_spent`, NOT `player.energy` (already drained).
    let hits = current_card_energy_spent(state);
    for _ in 0..hits {
        let _ = deal_damage_player_to_enemy(state, t, dmg);
    }
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Eradicate,
        name: "ERADICATE",
        cost: -1, // C# HasEnergyCostX => true
        card_type: CardType::Attack,
        target_type: TargetType::Enemy, // C# AnyEnemy collapses to single-player Enemy
        rarity: CardRarity::Rare,
        base_damage: 11,
        base_block: 0,
        keywords: CardKeywords::RETAIN,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade `Damage.UpgradeValueBy(3m)` (11 → 14) lives in the central
    // `card_upgrades_generated.rs` table (CardId::Eradicate), so no per-card
    // delta here.
    None
}
