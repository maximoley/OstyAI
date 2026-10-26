//! Beta mechanics from sts2-wiki/decompiled/v0.111.0/Models.Cards and Models.Powers.
use sts2_content::cards::{dispatch, spec_index::card_pod_from_id};
use sts2_core::card_effects::{self, EffectResult};
use sts2_core::cards::CardId;
use sts2_core::{Action, CardUid, CombatPhase, CombatState, PowerKind, Side, StepResult};

fn board() -> CombatState {
    dispatch::ensure_installed();
    let mut state = CombatState::default();
    state.player.current_hp = 50;
    state.player.max_hp = 80;
    state.player.energy = 20;
    state.player.max_energy = 20;
    let _ = state.init_player_creature();
    state.phase = CombatPhase::PlayerPlayPhase;
    state.side = Side::Player;
    let enemy = state.spawn_enemy(sts2_core::MonsterKind::Nibbit);
    let c = state
        .creatures
        .iter_mut()
        .find(|c| c.id == enemy.0)
        .unwrap();
    c.current_hp = 500;
    c.max_hp = 500;
    state
}
fn mint(state: &mut CombatState, id: CardId, upgraded: bool) -> CardUid {
    let mut card = card_pod_from_id(id, upgraded).unwrap();
    let uid = CardUid::new(u32::try_from(state.cards.len() + 1).unwrap()).unwrap();
    card.uid = uid;
    state.cards.push(card);
    uid
}
fn play(
    state: &mut CombatState,
    id: CardId,
    upgraded: bool,
    target: Option<sts2_core::CreatureUid>,
) {
    let uid = mint(state, id, upgraded);
    state.player.play.push(uid);
    assert_eq!(
        card_effects::dispatch(state, id, target),
        EffectResult::Resolved
    );
    state.player.play.retain(|u| *u != uid);
}
#[test]
fn beta_necrobinder_soul_storm_scaling_and_sacrifice_triple_max_hp() {
    let mut state = board();
    let enemy = state.enemy_uids[0];
    for _ in 0..3 {
        let uid = mint(&mut state, CardId::Soul, false);
        state.player.exhaust.push(uid);
    }
    play(&mut state, CardId::SoulStorm, false, Some(enemy));
    assert_eq!(
        state
            .creatures
            .iter()
            .find(|c| c.id == enemy.0)
            .unwrap()
            .current_hp,
        479
    );
    play(&mut state, CardId::SoulStorm, true, Some(enemy));
    assert_eq!(
        state
            .creatures
            .iter()
            .find(|c| c.id == enemy.0)
            .unwrap()
            .current_hp,
        452
    );
    let pet = state.summon_osty(7).unwrap();
    play(&mut state, CardId::Sacrifice, false, None);
    assert_eq!(state.player.block, 21);
    assert!(
        state
            .creatures
            .iter()
            .find(|c| c.id == pet.0)
            .unwrap()
            .current_hp
            <= 0
    );
    let resummoned = state.summon_osty(3).unwrap();
    assert_eq!(
        state
            .creatures
            .iter()
            .find(|c| c.id == resummoned.0)
            .unwrap()
            .max_hp,
        3
    );
}
#[test]
fn beta_eidolon_plays_exhaust_snapshot_and_preserves_hand() {
    let mut state = board();
    let eidolon = mint(&mut state, CardId::Eidolon, false);
    let hand = mint(&mut state, CardId::StrikeNecrobinder, false);
    state.player.hand.extend([eidolon, hand]);
    let ethereal = mint(&mut state, CardId::Fear, false);
    let ordinary = mint(&mut state, CardId::DefendNecrobinder, false);
    let unplayable = mint(&mut state, CardId::Dazed, false);
    state
        .player
        .exhaust
        .extend([ethereal, ordinary, unplayable]);
    let result = sts2_core::apply(
        &mut state,
        Action::PlayCard {
            hand_index: 0,
            target: None,
        },
    );
    assert_eq!(result, StepResult::Advanced);
    assert_eq!(state.player.hand.as_slice(), &[hand]);
    assert!(state.player.exhaust.contains(&eidolon));
    assert!(!state.player.exhaust.contains(&ethereal));
    assert!(state.player.exhaust.contains(&ordinary));
    assert!(state.player.exhaust.contains(&unplayable));
    assert!(state
        .powers
        .iter()
        .any(|p| p.kind == PowerKind::Vulnerable && p.amount > 0));
}
#[test]
fn beta_misery_copies_temporary_strength_once_and_artifact_blocks_it() {
    let mut state = board();
    let first = state.enemy_uids[0];
    let second = state.spawn_enemy(sts2_core::MonsterKind::Nibbit);
    play(&mut state, CardId::EnfeeblingTouch, false, Some(first));
    play(&mut state, CardId::Misery, false, Some(first));
    let amount = |s: &CombatState, target, kind| {
        s.creatures
            .iter()
            .find(|c| c.id == target)
            .unwrap()
            .powers
            .iter()
            .filter_map(|u| s.powers.get_by_id(u.0))
            .filter(|p| p.kind == kind)
            .map(|p| p.amount)
            .sum::<i32>()
    };
    assert_eq!(amount(&state, second.0, PowerKind::Strength), -8);
    assert_eq!(amount(&state, second.0, PowerKind::EnfeeblingTouch), 8);
    let third = state.spawn_enemy(sts2_core::MonsterKind::Nibbit);
    card_effects::apply_power_to_enemy(&mut state, third, PowerKind::Artifact, 1);
    play(&mut state, CardId::Misery, false, Some(first));
    assert_eq!(amount(&state, third.0, PowerKind::Strength), 0);
    assert_eq!(amount(&state, third.0, PowerKind::EnfeeblingTouch), 0);
}
#[test]
fn beta_eidolon_waits_for_nested_choice_before_next_snapshot_card() {
    let mut state = board();
    let eidolon = mint(&mut state, CardId::Eidolon, false);
    let hand = mint(&mut state, CardId::StrikeNecrobinder, false);
    state.player.hand.extend([eidolon, hand]);
    // Run-obtainable Ethereal enchantment on Dredge exercises its real select.
    let dredge = mint(&mut state, CardId::Dredge, false);
    card_effects::apply_ethereal_keyword(&mut state.cards[dredge.0.get() as usize - 1]);
    let fear = mint(&mut state, CardId::Fear, false);
    state.player.exhaust.extend([dredge, fear]);
    for _ in 0..4 {
        let uid = mint(&mut state, CardId::DefendNecrobinder, false);
        state.player.discard.push(uid);
    }
    assert_eq!(
        sts2_core::apply(
            &mut state,
            Action::PlayCard {
                hand_index: 0,
                target: None
            }
        ),
        StepResult::PendingChoice
    );
    assert!(
        state.player.exhaust.contains(&fear),
        "later snapshot card awaits Dredge"
    );
    let choice = Action::ResolveChoice {
        selection: smallvec::smallvec![0, 1, 2],
    };
    assert_eq!(sts2_core::apply(&mut state, choice), StepResult::Advanced);
    assert!(
        !state.player.exhaust.contains(&fear),
        "later card plays after selection: exhaust={:?}, discard={:?}, resume={:?}, pending={:?}",
        state.player.exhaust,
        state.player.discard,
        state.pending_autoplay_resume,
        state.pending_choice
    );
    assert!(state.pending_autoplay_resume.is_none());
    assert!(state.pending_choice.is_none());
    assert!(state.player.exhaust.contains(&eidolon));
}
#[test]
fn beta_regen_resolves_before_doom_on_both_sides() {
    use sts2_core::hooks::{dispatch_after_turn_end, dispatch_before_turn_end, TurnSideCtx};
    let mut state = board();
    let player = state.player.creature_uid.unwrap();
    card_effects::apply_power_to_creature(&mut state, player, PowerKind::Doom, 50);
    card_effects::apply_power_to_creature(&mut state, player, PowerKind::Regen, 5);
    dispatch_before_turn_end(&mut state, &TurnSideCtx { side: Side::Player });
    assert_eq!(state.player.current_hp, 55);
    dispatch_after_turn_end(&mut state, &TurnSideCtx { side: Side::Player });
    assert_eq!(state.player.current_hp, 55, "healing avoids player Doom");
    let enemy = state.enemy_uids[0];
    state
        .creatures
        .iter_mut()
        .find(|c| c.id == enemy.0)
        .unwrap()
        .current_hp = 50;
    card_effects::apply_power_to_enemy(&mut state, enemy, PowerKind::Doom, 50);
    card_effects::apply_power_to_enemy(&mut state, enemy, PowerKind::Regen, 5);
    dispatch_before_turn_end(&mut state, &TurnSideCtx { side: Side::Enemy });
    assert_eq!(
        state
            .creatures
            .iter()
            .find(|c| c.id == enemy.0)
            .unwrap()
            .current_hp,
        55
    );
}

