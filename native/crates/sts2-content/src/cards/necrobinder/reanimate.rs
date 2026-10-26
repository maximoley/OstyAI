//! Hand-authored (C#-first port). Keep the prelude import.
use super::prelude::*;

/// Reanimate (Rare, 3 energy, Skill, Self). Summon 20 (Osty). Exhaust.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Reanimate.cs
//
// C# `OnPlay`: `OstyCmd.Summon(ctx, Owner, Summon.BaseValue, this)` (the
// "Cast" anim is cosmetic and ignored). `OstyCmd.Summon(amount)` grows the
// live Osty's max HP by `amount`, or spawns/revives the Osty at `amount`
// max HP when it is dead/absent — the `CombatState::summon_osty` primitive
// implements exactly that. `CanonicalVars` is a single `SummonVar(20m)`, so
// the base summon amount is 20; `OnUpgrade` does `Summon.UpgradeValueBy(5m)`
// → 25 upgraded. The summon amount is a magnitude (not damage/block/cost),
// so the upgrade bump is gated here on `current_card_upgraded` and
// `upgrade()` returns `None`. `CanonicalKeywords` = `{ Exhaust }`.
pub(crate) fn play_reanimate(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    let amount = if current_card_upgraded(state) { 25 } else { 20 };
    let _ = state.summon_osty(amount);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Reanimate,
        name: "REANIMATE",
        cost: 3,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Rare,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::EXHAUST,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade bumps the SummonVar (20 → 25); applied in the handler via the
    // `current_card_upgraded` branch, so no stat-field delta here.
    None
}
