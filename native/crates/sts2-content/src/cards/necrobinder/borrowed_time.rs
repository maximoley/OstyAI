//! Hand-authored C#-first port. Keep the prelude import.
use super::prelude::*;

/// Borrowed Time (Uncommon, 1 energy, Skill, Self). Gain 4 energy now (upgraded:
/// 6), but cards cost 1 more energy this turn (a self-applied debuff that lasts
/// until end of turn).
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/BorrowedTime.cs
//
// CanonicalVars:
//   EnergyVar(4)             -> energy gained now      (OnUpgrade: +2 -> 6)
//   EnergyVar("ExtraCost",1) -> stacks of BorrowedTimePower applied to self (1)
// OnPlay:
//   PlayerCmd.GainEnergy(Energy.BaseValue, Owner)              -> gain 4 (up 6)
//   PowerCmd.Apply<BorrowedTimePower>(Owner.Creature, ExtraCost=1, Owner, this)
// OnUpgrade: DynamicVars.Energy.UpgradeValueBy(2) -> the gain becomes 6 (the
//   ExtraCost / BorrowedTimePower stack count is NOT upgraded).
//
// BorrowedTimePower (../sts2-wiki/.../Powers/BorrowedTimePower.cs):
//   Type=Debuff, StackType=Counter. TryModifyEnergyCostInCombat: for any card
//   owned by the power owner (`card.Owner.Creature == base.Owner`), returns
//   `originalCost + Amount` (i.e. +1 to EVERY player card this turn).
//   AfterTurnEnd: removes itself when the owner's side turn ends.
//
// WIRING: `PowerKind::BorrowedTime` is a hand-appended C#-sourced variant in
// crates/sts2-core/src/power_ids_generated.rs (no Python counterpart). Its two
// listener arms live in crate::hooks, structurally mirroring TangledPower
// (`originalCost + Amount`, removed at the owner's turn end — see hooks.rs
// PowerKind::Tangled) EXCEPT BorrowedTime surcharges ALL card types, not just
// Attacks, and gates on player ownership rather than card_type. Both the energy
// gain and the +1-cost debuff are now faithful.
pub(crate) fn play_borrowed_time(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    // PlayerCmd.GainEnergy(Energy.BaseValue, Owner). OnUpgrade bumps the gain
    // from 4 to 6 (Energy.UpgradeValueBy(2)).
    let gain = if current_card_upgraded(state) { 6 } else { 4 };
    state.gain_energy(gain);

    // PowerCmd.Apply<BorrowedTimePower>(Owner.Creature, ExtraCost=1, ...):
    // self-applied +1-cost-to-all-cards debuff, removed at end of turn. The
    // ExtraCost var is NOT upgraded (only the energy gain is), so the stack
    // count is always 1. `PowerKind::BorrowedTime` is now wired in
    // crate::hooks (modify_card_cost + after_turn_end arms).
    apply_power_to_player(state, PowerKind::BorrowedTime, 1);

    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::BorrowedTime,
        name: "BORROWED_TIME",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Uncommon,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade: DynamicVars.Energy.UpgradeValueBy(2). The energy-gain bump is
    // a PowerVar/EnergyVar magnitude (not a damage/block/cost stat), so it is
    // applied in the handler via the `current_card_upgraded` branch and this
    // returns None (no spec-stat delta to record).
    None
}
