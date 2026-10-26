//! Behavioral regressions against v0.111.0 Models.Potions and PotionModel.
use sts2_content::cards::{dispatch, spec_index::card_pod_from_id};
use sts2_core::cards::{CardId, VarKey};
use sts2_core::{
    Action, CardUid, CharacterId, CombatPhase, CombatState, PotionKind, PowerKind, Side, StepResult,
};

fn board() -> CombatState {
    dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    let mut s = CombatState::default();
    s.player.character = Some(CharacterId::Necrobinder);
    s.player.current_hp = 50;
    s.player.max_hp = 100;
    s.player.energy = 20;
    s.player.max_energy = 20;
    s.init_player_creature();
    s.round = 1;
    s.phase = CombatPhase::PlayerPlayPhase;
    s.side = Side::Player;
    let enemy = s.spawn_enemy(sts2_core::MonsterKind::Nibbit);
    let c = s.creatures.iter_mut().find(|c| c.id == enemy.0).unwrap();
    c.current_hp = 500;
    c.max_hp = 500;
    s
}
fn mint(s: &mut CombatState, id: CardId) -> CardUid {
    let mut card = card_pod_from_id(id, false).unwrap();
    let uid = CardUid::new(u32::try_from(s.cards.len() + 1).unwrap()).unwrap();
    card.uid = uid;
    s.cards.push(card);
    uid
}
fn drink(s: &mut CombatState, potion: PotionKind) -> StepResult {
    s.player.potions.clear();
    s.player.potions.push(potion);
    sts2_core::apply(
        s,
        Action::DrinkPotion {
            potion_slot: 0,
            target: None,
        },
    )
}
#[test]
fn necrobinder_can_use_held_off_class_potions_with_real_effects() {
    let mut s = board();
    assert!(drink(&mut s, PotionKind::FocusPotion) != StepResult::InvalidTarget);
    assert_eq!(s.player_power_amount(PowerKind::Focus), 2);
    assert!(drink(&mut s, PotionKind::CunningPotion) != StepResult::InvalidTarget);
    assert_eq!(s.player.hand.len(), 3);
    assert!(s
        .player
        .hand
        .iter()
        .all(|u| s.cards[u.0.get() as usize - 1].card_id == CardId::Shiv
            && s.cards[u.0.get() as usize - 1].upgraded));
    let _ = drink(&mut s, PotionKind::BloodPotion);
    assert_eq!(s.player.current_hp, 70);
    let _ = drink(&mut s, PotionKind::PotionOfCapacity);
    assert_eq!(s.player.orbs.capacity(), 2);
    let _ = drink(&mut s, PotionKind::GhostInAJar);
    assert_eq!(s.player_power_amount(PowerKind::Intangible), 1);
}
#[test]
fn enemy_potions_enumerate_live_targets_and_invalid_targets_keep_the_potion() {
    let mut s = board();
    let dead = s.enemy_uids[0];
    s.creatures
        .iter_mut()
        .find(|c| c.id == dead.0)
        .unwrap()
        .current_hp = 0;
    let first = s.spawn_enemy(sts2_core::MonsterKind::Nibbit);
    let second = s.spawn_enemy(sts2_core::MonsterKind::Nibbit);
    for c in &mut s.creatures {
        if [first.0, second.0].contains(&c.id) {
            c.current_hp = 100;
            c.max_hp = 100;
        }
    }
    s.player.potions.push(PotionKind::FirePotion);
    let targets: Vec<_> = sts2_core::legal_actions(&s)
        .iter()
        .filter_map(|a| match a {
            Action::DrinkPotion { target, .. } => Some(*target),
            _ => None,
        })
        .collect();
    assert_eq!(targets, vec![Some(1), Some(2)]);
    for target in [0, 99] {
        assert_eq!(
            sts2_core::apply(
                &mut s,
                Action::DrinkPotion {
                    potion_slot: 0,
                    target: Some(target)
                }
            ),
            StepResult::InvalidTarget
        );
        assert_eq!(s.player.potions.as_slice(), &[PotionKind::FirePotion]);
    }
    let _ = sts2_core::apply(
        &mut s,
        Action::DrinkPotion {
            potion_slot: 0,
            target: Some(2),
        },
    );
    assert_eq!(
        s.creatures
            .iter()
            .find(|c| c.id == first.0)
            .unwrap()
            .current_hp,
        100
    );
    assert_eq!(
        s.creatures
            .iter()
            .find(|c| c.id == second.0)
            .unwrap()
            .current_hp,
        80
    );
}
#[test]
fn manual_potion_use_cannot_interrupt_enemy_or_pending_choice_phases() {
    let mut s = board();
    s.phase = CombatPhase::EnemyTurnStart;
    assert_eq!(
        drink(&mut s, PotionKind::EnergyPotion),
        StepResult::InvalidTarget
    );
    assert_eq!(s.player.potions.as_slice(), &[PotionKind::EnergyPotion]);
    s.phase = CombatPhase::PlayerPlayPhase;
    s.pending_choice = Some(sts2_core::PendingChoice::default());
    assert_eq!(
        drink(&mut s, PotionKind::EnergyPotion),
        StepResult::InvalidTarget
    );
    assert_eq!(s.player.energy, 20);
    assert!(s.pending_choice.is_some());
}
#[test]
fn clarity_waits_for_suspended_draw_before_applying_its_power() {
    let mut s = board();
    let a = mint(&mut s, CardId::StrikeNecrobinder);
    let b = mint(&mut s, CardId::DefendNecrobinder);
    s.player.discard.extend([a, b]);
    let p = s.player.creature_uid.unwrap();
    sts2_core::card_effects::apply_power_to_creature(&mut s, p, PowerKind::Stratagem, 1);
    let _ = drink(&mut s, PotionKind::Clarity);
    assert!(s.pending_choice.is_some());
    assert_eq!(s.player_power_amount(PowerKind::Clarity), 0);
    for _ in 0..10 {
        if s.pending_choice.is_none() {
            break;
        }
        let action = sts2_core::legal_actions(&s)
            .into_iter()
            .find(|a| matches!(a, Action::ResolveChoice { .. }))
            .unwrap();
        let _ = sts2_core::apply(&mut s, action);
    }
    assert!(s.pending_choice.is_none());
    assert_eq!(s.player.hand.len(), 2);
    assert_eq!(s.player_power_amount(PowerKind::Clarity), 3);
}
#[test]
fn star_potion_triggers_black_hole_instead_of_only_editing_the_counter() {
    let mut s = board();
    let p = s.player.creature_uid.unwrap();
    sts2_core::card_effects::apply_power_to_creature(&mut s, p, PowerKind::BlackHole, 5);
    let enemy = s.enemy_uids[0];
    let _ = drink(&mut s, PotionKind::StarPotion);
    assert_eq!(s.player.stars, 3);
    assert_eq!(
        s.creatures
            .iter()
            .find(|c| c.id == enemy.0)
            .unwrap()
            .current_hp,
        495
    );
}
#[test]
fn touch_of_insanity_uses_local_or_global_costs() {
    let mut s = board();
    let uid = mint(&mut s, CardId::StrikeNecrobinder);
    s.player.hand.push(uid);
    s.cards[0].set_combat_var(VarKey::FreeUntilPlayed, 1);
    let _ = drink(&mut s, PotionKind::TouchOfInsanity);
    assert_eq!(s.cards[0].combat_var(VarKey::StarCostFreeThisCombat), 0);
    let p = s.player.creature_uid.unwrap();
    sts2_core::card_effects::apply_power_to_creature(&mut s, p, PowerKind::Tangled, 1);
    let _ = drink(&mut s, PotionKind::TouchOfInsanity);
    assert_eq!(s.cards[0].combat_var(VarKey::StarCostFreeThisCombat), 1);
}
#[test]
fn snecko_absolute_roll_shadows_prior_discount_but_later_discounts_apply() {
    let mut s = board();
    let uid = mint(&mut s, CardId::RocketPunch);
    let original = s.cards[0].cost;
    s.player.hand.push(uid);
    s.cards[0].set_combat_var(VarKey::EnergyDiscountUntilPlayed, 2);
    let mut rng = s.run_rng.combat_energy_costs.clone();
    let rolled = rng.next_int(0, 3);
    let _ = drink(&mut s, PotionKind::SneckoOil);
    assert_eq!(sts2_core::hooks::effective_card_cost(&s, uid), rolled);
    s.cards[0].set_combat_var(VarKey::EnergyDiscountUntilPlayed, 3);
    assert_eq!(
        sts2_core::hooks::effective_card_cost(&s, uid),
        (rolled - 1).max(0)
    );
    s.restore_snecko_costs_for_turn_start();
    assert_eq!(s.cards[0].cost, original);
    assert_eq!(
        sts2_core::hooks::effective_card_cost(&s, uid),
        (i32::from(original) - 3).max(0)
    );
}
#[test]
fn autoplay_expires_snecko_and_preserves_earlier_absolute_cost_even_for_equal_roll() {
    let mut s = board();
    let uid = mint(&mut s, CardId::StrikeNecrobinder);
    s.player.hand.push(uid);
    s.cards[0].cost = 0;
    s.cards[0].set_combat_var(VarKey::CostAbsolutelySet, 1);
    s.run_rng.combat_energy_costs = (0_u64..100)
        .map(sts2_core::dotnet_rng::Rng::new)
        .find(|rng| {
            let mut copy = rng.clone();
            copy.next_int(0, 3) == 0
        })
        .unwrap();
    let _ = drink(&mut s, PotionKind::SneckoOil);
    assert_eq!(s.cards[0].combat_var(VarKey::SneckoCostDeltaThisTurn), 0);
    s.player.hand.clear();
    s.player.draw.push(uid);
    sts2_core::combat_loop::auto_play_n_from_draw_top(&mut s, 1, false);
    assert_eq!(s.cards[0].cost, 0);
    assert_eq!(
        s.cards[0].combat_var(VarKey::SneckoPriorAbsoluteThisTurn),
        0
    );
    assert_eq!(s.cards[0].combat_var(VarKey::CostAbsolutelySet), 1);
}

