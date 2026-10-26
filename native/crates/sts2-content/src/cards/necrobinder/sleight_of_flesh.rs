//! Sleight Of Flesh (Necrobinder). Hand-authored C#-first port via character-port skill.
//! C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/SleightOfFlesh.cs
use super::prelude::*;

/// Sleight Of Flesh (Uncommon, 2 energy, Power, Self). Apply `SleightOfFlesh`
/// (base 9, +4 when upgraded → 13) to self.
///
/// C# `SleightOfFlesh.OnPlay`:
///   `await CreatureCmd.TriggerAnim(Owner.Creature, "Cast", ..);`  (cosmetic, ignored)
///   `await PowerCmd.Apply<SleightOfFleshPower>(Owner.Creature,
///        DynamicVars["SleightOfFleshPower"].BaseValue, Owner.Creature, this);`
/// `CanonicalVars` = `PowerVar<SleightOfFleshPower>(9m)` → the power amount (9).
///
/// `OnUpgrade`: `DynamicVars["SleightOfFleshPower"].UpgradeValueBy(4m)` — a
/// PowerVar *amount* bump (9 → 13), so per the newer-pool convention it is
/// applied here via the `current_card_upgraded` branch and `upgrade()` returns
/// `None` (no central-table row either).
///
/// `SleightOfFleshPower` (SleightOfFleshPower.cs) is a Buff/Counter. Its
/// `AfterPowerAmountChanged` listener: when a power whose net type for the
/// changed amount is a Debuff is applied by THIS owner to an ENEMY (and that
/// power is not an `ITemporaryPower`), it deals `Amount` (the SleightOfFlesh
/// counter) Unpowered damage to that enemy. That listener IS wired in
/// `crates/sts2-core/src/hooks.rs` (the `PowerKind::SleightOfFlesh` arm on the
/// after-power-applied dispatch, bead `sts2-rs-4wppb`), so the debuff-triggered
/// damage rider fires at runtime.
pub(crate) fn play_sleight_of_flesh(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    // Apply SleightOfFleshPower to the player. PowerVar base 9, +4 upgraded.
    let amount = if current_card_upgraded(state) { 13 } else { 9 };
    apply_power_to_player(state, PowerKind::SleightOfFlesh, amount);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::SleightOfFlesh,
        name: "SLEIGHT_OF_FLESH",
        cost: 2,
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
    // C# OnUpgrade bumps the SleightOfFleshPower PowerVar amount by 4 (9 → 13).
    // A PowerVar-amount bump is applied in the handler via `current_card_upgraded`,
    // so the per-card upgrade hook carries no stat delta.
    None
}
