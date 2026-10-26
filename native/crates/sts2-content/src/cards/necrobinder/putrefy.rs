//! Putrefy (Necrobinder pool). Hand-authored C#-first port.
use super::prelude::*;

/// Putrefy (Uncommon, 1 energy, Skill). Apply 2 Weak and 2 Vulnerable to the
/// target. Exhaust.
///
/// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Putrefy.cs
///
/// C# `CanonicalVars`: single `DynamicVar("Power", 2)`. C# ctor
/// `base(1, Skill, Uncommon, AnyEnemy)` → cost 1, Skill, Uncommon, Enemy.
/// C# `CanonicalKeywords`: `{ Exhaust }`.
///
/// C# `OnPlay` (after the cosmetic `CreatureCmd.TriggerAnim` cast):
/// `amount = DynamicVars["Power"].IntValue`, then
/// `PowerCmd.Apply<WeakPower>(target, amount)` and
/// `PowerCmd.Apply<VulnerablePower>(target, amount)`.
///
/// C# `OnUpgrade`: `DynamicVars["Power"].UpgradeValueBy(1)` (2 → 3). This is a
/// pure effect-var bump (no damage/block/cost/keyword stat field), so
/// `upgrade()` returns `None` and the bump is gated here on
/// `current_card_upgraded`.
pub(crate) fn play_putrefy(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult {
    let t = target.expect("Putrefy: enemy target required");
    let amount = if current_card_upgraded(state) { 3 } else { 2 };
    apply_power_to_enemy(state, t, PowerKind::Weak, amount);
    apply_power_to_enemy(state, t, PowerKind::Vulnerable, amount);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Putrefy,
        name: "PUTREFY",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Enemy,
        rarity: CardRarity::Uncommon,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::EXHAUST, // CanonicalKeywords { Exhaust }
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: DynamicVars["Power"] 2 → 3 is a pure effect-var bump
    // (applied to both Weak and Vulnerable), gated inline in play_putrefy via
    // the current_card_upgraded branch. No stat-field delta.
    None
}