#[test]
fn all_obtainable_beta_potion_metadata_matches_source_catalog() {
    let catalog: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../docs/beta-migration/beta_potion_catalog.json"
    )))
    .unwrap();
    let mut checked = 0;
    for row in catalog["potions"].as_array().unwrap() {
        if row["scope"] != "solo" {
            continue;
        }
        let name = row["potion"].as_str().unwrap();
        let kind = PotionKind::ALL
            .into_iter()
            .find(|k| format!("{k:?}") == name)
            .unwrap();
        let spec = sts2_content::potions::all::spec_for(kind);
        assert_eq!(
            format!("{:?}", spec.rarity),
            row["rarity"].as_str().unwrap()
        );
        assert_eq!(
            format!("{:?}", spec.usage_type),
            row["usage"].as_str().unwrap()
        );
        let target = match spec.target_type {
            sts2_core::PotionTargetType::Self_ => "Self",
            sts2_core::PotionTargetType::AnyEnemy => "AnyEnemy",
            sts2_core::PotionTargetType::AllEnemies => "AllEnemies",
            sts2_core::PotionTargetType::AnyPlayer => "AnyPlayer",
        };
        assert_eq!(target, row["combat_target"].as_str().unwrap(), "{name}");
        assert_eq!(
            kind.is_event_only(),
            matches!(row["rarity"].as_str().unwrap(), "Event" | "Token"),
            "{name} source pool membership"
        );
        assert_eq!(
            kind.can_be_generated_in_combat(),
            row["can_be_generated_in_combat"].as_bool().unwrap()
        );
        checked += 1;
    }
    assert_eq!(checked, 64);
}

