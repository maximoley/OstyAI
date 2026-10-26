//! Friendship (Necrobinder). C#-first port.
//! C# source:
//! ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Friendship.cs
use super::prelude::*;

/// Friendship (Uncommon, 1 energy, Power, Self).
///
/// C# `Friendship.OnPlay`:
///   * `PowerCmd.Apply<StrengthPower>(owner, -DynamicVars["StrengthPower"].BaseValue, …)`
///     → lose 2 Strength (base), 1 when upgraded. `OnUpgrade` does
///     `DynamicVars["StrengthPower"].UpgradeValueBy(-1)`, which *adds* -1 to the
///     StrengthPower base (2 → 1); OnPlay then applies `-BaseValue` = -1, so the
///     upgrade *reduces* the Strength penalty (-2 → -1) — it does not deepen it.
///   * `PowerCmd.Apply<FriendshipPower>(owner, DynamicVars.Energy.BaseValue, …)`
///     → gain 1 FriendshipPower (a Counter Buff whose `ModifyMaxEnergy` adds
///     `Amount` to the owner's max energy).
///
/// `CanonicalVars`: `PowerVar<StrengthPower>(2)`, `EnergyVar(1)`.
pub(crate) fn play_friendship(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    // StrengthPower amount: base 2, upgraded 1. C# OnUpgrade
    // `UpgradeValueBy(-1)` ADDS -1 to the StrengthPower base (2 → 1), so the
    // applied `-BaseValue` delta is -1 upgraded (a SMALLER penalty than the -2
    // base), not a deeper one. Applied as a NEGATIVE delta via the pool's
    // signed helper (the canonical `apply_power_to_*` paths early-return on
    // `amount <= 0`). Routes through the player's mirror creature, so the
    // Strength debuff is read by the damage path's mirror+legacy union.
    let strength_loss = if current_card_upgraded(state) { 1 } else { 2 };
    if let Some(uid) = state.player.creature_uid {
        apply_power_signed_to_creature(state, uid, PowerKind::Strength, -strength_loss);
    }

    // FriendshipPower: gain `EnergyVar` = 1 Counter stack. Its
    // `ModifyMaxEnergy` listener is wired in `hooks.rs` (the Pyre/Demesne
    // analogue, returning `amount + base.Amount` for the owning player —
    // bead `sts2-rs-4wppb`), so the stack grants +1 max energy at runtime.
    apply_power_to_player(state, PowerKind::Friendship, 1);

    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Friendship,
        name: "FRIENDSHIP",
        cost: 1,
        card_type: CardType::Power,
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
    // C# OnUpgrade: `DynamicVars["StrengthPower"].UpgradeValueBy(-1m)` — bumps a
    // PowerVar amount (the Strength magnitude), not a damage/block/cost stat.
    // Per conventions, that bump is applied in the handler via the
    // `current_card_upgraded` branch, so the spec-level upgrade delta is None.
    None
}
