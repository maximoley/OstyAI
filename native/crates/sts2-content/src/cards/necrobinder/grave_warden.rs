//! Hand-authored from the C# decompilation (character-port skill).
use super::prelude::*;

/// Grave Warden (Common, 1 energy, Skill, Self). Gain `BlockVar`(8) block,
/// then generate `CardsVar`(1) Soul token into the Draw pile at a random
/// position.
//
// C# source:
// ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/GraveWarden.cs
//   GainsBlock => true
//   ctor: base(1, CardType.Skill, CardRarity.Common, TargetType.Self)
//   OnPlay: CreatureCmd.GainBlock(Owner.Creature, Block);
//           souls = Soul.Create(Owner, Cards.IntValue=1, CombatState);
//           CardPileCmd.AddGeneratedCardsToCombat(souls, PileType.Draw,
//               addedByPlayer: true, CardPilePosition.Random);
//   OnUpgrade: Block.UpgradeValueBy(3m)  (8 -> 11; spec-level, via upgrade()).
//
// Note: GraveWarden's OnUpgrade only bumps Block; it does NOT upgrade the
// generated Soul(s) (unlike Reave). The Soul is always spawned un-upgraded.
pub(crate) fn play_grave_warden(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    gain_block_player(state, current_block(state));

    // C# Cards.IntValue == 1; OnUpgrade does not touch CardsVar, so always
    // generate exactly one Soul. CardPilePosition.Random -> insert at a random
    // index in the Draw pile.
    let soul = card_from_spec(&status_specs::soul());
    let _ = spawn_card_into_draw_pile_random_position(state, soul);

    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::GraveWarden,
        name: "GRAVE_WARDEN",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Common,
        base_damage: 0,
        base_block: 8,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: Block.UpgradeValueBy(3) -> 8 + 3 = 11.
    Some(CardUpgradeDelta {
        cost: None,
        base_damage: None,
        base_block: Some(11),
        keywords: None,
    })
}