#[test]
fn capacity_potion_respects_the_ten_slot_source_cap() {
    let mut s = board();
    for _ in 0..8 {
        let _ = drink(&mut s, PotionKind::PotionOfCapacity);
    }
    assert_eq!(s.player.orbs.capacity(), 10);
}

#[test]
fn ambergris_advances_player_turn_and_clears_block_without_advancing_round() {
    let mut s = board();
    s.player.block = 5;
    let initial_round = s.round;
    let initial_player_turn = s.player_turn_number();
    let _ = drink(&mut s, PotionKind::Ambergris);
    let _ = sts2_core::apply(&mut s, Action::EndTurn);
    assert_eq!(s.round, initial_round);
    assert_eq!(s.player_turn_number(), initial_player_turn + 1);
    assert_eq!(s.player.block, 0);
}

#[test]
fn history_course_shifts_on_each_player_turn_including_extra_turns() {
    let mut s = board();
    s.round = 2;
    let previous = (CardId::StrikeNecrobinder, false, None);
    let current = (CardId::Reave, false, None);
    s.combat_log.prev_round_last_attack_skill_played = Some(previous);
    s.combat_log.last_attack_skill_played = Some(current);
    s.combat_log.extra_player_turns_taken = 1;
    s.phase = CombatPhase::PlayerTurnStart;
    let _ = sts2_core::combat_loop::start_player_turn(&mut s);
    assert_eq!(
        s.combat_log.prev_round_last_attack_skill_played,
        Some(current)
    );
    assert_eq!(s.combat_log.last_attack_skill_played, None);
    assert_eq!(s.round, 2);
    s.phase = CombatPhase::PlayerTurnStart;
    let _ = sts2_core::combat_loop::start_player_turn(&mut s);
    assert_eq!(s.combat_log.prev_round_last_attack_skill_played, None);
}

