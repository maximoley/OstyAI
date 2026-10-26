//! Necrobinder Eidolon. Hand-authored C#-first port.
use super::prelude::*;
use sts2_core::CardUid;

/// Auto-play the snapshot of playable Ethereal cards in exhaust, in pile order.
/// Source: `sts2-wiki/decompiled/v0.111.0/MegaCrit.Sts2.Core.Models.Cards/Eidolon.cs`.
pub(crate) fn play_eidolon(state: &mut CombatState, _target: Option<CreatureUid>) -> EffectResult {
    let cards: Vec<CardUid> = state
        .player
        .exhaust
        .iter()
        .copied()
        .filter(|uid| {
            let card = &state.cards[uid.0.get() as usize - 1];
            sts2_core::card_effects::card_ethereal_inst(card)
                && !sts2_core::card_effects::card_unplayable_inst(card)
        })
        .collect();
    eidolon_run(state, &cards);
    EffectResult::Resolved
}

fn eidolon_run(state: &mut CombatState, cards: &[CardUid]) {
    for (index, &uid) in cards.iter().enumerate() {
        let _ = sts2_core::combat_loop::check_combat_end(state);
        if state.is_over {
            break;
        }
        // This primitive follows captured references across all live piles,
        // including exhaust, and rolls the target for Enemy cards.
        sts2_core::combat_loop::auto_play_specific_card_from_discard(state, uid, false);
        if let Some(resume) = state.pending_autoplay_resume.as_mut() {
            resume.continuation = Some(sts2_core::AutoPlayLoopCont {
                resume_fn: eidolon_resume,
                iters: 0,
                uids: cards[index + 1..].iter().copied().collect(),
                force_exhaust: false,
            });
            return;
        }
    }
}

fn eidolon_resume(state: &mut CombatState, _iters: u32, uids: &[CardUid], _force_exhaust: bool) {
    eidolon_run(state, uids);
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Eidolon,
        name: "EIDOLON",
        cost: 2,
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Rare,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::EXHAUST,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // Upgrade lowers the energy cost by one.
    Some(CardUpgradeDelta {
        cost: Some(1),
        base_damage: None,
        base_block: None,
        keywords: None,
    })
}
