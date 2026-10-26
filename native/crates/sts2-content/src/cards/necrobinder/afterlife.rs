//! Hand-authored (C#-first port). Keep the prelude import.
use super::prelude::*;

/// Afterlife (Common, 1 energy, Skill, Self, Exhaust). Summon 6 (Osty).
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Afterlife.cs
//
// C# `OnPlay`: `OstyCmd.Summon(ctx, Owner, Summon.BaseValue, this)`.
// `OstyCmd.Summon(amount)` grows the live Osty's max HP by `amount`, or
// spawns/revives the Osty at `amount` max HP when it is dead/absent — the
// `CombatState::summon_osty` primitive implements exactly that. SummonVar
// base is 6; `OnUpgrade` does `Summon.UpgradeValueBy(3m)` → 9 upgraded.
// The summon amount is a MagicVar-style magnitude (not damage/block/cost),
// so the upgrade bump is gated here on `current_card_upgraded` and
// `upgrade()` returns `None`. The "Cast" anim is cosmetic (ignored).
pub(crate) fn play_afterlife(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    let amount = if current_card_upgraded(state) { 9 } else { 6 };
    let _ = state.summon_osty(amount);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Afterlife,
        name: "AFTERLIFE",
        cost: 1,
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
    // OnUpgrade bumps the SummonVar (6 → 9); applied in the handler via the
    // `current_card_upgraded` branch, so no stat-field delta here.
    None
}
