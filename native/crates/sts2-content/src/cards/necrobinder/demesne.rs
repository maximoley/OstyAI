//! Hand-authored (C#-first port). Keep the prelude import.
use super::prelude::*;

/// Demesne (Rare, 3 energy, Power, Self, Ethereal). Apply 1 Demesne.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Demesne.cs
//
// C# `OnPlay`: `PowerCmd.Apply<DemesnePower>(Owner.Creature,
// DynamicVars.Cards.BaseValue, Owner.Creature, this)` — apply `Cards`
// (CardsVar base 1) stacks of `DemesnePower` to the player. The "Cast"
// anim is cosmetic (ignored). CanonicalVars are `EnergyVar(1)` (a
// hover-tip magnitude, not a stat) and `CardsVar(1)` (the power amount).
//
// `DemesnePower` (DemesnePower.cs) is a Buff/Counter that, for the owning
// player, returns `count + Amount` from `ModifyHandDraw` and
// `amount + Amount` from `ModifyMaxEnergy` — i.e. a permanent "+Amount
// cards drawn each turn and +Amount max energy" while the stack exists.
//
// `OnUpgrade` does `EnergyCost.UpgradeBy(-1)` (cost 3 → 2); that delta
// already lives in the central `card_upgrades_generated.rs` table
// (CardId::Demesne, bead `sts2-rs-eiznx`), so `upgrade()` returns `None`.
// The power amount is not upgraded, so no `current_card_upgraded` branch.
pub(crate) fn play_demesne(state: &mut CombatState, _target: Option<CreatureUid>) -> EffectResult {
    apply_power_to_player(state, PowerKind::Demesne, 1);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Demesne,
        name: "DEMESNE",
        cost: 3,
        card_type: CardType::Power,
        target_type: TargetType::Self_,
        rarity: CardRarity::Rare,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::ETHEREAL,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade `EnergyCost.UpgradeBy(-1)` (3 → 2) lives in the central
    // `card_upgrades_generated.rs` table (CardId::Demesne), so no per-card
    // delta here.
    None
}
