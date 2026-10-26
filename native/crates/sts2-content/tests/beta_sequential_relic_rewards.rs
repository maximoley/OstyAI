//! RewardsSet.Offer awaits each relic's AfterObtained before the next claim.
//! Source: v0.111.0 Rewards/RewardsSet.cs, Models.Relics/{ToyBox,Orrery,
//! Pomander,PhialHolster}.cs. These exercise real content listeners through
//! the generic event routing path, with controlled already-rolled relic order.
use sts2_content::cards::spec_index::card_pod_from_id;
use sts2_core::cards::CardId;
use sts2_core::run_manager::{ActionResult, Phase, RunAction, RunManager};
use sts2_core::{CharacterId, PotionKind, RelicId, RunState};

fn toybox_event(rs: &mut RunState, _: usize) -> bool {
    rs.obtain_relic(RelicId::ToyBox);
    rs.exit_event_room();
    true
}
fn manager(order: &[RelicId]) -> RunManager {
    sts2_content::relics::dispatch::ensure_installed();
    let mut callbacks = sts2_content::run_callbacks::ironclad_content_callbacks();
    callbacks.resolve_event_at = Some(toybox_event);
    callbacks.event_option_count = Some(|_| 1);
    callbacks.event_option_indices = None;
    let mut rm = RunManager::new(77, CharacterId::Necrobinder)
        .with_content_callbacks(callbacks)
        .with_neow_start();
    rm.run_state.direct_event_relic_grant = true;
    rm.run_state.interactive_card_upgrade = true;
    rm.run_state.relic_grab_bag.clear();
    rm.run_state.relic_grab_bag_by_rarity.clear();
    rm.run_state.relic_grab_bag_fallback = order.to_vec();
    rm.run_state.master_deck = [CardId::StrikeNecrobinder, CardId::DefendNecrobinder]
        .into_iter()
        .map(|id| card_pod_from_id(id, false).unwrap())
        .collect();
    rm
}
fn act(rm: &mut RunManager, action: RunAction) {
    assert_eq!(rm.take_action(action), ActionResult::Advanced);
}
#[test]
fn toybox_awaits_five_orrery_offers_then_pomander_before_other_wax_relics() {
    let order = [
        RelicId::Orrery,
        RelicId::Pomander,
        RelicId::Strawberry,
        RelicId::Anchor,
    ];
    let mut rm = manager(&order);
    rm.run_state.relics.push(RelicId::LuckyFysh);
    rm.run_state
        .pending_post_pick_cards
        .push(card_pod_from_id(CardId::Clumsy, false).unwrap());
    let gold = rm.run_state.gold;
    rm.run_state.potion_use_locked_by_event = Some(sts2_core::events::EventId::TheFutureOfPotions);
    act(&mut rm, RunAction::EventChoice { idx: 0 });
    assert_eq!(rm.phase(), Phase::CardReward);
    assert!(rm.run_state.relics.contains(&RelicId::Orrery));
    assert!(!rm.run_state.relics.contains(&RelicId::Pomander));
    assert_eq!(
        rm.run_state.wax_relics.iter().copied().collect::<Vec<_>>(),
        vec![RelicId::Orrery]
    );
    assert_eq!(rm.run_state.auto_reward_frames.len(), 1);
    let mut replay = rm.clone();
    for i in 0..5 {
        assert!(!rm.card_reward_options().is_empty());
        assert_eq!(
            rm.run_state.master_deck.len(),
            2,
            "deferred curse awaits the complete ToyBox grant"
        );
        assert_eq!(rm.run_state.gold, gold);
        assert!(!rm.run_state.relics.contains(&RelicId::Strawberry));
        act(&mut rm, RunAction::PickCardReward { idx: None });
        act(&mut replay, RunAction::PickCardReward { idx: None });
        assert_eq!(
            sts2_core::hashing::run_hash(&rm),
            sts2_core::hashing::run_hash(&replay)
        );
        if i < 4 {
            assert!(!rm.run_state.relics.contains(&RelicId::Pomander));
        }
    }
    assert!(rm.run_state.relics.contains(&RelicId::Pomander));
    assert_eq!(rm.run_state.pending_deck_upgrades, 1);
    assert!(!rm.run_state.relics.contains(&RelicId::Strawberry));
    assert!(rm.run_state.potion_use_locked_by_event.is_some());
    act(&mut rm, RunAction::UpgradeDeckCard { idx: 0 });
    act(&mut replay, RunAction::UpgradeDeckCard { idx: 0 });
    assert_eq!(
        sts2_core::hashing::run_hash(&rm),
        sts2_core::hashing::run_hash(&replay)
    );
    for relic in order {
        assert!(
            rm.run_state.relics.contains(&relic),
            "missing {relic:?}: relics={:?} frames={:?} pending={:?} phase={:?}",
            rm.run_state.relics,
            rm.run_state.auto_reward_frames,
            rm.run_state.pending_rewards,
            (
                rm.phase(),
                rm.card_reward_options(),
                rm.run_state.pending_deck_upgrades,
                &rm.run_state.pending_card_selects,
                &rm.run_state.pending_potion_swaps
            )
        );
        assert!(rm.run_state.wax_relics.contains(&relic));
    }
    assert!(rm.run_state.auto_reward_frames.is_empty());
    assert!(rm.run_state.pending_rewards.is_empty());
    assert_eq!(rm.phase(), Phase::MapChoice);
    assert!(rm.run_state.potion_use_locked_by_event.is_none());
    assert_eq!(
        rm.run_state.master_deck.last().unwrap().card_id,
        CardId::Clumsy
    );
    assert_eq!(
        rm.run_state.gold,
        gold + 15,
        "deferred curse dispatches LuckyFysh once"
    );
}
#[test]
fn toybox_waits_for_full_belt_phial_decision_before_next_relic() {
    let mut rm = manager(&[
        RelicId::PhialHolster,
        RelicId::Strawberry,
        RelicId::Anchor,
        RelicId::OddlySmoothStone,
    ]);
    rm.run_state.interactive_potion_swap = true;
    rm.run_state.potions = vec![PotionKind::FirePotion; rm.run_state.max_potion_slots as usize];
    act(&mut rm, RunAction::EventChoice { idx: 0 });
    assert!(rm.run_state.relics.contains(&RelicId::PhialHolster));
    assert!(!rm.run_state.relics.contains(&RelicId::Strawberry));
    assert_eq!(rm.run_state.pending_potion_swaps.len(), 1);
    // PhialHolster grants one slot, so its first potion fits and only the
    // second needs a full-belt decision.
    act(&mut rm, RunAction::PotionSwap { discard_slot: None });
    assert!(rm.run_state.relics.contains(&RelicId::Strawberry));
    assert!(rm.run_state.auto_reward_frames.is_empty());
    assert_eq!(rm.phase(), Phase::MapChoice);
}
#[test]
fn generic_relics_wait_for_active_prepared_card_offer_without_overwriting_it() {
    fn event(rs: &mut RunState, _: usize) -> bool {
        let mut card = sts2_core::CardReward::from_options(vec![CardId::Soul]);
        card.populated = true;
        card.prepared_cards = vec![card_pod_from_id(CardId::Soul, false).unwrap()];
        rs.pending_rewards.push(sts2_core::Reward::Card(card));
        rs.relic_rewards
            .push(sts2_core::RelicReward::new(RelicId::Strawberry));
        rs.exit_event_room();
        true
    }
    let mut rm = manager(&[]);
    let mut callbacks = sts2_content::run_callbacks::ironclad_content_callbacks();
    callbacks.resolve_event_at = Some(event);
    rm = rm.with_content_callbacks(callbacks);
    act(&mut rm, RunAction::EventChoice { idx: 0 });
    assert_eq!(rm.card_reward_options(), &[CardId::Soul]);
    assert_eq!(rm.card_reward_prepared_cards()[0].card_id, CardId::Soul);
    assert!(!rm.run_state.relics.contains(&RelicId::Strawberry));
    act(&mut rm, RunAction::PickCardReward { idx: Some(0) });
    assert_eq!(
        rm.run_state.master_deck.last().unwrap().card_id,
        CardId::Soul
    );
    assert!(rm.run_state.relics.contains(&RelicId::Strawberry));
    assert_eq!(rm.phase(), Phase::MapChoice);
}