#[test]
fn extra_player_turns_skip_opening_draw_bonus_and_trigger_later_turn_relics() {
    let mut s = board();
    for (i, kind) in [
        sts2_core::RelicKind::BagOfPreparation,
        sts2_core::RelicKind::HornCleat,
        sts2_core::RelicKind::CaptainsWheel,
    ]
    .into_iter()
    .enumerate()
    {
        s.relics.push(sts2_core::Relic {
            id: std::num::NonZeroU32::new(i as u32 + 1).unwrap(),
            kind,
            counter: 0,
            is_wax: false,
            is_melted: false,
        });
    }
    for _ in 0..8 {
        let u = mint(&mut s, CardId::StrikeNecrobinder);
        s.player.draw.push(u);
    }
    s.combat_log.extra_player_turns_taken = 1;
    s.phase = CombatPhase::PlayerTurnStart;
    let _ = sts2_core::combat_loop::start_player_turn(&mut s);
    assert_eq!(
        s.player.hand.len(),
        5,
        "Bag of Preparation must not repeat on the same-round extra turn"
    );
    assert_eq!(s.player.block, 14, "Horn Cleat uses player turn 2");
    s.player.hand.clear();
    s.combat_log.extra_player_turns_taken = 2;
    s.phase = CombatPhase::PlayerTurnStart;
    let _ = sts2_core::combat_loop::start_player_turn(&mut s);
    assert_eq!(s.player.block, 18, "Captain's Wheel uses player turn 3");
    assert_eq!(s.round, 1);
}

#[test]
fn touch_after_snecko_supersedes_the_expiring_absolute_roll() {
    let mut s = board();
    let uid = mint(&mut s, CardId::StrikeNecrobinder);
    s.player.hand.push(uid);
    let _ = drink(&mut s, PotionKind::SneckoOil);
    let player = s.player.creature_uid.unwrap();
    sts2_core::card_effects::apply_power_to_creature(&mut s, player, PowerKind::Tangled, 1);
    let _ = drink(&mut s, PotionKind::TouchOfInsanity);
    assert_eq!(s.cards[0].cost, 0);
    s.clear_card_play_cost_modifiers(uid);
    s.restore_snecko_costs_for_turn_start();
    assert_eq!(s.cards[0].cost, 0);
    assert_eq!(s.cards[0].combat_var(VarKey::CostAbsolutelySet), 1);
}

#[test]
fn stacked_potion_cost_cleanup_preserves_all_twenty_accumulated_slots() {
    let mut s = board();
    let uid = mint(&mut s, CardId::Thrash);
    s.player.hand.push(uid);
    for key in [
        VarKey::FreeThisTurn,
        VarKey::EtherealThisCombat,
        VarKey::IsDupe,
        VarKey::GeneratedInCombat,
        VarKey::PendingBatchAutoPlay,
        VarKey::DampenDowngradedMarker,
        VarKey::Generic,
        VarKey::MasterDeckIndex,
        VarKey::WireTransientCostThisTurn,
        VarKey::WireCostExpiringOnPlay,
        VarKey::WireCostStable,
        VarKey::ThrashFoldRemNum,
        VarKey::ThrashFoldRemDen,
        VarKey::FreeUntilPlayed,
    ] {
        s.cards[0].set_combat_var(key, 1);
    }
    s.clear_card_play_cost_modifiers(uid);
    for _ in 0..3 {
        let _ = drink(&mut s, PotionKind::SneckoOil);
        s.clear_card_play_cost_modifiers(uid);
    }
    let player = s.player.creature_uid.unwrap();
    sts2_core::card_effects::apply_power_to_creature(&mut s, player, PowerKind::Tangled, 1);
    let _ = drink(&mut s, PotionKind::TouchOfInsanity);
    assert_eq!(s.cards[0].combat_vars.len(), 20);
    s.clear_card_play_cost_modifiers(uid);
    s.restore_snecko_costs_for_turn_start();
    assert_eq!(s.cards[0].cost, 0);
}

