//! Shroud (Necrobinder). Hand-authored C#-first port via character-port skill.
//! C# source: ../sts2-wiki/decompiled/v0.111.0/MegaCrit.Sts2.Core.Models.Cards/Shroud.cs
use super::prelude::*;

/// Apply Shroud 3 (4 upgraded); its listener grants unpowered block on Doom.
/// Source: `sts2-wiki/decompiled/v0.111.0/MegaCrit.Sts2.Core.Models.Cards/Shroud.cs`.
pub(crate) fn play_shroud(state: &mut CombatState, _target: Option<CreatureUid>) -> EffectResult {
    let amount = current_block(state);
    apply_power_to_player(state, PowerKind::Shroud, amount);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Shroud,
        name: "SHROUD",
        cost: 1,
        card_type: CardType::Power,
        target_type: TargetType::Self_,
        rarity: CardRarity::Uncommon,
        base_damage: 0,
        base_block: 3,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: DynamicVars.Block.UpgradeValueBy(1m) → 3 → 4. The delta lives
    // in the central table (`card_upgrades_generated.rs`, `CardId::Shroud` =>
    // base_block: Some(4)), per the Necrobinder/newer-pool convention.
    None
}
