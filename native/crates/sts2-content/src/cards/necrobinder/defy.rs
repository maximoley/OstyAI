//! Hand-authored (C#-first port). Keep the prelude import.
use super::prelude::*;

/// Defy (Common, 1 energy, Skill, Enemy). Gain 6 Block; apply 1 Weak to the
/// target. Ethereal.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Defy.cs
//
// C# `CanonicalVars`: `BlockVar(6m, Move)` + `PowerVar<WeakPower>(1m)`.
// `OnPlay`: GainBlock(DynamicVars.Block) then Apply<WeakPower>(Target,
// Weak.BaseValue). `CanonicalKeywords`: Ethereal. `OnUpgrade`:
// `Block.UpgradeValueBy(3m)` → 9 Block upgraded — a block stat bump, so it
// lives in `upgrade()` (the `current_card_*` branch is not needed here). The
// Weak amount has no `OnUpgrade` (stays 1), and `Weak.BaseValue` is read
// directly (not the upgrade-aware var), so 1 Weak both base and upgraded.
pub(crate) fn play_defy(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult {
    let t = target.expect("Defy: Enemy-targeted");
    gain_block_player(state, current_block(state));
    apply_power_to_enemy(state, t, PowerKind::Weak, 1);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Defy,
        name: "DEFY",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Enemy,
        rarity: CardRarity::Common,
        base_damage: 0,
        base_block: 6,
        keywords: CardKeywords::ETHEREAL,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: Block.UpgradeValueBy(3) → 6 + 3 = 9. Weak unchanged.
    Some(CardUpgradeDelta {
        cost: None,
        base_damage: None,
        base_block: Some(9),
        keywords: None,
    })
}