// Constructor metadata and simple Damage/Block variables for every solo card.
// Formula and power magnitudes are checked in behavioral tests.
#[test]
fn beta_necrobinder_entire_solo_catalog_metadata_and_upgrades() {
    use sts2_content::cards::{spec_index::spec_for_card_id, CardKeywords};
    dispatch::ensure_installed();
    #[rustfmt::skip]
    let rows: &[(CardId, i8, u16, Option<(i32, i32)>, Option<(i32, i32)>)] = &[
        (CardId::Afterlife, 1, 8, None, None),
        (CardId::BansheesCry, 9, 0, Some((33, 33)), None),
        (CardId::BlightStrike, 1, 0, Some((8, 10)), None),
        (CardId::Bodyguard, 1, 0, None, None),
        (CardId::BoneShards, 1, 0, None, None),
        (CardId::BorrowedTime, 1, 0, None, None),
        (CardId::Bury, 4, 0, Some((52, 63)), None),
        (CardId::CalcifyCard, 1, 0, None, None),
        (CardId::CallOfTheVoid, 1, 0, None, None),
        (CardId::CaptureSpirit, 1, 0, Some((3, 4)), None),
        (CardId::Cleanse, 1, 0, None, None),
        (CardId::CountdownCard, 1, 0, None, None),
        (CardId::DanseMacabre, 1, 0, None, None),
        (CardId::DeathMarch, 1, 0, None, None),
        (CardId::Deathbringer, 2, 0, None, None),
        (CardId::DeathsDoor, 1, 0, None, Some((6, 7))),
        (CardId::DebilitateCard, 1, 0, Some((10, 12)), None),
        (CardId::DefendNecrobinder, 1, 0, None, Some((5, 8))),
        (CardId::Defile, 1, 2, Some((13, 17)), None),
        (CardId::Defy, 1, 2, None, Some((6, 9))),
        (CardId::Delay, 2, 0, None, Some((11, 13))),
        (CardId::Demesne, 3, 2, None, None),
        (CardId::DevourLifeCard, 1, 0, None, None),
        (CardId::Dirge, -1, 8, None, None),
        (CardId::DrainPower, 1, 0, Some((10, 12)), None),
        (CardId::Dredge, 1, 8, None, None),
        (CardId::Eidolon, 2, 8, None, None),
        (CardId::EndOfDays, 3, 0, None, None),
        (CardId::EnfeeblingTouch, 1, 2, None, None),
        (CardId::Eradicate, -1, 4, Some((11, 14)), None),
        (CardId::Fear, 1, 2, Some((7, 8)), None),
        (CardId::Fetch, 0, 0, None, None),
        (CardId::Flatten, 2, 0, None, None),
        (CardId::ForbiddenGrimoire, 2, 0, None, None),
        (CardId::Friendship, 1, 0, None, None),
        (CardId::GraveWarden, 1, 0, None, Some((8, 11))),
        (CardId::Graveblast, 1, 8, Some((4, 6)), None),
        (CardId::Hang, 1, 0, Some((10, 13)), None),
        (CardId::Haunt, 1, 0, None, None),
        (CardId::HighFive, 2, 0, None, None),
        (CardId::Invoke, 1, 0, None, None),
        (CardId::LethalityCard, 1, 2, None, None),
        (CardId::Melancholy, 3, 0, None, Some((13, 17))),
        (CardId::Misery, 0, 0, Some((7, 9)), None),
        (CardId::NecroMasteryCard, 2, 0, None, None),
        (CardId::NegativePulse, 1, 0, None, Some((5, 6))),
        (CardId::Neurosurge, 0, 0, None, None),
        (CardId::NoEscape, 1, 0, None, None),
        (CardId::Oblivion, 0, 0, None, None),
        (CardId::Pagestorm, 1, 0, None, None),
        (CardId::Parse, 1, 2, None, None),
        (CardId::Poke, 0, 0, None, None),
        (CardId::Protector, 1, 0, None, None),
        (CardId::PullAggro, 2, 0, None, Some((7, 9))),
        (CardId::PullFromBelow, 1, 0, Some((5, 7)), None),
        (CardId::Putrefy, 1, 8, None, None),
        (CardId::Rattle, 1, 0, None, None),
        (CardId::Reanimate, 3, 8, None, None),
        (CardId::Reap, 3, 4, Some((27, 33)), None),
        (CardId::ReaperForm, 3, 0, None, None),
        (CardId::Reave, 1, 0, Some((10, 13)), None),
        (CardId::RightHandHand, 0, 0, None, None),
        (CardId::Sacrifice, 1, 4, None, None),
        (CardId::Scourge, 1, 0, None, None),
        (CardId::SculptingStrike, 1, 0, Some((9, 12)), None),
        (CardId::Seance, 1, 2, None, None),
        (CardId::SentryMode, 2, 0, None, None),
        (CardId::Severance, 2, 0, Some((13, 18)), None),
        (CardId::SharedFate, 0, 8, None, None),
        (CardId::Shroud, 1, 0, None, Some((3, 4))),
        (CardId::SicEm, 1, 0, None, None),
        (CardId::SleightOfFlesh, 2, 0, None, None),
        (CardId::Snap, 1, 0, None, None),
        (CardId::SoulStorm, 1, 0, None, None),
        (CardId::Sow, 1, 4, Some((8, 11)), None),
        (CardId::SpiritOfAsh, 1, 0, None, None),
        (CardId::Spur, 1, 4, None, None),
        (CardId::Squeeze, 3, 0, None, None),
        (CardId::StrikeNecrobinder, 1, 0, Some((6, 9)), None),
        (CardId::TheScythe, 2, 8, Some((13, 13)), None),
        (CardId::TimesUp, 2, 0, None, None),
        (CardId::Transfigure, 1, 8, None, None),
        (CardId::Undeath, 0, 0, None, Some((7, 9))),
        (CardId::Unleash, 1, 0, None, None),
        (CardId::Veilpiercer, 1, 0, Some((10, 13)), None),
        (CardId::Wisp, 0, 8, None, None),
    ];
    for &(id, cost, flags, damage, block) in rows {
        let spec = spec_for_card_id(id).expect("solo catalog spec");
        assert_eq!(spec.cost, cost, "{id:?} cost");
        for flag in [
            CardKeywords::EXHAUST,
            CardKeywords::ETHEREAL,
            CardKeywords::RETAIN,
        ] {
            assert_eq!(
                spec.keywords.contains(flag),
                flags & flag.0 != 0,
                "{id:?} keyword {flag:?}"
            );
        }
        for upgraded in [false, true] {
            let card = card_pod_from_id(id, upgraded).unwrap();
            if let Some((base, upgrade)) = damage {
                assert_eq!(
                    card.base_damage,
                    if upgraded { upgrade } else { base },
                    "{id:?} damage upgraded={upgraded}"
                );
            }
            if let Some((base, upgrade)) = block {
                assert_eq!(
                    card.base_block,
                    if upgraded { upgrade } else { base },
                    "{id:?} block upgraded={upgraded}"
                );
            }
        }
    }
}

