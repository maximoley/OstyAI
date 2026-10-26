//! Event→combat bridge content wiring (bead `sts2-rs-ck9z6`, GH #166).
//!
//! Covers the content half of the bridge: the affected event handlers
//! stage the right [`EventCombatRequest`], `build_event_combat` maps each
//! encounter to the correct monster roster (with its spawn powers), and the
//! BattlewornDummy resume grants the per-setting reward only on a clean
//! victory. The engine-side phase routing is covered in
//! `sts2-core::run_manager` tests; the timer itself in
//! `sts2-core::combat_loop`.

use sts2_content::events::dispatch;
use sts2_content::run_bridge::build_event_combat;
use sts2_core::events::{BattlewornDummySetting, EventEncounter};
use sts2_core::monsters::MonsterKind;
use sts2_core::{EventId, PowerKind, RelicId, RunState};

fn enemy_has_power(combat: &sts2_core::CombatState, kind: PowerKind) -> bool {
    combat
        .creatures
        .iter()
        .filter(|c| c.species.is_some())
        .any(|c| {
            c.powers.iter().any(|p| {
                combat
                    .powers
                    .get_by_id(p.0)
                    .is_some_and(|pw| pw.kind == kind)
            })
        })
}

fn first_enemy_kind(combat: &sts2_core::CombatState) -> MonsterKind {
    combat
        .creatures
        .iter()
        .find_map(|c| c.species)
        .expect("an enemy was spawned")
}

#[test]
fn battleworn_settings_stage_resuming_dummy_fight() {
    for (idx, setting) in [
        BattlewornDummySetting::Setting1,
        BattlewornDummySetting::Setting2,
        BattlewornDummySetting::Setting3,
    ]
    .into_iter()
    .enumerate()
    {
        let mut rs = RunState::new();
        rs.enter_event_room(EventId::BattlewornDummy);
        let out = dispatch::resolve_by_index(&mut rs, idx).expect("event active");
        assert!(!out.finished, "fight defers event finish to post-combat");
        let req = rs.event_combat.as_ref().expect("combat staged");
        assert!(req.resume, "BattlewornDummy resumes after combat");
        assert_eq!(req.parent_event, EventId::BattlewornDummy);
        assert!(req.victory_rewards.is_empty(), "reward rolled at resume");
        assert_eq!(req.encounter, EventEncounter::BattlewornDummy(setting));
        // current_event must stay open across the fight.
        assert_eq!(rs.current_event, Some(EventId::BattlewornDummy));
    }
}

#[test]
fn build_event_combat_spawns_correct_dummy_with_timer() {
    for (setting, kind, hp) in [
        (
            BattlewornDummySetting::Setting1,
            MonsterKind::BattleFriendV1,
            75,
        ),
        (
            BattlewornDummySetting::Setting2,
            MonsterKind::BattleFriendV2,
            150,
        ),
        (
            BattlewornDummySetting::Setting3,
            MonsterKind::BattleFriendV3,
            300,
        ),
    ] {
        let mut rs = RunState::new();
        let req = sts2_core::events::EventCombatRequest::resuming(
            EventEncounter::BattlewornDummy(setting),
            EventId::BattlewornDummy,
        );
        let combat = build_event_combat(&mut rs, &req);
        assert_eq!(first_enemy_kind(&combat), kind);
        let enemy = combat
            .creatures
            .iter()
            .find(|c| c.species == Some(kind))
            .expect("dummy present");
        assert_eq!(enemy.max_hp, hp, "fixed HP per setting");
        assert!(
            enemy_has_power(&combat, PowerKind::BattlewornDummyTimeLimit),
            "dummy carries the 3-turn time limit"
        );
    }
}

#[test]
fn build_event_combat_advances_niche_exactly_like_a_real_hp_roll() {
    // C# rolls each event-combat enemy's HP off the shared `niche` stream
    // (CreateCreature → SetUniqueMonsterHpValue), even when Min == Max.
    // build_event_combat must consume `niche` identically so the cursor stays
    // aligned with C# for everything drawn afterward.
    use sts2_core::monsters::roll_unique_max_hps;
    let kind = MonsterKind::BattleFriendV3;

    // Reference: roll HP directly off a fresh state's niche, then sample.
    let mut ref_rs = RunState::new();
    let ref_hp = roll_unique_max_hps(&[kind], &mut ref_rs.rng.niche, ref_rs.ascension_level)[0];
    let ref_after = ref_rs.rng.niche.next_int(0, 1_000_000);

    // build_event_combat on an identical fresh state must match both.
    let mut rs = RunState::new();
    let req = sts2_core::events::EventCombatRequest::resuming(
        EventEncounter::BattlewornDummy(BattlewornDummySetting::Setting3),
        EventId::BattlewornDummy,
    );
    let combat = build_event_combat(&mut rs, &req);
    let built_hp = combat
        .creatures
        .iter()
        .find(|c| c.species == Some(kind))
        .expect("dummy")
        .max_hp;
    let built_after = rs.rng.niche.next_int(0, 1_000_000);

    assert_eq!(built_hp, ref_hp, "HP value matches the canonical roll");
    assert_eq!(
        built_after, ref_after,
        "niche cursor advanced by exactly the HP roll (C# parity)"
    );
}

