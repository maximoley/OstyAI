//! Hand-authored (C#-first port). Keep the prelude import.
use super::prelude::*;

/// Death's Door (Uncommon, 1 energy, Skill, Self). Gain 6 Block. If you applied
/// Doom this turn, gain it 2 additional times (3× total).
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/DeathsDoor.cs
//
// C# `CanonicalVars`: `BlockVar(6m, Move)` + `RepeatVar(2)`. ctor
// `base(1, CardType.Skill, CardRarity.Uncommon, TargetType.Self)`.
//
// C# `OnPlay` (anim ignored as cosmetic):
//   int blockGains = 1;
//   if (WasDoomAppliedThisTurn) blockGains += Repeat.IntValue;  // Repeat == 2
//   for (i in 0..blockGains) GainBlock(Owner, Block);            // Block == 6 (7 upgraded)
//
// `WasDoomAppliedThisTurn` (C#) scans `CombatManager.History.Entries
// .OfType<PowerReceivedEntry>()` for a `DoomPower` entry whose `Applier ==
// Owner.Creature` that `HappenedThisTurn`. i.e. "did the PLAYER apply a Doom
// power to anyone this turn".
//
// The engine tracks this on `TurnEvents::doom_applied_by_player_this_turn`
// (crates/sts2-core/src/lib.rs): set in `card_effects::apply_power_to_enemy`
// whenever a player Doom card resolves (the single player→enemy power-apply
// helper), cleared at player-turn-start in `TurnEvents::reset_per_turn`. The
// handler reads it to arm the 3× (1 + 2) Block gain.
//
// `OnUpgrade` bumps the BlockVar 6 → 7. That delta lives in the central table
// `cards/card_upgrades_generated.rs` (`CardId::DeathsDoor => base_block:
// Some(7)`), so the per-card `upgrade()` returns `None`. Because the block
// amount is a spec stat (not a magic var), the engine stamps the upgraded
// base_block and `current_block(state)` already returns 6/7 correctly — no
// `current_card_upgraded` branch needed here.
pub(crate) fn play_deaths_door(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    let block = current_block(state);
    let mut block_gains = 1;
    // C#: if (WasDoomAppliedThisTurn) block_gains += Repeat.IntValue; (Repeat == 2).
    // The engine tracks "player applied Doom this turn" on
    // `TurnEvents::doom_applied_by_player_this_turn` (set in
    // card_effects::apply_power_to_enemy when a Doom card resolves, cleared at
    // player-turn-start), so a Doom card played earlier this turn arms the 3×
    // (1 + 2) block gain.
    if state.combat_log.doom_applied_by_player_this_turn {
        block_gains += 2;
    }
    for _ in 0..block_gains {
        gain_block_player(state, block);
    }
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::DeathsDoor,
        name: "DEATHS_DOOR",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Uncommon,
        base_damage: 0,
        base_block: 6,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade `Block.UpgradeValueBy(1m)` (6 → 7) lives in the central
    // `card_upgrades_generated.rs` table (CardId::DeathsDoor), so no per-card
    // delta here.
    None
}
