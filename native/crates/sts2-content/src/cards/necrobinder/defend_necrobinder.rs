//! Hand-authored via the character-port skill (C#-first).
//! C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/DefendNecrobinder.cs
//!
//! `OnPlay` is `CreatureCmd.GainBlock(Owner, BlockVar)` on Self. An unregistered
//! card no-ops in this engine (it does NOT auto-apply base_block), so a handler
//! is required. `OnUpgrade` Block.UpgradeValueBy(3m) (5→8) lives in the central
//! `card_upgrades_generated.rs` table, so `upgrade()` returns `None`.
use super::prelude::*;

/// DefendNecrobinder (Basic, 1 energy, Skill). Gain 5 block. Upgrade: +3 (→8).
pub(crate) fn play_defend_necrobinder(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    gain_block_player(state, current_block(state));
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::DefendNecrobinder,
        name: "DEFEND_NECROBINDER",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Basic,
        base_damage: 0,
        base_block: 5,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade Block.UpgradeValueBy(3m) (5 → 8) lives in the central
    // card_upgrades_generated.rs table for the Necrobinder pool.
    None
}
