//! GH #369: a relic whose `before_hand_draw` opens a card-select suspends the
//! natural hand draw until the select resolves.
//!
//! C# `Toolbox.BeforeHandDraw` (`Toolbox.cs:22-33`) `await`s
//! `CardSelectCmd.FromChooseACardScreen` and only then calls
//! `AddGeneratedCardToCombat(.., PileType.Hand)`. The whole listener runs
//! inside the awaited `Hook.BeforeHandDraw` at `CombatManager.cs:654`, which
//! precedes the `Draw` at `:675` — so the chosen card is in hand BEFORE the
//! natural draw of 5, and the draw sees one fewer free hand slot.
//!
//! Rust used to fire the listener, leave the choice sitting in
//! `pending_choice`, and run the draw plus the rest of turn setup straight
//! past it — the player answered during `PlayerPlayPhase` against an
//! already-drawn hand. Two consequences: the grant landed after the draw
//! instead of before (so a full hand dropped a card C# had room for), and the
//! mirror never parked at the moment the world did, which is why every live
//! Toolbox screen arrived `unparked`.

use std::num::NonZeroU32;

use sts2_content::cards::dispatch as content_dispatch;
use sts2_core::cards::{Card, CardId, CardType, TargetType, HAND_LIMIT};
use sts2_core::potions::CharacterId;
use sts2_core::{combat_loop, Action, ChoiceKind, CombatState, Relic, RelicKind, StepResult};

/// A round-1 Ironclad state holding Toolbox, with `draw_pile_size` distinct
/// cards in the draw pile and an empty hand.
fn state_with_toolbox(draw_pile_size: usize) -> CombatState {
    // BOTH registries: the relic bus is what raises the Toolbox listener, the
    // card bus is what the colorless-pool sampler behind its options needs.
    sts2_content::relics::dispatch::ensure_installed();
    content_dispatch::install_default_registry();
    let mut state = CombatState {
        run_rng: sts2_core::run_rng_set::RunRngSet::new(12345_i32),
        ..Default::default()
    };
    state.player.character = Some(CharacterId::Ironclad);
    state.player.current_hp = 80;
    state.player.max_hp = 80;
    let _ = state.init_player_creature();
    state.round = 1;
    state.phase = sts2_core::CombatPhase::PlayerTurnStart;
    state.side = sts2_core::Side::Player;
    state.relics.push(Relic {
        id: NonZeroU32::new(1).expect("nz"),
        kind: RelicKind::Toolbox,
        counter: 0,
        is_wax: false,
        is_melted: false,
    });
    for _ in 0..draw_pile_size {
        let raw = u32::try_from(state.cards.len() + 1).expect("u32");
        let uid = sts2_core::CardUid::new(raw).expect("nz");
        state.cards.push(Card {
            uid,
            card_id: CardId::StrikeIronclad,
            cost: 1,
            card_type: CardType::Attack,
            target_type: TargetType::Enemy,
            upgraded: false,
            base_damage: 6,
            base_block: 0,
            ..Card::pod_defaults()
        });
        state.player.draw.push(uid);
    }
    state
}

/// The whole point: turn setup STOPS at the Toolbox screen with the hand still
/// empty, and the draw only runs once the pick is answered.
#[test]
fn toolbox_suspends_the_natural_draw_until_its_pick_resolves() {
    let mut state = state_with_toolbox(10);

    let result = combat_loop::start_player_turn(&mut state);

    assert_eq!(
        result,
        StepResult::PendingChoice,
        "Toolbox's select must suspend turn setup"
    );
    let pc = state
        .pending_choice
        .as_ref()
        .expect("Toolbox parks a pending choice");
    assert_eq!(pc.kind, ChoiceKind::AddGeneratedToHand);
    assert!(
        state.player.hand.is_empty(),
        "the natural draw must NOT have run yet — C# awaits the select first, \
         hand was {:?}",
        state.player.hand.len()
    );
    assert!(
        state.pending_relic_hand_draw_resume.is_some(),
        "the stashed draw count is what lets the tail resume"
    );

    // Answering the screen runs the deferred tail: the grant lands, THEN the
    // natural draw of 5 on top of it.
    let after = sts2_core::apply(
        &mut state,
        Action::ResolveChoice {
            selection: smallvec::smallvec![0],
        },
    );

    assert_ne!(
        after,
        StepResult::PendingChoice,
        "the tail must run to completion"
    );
    assert!(
        state.pending_relic_hand_draw_resume.is_none(),
        "the resume marker is consumed"
    );
    assert_eq!(
        state.player.hand.len(),
        6,
        "1 Toolbox grant + the natural draw of 5"
    );
    assert_eq!(
        state.relics[0].counter, 1,
        "the once-per-combat latch tripped exactly once"
    );
}

/// Ordering has an observable consequence at the hand cap: because the grant
/// precedes the draw, it is the DRAW that gets squeezed, not the grant. Before
/// the fix the draw ran first and the grant was the card that fell off.
#[test]
fn the_grant_precedes_the_draw_so_the_draw_is_what_gets_capped() {
    let mut state = state_with_toolbox(10);
    // Pre-fill the hand to one slot below the cap so exactly one more card
    // fits. C# order (grant, then draw) puts the Toolbox card in that slot.
    for _ in 0..(HAND_LIMIT - 1) {
        let raw = u32::try_from(state.cards.len() + 1).expect("u32");
        let uid = sts2_core::CardUid::new(raw).expect("nz");
        state.cards.push(Card {
            uid,
            card_id: CardId::DefendIronclad,
            cost: 1,
            card_type: CardType::Skill,
            target_type: TargetType::Self_,
            upgraded: false,
            base_damage: 0,
            base_block: 5,
            ..Card::pod_defaults()
        });
        state.player.hand.push(uid);
    }
    let filler = state.player.hand.len();

    assert_eq!(
        combat_loop::start_player_turn(&mut state),
        StepResult::PendingChoice
    );
    let granted_id = state
        .pending_choice
        .as_ref()
        .expect("pending choice")
        .options[0]
        .card_id;
    let _ = sts2_core::apply(
        &mut state,
        Action::ResolveChoice {
            selection: smallvec::smallvec![0],
        },
    );

    assert_eq!(
        state.player.hand.len(),
        HAND_LIMIT,
        "the hand fills to the cap"
    );
    assert_eq!(
        state.player.hand.len() - filler,
        1,
        "exactly one card was added"
    );
    let added = state.player.hand[filler];
    assert_eq!(
        state
            .cards
            .iter()
            .find(|c| c.uid == added)
            .map(|c| c.card_id),
        Some(granted_id),
        "the card that made it in is the Toolbox grant, not a drawn card — \
         the grant runs BEFORE the draw"
    );
}

/// Round 2+ must not suspend: Toolbox is round-1 only, so turn setup runs
/// straight through and the draw happens as normal.
#[test]
fn a_later_round_does_not_suspend() {
    let mut state = state_with_toolbox(10);
    state.round = 2;

    let result = combat_loop::start_player_turn(&mut state);

    assert_ne!(result, StepResult::PendingChoice);
    assert!(state.pending_relic_hand_draw_resume.is_none());
    assert_eq!(state.player.hand.len(), 5, "the natural draw ran");
    assert_eq!(state.relics[0].counter, 0, "the latch never tripped");
}
