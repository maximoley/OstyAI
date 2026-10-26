//! Lethality (Necrobinder). Hand-authored C#-first port via character-port skill.
//! C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Lethality.cs
use super::prelude::*;

/// Lethality (Uncommon, 1 energy, Power, Self — Ethereal). Apply 50 `Lethality`
/// to self (75 upgraded).
///
/// C# `Lethality.OnPlay`: `PowerCmd.Apply<LethalityPower>(Owner.Creature,
/// DynamicVars["LethalityPower"].BaseValue, ..)` — base 50. `OnUpgrade` bumps
/// the `LethalityPower` var by 25 (→ 75).
///
/// The `LethalityPower` listener (the first powered Attack the player plays each
/// turn deals `+Amount%` damage) is wired in
/// `crate::hooks::modify_damage_multiplicative`, so this card is fully live. The
/// power amount IS the upgradeable value (the +% magnitude), so the upgrade is
/// gated here on `current_card_upgraded`; `upgrade()` carries no stat delta.
pub(crate) fn play_lethality(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    let amount = if current_card_upgraded(state) { 75 } else { 50 };
    apply_power_to_player(state, PowerKind::Lethality, amount);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::LethalityCard,
        name: "LETHALITY_CARD",
        cost: 1,
        card_type: CardType::Power,
        target_type: TargetType::Self_,
        rarity: CardRarity::Uncommon,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::ETHEREAL,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: DynamicVars["LethalityPower"].UpgradeValueBy(25m) → 50 → 75.
    // The LethalityPower var is the +% damage magnitude, not a damage/block/cost
    // stat, so it is handled in the play handler via the `current_card_upgraded`
    // branch. No stat-field delta.
    None
}
