//! Necrobinder Defile. Ported C#-first via the character-port skill.
//! C# source:
//! ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Defile.cs
//!
//! `OnPlay` deals `DamageVar.BaseValue` (13) to the target. The card carries the
//! Ethereal keyword. An unregistered card is a no-op in this engine
//! (combat_loop::play_card treats it as an empty effect list — it does NOT
//! auto-apply base_damage), so even a plain attack needs a handler. Upgrade
//! (13 → 17) lives in the central `card_upgrades_generated.rs` table.
use super::prelude::*;

/// Defile (Common, 1 energy, Attack, Ethereal). Deal 13 damage. Upgrade: +4 (→17).
pub(crate) fn play_defile(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult {
    let t = target.expect("Defile: AnyEnemy-targeted");
    let _ = deal_damage_player_to_enemy(state, t, current_damage(state));
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Defile,
        name: "DEFILE",
        cost: 1,
        card_type: CardType::Attack,
        target_type: TargetType::Enemy,
        rarity: CardRarity::Common,
        base_damage: 13,
        base_block: 0,
        keywords: CardKeywords::ETHEREAL,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: DynamicVars.Damage.UpgradeValueBy(4m) → 13 + 4 = 17.
    // The delta lives in the central `card_upgrades_generated.rs` table
    // (CardId::Defile row, bead sts2-rs-tk230); per-card returns None.
    None
}
