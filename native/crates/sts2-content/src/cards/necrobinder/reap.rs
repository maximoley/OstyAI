//! Necrobinder Reap. Ported C#-first via the character-port skill.
//! C# source:
//! ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Reap.cs
//!
//! `OnPlay` deals `DamageVar.BaseValue` (27) to the target. An unregistered
//! card is a no-op in this engine (combat_loop::play_card treats it as an empty
//! effect list — it does NOT auto-apply base_damage), so even a plain attack
//! needs a handler. The Retain keyword is spec metadata. Upgrade (27→33) lives
//! in the central `card_upgrades_generated.rs` table.
use super::prelude::*;

/// Reap (Common, 3 energy, Attack, Retain). Deal 27 damage. Upgrade: +6 (→33).
pub(crate) fn play_reap(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult {
    let t = target.expect("Reap: AnyEnemy-targeted");
    let _ = deal_damage_player_to_enemy(state, t, current_damage(state));
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Reap,
        name: "REAP",
        cost: 3,
        card_type: CardType::Attack,
        target_type: TargetType::Enemy,
        rarity: CardRarity::Common,
        base_damage: 27,
        base_block: 0,
        keywords: CardKeywords::RETAIN,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: DynamicVars.Damage.UpgradeValueBy(6m) → 27 + 6 = 33.
    // Delta lives in the central `card_upgrades_generated.rs` table.
    None
}
