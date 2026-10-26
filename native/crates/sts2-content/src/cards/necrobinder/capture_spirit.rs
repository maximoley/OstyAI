//! Hand-authored from the C# decompilation (character-port skill).
use super::prelude::*;

use sts2_core::cards::CardId as CoreCardId;
use sts2_core::damage::apply_damage;
use sts2_core::hooks::{CreatureRef, ValueProps};

/// CaptureSpirit (Uncommon, 1 energy, Skill). Deal `DamageVar`(3)
/// *unblockable, unpowered* damage to the target, then generate
/// `CardsVar`(3) Soul token(s) into the Draw pile at a random position.
/// When CaptureSpirit is upgraded the damage is 4 (central upgrade table)
/// and 4 Souls are generated (CardsVar upgraded by 1).
//
// C# source:
// ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/CaptureSpirit.cs
//   CanonicalVars: DamageVar(3m, Unblockable | Unpowered | Move), CardsVar(3)
//   OnPlay: CreatureCmd.Damage(ctx, Target, DynamicVars.Damage, this);
//           souls = Soul.Create(Owner, DynamicVars.Cards.IntValue);
//           CardPileCmd.AddGeneratedCardsToCombat(souls, PileType.Draw,
//               addedByPlayer: true, CardPilePosition.Random);
//   OnUpgrade: Damage.UpgradeValueBy(1m) (3 -> 4; central upgrade table)
//              Cards.UpgradeValueBy(1m) (3 -> 4 Souls; effect-level, gated here).
pub(crate) fn play_capture_spirit(
    state: &mut CombatState,
    target: Option<CreatureUid>,
) -> EffectResult {
    let t = target.expect("CaptureSpirit: AnyEnemy-targeted");

    // C# DamageVar carries ValueProp.Unblockable | Unpowered | Move, so route
    // through the canonical damage pipeline with those props rather than the
    // plain `deal_damage_player_to_enemy` (MOVE-only) helper. `this` (the
    // played CaptureSpirit) is the damage source.
    let dmg = current_damage(state);
    let props = ValueProps::MOVE
        .union(ValueProps::UNBLOCKABLE)
        .union(ValueProps::UNPOWERED);
    let _ = apply_damage(
        state,
        CreatureRef::Player,
        t,
        dmg,
        props,
        Some(CoreCardId::CaptureSpirit),
    );

    // CardsVar(3); OnUpgrade bumps it to 4 (effect-level). Generate that many
    // Soul tokens into the Draw pile at random positions (CardPilePosition.Random).
    let souls = if current_card_upgraded(state) { 4 } else { 3 };
    for _ in 0..souls {
        let soul = card_from_spec(&status_specs::soul());
        let _ = spawn_card_into_draw_pile_random_position(state, soul);
    }

    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::CaptureSpirit,
        name: "CAPTURE_SPIRIT",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Enemy,
        rarity: CardRarity::Uncommon,
        base_damage: 3,
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // Spec-level damage upgrade (3 -> 4) lives in the central
    // `card_upgrades_generated::upgrade_delta` table (the authoritative path
    // for this pool, mirroring Regent/newer-pool convention). The CardsVar
    // bump (3 -> 4 Souls) is effect-level, gated in the handler on
    // `current_card_upgraded`. Returning None here avoids a duplicate source
    // of truth.
    None
}
