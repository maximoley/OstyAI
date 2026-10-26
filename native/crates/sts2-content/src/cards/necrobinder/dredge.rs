//! Hand-authored C#-first port. Keep the prelude import.
use super::prelude::*;

/// Dredge (Uncommon, 1 energy, Skill, Self). C# `Dredge.cs`: choose up to
/// `CardsVar(3)` cards from the **discard** pile and add them to your hand,
/// where the count is additionally capped by remaining hand space
/// (`Math.Min(3, 10 - hand.Count)`). Exhaust. OnUpgrade adds Retain (the
/// keyword delta lives in the central `card_upgrades_generated.rs` table).
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Dredge.cs
//
// CanonicalVars:   CardsVar(3)                 -> up to 3 cards (unchanged on upgrade)
// CanonicalKeywords: Exhaust
// ctor: base(1, CardType.Skill, CardRarity.Uncommon, TargetType.Self)
// OnPlay (Dredge.cs:26-30):
//   num = Math.Min(DynamicVars.Cards.IntValue, 10 - Hand.Count)   // HAND_LIMIT = 10
//   if num > 0:
//     selected = CardSelectCmd.FromSimpleGrid(ctx, Discard.Cards, owner,
//                  CardSelectorPrefs(prompt, num))                 // min == max == num
//     CardPileCmd.Add(selected, Hand)
// OnUpgrade: AddKeyword(Retain)  -> central table (CardId::Dredge row).
//
// `CardSelectorPrefs(prompt, num)` sets MinSelect = MaxSelect = num
// (`CardSelectorPrefs.cs:41-45`). `CardSelectCmd.FromSimpleGrid` AUTO-resolves
// when the source pile count <= MinSelect — i.e. when there are no more discard
// cards than the player must pick, it takes them all without a prompt
// (`CardSelectCmd.cs:232-234`). Only a discard pile larger than `num` opens the
// selection screen.
//
// Faithful surfacing: when `discard.len() <= num` we take ALL discard cards
// directly (the FromSimpleGrid `cards.Count <= MinSelect` fast path — no prompt,
// no RNG). When `discard.len() > num` we open a multi-select
// `ChoiceKind::AddFromDiscardToHand` pending choice with `min == max == num`
// (the kind shared with Graveblast's single-pick flavor; the core resolver
// iterates the full selection) so the agent picks exactly `num` cards —
// mirroring the C# player multi-select. The prior port auto-picked the front
// `num` discard cards deterministically and never paused, so the harness's
// replayed `select_cards` verb was rejected (Rust already back in combat),
// permanently desyncing the run (sweep RP115, `combat/state/enemies_hp`
// cs=[33] rs=[27]).
pub(crate) fn play_dredge(state: &mut CombatState, _target: Option<CreatureUid>) -> EffectResult {
    // num = min(CardsVar(3), HAND_LIMIT - hand.len()). HAND_LIMIT = 10.
    // CardsVar stays 3 on upgrade (upgrade only adds Retain).
    let cards_var: usize = 3;
    let hand_space = sts2_core::cards::HAND_LIMIT.saturating_sub(state.player.hand.len());
    let num = cards_var.min(hand_space);

    // C# `if (num > 0)` — num == 0 (full hand) is a no-op.
    if num == 0 {
        return EffectResult::Resolved;
    }

    if state.player.discard.len() <= num {
        // FromSimpleGrid auto-takes every discard card (`cards.Count <=
        // MinSelect`, CardSelectCmd.cs:232-234) — move directly, no prompt,
        // no RNG. Preserve discard-pile (oldest-first) order.
        while !state.player.discard.is_empty() {
            let uid = state.player.discard[0];
            card_effects::move_card_to_pile(state, uid, PileKind::Hand, false);
        }
    } else {
        // Surplus: open a multi-select over the discard pile (min == max ==
        // num), in discard-pile order (C# passes `…Cards` with NO OrderBy).
        let options: Vec<sts2_core::ChoiceOption> = state
            .player
            .discard
            .iter()
            .filter_map(|&uid| {
                let card = state.cards.iter().find(|c| c.uid == uid)?;
                Some(sts2_core::ChoiceOption {
                    card_id: card.card_id,
                    upgraded: card.upgraded,
                    card_uid: Some(uid),
                    source_pile: Some(sts2_core::cards::PileKind::Discard),
                })
            })
            .collect();
        let n = num as u8;
        state.request_card_choice(
            sts2_core::ChoiceKind::AddFromDiscardToHand,
            options,
            n,
            n,
            false,
        );
    }
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Dredge,
        name: "DREDGE",
        cost: 1,
        card_type: CardType::Skill,
        target_type: TargetType::Self_, // C# TargetType.Self
        rarity: CardRarity::Uncommon,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::EXHAUST, // CanonicalKeywords { Exhaust }
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade `AddKeyword(CardKeyword.Retain)` lives in the central
    // `card_upgrades_generated.rs` table (CardId::Dredge row: EXHAUST -> EXHAUST
    // + RETAIN), per the Necrobinder/Regent newer-pool convention.
    None
}