#[test]
fn mandatory_card_grant_finishes_all_picks_and_add_hooks_before_parent_relic() {
    fn event(rs: &mut RunState, _: usize) -> bool {
        rs.pending_card_grants
            .push(sts2_core::runstate::PendingCardGrant {
                options: [CardId::Reave, CardId::Shroud, CardId::Haunt]
                    .into_iter()
                    .map(|id| card_pod_from_id(id, false).unwrap())
                    .collect(),
                pick: 2,
                selected: Vec::new(),
            });
        rs.relic_rewards
            .push(sts2_core::RelicReward::new(RelicId::Strawberry));
        rs.exit_event_room();
        true
    }
    let mut rm = manager(&[]);
    let mut callbacks = sts2_content::run_callbacks::ironclad_content_callbacks();
    callbacks.resolve_event_at = Some(event);
    rm = rm.with_content_callbacks(callbacks);
    rm.run_state
        .relics
        .extend([RelicId::LuckyFysh, RelicId::FrozenEgg]);
    let gold = rm.run_state.gold;
    act(&mut rm, RunAction::EventChoice { idx: 0 });
    assert!(!rm.run_state.relics.contains(&RelicId::Strawberry));
    act(&mut rm, RunAction::GrantDeckCard { idx: 0 });
    assert_eq!(
        rm.run_state.gold, gold,
        "no deck-add hooks before complete selection"
    );
    assert_eq!(rm.run_state.master_deck.len(), 2);
    assert!(!rm.run_state.pending_card_grants[0].selected[0].upgraded);
    assert!(!rm.run_state.relics.contains(&RelicId::Strawberry));
    act(&mut rm, RunAction::GrantDeckCard { idx: 0 });
    assert_eq!(rm.run_state.gold, gold + 30);
    assert_eq!(
        rm.run_state.master_deck.last().unwrap().card_id,
        CardId::Shroud
    );
    assert!(rm.run_state.master_deck.last().unwrap().upgraded);
    assert!(rm.run_state.relics.contains(&RelicId::Strawberry));
    assert_eq!(rm.phase(), Phase::MapChoice);
    assert!(rm.run_state.auto_reward_frames.is_empty());
}

