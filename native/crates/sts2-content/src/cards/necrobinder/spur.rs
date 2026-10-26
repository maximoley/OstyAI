//! Hand-authored (C#-first port). Keep the prelude import.
use super::prelude::*;

/// Spur (Uncommon, 1 energy, Skill, Self, Retain). Summon 3 (Osty),
/// then heal the Osty by 5.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Spur.cs
//
// C# `OnPlay`:
//   `CreatureCmd.TriggerAnim(Owner.Creature, "Cast", …)` — cosmetic, ignored.
//   `OstyCmd.Summon(ctx, Owner, Summon.BaseValue, this)` — summon 3.
//   `CreatureCmd.Heal(Owner.Osty, Heal.BaseValue)` — heal the Osty by 5.
// `CanonicalVars` = `{ SummonVar(3m), HealVar(5m) }`; `CanonicalKeywords` =
// `{ Retain }`. `OstyCmd.Summon(amount)` grows the live Osty's max HP by
// `amount`, or spawns/revives the Osty at `amount` max HP when dead/absent
// (the `CombatState::summon_osty` primitive). It returns the pet uid, which
// the subsequent heal targets via `creature_heal` (C# `CreatureCmd.Heal`,
// a plain clamp-to-max-HP mutation with no hook).
// `OnUpgrade`: `Summon.UpgradeValueBy(2m)` (3 → 5) and
// `Heal.UpgradeValueBy(2m)` (5 → 7). Both are magnitudes (not damage/block/
// cost stats), so the bumps are gated here on `current_card_upgraded` and
// `upgrade()` returns `None`.
pub(crate) fn play_spur(state: &mut CombatState, _target: Option<CreatureUid>) -> EffectResult {
    let upgraded = current_card_upgraded(state);
    let summon = if upgraded { 5 } else { 3 };
    let heal = if upgraded { 7 } else { 5 };
    if let Some(osty) = state.summon_osty(summon) {
        let _ = state.creature_heal(osty, heal);
    }
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Spur,
        name: "SPUR",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Uncommon,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::RETAIN,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade bumps the SummonVar (3 → 5) and HealVar (5 → 7); both are
    // applied in the handler via the `current_card_upgraded` branch, so no
    // stat-field delta here.
    None
}
