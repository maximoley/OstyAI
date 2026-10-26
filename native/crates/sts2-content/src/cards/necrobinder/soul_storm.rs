//! Soul Storm (Necrobinder). Hand-authored C#-first port via the character-port skill.
//! C# source: ../sts2-wiki/decompiled/v0.111.0/MegaCrit.Sts2.Core.Models.Cards/SoulStorm.cs
use super::prelude::*;

/// Deal 9 + 4 (6 upgraded) damage per Soul in the exhaust pile.
pub(crate) fn play_soul_storm(
    state: &mut CombatState,
    target: Option<CreatureUid>,
) -> EffectResult {
    let t = target.expect("Soul Storm: Enemy-targeted");

    let damage = resolving_preview(state, preview_soul_storm, target);

    let _ = deal_damage_player_to_enemy(state, t, damage);
    EffectResult::Resolved
}

/// `CalculatedDamage`: `CalculationBase(9) + ExtraDamage x <Souls in your
/// Exhaust Pile>`. Shared with the card face — see `crate::cards::preview`.
pub(crate) fn preview_soul_storm(
    state: &CombatState,
    card: &Card,
    _target: Option<CreatureUid>,
) -> i32 {
    let soul_count: i32 = state
        .player
        .exhaust
        .iter()
        .filter(|uid| {
            let idx = uid.0.get() as usize - 1;
            state.cards[idx].card_id == CardId::Soul
        })
        .count()
        .try_into()
        .unwrap_or(i32::MAX);
    let extra = if card.upgraded { 6 } else { 4 };
    9 + extra * soul_count
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::SoulStorm,
        name: "SOUL_STORM",
        cost: 1,
        card_type: CardType::Attack,
        target_type: TargetType::Enemy,
        rarity: CardRarity::Rare,
        base_damage: 9,
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: ExtraDamage.UpgradeValueBy(2) (4 → 6). The Extra multiplier
    // feeds the calculated damage computed in the handler (gated on
    // current_card_upgraded), not a plain spec field, so this returns None to
    // keep one source of truth.
    None
}
