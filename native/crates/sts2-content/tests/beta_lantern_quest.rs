//! Reachable solo Lantern Key / Repy lifecycle from v0.111.0 source.
use sts2_content::events::dispatch::{pick_event, present, resolve};
use sts2_content::run_callbacks::{
    initialize_run_setup, new_run_manager_for_character_with_ascension,
};
use sts2_core::cards::CardId;
use sts2_core::run_manager::{ActionResult, Phase, RunAction};
use sts2_core::runstate::{RelicRarity, RoomType, RunState};
use sts2_core::{CharacterId, EventId, PotionKind, RelicId};

fn keys(count: usize) -> Vec<sts2_core::Card> {
    (0..count)
        .map(|index| {
            let mut card =
                sts2_content::cards::spec_index::card_pod_from_id(CardId::LanternKey, false)
                    .unwrap();
            card.uid = sts2_core::CardUid::new(u32::try_from(index + 1).unwrap()).unwrap();
            card
        })
        .collect()
}

#[test]
fn beta_lantern_key_forces_glory_event_without_changing_odds_or_rng_budget() {
    sts2_content::relics::dispatch::ensure_installed();
    let mut rs = RunState {
        master_deck: keys(1),
        act_index: 3,
        ..RunState::default()
    };
    let odds = rs.unknown_map_point_odds.clone();
    let mut expected_rng = rs.rng.unknown_map_point.clone();
    expected_rng.next_float(1.0);
    assert_eq!(
        rs.resolve_unknown_room(std::collections::BTreeSet::default()),
        RoomType::Event
    );
    assert_eq!(rs.unknown_map_point_odds, odds);
    assert_eq!(rs.rng.unknown_map_point, expected_rng);
    rs.act_index = 2;
    assert!(!rs.has_glory_lantern_key());
    let rooms: std::collections::BTreeSet<_> =
        [RoomType::Monster, RoomType::Event].into_iter().collect();
    assert_eq!(rs.modify_unknown_map_point_room_types(rooms.clone()), rooms);
}

#[test]
fn beta_lantern_key_redirects_after_validating_pool_and_records_only_repy() {
    let mut rs = RunState {
        master_deck: keys(1),
        act_index: 3,
        ..RunState::default()
    };
    let rng = rs.rng.clone();
    assert_eq!(
        pick_event(&mut rs, Some(&["TrashHeap"])),
        Some(EventId::WarHistorianRepy)
    );
    assert!(rs.visited_event_ids.contains(&EventId::WarHistorianRepy));
    assert!(!rs.visited_event_ids.contains(&EventId::TrashHeap));
    assert_eq!(rs.rng, rng);
}

#[test]
fn beta_repy_consumes_one_key_first_then_all_remaining_for_other_reward() {
    sts2_content::relics::dispatch::ensure_installed();
    for count in 1..=3 {
        for cage_first in [true, false] {
            let mut rs = RunState {
                master_deck: keys(count),
                ..RunState::default()
            };
            rs.enter_event_room(EventId::WarHistorianRepy);
            let first = if cage_first {
                "unlock_cage"
            } else {
                "unlock_chest"
            };
            let second = if cage_first {
                "unlock_chest"
            } else {
                "unlock_cage"
            };
            let outcome = resolve(&mut rs, first).unwrap();
            assert_eq!(rs.master_deck.len(), count - 1);
            assert_eq!(
                rs.master_deck,
                keys(count)[1..],
                "only the first physical key is consumed"
            );
            assert_eq!(outcome.finished, count == 1);
            assert_eq!(rs.freed_repy, cage_first);
            assert_eq!(rs.relics.contains(&RelicId::HistoryCourse), cage_first);
            if count > 1 {
                let options = present(&rs).unwrap();
                assert_eq!(options.len(), 1);
                assert_eq!(options[0].option_id, second);
                assert!(
                    resolve(&mut rs, first).is_none(),
                    "cannot repeat first reward"
                );
                assert!(resolve(&mut rs, second).unwrap().finished);
                assert!(rs.master_deck.is_empty());
                assert!(rs.freed_repy);
                assert!(rs.relics.contains(&RelicId::HistoryCourse));
            }
            assert!(rs.current_event.is_none());
            if !cage_first || count > 1 {
                assert_eq!(rs.pending_rewards.len(), 4);
                assert!(matches!(
                    rs.pending_rewards[0],
                    sts2_core::Reward::Potion(_)
                ));
                assert!(matches!(
                    rs.pending_rewards[1],
                    sts2_core::Reward::Potion(_)
                ));
                assert!(matches!(
                    rs.pending_rewards[2],
                    sts2_core::Reward::UnpopulatedRelic { rarity: None }
                ));
                assert!(matches!(
                    rs.pending_rewards[3],
                    sts2_core::Reward::UnpopulatedRelic { rarity: None }
                ));
            }
        }
    }
}

