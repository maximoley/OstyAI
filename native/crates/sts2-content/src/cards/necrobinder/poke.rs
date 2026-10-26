//! Poke (Necrobinder). C#-first port via the character-port skill.
use super::prelude::*;

/// Poke (Common, 0 energy, Attack). The Osty pokes the target for 6 (9
/// upgraded). Does nothing if the Osty is missing.
//
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Poke.cs
//
// C# OnPlay:
//   ArgumentNullException.ThrowIfNull(cardPlay.Target);
//   if (!Osty.CheckMissingWithAnim(Owner)) {
//       DamageCmd.Attack(OstyDamage.BaseValue)   // 6, +3 on upgrade → 9
//           .FromOsty(Owner.Osty, this)          // attacker is the OSTY, not the player
//           .Targeting(cardPlay.Target)
//           .Execute(...);
//   }
//
// CanonicalVars: OstyDamageVar(6m, Move)   → base_damage 6 (Move value-prop).
// CanonicalTags: OstyAttack                → CardTag::OstyAttack (cosmetic glow tag;
//                already exists core-side, not carried in CardSpec metadata here).
// ShouldGlowRedInternal => Owner.IsOstyMissing — pure UI, ignored.
//
// Osty-missing guard IS faithful here: when the Osty is absent/dead the card
// is a no-op (matching `!CheckMissingWithAnim`). The attacker-attribution
// (`FromOsty`) is NOT expressible — see the BLOCKER note below.
pub(crate) fn play_poke(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult {
    let target = target.expect("Poke requires an enemy target");

    // C# `!Osty.CheckMissingWithAnim(Owner)` — only attack when the Osty is
    // present and alive. Mirror via the player's pet creature.
    let osty_alive = state
        .player
        .pet_uid
        .and_then(|uid| state.creatures.iter().find(|c| c.id == uid.0))
        .is_some_and(|c| c.current_hp > 0);
    if !osty_alive {
        return EffectResult::Resolved;
    }

    // OstyDamage.BaseValue: 6 base, +3 on upgrade (OnUpgrade UpgradeValueBy(3m)).
    let dmg = if current_card_upgraded(state) { 9 } else { 6 };

    // C# `DamageCmd.Attack(dmg).FromOsty(Owner.Osty, this).Targeting(target)` —
    // the Osty is the dealer, so the hit reads the Osty's dealer-side modifiers
    // (Strength, Calcify's `dealer is Osty` gate) and player-dealer-gated
    // effects correctly do not fire. `deal_damage_from_osty_to_enemy` resolves
    // the dealer as the pet creature (see `osty_dealer_ref`).
    let _ = deal_damage_from_osty_to_enemy(state, target, dmg);

    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Poke,
        name: "POKE",
        cost: 0,
        card_type: CardType::Attack,
        target_type: TargetType::Enemy, // C# AnyEnemy → single-player Enemy
        rarity: CardRarity::Common,
        base_damage: 6, // OstyDamageVar(6m)
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: OstyDamage.UpgradeValueBy(3m) → 6 → 9.
    Some(CardUpgradeDelta {
        cost: None,
        base_damage: Some(9),
        base_block: None,
        keywords: None,
    })
}
