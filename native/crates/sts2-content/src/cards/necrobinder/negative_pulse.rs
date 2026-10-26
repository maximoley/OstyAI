//! Negative Pulse (Necrobinder pool). Hand-authored C#-first port.
use super::prelude::*;

/// Negative Pulse (Common, 1 energy, Skill, AllEnemies). Gain 5 Block, then
/// apply 7 Doom to ALL enemies.
///
/// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/NegativePulse.cs
///
/// C# ctor `base(1, Skill, Common, AllEnemies)` → cost 1, Skill, Common,
/// AllEnemies. `GainsBlock => true`. `CanonicalVars`: `BlockVar(5, Move)` +
/// `PowerVar<DoomPower>(7)`. C# `OnPlay`: `CreatureCmd.GainBlock(owner,
/// Block)` then for each `CombatState.HittableEnemies`
/// `PowerCmd.Apply<DoomPower>(enemy, Doom.BaseValue, owner, this)`.
///
/// C# `OnUpgrade`: `Block.UpgradeValueBy(1)` (5 → 6) and
/// `Doom.UpgradeValueBy(4)` (7 → 11). The Block delta is a stat-field bump →
/// lives in the central `card_upgrades_generated` table (CardId::NegativePulse);
/// the Doom power amount is an effect-var bump → gated here on
/// `current_card_upgraded`.
pub(crate) fn play_negative_pulse(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    // C# GainBlock(owner, Block) — Block is the card stat (5/6 via the central
    // upgrade table), read through current_block.
    gain_block_player(state, current_block(state));
    // C# Doom.BaseValue: 7 base, 11 upgraded (OnUpgrade +4).
    let doom_amt = if current_card_upgraded(state) { 11 } else { 7 };
    // C# iterates CombatState.HittableEnemies, applying DoomPower to each.
    let uids: Vec<CreatureUid> = state.enemy_uids.iter().copied().collect();
    for uid in uids {
        if state
            .creatures
            .iter()
            .any(|c| c.id == uid.0 && c.current_hp > 0)
        {
            apply_power_to_enemy(state, uid, PowerKind::Doom, doom_amt);
        }
    }
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::NegativePulse,
        name: "NEGATIVE_PULSE",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::AllEnemies,
        rarity: CardRarity::Common,
        base_damage: 0,
        base_block: 5,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: Block 5 → 6 lives in the central card_upgrades_generated
    // table (CardId::NegativePulse). The Doom power amount 7 → 11 effect-var
    // bump is handled inline in play_negative_pulse via current_card_upgraded.
    None
}
