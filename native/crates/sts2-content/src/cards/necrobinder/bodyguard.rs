//! Hand-authored (C#-first port). Keep the prelude import.
use super::prelude::*;

/// Bodyguard (Basic, 1 energy, Skill, Self). Summon 5 (Osty).
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Bodyguard.cs
//
// C# `OnPlay`: `OstyCmd.Summon(ctx, Owner, Summon.BaseValue, this)` (the
// "Cast" anim is cosmetic and ignored). `OstyCmd.Summon(amount)` grows the
// live Osty's max HP by `amount`, or spawns/revives the Osty at `amount`
// max HP when it is dead/absent — the `CombatState::summon_osty` primitive
// implements exactly that. `CanonicalVars` is a single `SummonVar(5m)`, so
// the base summon amount is 5; `OnUpgrade` does `Summon.UpgradeValueBy(2m)`
// → 7 upgraded. The summon amount is a magnitude (not damage/block/cost),
// so the upgrade bump is gated here on `current_card_upgraded` and
// `upgrade()` returns `None`. No `CanonicalKeywords` (no Exhaust).
pub(crate) fn play_bodyguard(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    let amount = if current_card_upgraded(state) { 7 } else { 5 };
    let _ = state.summon_osty(amount);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Bodyguard,
        name: "BODYGUARD",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Basic,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade bumps the SummonVar (5 → 7); applied in the handler via the
    // `current_card_upgraded` branch, so no stat-field delta here.
    None
}