#[test]
fn build_event_combat_mysterious_knight_carries_spawn_buffs() {
    let mut rs = RunState::new();
    let req = sts2_core::events::EventCombatRequest::no_resume(
        EventEncounter::MysteriousKnight,
        EventId::TheLanternKey,
        Vec::new(),
    );
    let combat = build_event_combat(&mut rs, &req);
    assert_eq!(first_enemy_kind(&combat), MonsterKind::MysteriousKnight);
    // MysteriousKnight.AfterAddedToRoom applies Strength(6) + Plating(6).
    assert!(enemy_has_power(&combat, PowerKind::Strength));
    assert!(enemy_has_power(&combat, PowerKind::Plating));
}

#[test]
fn lantern_key_fight_stages_card_reward_no_resume() {
    let mut rs = RunState::new();
    rs.enter_event_room(EventId::TheLanternKey);
    // Page 0: KeepTheKey opens the Fight page (multi-page).
    let _ = dispatch::resolve(&mut rs, "keep_key").expect("event active");
    assert!(rs.event_combat.is_none(), "page 0 stages no fight");
    // Page 1: Fight stages the real combat.
    let out = dispatch::resolve(&mut rs, "fight").expect("event active");
    assert!(!out.finished);
    let req = rs.event_combat.as_ref().expect("combat staged");
    assert!(!req.resume, "TheLanternKey does not resume");
    assert_eq!(req.encounter, EventEncounter::MysteriousKnight);
    assert_eq!(req.victory_rewards.len(), 1, "LanternKey card reward");
    assert!(matches!(
        req.victory_rewards[0],
        sts2_core::runstate::Reward::Card(_)
    ));
}

#[test]
fn fake_merchant_throw_foul_stages_full_inventory_and_consumes_potion() {
    use sts2_core::potions::PotionKind;
    use sts2_core::runstate::Reward;
    let mut rs = RunState::new();
    // The player holds the FoulPotion they throw (+ an unrelated potion to
    // prove only one FoulPotion is consumed).
    rs.potions = vec![PotionKind::FoulPotion, PotionKind::FoulPotion];
    rs.enter_event_room(EventId::FakeMerchant);
    let out = dispatch::resolve(&mut rs, "throw_foul").expect("event active");
    assert!(!out.finished, "throw_foul defers finish to post-combat");

    // Exactly one FoulPotion consumed (the thrown one).
    assert_eq!(
        rs.potions,
        vec![PotionKind::FoulPotion],
        "one FoulPotion thrown/consumed"
    );

    let req = rs.event_combat.as_ref().expect("combat staged");
    assert!(!req.resume);
    assert_eq!(req.encounter, EventEncounter::FakeMerchant);
    // FakeMerchantsRug + the 6 stocked fake-inventory relics (C# extra rewards).
    assert_eq!(
        req.victory_rewards.len(),
        7,
        "Rug + 6 stocked fake relics, got {:?}",
        req.victory_rewards
    );
    assert!(
        matches!(&req.victory_rewards[0], Reward::Relic(rr) if rr.relic_id == RelicId::FakeMerchantsRug),
        "first extra reward is FakeMerchantsRug"
    );
    // Every reward is a relic, the 6 stock entries are distinct fake relics.
    let mut stock_ids: Vec<RelicId> = req.victory_rewards[1..]
        .iter()
        .map(|r| match r {
            Reward::Relic(rr) => rr.relic_id,
            other => panic!("non-relic extra reward: {other:?}"),
        })
        .collect();
    let before_dedup = stock_ids.len();
    stock_ids.sort_unstable();
    stock_ids.dedup();
    assert_eq!(stock_ids.len(), before_dedup, "stocked relics are distinct");
}

