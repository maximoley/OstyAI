use super::prelude::*;

/// Deal damage equal to the target's Doom. Upgrade adds Retain.
/// Source: `sts2-wiki/decompiled/v0.111.0/MegaCrit.Sts2.Core.Models.Cards/TimesUp.cs`.
pub(crate) fn play_times_up(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult {
    let t = target.expect("Times Up: enemy target required");
    // CalculatedDamage = 0 (base) + 1 (extra) × target's Doom amount = Doom.
    let dmg = resolving_preview(state, preview_times_up, target);
    let _ = deal_damage_player_to_enemy(state, t, dmg);
    EffectResult::Resolved
}

/// `CalculatedDamage`: the Doom stacked on the target. Shared with the card
/// face — see `crate::cards::preview`.
///
/// Target-reading: untargeted this is zero, which is what the game shows before
/// the player hovers an enemy (the card's own text carries the meaning).
pub(crate) fn preview_times_up(
    state: &CombatState,
    _card: &Card,
    target: Option<CreatureUid>,
) -> i32 {
    target.map_or(0, |t| creature_power_amount(state, t, PowerKind::Doom))
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::TimesUp,
        name: "TIMES_UP",
        cost: 2,
        card_type: CardType::Attack,
        target_type: TargetType::Enemy,
        rarity: CardRarity::Rare,
        base_damage: 0, // computed at play time from the target's Doom amount
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: AddKeyword(Retain). No damage/cost change.
    Some(CardUpgradeDelta {
        cost: None,
        base_damage: None,
        base_block: None,
        keywords: Some(CardKeywords::RETAIN),
    })
}