#[test]
fn beta_forbidden_grimoire_reward_survives_battleworn_dummy_timeout() {
    use sts2_core::events::{BattlewornDummySetting, EventCombatRequest, EventEncounter};
    use sts2_core::runstate::Reward;
    let mut run = sts2_core::RunState::new();
    run.current_hp = 50;
    run.max_hp = 50;
    let request = EventCombatRequest::resuming(
        EventEncounter::BattlewornDummy(BattlewornDummySetting::Setting1),
        sts2_core::EventId::BattlewornDummy,
    );
    let mut combat = sts2_content::run_bridge::build_event_combat(&mut run, &request);
    let player = combat.player.creature_uid.unwrap();
    card_effects::apply_power_to_creature(&mut combat, player, PowerKind::ForbiddenGrimoire, 1);
    combat.event_combat_timed_out = true;
    combat.player_won = true;
    combat.is_over = true;
    sts2_content::run_bridge::exit_combat(&mut run, &combat);
    assert!(run
        .pending_rewards
        .contains(&Reward::RemoveCard { count: 1 }));
}

#[test]
fn beta_haunt_shroud_reave_and_times_up_use_upgraded_values() {
    for (upgraded, haunt, shroud, reave) in [(false, 7, 3, 10), (true, 9, 4, 13)] {
        let mut state = board();
        let enemy = state.enemy_uids[0];
        play(&mut state, CardId::Haunt, upgraded, None);
        assert_eq!(state.player_power_amount(PowerKind::Haunt), haunt);
        play(&mut state, CardId::Shroud, upgraded, None);
        card_effects::apply_power_to_enemy(&mut state, enemy, PowerKind::Doom, 15);
        assert_eq!(state.player.block, shroud, "Shroud on Doom");
        play(&mut state, CardId::Reave, upgraded, Some(enemy));
        assert_eq!(
            state
                .creatures
                .iter()
                .find(|c| c.id == enemy.0)
                .unwrap()
                .current_hp,
            500 - reave
        );
        let soul = state
            .player
            .draw
            .iter()
            .map(|u| &state.cards[u.0.get() as usize - 1])
            .find(|c| c.card_id == CardId::Soul)
            .unwrap();
        assert_eq!(soul.upgraded, upgraded);
        let times_up = mint(&mut state, CardId::TimesUp, upgraded);
        state.player.hand.push(times_up);
        assert_eq!(
            sts2_core::apply(
                &mut state,
                Action::PlayCard {
                    hand_index: 0,
                    target: Some(0)
                }
            ),
            StepResult::Advanced
        );
        assert!(
            state.player.discard.contains(&times_up),
            "Times Up no longer exhausts"
        );
    }
}

