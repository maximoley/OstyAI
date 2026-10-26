//! Spirit of Ash (Necrobinder pool). C#-first port.
//! C# source:
//! ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/SpiritOfAsh.cs
use super::prelude::*;

/// Spirit of Ash (Rare, 1 energy, Power, Self).
///
/// C# `SpiritOfAsh.OnPlay`:
///   * `CreatureCmd.TriggerAnim(...)` → cosmetic, ignored.
///   * `PowerCmd.Apply<SpiritOfAshPower>(Owner.Creature,
///     DynamicVars["BlockOnExhaust"].BaseValue, Owner.Creature, this)`
///     → apply N stacks of SpiritOfAsh to self, where N = the `BlockOnExhaust`
///     var (base 4, upgraded 5).
///
/// `CanonicalVars`: a single `DynamicVar("BlockOnExhaust", 4)` (the applied
/// amount). `OnUpgrade`: `DynamicVars["BlockOnExhaust"].UpgradeValueBy(1)` —
/// a PowerVar magnitude bump (4 → 5), NOT a spec damage/block/cost stat, so it
/// is gated here on `current_card_upgraded` and `upgrade()` returns None (no
/// central `card_upgrades_generated.rs` row).
///
/// `SpiritOfAshPower` (Buff, Counter, Amount=N) reacts on `BeforeCardPlayed`:
/// when the player plays a card with the Ethereal keyword, the owner gains
/// `Amount` Block (`ValueProp.Unpowered`). That listener IS wired in
/// `crates/sts2-core/src/hooks.rs` (the `PowerKind::SpiritOfAsh` arm on the
/// card-played block dispatch, bead `sts2-rs-4wppb`), so the applied stack
/// grants unpowered block on each Ethereal play at runtime.
pub(crate) fn play_spirit_of_ash(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    let amount = if current_card_upgraded(state) { 5 } else { 4 };
    apply_power_to_player(state, PowerKind::SpiritOfAsh, amount);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::SpiritOfAsh,
        name: "SPIRIT_OF_ASH",
        cost: 1,
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
    // C# OnUpgrade only bumps the `BlockOnExhaust` PowerVar magnitude (4 → 5),
    // which is applied in the handler via the `current_card_upgraded` branch —
    // not a spec damage/block/cost/keyword change. So no per-card delta and no
    // central card_upgrades_generated.rs row.
    None
}
