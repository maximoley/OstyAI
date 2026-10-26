//! Hand-authored (C#-first port via character-port skill).
use super::prelude::*;

/// Undeath (Rare, 0 energy, Skill, Self). Gain 7 Block (9 upgraded), then add a
/// clone of this card to the discard pile.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Undeath.cs
//
// GainsBlock => true. CanonicalVars = BlockVar(7m, Move). base(0, Skill, Rare, Self).
// OnPlay:
//   await CreatureCmd.GainBlock(Owner.Creature, DynamicVars.Block, cardPlay);  → gain Block
//   CardModel card = CreateClone();                                             → exact self-copy
//   CardCmd.PreviewCardPileAdd(await CardPileCmd.AddGeneratedCardToCombat(card,
//                                  PileType.Discard, addedByPlayer: true), 2.2f);
// OnUpgrade: DynamicVars.Block.UpgradeValueBy(2m)  → 7 → 9 (sourced from the central
//            card_upgrades_generated.rs table; the runtime card already holds the
//            upgraded base_block, which both current_block(state) and the clone read).
//
// CreateClone() produces an exact copy of THIS card instance, preserving its
// upgrade level (and thus its 7-vs-9 base_block). We clone the resolving card's
// POD directly so the discard copy is identical (upgraded? base_block? tags),
// matching C# CardModel.CreateClone() semantics. spawn_card_into_pile reassigns
// the uid. The 2.2f PreviewCardPileAdd arg is a cosmetic animation delay — ignored.
pub(crate) fn play_undeath(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult {
    let _ = target; // Self-targeted; no explicit enemy target.
    gain_block_player(state, current_block(state));
    // CreateClone(): exact copy of the resolving Undeath instance (preserves
    // upgrade level + base_block + tags). spawn_card_into_pile overwrites uid.
    if let Some(clone) = current_card_idx(state).and_then(|i| state.cards.get(i).copied()) {
        spawn_card_into_pile(state, clone, PileKind::Discard);
    }
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Undeath,
        name: "UNDEATH",
        cost: 0,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Rare,
        base_damage: 0,
        base_block: 7,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: DynamicVars.Block.UpgradeValueBy(2m) → base_block 7 → 9.
    // The Necrobinder pool sources upgrades from the central
    // `card_upgrades_generated.rs` table (CardId::Undeath => base_block Some(9),
    // bead sts2-rs-t8b3k), so the per-card upgrade() returns None.
    None
}
