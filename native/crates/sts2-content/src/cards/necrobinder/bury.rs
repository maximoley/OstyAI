//! Hand-authored via the character-port skill (C#-first).
use super::prelude::*;

/// Bury (Uncommon, 4 energy, Attack). Deal 52 damage to a single enemy.
/// `OnPlay` is a single `DamageCmd.Attack(BaseValue)`. An unregistered card
/// no-ops in this engine (no auto-apply of base_damage), so a handler is
/// required. Upgrade (52 → 63) lives in the central
/// `card_upgrades_generated.rs` table.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Bury.cs
pub(crate) fn play_bury(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult {
    let t = target.expect("Bury: Enemy-targeted");
    let _ = deal_damage_player_to_enemy(state, t, current_damage(state));
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Bury,
        name: "BURY",
        cost: 4,
        card_type: CardType::Attack,
        target_type: TargetType::Enemy, // C# TargetType.AnyEnemy → single-player collapse
        rarity: CardRarity::Uncommon,
        base_damage: 52, // DamageVar(52m, ValueProp.Move)
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: DynamicVars.Damage.UpgradeValueBy(11m) → 52 → 63.
    // Delta lives in the central card_upgrades_generated.rs table
    // (CardId::Bury => base_damage Some(63)); per-card upgrade() returns None.
    None
}
