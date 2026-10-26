//! Hand-authored C#-first port. Keep the prelude import.
use super::prelude::*;

/// Unleash (Basic, 1 energy, Attack). Osty-attack: if the Osty pet is alive,
/// deal `CalculationBase + ExtraDamage * Osty.CurrentHp` damage to the target;
/// if the Osty is missing, the card does nothing.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Unleash.cs
//
// CanonicalVars:
//   CalculationBaseVar(6)             -> base (OnUpgrade: +3 -> 9)
//   ExtraDamageVar(1).FromOsty()      -> extra multiplier coefficient
//   CalculatedDamageVar(Move).FromOsty().WithMultiplier(osty.CurrentHp if alive else 0)
// CalculatedVar.Calculate = base + extra * multiplier  (reference/CalculatedVar.cs)
//   => damage = 6 + 1 * Osty.CurrentHp        (upgraded: 9 + Osty.CurrentHp)
// OnPlay guards on `!Osty.CheckMissingWithAnim(Owner)`, i.e. only attacks when
// the Osty is alive (Owner.IsOstyMissing == !IsOstyAlive == !(Osty.IsAlive)).
/// `CalculatedDamage`: base + Osty's CURRENT HP (0 with no living Osty, which
/// is also when the attack is skipped entirely). Shared with the card face —
/// see `crate::cards::preview`.
///
/// CalculationBase: 6 base, +3 on upgrade (OnUpgrade
/// CalculationBase.UpgradeValueBy(3)).
pub(crate) fn preview_unleash(
    state: &CombatState,
    card: &Card,
    _target: Option<CreatureUid>,
) -> i32 {
    let osty_hp = state
        .player
        .pet_uid
        .and_then(|uid| state.creatures.iter().find(|c| c.id == uid.0))
        .filter(|c| c.current_hp > 0)
        .map_or(0, |c| c.current_hp);
    let base = if card.upgraded { 9 } else { 6 };
    base + osty_hp
}

pub(crate) fn play_unleash(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult {
    let target = target.expect("Unleash: Enemy-targeted card requires a target");

    // Osty.CurrentHp when the pet is alive (current_hp > 0), else the attack
    // is skipped entirely (C# IsOstyMissing guard).
    let osty_hp = state
        .player
        .pet_uid
        .and_then(|uid| state.creatures.iter().find(|c| c.id == uid.0))
        .filter(|c| c.current_hp > 0)
        .map(|c| c.current_hp);

    let Some(osty_hp) = osty_hp else {
        // Osty missing -> no damage (C# `if (!Osty.CheckMissingWithAnim(...))`).
        return EffectResult::Resolved;
    };

    let _ = osty_hp;
    let dmg = resolving_preview(state, preview_unleash, Some(target));

    // C# `.FromOsty(Owner.Osty, this)` — the Osty is the dealer (the damage
    // VALUE is `base + Osty.CurrentHp`, computed above; the dealer-side
    // attribution routes through the pet so Calcify/Strength on the Osty apply).
    deal_damage_from_osty_to_enemy(state, target, dmg);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Unleash,
        name: "UNLEASH",
        cost: 1,
        card_type: CardType::Attack,
        target_type: TargetType::Enemy, // C# TargetType.AnyEnemy -> single-player Enemy
        rarity: CardRarity::Basic,
        base_damage: 0, // computed at play time from Osty.CurrentHp
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade bumps CalculationBase by 3, but damage is computed in the
    // handler (base_damage is 0/computed), so the bump is applied via the
    // current_card_upgraded branch rather than a static base_damage delta.
    None
}