#[test]
fn fake_merchant_buy_purchases_a_stocked_fake_relic_for_50() {
    // The 9 fake-inventory relics (C# FakeMerchant._inventoryRelics).
    const FAKE_INVENTORY: [RelicId; 9] = [
        RelicId::FakeAnchor,
        RelicId::FakeBloodVial,
        RelicId::FakeHappyFlower,
        RelicId::FakeLeesWaffle,
        RelicId::FakeMango,
        RelicId::FakeOrichalcum,
        RelicId::FakeSneckoEye,
        RelicId::FakeStrikeDummy,
        RelicId::FakeVenerableTeaSet,
    ];
    let mut rs = RunState::new();
    rs.gold = 100;
    rs.direct_event_relic_grant = true; // a merchant purchase is obtained inline
    rs.enter_event_room(EventId::FakeMerchant);
    let out = dispatch::resolve(&mut rs, "buy").expect("event active");
    assert!(out.finished);
    assert_eq!(rs.gold, 50, "paid the 50-gold relic cost");
    assert_eq!(rs.relics.len(), 1, "obtained exactly one relic");
    assert!(
        FAKE_INVENTORY.contains(&rs.relics[0]),
        "bought a stocked fake-inventory relic, got {:?}",
        rs.relics[0]
    );
}

#[test]
fn battleworn_resume_grants_only_on_clean_victory() {
    // Setting1: clean victory queues a potion reward; a timeout grants nothing.
    let mut win = RunState::new();
    sts2_content::events::shared::battleworn_dummy_resume(
        &mut win,
        BattlewornDummySetting::Setting1,
        true,
    );
    assert_eq!(win.pending_rewards.len(), 1, "potion queued on victory");

    let mut timeout = RunState::new();
    sts2_content::events::shared::battleworn_dummy_resume(
        &mut timeout,
        BattlewornDummySetting::Setting1,
        false,
    );
    assert!(
        timeout.pending_rewards.is_empty(),
        "RanOutOfTime grants no reward"
    );

    // Setting2: clean victory upgrades cards in place; timeout leaves the
    // deck untouched. (Setting3's relic grant needs a populated grab bag —
    // exercised end-to-end via the run loop, not in this bare-state unit.)
    let mut up_win = RunState::new();
    up_win
        .master_deck
        .push(sts2_core::cards::Card::pod_defaults());
    sts2_content::events::shared::battleworn_dummy_resume(
        &mut up_win,
        BattlewornDummySetting::Setting2,
        false,
    );
    assert!(
        !up_win.master_deck[0].upgraded,
        "no upgrade on RanOutOfTime"
    );
}

#[test]
fn punch_off_arrival_rolls_layout_once_and_fight_reuses_damaged_constructs() {
    let _ = sts2_content::run_callbacks::ironclad_content_callbacks();
    let mut rs = RunState::new();
    rs.total_floor = 8;
    rs.enter_event_room(EventId::PunchOff);
    let scratch = rs.event_state.scratch.clone();
    assert_eq!(scratch.len(), 4);
    assert!((2..10).contains(&scratch[2]));
    assert!((2..10).contains(&scratch[3]));
    let niche = rs.rng.niche.counter();
    assert!(!dispatch::resolve_by_index(&mut rs, 1).unwrap().finished);
    let offers = dispatch::present(&rs).unwrap();
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].option_id, "fight");
    assert!(!dispatch::resolve_by_index(&mut rs, 0).unwrap().finished);
    let req = rs.event_combat.clone().unwrap();
    let combat = build_event_combat(&mut rs, &req);
    assert_eq!(
        rs.rng.niche.counter(),
        niche,
        "no reroll when entering fight"
    );
    assert_eq!(combat.enemy_uids.len(), 2);
    for (i, uid) in combat.enemy_uids.iter().enumerate() {
        let creature = combat.creatures.iter().find(|c| c.id == uid.0).unwrap();
        assert_eq!(creature.max_hp, i32::from(scratch[i]));
        assert_eq!(creature.current_hp, i32::from(scratch[i] - scratch[i + 2]));
    }
}

#[test]
fn fake_merchant_reward_bounds_stay_three_hundred_at_poverty() {
    let mut rs = RunState::new();
    rs.ascension_level = 10;
    rs.event_combat = Some(sts2_core::events::EventCombatRequest::no_resume(
        EventEncounter::FakeMerchant,
        EventId::FakeMerchant,
        vec![],
    ));
    assert_eq!(
        rs.roll_encounter_gold(sts2_core::runstate::RoomType::Monster, 1.0),
        Some(300)
    );
}
