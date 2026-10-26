//! No Escape (Necrobinder pool). Hand-authored C#-first port.
use super::prelude::*;

/// No Escape (Uncommon, 1 energy, Skill, AnyEnemy). Apply Doom to the target
/// equal to `CalculatedDoom`: a base amount plus 5 per full 10 Doom already on
/// the target.
///
/// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/NoEscape.cs
///
/// C# `CanonicalVars`:
///   - `DynamicVar("DoomThreshold", 10)` — the per-stack threshold.
///   - `CalculationBaseVar(10)` + `CalculationExtraVar(5)`.
///   - `CalculatedVar("CalculatedDoom").WithMultiplier(...)` where the
///     multiplier = `floor(target.GetPowerAmount<DoomPower>() / DoomThreshold)`
///     = `floor(existing_doom / 10)`.
/// Per `CalculatedVar.Calculate`, the result is
///   `Base + Extra * multiplier` = `10 + 5 * floor(existing_doom / 10)`.
///
/// C# ctor `base(1, Skill, Uncommon, AnyEnemy)` → cost 1, Skill, Uncommon,
/// Enemy (AnyEnemy collapses to Enemy in single-player).
///
/// C# `OnPlay`: `PowerCmd.Apply<DoomPower>(target, CalculatedDoom.Calculate(target))`.
///
/// C# `OnUpgrade`: `DynamicVars.CalculationBase.UpgradeValueBy(5)` → the Base
/// term 10 → 15. The DoomThreshold (10) and Extra (5) are unchanged, so the
/// upgraded result is `15 + 5 * floor(existing_doom / 10)`. This is an
/// effect-var bump (not a stat field) → `upgrade()` returns `None` and the
/// bump is gated here on `current_card_upgraded`.
pub(crate) fn play_no_escape(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult {
    let t = target.expect("No Escape: enemy target required");
    let doom_amt = resolving_preview(state, preview_no_escape, target);
    apply_power_to_enemy(state, t, PowerKind::Doom, doom_amt);
    EffectResult::Resolved
}

/// `CalculatedDoom`: Base + Extra(5) * floor(existing_doom /
/// DoomThreshold(10)). Shared with the card face — see `crate::cards::preview`.
///
/// Target-reading: untargeted the multiplier is zero and the face shows the
/// plain base, which is what the game shows before the player hovers.
pub(crate) fn preview_no_escape(
    state: &CombatState,
    card: &Card,
    target: Option<CreatureUid>,
) -> i32 {
    let existing_doom = target.map_or(0, |t| creature_power_amount(state, t, PowerKind::Doom));
    let base_amt = if card.upgraded { 15 } else { 10 };
    base_amt + 5 * (existing_doom / 10)
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::NoEscape,
        name: "NO_ESCAPE",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Enemy,
        rarity: CardRarity::Uncommon,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: CalculationBase 10 → 15 is an effect-var (Doom amount)
    // bump, handled inline in play_no_escape via the current_card_upgraded
    // branch. No stat-field change.
    None
}
