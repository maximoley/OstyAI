//! Pagestorm (Necrobinder). Hand-authored C#-first port via character-port skill.
//! C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Pagestorm.cs
use super::prelude::*;

/// Pagestorm (Uncommon, 1 energy, Power, Self). Apply 1 `Pagestorm`
/// to self (the `Cards` var, base 1).
///
/// C# `Pagestorm.OnPlay`:
///   `await CreatureCmd.TriggerAnim(Owner.Creature, "Cast", ..);`  (cosmetic, ignored)
///   `await PowerCmd.Apply<PagestormPower>(Owner.Creature, DynamicVars.Cards.BaseValue, ..);`
/// `CanonicalVars` = `CardsVar(1)` → the power amount (1). The card itself is
/// NOT Ethereal — C# exposes Ethereal only via `ExtraHoverTips` (a tooltip for
/// `PagestormPower`'s on-draw rider), never in `CanonicalKeywords`, so the spec
/// carries no keyword.
///
/// `PagestormPower` (PagestormPower.cs) is a Buff/Counter: `AfterCardDrawn`,
/// when the owning player draws a card that has the `Ethereal` keyword, draws
/// `Amount` additional cards. That on-draw rider is wired in sts2-core's
/// `CombatState::invoke_card_drawn_for_newly_drawn` (bead `sts2-rs-4wppb`);
/// the draw recurses through that surface so a chain of Ethereal-into-Ethereal
/// draws cascades faithfully.
///
/// `OnUpgrade`: `EnergyCost.UpgradeBy(-1)` (1 → 0) — carried by the central
/// `card_upgrades_generated.rs` table (CardId::Pagestorm), so `upgrade()` here
/// returns `None`.
pub(crate) fn play_pagestorm(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    // Apply PagestormPower(1) to the player. The Cards var (1) is not bumped by
    // OnUpgrade (only the energy cost is), so the amount is fixed at 1.
    apply_power_to_player(state, PowerKind::Pagestorm, 1);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Pagestorm,
        name: "PAGESTORM",
        cost: 1,
        card_type: CardType::Power,
        target_type: TargetType::Self_,
        rarity: CardRarity::Uncommon,
        base_damage: 0,
        base_block: 0,
        // NOT Ethereal: C# `Pagestorm.cs` has no `CanonicalKeywords` override —
        // Ethereal appears only as an `ExtraHoverTip` describing the power.
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: `EnergyCost.UpgradeBy(-1)` (cost 1 → 0). This delta lives in
    // the central `card_upgrades_generated.rs` table (CardId::Pagestorm), per
    // the newer-pool convention, so the per-card hook carries no delta.
    None
}
