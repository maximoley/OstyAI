//! Source: v0.111.0 Models.Cards/Abundance.cs.
use super::prelude::*;
pub(crate) fn play_abundance(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    let Some(character) = state.player.character else {
        return EffectResult::Resolved;
    };
    let ids = distinct_character_card_ids(state, character, Some(CardType::Power), 3);
    let options = ids
        .into_iter()
        .map(|card_id| sts2_core::ChoiceOption {
            card_id,
            upgraded: true,
            card_uid: None,
            source_pile: None,
        })
        .collect();
    state.request_card_choice(
        sts2_core::ChoiceKind::AddGeneratedFreeToHand,
        options,
        1,
        1,
        true,
    );
    EffectResult::Resolved
}
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Abundance,
        name: "ABUNDANCE",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Ancient,
        keywords: CardKeywords::EXHAUST,
        ..CardSpec::base()
    }
}
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    Some(CardUpgradeDelta {
        cost: Some(0),
        ..CardUpgradeDelta::default()
    })
}
