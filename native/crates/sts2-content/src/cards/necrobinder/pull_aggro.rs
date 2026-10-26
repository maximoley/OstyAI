//! Hand-authored (C#-first port). Keep the prelude import.
use super::prelude::*;

/// Pull Aggro (Common, 2 energy, Skill, Self). Summon 4 (Osty); gain 7 Block.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/PullAggro.cs
//
// C# `OnPlay`: `OstyCmd.Summon(ctx, Owner, Summon.BaseValue, this)` then
// `CreatureCmd.GainBlock(Owner.Creature, Block, cardPlay)` (the "Cast" anim is
// cosmetic and ignored). `OstyCmd.Summon(amount)` grows the live Osty's max HP
// by `amount`, or spawns/revives the Osty at `amount` max HP when it is
// dead/absent — `CombatState::summon_osty` implements exactly that.
// `CanonicalVars` = `SummonVar(4m)` + `BlockVar(7m, ValueProp.Move)`, so the
// base summon amount is 4 and base block is 7. `GainsBlock => true`.
// `OnUpgrade` does `Summon.UpgradeValueBy(1m)` (4 → 5) and
// `Block.UpgradeValueBy(2m)` (7 → 9). Block is a stat field, so its upgrade
// delta lives in `upgrade()`; the summon amount is a magnitude (not
// damage/block/cost), so that bump is gated here on `current_card_upgraded`.
// No `CanonicalKeywords` (no Exhaust).
pub(crate) fn play_pull_aggro(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    let summon_amount = if current_card_upgraded(state) { 5 } else { 4 };
    let _ = state.summon_osty(summon_amount);
    gain_block_player(state, current_block(state));
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::PullAggro,
        name: "PULL_AGGRO",
        cost: 2,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Common,
        base_damage: 0,
        base_block: 7,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade: Block 7 → 9 (stat field, delta here); Summon 4 → 5 is gated
    // in the handler via `current_card_upgraded`, so no delta for it.
    Some(CardUpgradeDelta {
        cost: None,
        base_damage: None,
        base_block: Some(9),
        keywords: None,
    })
}
