//! Actual PaelsWing.OnSacrifice source rarity and awaited grant routing.
use sts2_content::cards::spec_index::card_pod_from_id;
use sts2_core::cards::CardId;
use sts2_core::run_manager::{ActionResult, Phase, RunAction, RunManager};
use sts2_core::runstate::{CardReward, PendingCardSelect, Reward};
use sts2_core::{CharacterId, RelicId, RelicRarity, RunState};

fn offer(rs: &mut RunState, _: usize) -> bool {
    let mut reward = CardReward::from_options(vec![CardId::StrikeNecrobinder]);
    reward.prepared_cards = vec![card_pod_from_id(CardId::StrikeNecrobinder, false).unwrap()];
    reward.refresh_on_relic_obtained = true;
    rs.pending_rewards.push(Reward::Card(reward));
    rs.exit_event_room();
    true
}
fn choose_screen(rs: &mut RunState, _: usize) -> bool {
    rs.pending_card_selects
        .push(PendingCardSelect::with_post_add(
            vec![CardId::Reap],
            vec![card_pod_from_id(CardId::Injury, false).unwrap()],
        ));
    rs.exit_event_room();
    true
}
fn manager(seed: u64, event: fn(&mut RunState, usize) -> bool) -> RunManager {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    let mut callbacks = sts2_content::run_callbacks::ironclad_content_callbacks();
    callbacks.resolve_event_at = Some(event);
    let mut rm = RunManager::new(seed, CharacterId::Necrobinder)
        .with_content_callbacks(callbacks)
        .with_neow_start();
    rm.run_state.relics = vec![RelicId::PaelsWing];
    assert_eq!(
        rm.take_action(RunAction::EventChoice { idx: 0 }),
        ActionResult::Advanced
    );
    assert_eq!(rm.phase(), Phase::CardReward);
    rm
}

#[test]
fn beta_paels_wing_rolls_source_rarity_instead_of_forcing_common() {
    let mut exercised = false;
    for seed in 0_u64..64 {
        let mut rm = manager(seed, offer);
        let mut expected = rm.run_state.clone();
        if expected.roll_reward_relic_rarity() != RelicRarity::Rare {
            continue;
        }
        rm.run_state.pael_record_sacrifice();
        rm.run_state.relic_grab_bag.clear();
        rm.run_state.relic_grab_bag_fallback.clear();
        rm.run_state.relic_grab_bag_by_rarity.clear();
        rm.run_state.shared_relic_grab_bag_by_rarity.clear();
        rm.run_state
            .relic_grab_bag_by_rarity
            .insert(RelicRarity::Common, vec![RelicId::Anchor]);
        rm.run_state
            .relic_grab_bag_by_rarity
            .insert(RelicRarity::Rare, vec![RelicId::IceCream]);
        assert_eq!(
            rm.take_action(RunAction::SacrificeCardReward),
            ActionResult::Advanced
        );
        assert!(rm.run_state.relics.contains(&RelicId::IceCream));
        assert!(!rm.run_state.relics.contains(&RelicId::Anchor));
        assert_eq!(rm.run_state.rng.rewards, expected.rng.rewards);
        assert_eq!(rm.phase(), Phase::MapChoice);
        exercised = true;
        break;
    }
    assert!(exercised, "fixed panel must contain a rare rarity roll");
}

#[test]
fn beta_paels_wing_cannot_sacrifice_a_choose_screen_or_drop_its_curse() {
    let mut rm = manager(111, choose_screen);
    let before = sts2_core::hashing::run_hash(&rm);
    assert!(!rm
        .get_available_actions()
        .contains(&RunAction::SacrificeCardReward));
    assert_eq!(
        rm.take_action(RunAction::SacrificeCardReward),
        ActionResult::InvalidForPhase
    );
    assert_eq!(sts2_core::hashing::run_hash(&rm), before);
    rm.run_state.relics.push(RelicId::LuckyFysh);
    let gold = rm.run_state.gold;
    assert_eq!(
        rm.take_action(RunAction::PickCardReward { idx: None }),
        ActionResult::Advanced
    );
    assert_eq!(
        rm.run_state.master_deck.last().unwrap().card_id,
        CardId::Injury
    );
    assert_eq!(rm.run_state.gold, gold + 15);
}

#[test]
fn beta_paels_wing_awaits_orrery_grants_after_consuming_sacrificed_offer() {
    let mut rm = manager(111, offer);
    rm.run_state.pael_record_sacrifice();
    rm.run_state.relic_grab_bag.clear();
    rm.run_state.relic_grab_bag_by_rarity.clear();
    rm.run_state.shared_relic_grab_bag_by_rarity.clear();
    rm.run_state.relic_grab_bag_fallback = vec![RelicId::Orrery];
    assert_eq!(
        rm.take_action(RunAction::SacrificeCardReward),
        ActionResult::Advanced
    );
    assert_eq!(rm.phase(), Phase::CardReward);
    assert!(rm.run_state.relics.contains(&RelicId::Orrery));
    for _ in 0..5 {
        assert!(rm
            .get_available_actions()
            .contains(&RunAction::PickCardReward { idx: None }));
        assert_eq!(
            rm.take_action(RunAction::PickCardReward { idx: None }),
            ActionResult::Advanced
        );
    }
    assert_eq!(rm.phase(), Phase::MapChoice);
    assert!(rm.run_state.pending_rewards.is_empty());
    assert!(rm.run_state.auto_reward_frames.is_empty());
}