#[test]
fn post_removal_additions_run_eggs_and_lucky_fysh_then_finish_empty_reward_phase() {
    fn event(rs: &mut RunState, _: usize) -> bool {
        rs.pending_deck_removals = 1;
        rs.pending_post_removal_cards = [CardId::Reave, CardId::Shroud]
            .into_iter()
            .map(|id| card_pod_from_id(id, false).unwrap())
            .collect();
        rs.exit_event_room();
        true
    }
    let mut rm = manager(&[]);
    let mut callbacks = sts2_content::run_callbacks::ironclad_content_callbacks();
    callbacks.resolve_event_at = Some(event);
    rm = rm.with_content_callbacks(callbacks);
    rm.run_state
        .relics
        .extend([RelicId::LuckyFysh, RelicId::MoltenEgg, RelicId::FrozenEgg]);
    let gold = rm.run_state.gold;
    act(&mut rm, RunAction::EventChoice { idx: 0 });
    assert_eq!(rm.phase(), Phase::CardReward);
    assert_eq!(rm.run_state.master_deck.len(), 2);
    assert_eq!(rm.run_state.gold, gold);
    act(&mut rm, RunAction::RemoveDeckCard { idx: 0 });
    assert_eq!(rm.run_state.master_deck.len(), 3);
    assert_eq!(rm.run_state.master_deck[1].card_id, CardId::Reave);
    assert_eq!(rm.run_state.master_deck[2].card_id, CardId::Shroud);
    assert!(rm.run_state.master_deck[1..]
        .iter()
        .all(|card| card.upgraded));
    assert_eq!(rm.run_state.gold, gold + 30);
    assert_eq!(rm.phase(), Phase::MapChoice);
}
