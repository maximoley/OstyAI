//! Hand-authored C#-first port (Necrobinder pool).
use super::prelude::*;

/// Melancholy (Uncommon, 3 energy, Skill, Self). Gain 13 Block (17 upgraded).
///
/// C# source of truth:
/// `../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Melancholy.cs`
///
/// `OnPlay` → `CreatureCmd.GainBlock(Owner.Creature, DynamicVars.Block)` —
/// the pure-stats block gain re-expressed below.
///
/// `AfterDeath` rider (RP85): C# `Melancholy.cs::AfterDeath` (lines 35-48)
/// fires for every non-removal-prevented creature death while Melancholy sits
/// in a combat pile, running
/// `EnergyCost.AddThisCombat(-DynamicVars.Energy.IntValue)` (Energy = 1) — each
/// creature death this combat permanently lowers this card instance's energy
/// cost by 1, clamped at 0. C# fires `CardModel.AfterDeath` on every card model
/// for ALL creature deaths (player- and enemy-side; the
/// `CreatureCmd.KillWithoutCheckingWinCondition` death sweep calls
/// `Hook.AfterDeath(.., wasRemovalPrevented: false, ..)` unconditionally —
/// `CreatureCmd.cs:371`). The Necrobinder Osty resurrect/die loop racks up
/// deaths fast, so a Melancholy in hand drops from cost 3 to 0 after three
/// deaths.
///
/// Ported in `sts2_core` as a core-side, `CardId::Melancholy`-keyed cost
/// mutation ([`CombatState::reduce_melancholy_cost_on_death`], mirroring the
/// `KinglyKick` per-draw cost-decrement precedent), fired from every
/// creature-death chokepoint the Necrobinder loop reaches: the damage-kill
/// witness dispatch (`hooks::dispatch_after_death`), the direct `kill_osty`
/// (SACRIFICE / BONE_SHARDS), and the inline DieForYou Osty-redirect kill.
pub(crate) fn play_melancholy(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    // OnPlay: gain Block (BlockVar 13; OnUpgrade UpgradeValueBy(4) → 17).
    let block = if current_card_upgraded(state) { 17 } else { 13 };
    gain_block_player(state, block);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Melancholy,
        name: "MELANCHOLY",
        cost: 3,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Uncommon,
        base_damage: 0,
        base_block: 13,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: DynamicVars.Block.UpgradeValueBy(4m) → 13 + 4 = 17.
    Some(CardUpgradeDelta {
        cost: None,
        base_damage: None,
        base_block: Some(17),
        keywords: None,
    })
}
