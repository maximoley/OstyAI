//! Hand-authored from the C# decomp (C#-first port).
use super::prelude::*;

/// Parse (Uncommon, 1 energy, Skill, Ethereal). Draw `CardsVar` cards
/// (3 base, 4 upgraded).
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Parse.cs
// OnPlay: CardPileCmd.Draw(ctx, DynamicVars.Cards.IntValue, Owner).
// OnUpgrade: DynamicVars.Cards.UpgradeValueBy(1m) → 3 → 4.
pub(crate) fn play_parse(state: &mut CombatState, _t: Option<CreatureUid>) -> EffectResult {
    let draws = if current_card_upgraded(state) { 4 } else { 3 };
    let _ = sts2_core::cards::draw_via_state(state, draws);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Parse,
        name: "PARSE",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Uncommon,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::ETHEREAL,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade bumps the CardsVar (draw count), handled in play_parse via
    // current_card_upgraded — no spec stat delta.
    None
}