#[test]
fn beta_repy_full_belt_and_parent_relics_finish_before_second_choice() {
    let mut rm = new_run_manager_for_character_with_ascension(111, CharacterId::Necrobinder, 0)
        .with_neow_start();
    initialize_run_setup(&mut rm);
    rm.run_state.master_deck = keys(3);
    rm.run_state.act_index = 3;
    rm.run_state.interactive_potion_swap = true;
    rm.run_state.potions =
        vec![PotionKind::FirePotion; usize::try_from(rm.run_state.max_potion_slots).unwrap()];
    rm.run_state.relic_grab_bag.clear();
    rm.run_state.relic_grab_bag_fallback.clear();
    rm.run_state.relic_grab_bag_by_rarity.clear();
    rm.run_state.shared_relic_grab_bag_by_rarity.clear();
    for rarity in [
        RelicRarity::Common,
        RelicRarity::Uncommon,
        RelicRarity::Rare,
    ] {
        rm.run_state
            .relic_grab_bag_by_rarity
            .insert(rarity, vec![RelicId::Anchor, RelicId::BagOfPreparation]);
        rm.run_state
            .shared_relic_grab_bag_by_rarity
            .insert(rarity, vec![RelicId::Anchor, RelicId::BagOfPreparation]);
    }
    rm.run_state.enter_event_room(EventId::WarHistorianRepy);
    let old_relics = rm.run_state.relics.len();
    assert_eq!(
        rm.take_action(RunAction::EventChoice { idx: 1 }),
        ActionResult::Advanced
    );
    assert_eq!(rm.phase(), Phase::CardReward);
    assert_eq!(rm.run_state.master_deck.len(), 2);
    assert_eq!(rm.run_state.relics.len(), old_relics);
    assert!(!rm
        .get_available_actions()
        .iter()
        .any(|a| matches!(a, RunAction::EventChoice { .. })));
    assert_eq!(
        rm.take_action(RunAction::PotionSwap { discard_slot: None }),
        ActionResult::Advanced
    );
    assert_eq!(rm.phase(), Phase::CardReward);
    assert_eq!(rm.run_state.relics.len(), old_relics);
    assert_eq!(
        rm.take_action(RunAction::PotionSwap { discard_slot: None }),
        ActionResult::Advanced
    );
    assert_eq!(rm.phase(), Phase::Event);
    assert_eq!(rm.run_state.relics.len(), old_relics + 2);
    assert_eq!(
        rm.take_action(RunAction::EventChoice { idx: 0 }),
        ActionResult::Advanced
    );
    assert_eq!(rm.phase(), Phase::MapChoice);
    assert!(rm.run_state.master_deck.is_empty());
    assert!(rm.run_state.freed_repy);
    assert!(rm.run_state.relics.contains(&RelicId::HistoryCourse));
}

#[test]
fn beta_repy_run_flag_survives_clone_and_distinguishes_future_hash() {
    let mut rm = sts2_core::run_manager::RunManager::new(111, CharacterId::Necrobinder);
    let before = sts2_core::hashing::run_hash(&rm);
    rm.run_state.freed_repy = true;
    assert_ne!(sts2_core::hashing::run_hash(&rm), before);
    assert!(rm.clone().run_state.freed_repy);
    assert_eq!(
        sts2_core::hashing::run_hash(&rm),
        sts2_core::hashing::run_hash(&rm.clone())
    );
}

#[test]
fn beta_removal_clamps_to_non_eternal_cards_and_preserves_survivors() {
    sts2_content::cards::dispatch::ensure_installed();
    let deck = [
        CardId::Greed,
        CardId::StrikeNecrobinder,
        CardId::Injury,
        CardId::AscendersBane,
        CardId::LanternKey,
    ]
    .into_iter()
    .map(|id| sts2_content::cards::spec_index::card_pod_from_id(id, false).unwrap())
    .collect::<Vec<_>>();
    let mut automatic = RunState {
        master_deck: deck.clone(),
        ..RunState::default()
    };
    assert_eq!(automatic.remove_cards_from_deck(1), 1);
    assert_eq!(
        automatic
            .master_deck
            .iter()
            .map(|c| c.card_id)
            .collect::<Vec<_>>(),
        vec![
            CardId::Greed,
            CardId::StrikeNecrobinder,
            CardId::AscendersBane,
            CardId::LanternKey
        ]
    );
    assert_eq!(automatic.remove_cards_from_deck(10), 2);
    assert_eq!(
        automatic
            .master_deck
            .iter()
            .map(|c| c.card_id)
            .collect::<Vec<_>>(),
        vec![CardId::Greed, CardId::AscendersBane]
    );
    let mut interactive = RunState {
        master_deck: deck.clone(),
        interactive_card_removal: true,
        ..RunState::default()
    };
    assert_eq!(interactive.remove_cards_from_deck(10), 0);
    assert_eq!(interactive.pending_deck_removals, 3);
    assert_eq!(interactive.master_deck, deck);
}
