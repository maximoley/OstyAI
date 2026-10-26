//! Hand-authored (C#-first port). Keep the prelude import.
use super::prelude::*;

/// Countdown (Uncommon, 1 energy, Power, Self). Apply 6 Countdown to self.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Countdown.cs
//
// C# `CanonicalVars`: `PowerVar<CountdownPower>(6m)`.
// C# `OnPlay`: `PowerCmd.Apply<CountdownPower>(Owner.Creature, BaseValue, Owner.Creature, this)`
// — applies CountdownPower to the player at the var base value. The "Cast"
// anim is cosmetic (ignored).
// C# `OnUpgrade`: `DynamicVars["CountdownPower"].UpgradeValueBy(3m)` → 9 upgraded.
// CountdownPower is a PowerVar magnitude (not damage/block/cost), so the
// upgrade bump is gated here on `current_card_upgraded` and `upgrade()`
// returns `None`.
//
// The CountdownPower listener (AfterSideTurnStart → apply Doom to a random
// hittable enemy via the CombatTargets RNG) and DoomPower's lethal-threshold
// kill (BeforeTurnEnd) are both wired in `crate::hooks` (sts2-core), so this
// card is fully live.
pub(crate) fn play_countdown(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    let amount = if current_card_upgraded(state) { 9 } else { 6 };
    apply_power_to_player(state, PowerKind::Countdown, amount);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::CountdownCard,
        name: "COUNTDOWN_CARD",
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
    // OnUpgrade bumps the CountdownPower var (6 → 9); applied in the handler
    // via the `current_card_upgraded` branch, so no stat-field delta here.
    None
}
