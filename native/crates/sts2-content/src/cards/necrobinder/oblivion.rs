//! Oblivion (Necrobinder). C#-first port.
//! C# source:
//! ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Oblivion.cs
use super::prelude::*;

/// Oblivion (Rare, 0 energy, Skill, Enemy). Apply 3 Oblivion to an enemy
/// (4 when upgraded).
///
/// C# `Oblivion`: ctor `base(0, Skill, Rare, AnyEnemy)` → cost 0, Skill,
/// Rare, Enemy (multiplayer `AnyEnemy` collapses to `Enemy`).
/// `CanonicalVars`: `PowerVar<DoomPower>("Doom", 3)`. `OnPlay`:
/// `PowerCmd.Apply<OblivionPower>(target, Doom.BaseValue=3, owner, this)`
/// (the `TriggerAnim("Cast")` is cosmetic and ignored). `OnUpgrade`:
/// `DynamicVars.Doom.UpgradeValueBy(1)` (3 → 4) — a PowerVar magnitude bump,
/// so it lives in the handler's `current_card_upgraded` branch and
/// `upgrade()` returns None.
///
/// NOTE: the deferred behaviour lives entirely in the powers, not this card,
/// and IS wired in `crates/sts2-core/src/hooks.rs`:
/// * `OblivionPower` records its `Amount` for each card the applier plays this
///   turn (the `Oblivion` arm in `dispatch_before_card_played_powers` arms
///   `counter == 1`), and after that card resolves applies `DoomPower` equal to
///   that amount onto the Oblivion owner (the Oblivion block in
///   `dispatch_after_card_played`); it removes itself at the player's turn end
///   (the `Oblivion` arm in `after_turn_end`). Bead `sts2-rs-4wppb`.
/// * `DoomPower.BeforeTurnEnd`: if the owner's `CurrentHp <= Amount`, the owner
///   is doom-killed via `Effect::DoomKill`, which also fires the
///   `after_died_to_doom` relic bus (consumed by BookRepairKnife) — the `Doom`
///   arm in `before_turn_end`. Beads `sts2-rs-70anj.3.8`, `sts2-rs-z1z5t`.
pub(crate) fn play_oblivion(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult {
    let t = target.expect("Oblivion: enemy target required");
    let amount = if current_card_upgraded(state) { 4 } else { 3 };
    apply_power_to_enemy(state, t, PowerKind::Oblivion, amount);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Oblivion,
        name: "OBLIVION",
        cost: 0,
        card_type: CardType::Skill,
        target_type: TargetType::Enemy,
        rarity: CardRarity::Rare,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: `DynamicVars.Doom.UpgradeValueBy(1)` (3 → 4) — a PowerVar
    // magnitude bump, not a damage/block/cost stat. Per conventions that bump
    // is applied in the handler via the `current_card_upgraded` branch, so the
    // spec-level upgrade delta is None.
    None
}
