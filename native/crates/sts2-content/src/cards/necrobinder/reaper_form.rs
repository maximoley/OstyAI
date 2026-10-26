//! Reaper Form (Necrobinder pool). C#-first port.
//! C# source:
//! ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/ReaperForm.cs
use super::prelude::*;

/// Reaper Form (Rare, 3 energy, Power, Self).
///
/// C# `ReaperForm.OnPlay`:
///   * `CreatureCmd.TriggerAnim(...)` → cosmetic, ignored.
///   * `PowerCmd.Apply<ReaperFormPower>(Owner.Creature, 1, Owner.Creature, this)`
///     → apply 1 stack of ReaperForm to self.
///
/// `ReaperFormPower` (Buff, Counter, Amount=1) reacts on `AfterDamageGiven`:
/// when the owner (or its pet) deals a powered attack hit with TotalDamage > 0,
/// apply `DoomPower(TotalDamage * Amount)` to the target. That listener IS
/// wired in `crates/sts2-core/src/hooks.rs` (the `PowerKind::ReaperForm` arm on
/// the `after_damage_given` dispatch, bead `sts2-rs-4wppb`), so the applied
/// stack doom-marks struck targets at runtime.
///
/// `CanonicalVars`: `PowerVar<ReaperFormPower>(1)` (the applied amount).
/// `OnUpgrade`: `AddKeyword(Retain)` — spec keywords NONE → upgraded RETAIN.
/// That spec-level keyword bump lives in the central
/// `card_upgrades_generated.rs` table (CardId::ReaperForm row), matching the
/// rest of the Necrobinder pool, so `upgrade()` returns None here.
pub(crate) fn play_reaper_form(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    apply_power_to_player(state, PowerKind::ReaperForm, 1);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::ReaperForm,
        name: "REAPER_FORM",
        cost: 3,
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
    // C# OnUpgrade `AddKeyword(Retain)` (spec NONE → RETAIN) is registered in
    // the central card_upgrades_generated.rs table (CardId::ReaperForm row),
    // matching the rest of the Necrobinder pool (Friendship/ForbiddenGrimoire
    // pattern), so the per-card delta is None.
    None
}