#[test]
fn beta_necrobinder_all_solo_base_and_upgraded_handlers_resolve() {
    #[rustfmt::skip]
    let ids = [
        CardId::Afterlife, CardId::BansheesCry, CardId::BlightStrike, CardId::Bodyguard,
        CardId::BoneShards, CardId::BorrowedTime, CardId::Bury, CardId::CalcifyCard,
        CardId::CallOfTheVoid, CardId::CaptureSpirit, CardId::Cleanse, CardId::CountdownCard,
        CardId::DanseMacabre, CardId::DeathMarch, CardId::Deathbringer, CardId::DeathsDoor,
        CardId::DebilitateCard, CardId::DefendNecrobinder, CardId::Defile, CardId::Defy,
        CardId::Delay, CardId::Demesne, CardId::DevourLifeCard, CardId::Dirge,
        CardId::DrainPower, CardId::Dredge, CardId::Eidolon, CardId::EndOfDays,
        CardId::EnfeeblingTouch, CardId::Eradicate, CardId::Fear, CardId::Fetch,
        CardId::Flatten, CardId::ForbiddenGrimoire, CardId::Friendship, CardId::GraveWarden,
        CardId::Graveblast, CardId::Hang, CardId::Haunt, CardId::HighFive,
        CardId::Invoke, CardId::LethalityCard, CardId::Melancholy, CardId::Misery,
        CardId::NecroMasteryCard, CardId::NegativePulse, CardId::Neurosurge, CardId::NoEscape,
        CardId::Oblivion, CardId::Pagestorm, CardId::Parse, CardId::Poke,
        CardId::Protector, CardId::PullAggro, CardId::PullFromBelow, CardId::Putrefy,
        CardId::Rattle, CardId::Reanimate, CardId::Reap, CardId::ReaperForm,
        CardId::Reave, CardId::RightHandHand, CardId::Sacrifice, CardId::Scourge,
        CardId::SculptingStrike, CardId::Seance, CardId::SentryMode, CardId::Severance,
        CardId::SharedFate, CardId::Shroud, CardId::SicEm, CardId::SleightOfFlesh,
        CardId::Snap, CardId::SoulStorm, CardId::Sow, CardId::SpiritOfAsh,
        CardId::Spur, CardId::Squeeze, CardId::StrikeNecrobinder, CardId::TheScythe,
        CardId::TimesUp, CardId::Transfigure, CardId::Undeath, CardId::Unleash,
        CardId::Veilpiercer, CardId::Wisp,
    ];
    for id in ids {
        for upgraded in [false, true] {
            let mut state = board();
            state.player.energy = 2;
            state.summon_osty(10);
            for filler in [
                CardId::Soul,
                CardId::DefendNecrobinder,
                CardId::StrikeNecrobinder,
            ] {
                let uid = mint(&mut state, filler, false);
                state.player.draw.push(uid);
                let uid = mint(&mut state, filler, false);
                state.player.discard.push(uid);
                let uid = mint(&mut state, filler, false);
                state.player.hand.push(uid);
            }
            let card = mint(&mut state, id, upgraded);
            state.cards[card.0.get() as usize - 1].energy_spent = 2;
            state.player.play.push(card);
            let enemy = state.enemy_uids[0];
            assert_eq!(
                card_effects::dispatch(&mut state, id, Some(enemy)),
                EffectResult::Resolved,
                "{id:?} upgraded={upgraded}"
            );
        }
    }
}

