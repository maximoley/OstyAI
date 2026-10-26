//! End of Days (Necrobinder pool). Hand-authored C#-first port.
use super::prelude::*;

/// End of Days (Rare, 3 energy, Skill, AllEnemies). Apply 29 Doom to ALL
/// enemies, then immediately kill any enemy already at/under its Doom
/// threshold. C# `EndOfDays.cs` OnPlay:
/// `foreach (HittableEnemies) PowerCmd.Apply<DoomPower>(e, 29)` then
/// `DoomPower.DoomKill(DoomPower.GetDoomedCreatures(HittableEnemies))`.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/EndOfDays.cs
//
// C# `CanonicalVars`: `PowerVar<DoomPower>(29m)`; `doomAmount = 29`.
// C# `OnUpgrade`: `DynamicVars.Doom.UpgradeValueBy(8m)` → 37 upgraded. This is a
// PowerVar magnitude (not damage/block/cost), so the bump is gated here on
// `current_card_upgraded` and `upgrade()` returns `None`.
//
// The second half of OnPlay —
// `DoomPower.DoomKill(GetDoomedCreatures(HittableEnemies))` — immediately kills
// every enemy whose `CurrentHp <= DoomAmount` AT PLAY TIME (DoomPower.cs:28-46,
// `IsOwnerDoomed`) and fires `Hook.AfterDiedToDoom` (BookRepairKnife heal). This
// is now wired via `CombatState::doom_kill_doomed_enemies` (bead `sts2-rs-z1z5t`),
// so an enemy already at/under 29 (or 37 upgraded) HP dies the same turn, matching
// the C# rather than waiting for the next turn-end Doom tick.
pub(crate) fn play_end_of_days(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    let doom = if current_card_upgraded(state) { 37 } else { 29 };
    // C# applies to CombatState.HittableEnemies (all alive enemies). Snapshot
    // uids and recheck alive per target, mirroring deal_damage_all_enemies /
    // the deathbringer port.
    let uids: Vec<CreatureUid> = state.enemy_uids.iter().copied().collect();
    for uid in uids {
        if state
            .creatures
            .iter()
            .any(|c| c.id == uid.0 && c.current_hp > 0)
        {
            apply_power_to_enemy(state, uid, PowerKind::Doom, doom);
        }
    }
    // C# `DoomPower.DoomKill(GetDoomedCreatures(HittableEnemies))`: immediately
    // kill every enemy now at/under its Doom threshold and fire after_died_to_doom.
    state.doom_kill_doomed_enemies();
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::EndOfDays,
        name: "END_OF_DAYS",
        cost: 3,
        card_type: CardType::Skill,
        target_type: TargetType::AllEnemies,
        rarity: CardRarity::Rare,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade only bumps the DoomPower amount (29 → 37), a PowerVar bump
    // handled in play_end_of_days via current_card_upgraded — no stat delta.
    None
}
