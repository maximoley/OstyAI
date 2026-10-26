//! `NecroMastery` (Necrobinder). Hand-authored C#-first port via character-port skill.
//! C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/NecroMastery.cs
use super::prelude::*;

/// `NecroMastery` (Rare, 2 energy, Power, Self). Summon Osty by the Summon var
/// (5, or 8 upgraded), then apply 1 `NecroMastery` to self.
///
/// C# `NecroMastery.OnPlay`:
///   `await OstyCmd.Summon(.., Summon.BaseValue, this);`
///   `await PowerCmd.Apply<NecroMasteryPower>(Owner.Creature, 1m, Owner.Creature, this);`
///
/// The `NecroMasteryPower` listener (when the player's Osty loses HP, deal
/// `lostHp * Amount` Unblockable|Unpowered damage to all hittable enemies) is
/// wired in `crate::hooks::dispatch_after_current_hp_changed`, fired from the
/// DieForYou redirect in `combat_loop::apply_damage_to_player`, so this card is
/// fully live. The power amount (1) is fixed — `OnUpgrade` bumps only the
/// Summon var, not the power magnitude.
pub(crate) fn play_necro_mastery(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    // Summon: grow/revive/spawn the Osty by the Summon var (5, or 8 upgraded).
    let summon = if current_card_upgraded(state) { 8 } else { 5 };
    state.summon_osty(summon);
    // Apply NecroMasteryPower(1) to the player.
    apply_power_to_player(state, PowerKind::NecroMastery, 1);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::NecroMasteryCard,
        name: "NECRO_MASTERY_CARD",
        cost: 2,
        card_type: CardType::Power,
        target_type: TargetType::Self_,
        rarity: CardRarity::Rare,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: DynamicVars.Summon.UpgradeValueBy(3m) → 5 → 8. The Summon
    // var is the Osty-summon magnitude, not a damage/block/cost stat, so it is
    // handled in the play handler via the `current_card_upgraded` branch. No
    // stat-field delta.
    None
}
