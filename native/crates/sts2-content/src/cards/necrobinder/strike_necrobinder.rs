//! Necrobinder basic Strike. Ported C#-first via the character-port skill.
//! C# source:
//! ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/StrikeNecrobinder.cs
//!
//! `OnPlay` deals `DamageVar.BaseValue` to the target. An unregistered card is a
//! no-op in this engine (combat_loop::play_card treats it as an empty effect
//! list — it does NOT auto-apply base_damage), so even a plain Strike needs a
//! handler. Upgrade (6→9) lives in the central `card_upgrades_generated.rs` table.
use super::prelude::*;

/// StrikeNecrobinder (Basic, 1 energy, Attack). Deal 6 damage. Upgrade: +3 (→9).
pub(crate) fn play_strike_necrobinder(
    state: &mut CombatState,
    target: Option<CreatureUid>,
) -> EffectResult {
    let t = target.expect("StrikeNecrobinder: Enemy-targeted");
    let _ = deal_damage_player_to_enemy(state, t, current_damage(state));
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::StrikeNecrobinder,
        name: "STRIKE_NECROBINDER",
        cost: 1,
        card_type: CardType::Attack,
        target_type: TargetType::Enemy,
        rarity: CardRarity::Basic,
        base_damage: 6,
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: DynamicVars.Damage.UpgradeValueBy(3m) → 6 + 3 = 9.
    Some(CardUpgradeDelta {
        cost: None,
        base_damage: Some(9),
        base_block: None,
        keywords: None,
    })
}