#[test]
fn binding_consumes_each_targets_artifact_on_weak_before_vulnerable() {
    let mut s = board();
    let second = s.spawn_enemy(sts2_core::MonsterKind::Nibbit);
    let first = s.enemy_uids[0];
    for target in [first, second] {
        sts2_core::card_effects::apply_power_to_creature(&mut s, target, PowerKind::Artifact, 1);
    }
    let _ = drink(&mut s, PotionKind::PotionOfBinding);
    for target in [first, second] {
        let amount = |kind| {
            s.creatures
                .iter()
                .find(|c| c.id == target.0)
                .unwrap()
                .powers
                .iter()
                .filter_map(|u| s.powers.iter().find(|p| p.id == u.0 && p.kind == kind))
                .map(|p| p.amount)
                .sum::<i32>()
        };
        assert_eq!(amount(PowerKind::Artifact), 0);
        assert_eq!(amount(PowerKind::Weak), 0);
        assert_eq!(amount(PowerKind::Vulnerable), 1);
    }
}

#[test]
fn temporary_potion_stat_loss_observes_artifact_at_turn_end() {
    for (potion, stat) in [
        (PotionKind::SpeedPotion, PowerKind::Dexterity),
        (PotionKind::FlexPotion, PowerKind::Strength),
    ] {
        let mut s = board();
        let player = s.player.creature_uid.unwrap();
        sts2_core::card_effects::apply_power_to_creature(&mut s, player, PowerKind::Artifact, 1);
        let _ = drink(&mut s, potion);
        assert_eq!(s.player_power_amount(stat), 5);
        assert_eq!(s.player_power_amount(PowerKind::Artifact), 1);
        let _ = sts2_core::apply(&mut s, Action::EndTurn);
        assert_eq!(s.player_power_amount(stat), 5);
        assert_eq!(s.player_power_amount(PowerKind::Artifact), 0);
    }
}

#[test]
fn event_potion_lock_rejects_reward_swap_discard_but_allows_declining_reward() {
    use sts2_core::run_manager::{ActionResult, RunAction, RunManager};
    let mut manager = RunManager::new(0_u64, CharacterId::Necrobinder);
    manager.run_state.potion_use_locked_by_event =
        Some(sts2_core::events::EventId::TheFutureOfPotions);
    manager.run_state.potions.push(PotionKind::FruitJuice);
    manager
        .run_state
        .pending_potion_swaps
        .push(PotionKind::BlockPotion);
    assert_eq!(
        manager.get_available_actions(),
        vec![RunAction::PotionSwap { discard_slot: None }]
    );
    assert_eq!(
        manager.take_action(RunAction::PotionSwap {
            discard_slot: Some(0)
        }),
        ActionResult::InvalidForPhase
    );
    assert_eq!(manager.run_state.potions, vec![PotionKind::FruitJuice]);
    assert_eq!(
        manager.take_action(RunAction::PotionSwap { discard_slot: None }),
        ActionResult::Advanced
    );
    assert!(manager.run_state.pending_potion_swaps.is_empty());
}

#[test]
fn signed_stat_hook_applications_consume_artifact_before_changing_positive_or_negative_stats() {
    for kind in [PowerKind::Strength, PowerKind::Dexterity, PowerKind::Focus] {
        for player_target in [true, false] {
            let mut s = board();
            let uid = if player_target {
                s.player.creature_uid.unwrap()
            } else {
                s.enemy_uids[0]
            };
            let target = if player_target {
                sts2_core::hooks::CreatureRef::Player
            } else {
                sts2_core::hooks::CreatureRef::Enemy(uid)
            };
            sts2_core::card_effects::apply_power_to_creature(&mut s, uid, kind, 5);
            sts2_core::card_effects::apply_power_to_creature(&mut s, uid, PowerKind::Artifact, 1);
            sts2_core::hooks::apply_effect(
                &mut s,
                sts2_core::hooks::Effect::ApplyPower {
                    target,
                    kind,
                    amount: -3,
                    applier: None,
                },
            );
            let amount = |s: &CombatState, stat| {
                s.creatures
                    .iter()
                    .find(|c| c.id == uid.0)
                    .unwrap()
                    .powers
                    .iter()
                    .filter_map(|u| s.powers.iter().find(|p| p.id == u.0 && p.kind == stat))
                    .map(|p| p.amount)
                    .sum::<i32>()
            };
            assert_eq!(amount(&s, kind), 5);
            assert_eq!(amount(&s, PowerKind::Artifact), 0);
            sts2_core::hooks::apply_effect(
                &mut s,
                sts2_core::hooks::Effect::ApplyPower {
                    target,
                    kind,
                    amount: -8,
                    applier: None,
                },
            );
            assert_eq!(amount(&s, kind), -3);
        }
    }
}
