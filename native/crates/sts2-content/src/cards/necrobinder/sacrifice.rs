//! Hand-authored (C#-first port). Keep the prelude import.
use super::prelude::*;

/// Sacrifice (Rare, 1 energy, Skill, Self, Retain). Kill the Osty and gain
/// block equal to three times its max HP.
// C# source: ../sts2-wiki/decompiled/v0.111.0/MegaCrit.Sts2.Core.Models.Cards/Sacrifice.cs
//
// C# `OnPlay`:
//   if (!Osty.CheckMissingWithAnim(Owner)) {     // gate: Osty must be ALIVE
//       int blockGain = Owner.Osty.MaxHp * 3;     // read MaxHp BEFORE the kill
//       CreatureCmd.TriggerAnim(Owner.Creature, "Cast", ...);   // cosmetic, ignored
//       CreatureCmd.Kill(Owner.Osty);             // kill the pet
//       CreatureCmd.GainBlock(Owner.Creature, blockGain, Move, cardPlay);
//   }
// `Osty.CheckMissingWithAnim` returns `Owner.IsOstyMissing` (= `!IsOstyAlive`,
// i.e. no pet, or the pet is dead). Mirror that with the live-pet check used by
// `osty_dealer_ref`: `pet_uid` set AND the pet creature `current_hp > 0`. When
// the Osty is missing the whole effect is skipped (no block, no kill).
//
// Spec: `base(1, Skill, Rare, Self)`; `CanonicalKeywords{ Retain }`;
// `GainsBlock => true` (UI flag; not a CardSpec field). The block magnitude is
// `Osty.MaxHp * 3` (computed at play time from the live pet), NOT a BlockVar, so
// `base_block` stays 0 and the value is computed in the handler.
//
// `OnUpgrade`: `EnergyCost.UpgradeBy(-1)` (cost 1 → 0). That delta lives in the
// central `card_upgrades_generated.rs` table (`CardId::Sacrifice` row,
// `cost: Some(0)`), matching the Necrobinder pool's convention, so `upgrade()`
// returns `None` here.
/// `CalculatedBlock`: triple Osty's Max HP, or 0 with no living Osty. Shared
/// with the card face — see `crate::cards::preview`.
pub(crate) fn preview_sacrifice(
    state: &CombatState,
    _card: &Card,
    _target: Option<CreatureUid>,
) -> i32 {
    state
        .player
        .pet_uid
        .and_then(|pet_uid| {
            state
                .creatures
                .iter()
                .find(|c| c.id == pet_uid.0 && c.current_hp > 0)
        })
        .map_or(0, |pet| pet.max_hp.saturating_mul(3))
}

pub(crate) fn play_sacrifice(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    // Gate: Osty must be present and alive (C# `!IsOstyMissing`).
    let Some(pet_uid) = state.player.pet_uid else {
        return EffectResult::Resolved;
    };
    let Some(pet) = state
        .creatures
        .iter()
        .find(|c| c.id == pet_uid.0 && c.current_hp > 0)
    else {
        return EffectResult::Resolved;
    };

    // Read MaxHp BEFORE killing (C# `Owner.Osty.MaxHp * 3`).
    let _ = pet;
    let block_gain = resolving_preview(state, preview_sacrifice, None);

    state.kill_osty();
    gain_block_player(state, block_gain);

    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Sacrifice,
        name: "SACRIFICE",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Rare,
        base_damage: 0,
        base_block: 0, // block = Osty.MaxHp * 3, computed at play time
        keywords: CardKeywords::RETAIN,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade `EnergyCost.UpgradeBy(-1)` (cost 1 → 0) lives in the central
    // `card_upgrades_generated.rs` table for the Necrobinder pool.
    None
}