#[test]
fn beta_lightning_kills_last_enemy_before_lethal_player_doom() {
    let mut state = board();
    let player = state.player.creature_uid.unwrap();
    card_effects::apply_power_to_creature(&mut state, player, PowerKind::Doom, 50);
    state
        .creatures
        .iter_mut()
        .find(|c| c.id == state.enemy_uids[0].0)
        .unwrap()
        .current_hp = 3;
    state.player.orbs = sts2_core::orbs::OrbSlots::with_default_capacity();
    state.player.orbs.channel(sts2_core::orbs::Orb::Lightning);
    assert_eq!(
        sts2_core::apply(&mut state, Action::EndTurn),
        StepResult::CombatWon
    );
    assert!(state.player.current_hp > 0);
    assert!(state.player_won);
}
#[test]
fn beta_last_enemy_doom_death_finishes_combat_without_stale_creature_access() {
    let mut state = board();
    let enemy = state.enemy_uids[0];
    card_effects::apply_power_to_enemy(&mut state, enemy, PowerKind::Doom, 500);
    assert_eq!(
        sts2_core::apply(&mut state, Action::EndTurn),
        StepResult::CombatWon
    );
    assert!(state.player_won);
}
#[test]
fn beta_quest_cards_are_not_offered_for_deck_transformation() {
    let mut rm = sts2_core::run_manager::RunManager::new(0, sts2_core::CharacterId::Necrobinder);
    rm.run_state.master_deck = [
        CardId::Dowsing,
        CardId::StrikeNecrobinder,
        CardId::ByrdonisEgg,
    ]
    .into_iter()
    .map(|id| card_pod_from_id(id, false).unwrap())
    .collect();
    assert_eq!(rm.transform_eligible_deck_indices(), vec![1]);
    for index in [0, 2] {
        assert!(!rm.run_state.master_deck[index].is_upgradable());
        assert!(!rm.run_state.master_deck[index].is_x_cost());
    }
}

#[test]
fn beta_regen_saves_player_from_doom_in_real_end_turn_pipeline() {
    for regen in [false, true] {
        let mut state = board();
        let player = state.player.creature_uid.unwrap();
        card_effects::apply_power_to_creature(&mut state, player, PowerKind::Doom, 50);
        if regen {
            card_effects::apply_power_to_creature(&mut state, player, PowerKind::Regen, 5);
        }
        let result = sts2_core::apply(&mut state, Action::EndTurn);
        if regen {
            assert_ne!(result, StepResult::CombatLost);
            assert!(state.player.current_hp > 0);
        } else {
            assert_eq!(result, StepResult::CombatLost);
        }
    }
}
