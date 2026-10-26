//! Necrobinder — Call of the Void.
use super::prelude::*;

/// Call of the Void (Rare, 1 energy, Power). Applies CallOfTheVoidPower
/// (Counter) to self with amount = Cards base value (1). The power's
/// `BeforeHandDraw` hook then generates that many random non-Basic/non-Ancient
/// cards from the character's pool, makes each Ethereal, and adds them to hand
/// at the start of each turn.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/CallOfTheVoid.cs
pub(crate) fn play_call_of_the_void(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    // C# CallOfTheVoid.OnPlay:
    //   PowerCmd.Apply<CallOfTheVoidPower>(Owner.Creature, DynamicVars.Cards.BaseValue,
    //                                      Owner.Creature, this).
    // CardsVar(1) → amount 1. TargetType::Self_ — no enemy target.
    // OnUpgrade only AddKeyword(Innate) (handled in the central upgrade table),
    // so the applied amount is not gated on upgrade.
    apply_power_to_player(state, PowerKind::CallOfTheVoid, 1);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::CallOfTheVoid,
        name: "CALL_OF_THE_VOID",
        cost: 1,
        card_type: CardType::Power,
        target_type: TargetType::Self_,
        rarity: CardRarity::Rare,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: AddKeyword(CardKeyword.Innate). The keyword delta lives in
    // the central `card_upgrades_generated.rs` table (bead sts2-rs-nidl4), so
    // this per-card `upgrade()` returns None.
    None
}
