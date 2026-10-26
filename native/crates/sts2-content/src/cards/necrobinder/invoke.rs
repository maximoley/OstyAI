//! Hand-authored (C#-first port). Keep the prelude import.
use super::prelude::*;

/// Invoke (Common, 1 energy, Skill, Self). Summon 2 next turn and gain 2 energy
/// next turn.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Invoke.cs
//
// C# `OnPlay`:
//   - `CreatureCmd.TriggerAnim("Cast")` — cosmetic, ignored.
//   - `PowerCmd.Apply<SummonNextTurnPower>(Owner.Creature,
//      DynamicVars.Summon.IntValue, Owner.Creature, this)` — self-applied
//      SummonNextTurn. SummonNextTurnPower (Buff/Counter) fires
//      `AfterPlayerTurnStart` → `OstyCmd.Summon(amount)` then removes itself.
//   - `PowerCmd.Apply<EnergyNextTurnPower>(Owner.Creature,
//      DynamicVars.Energy.IntValue, Owner.Creature, this)` — self-applied
//      EnergyNextTurn. EnergyNextTurnPower (Buff/Counter) fires
//      `AfterEnergyReset` → `PlayerCmd.GainEnergy(amount)` then removes itself.
// `CanonicalVars` = `SummonVar(2m)` + `EnergyVar(2)`, so base Summon is 2 and
// base Energy is 2. Both are magnitudes (not stat fields), so their upgrade
// bumps are gated in the handler on `current_card_upgraded`, and `upgrade()`
// carries no stat delta.
// `OnUpgrade`: `Summon.UpgradeValueBy(1m)` → 3 Summon, and
// `Energy.UpgradeValueBy(1m)` → 3 EnergyNextTurn.
// No `CanonicalKeywords` (no Exhaust/Ethereal).
//
// NOTE: both riders are wired in `crates/sts2-core/src/hooks.rs` (bead
// `sts2-rs-4wppb`): the `PowerKind::SummonNextTurn` arm summons Osty and
// self-removes at the player's turn start (folded onto `after_side_turn_start`),
// and the `PowerKind::EnergyNextTurn` arm grants the deferred energy. Both fire
// at runtime.
pub(crate) fn play_invoke(state: &mut CombatState, _target: Option<CreatureUid>) -> EffectResult {
    let upgraded = current_card_upgraded(state);
    let summon = if upgraded { 3 } else { 2 };
    let energy = if upgraded { 3 } else { 2 };
    apply_power_to_player(state, PowerKind::SummonNextTurn, summon);
    apply_power_to_player(state, PowerKind::EnergyNextTurn, energy);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Invoke,
        name: "INVOKE",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Common,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade bumps Summon (2 → 3) and Energy (2 → 3). Both are magnitudes
    // applied in the handler's current_card_upgraded branch, so there is no
    // stat-field delta here.
    None
}
