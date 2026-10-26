//! Protector (Necrobinder). Hand-authored C#-first port via the character-port skill.
//! C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Protector.cs
use super::prelude::*;

/// Protector (Ancient, 1 energy, Attack, AnyEnemy). The Osty attacks the target
/// for `10 + Osty.MaxHp` (`15 + Osty.MaxHp` upgraded). OstyAttack card.
//
// C# OnPlay:
//   ArgumentNullException.ThrowIfNull(cardPlay.Target);
//   if (!Osty.CheckMissingWithAnim(Owner))
//       DamageCmd.Attack(DynamicVars.CalculatedDamage)   // the scaled MOVE damage
//           .FromOsty(Owner.Osty, this)                  // attacker is the OSTY
//           .Targeting(cardPlay.Target)
//           .WithHitFx(...)                              // cosmetic, ignored
//           .Execute(...);
//
// CanonicalVars (3):
//   CalculationBaseVar(10m)                       → base of the calculated damage.
//                                                   OnUpgrade +5 → 15.
//   ExtraDamageVar(1m).FromOsty()                 → the per-multiplier-unit extra.
//   CalculatedDamageVar(Move).FromOsty()
//     .WithMultiplier(card => osty alive ? osty.MaxHp : 0)
//                                                 → final = Base + Extra * multiplier
//                                                   = 10 + 1 * Osty.MaxHp.
//   (CalculatedVar = Base + Extra × multiplier; see CalculatedVar.cs — the Extra
//   factor of 1 means the scaling term is exactly the Osty's MaxHp.)
// CanonicalTags: OstyAttack (cosmetic glow tag; not carried in CardSpec here).
// ShouldGlowRedInternal => Owner.IsOstyMissing — pure UI, ignored.
//
// The damage is dealt by the OSTY (FromOsty), so the hit is a no-op when the Osty
// is missing/dead: `deal_damage_from_osty_to_enemy` early-returns 0 when
// `osty_dealer_ref` → None, reproducing `CheckMissingWithAnim` (minus the cosmetic
// summon-prompt animation we don't model). The multiplier reads the Osty's MaxHp
// only when the pet is alive, which is exactly the same gate, so the computed
// damage is 10/15 (base only) in the missing case — but it is never dealt then.
pub(crate) fn play_protector(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult {
    let t = target.expect("Protector: enemy target required");

    let dmg = resolving_preview(state, preview_protector, target);
    deal_damage_from_osty_to_enemy(state, t, dmg);
    EffectResult::Resolved
}

/// `CalculatedDamage`: base + Osty's Max HP. Shared with the card face — see
/// `crate::cards::preview`.
///
/// CalculationBase: 10 base, +5 on upgrade. Not a plain player-attack stat (the
/// hit is dealt by the Osty and the amount is computed), so the bump is gated on
/// the card's own upgrade flag like the sibling OstyAttack cards
/// (sic_em/flatten). ExtraDamage(1) x multiplier, where multiplier = Osty.MaxHp
/// when the Osty is alive, else 0 (C# `(osty != null && osty.IsAlive) ?
/// osty.MaxHp : 0`).
pub(crate) fn preview_protector(
    state: &CombatState,
    card: &Card,
    _target: Option<CreatureUid>,
) -> i32 {
    let base = if card.upgraded { 15 } else { 10 };
    let osty_max_hp = state
        .player
        .pet_uid
        .and_then(|pet_uid| {
            state
                .creatures
                .iter()
                .find(|c| c.id == pet_uid.0 && c.current_hp > 0)
        })
        .map_or(0, |pet| pet.max_hp);
    base + osty_max_hp
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Protector,
        name: "PROTECTOR",
        cost: 1,
        card_type: CardType::Attack,
        target_type: TargetType::Enemy, // C# TargetType.AnyEnemy → single-player Enemy
        rarity: CardRarity::Ancient,
        base_damage: 10, // CalculationBaseVar(10m) — cosmetic; the scaled hit is computed
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade:
    //   EnergyCost.UpgradeBy(-1)             → cost 1 → 0 (a real spec field).
    //   CalculationBase.UpgradeValueBy(5)    → 10 → 15. The CalculationBase is not a
    //                                          plain attack stat (the hit is dealt by
    //                                          the Osty and the damage is computed),
    //                                          so this bump is applied in the handler
    //                                          via current_card_upgraded rather than a
    //                                          static base_damage delta.
    Some(CardUpgradeDelta {
        cost: Some(0),
        base_damage: None,
        base_block: None,
        keywords: None,
    })
}
