//! Source-backed regressions for v0.111.0 Models.Cards and Models.Powers.
use sts2_content::cards::{dispatch, spec_index::card_pod_from_id};
use sts2_core::card_effects::{self, EffectResult};
use sts2_core::cards::{CardId, VarKey};
use sts2_core::{
    Action, CardUid, CombatPhase, CombatState, CreatureUid, PowerKind, Side, StepResult,
};
fn board() -> CombatState {
    dispatch::ensure_installed();
    let mut state = CombatState::default();
    state.player.current_hp = 80;
    state.player.max_hp = 80;
    state.player.energy = 20;
    state.player.max_energy = 20;
    state.init_player_creature();
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
fn mint(state: &mut CombatState, id: CardId, up: bool) -> CardUid {
    let mut card = card_pod_from_id(id, up).unwrap();
    let uid = CardUid::new(u32::try_from(state.cards.len() + 1).unwrap()).unwrap();
    card.uid = uid;
    state.cards.push(card);
    uid
}
fn play(state: &mut CombatState, id: CardId, up: bool) {
    let uid = mint(state, id, up);
    state.player.play.push(uid);
    let enemy = state.enemy_uids[0];
    assert_eq!(
        card_effects::dispatch(state, id, Some(enemy)),
        EffectResult::Resolved
    );
    state.player.play.retain(|u| *u != uid);
}
fn power(state: &CombatState, enemy: CreatureUid, kind: PowerKind) -> i32 {
    state
        .creatures
        .iter()
        .find(|c| c.id == enemy.0)
        .unwrap()
        .powers
        .iter()
        .filter_map(|u| state.powers.get_by_id(u.0))
        .filter(|p| p.kind == kind)
        .map(|p| p.amount)
        .sum()
}
fn hp(state: &CombatState, enemy: CreatureUid) -> i32 {
    state
        .creatures
        .iter()
        .find(|c| c.id == enemy.0)
        .unwrap()
        .current_hp
}
#[test]
fn expect_a_fight_uses_positive_strength_instead_of_energy() {
    for (up, strength, expected) in [(false, -3, 15), (false, 2, 25), (true, 2, 32)] {
        let mut s = board();
        let player = s.player.creature_uid.unwrap();
        card_effects::apply_signed_attr_power(&mut s, player, PowerKind::Strength, strength);
        play(&mut s, CardId::ExpectAFight, up);
        assert_eq!(s.player.block, expected);
        assert_eq!(s.player.energy, 20);
        assert_eq!(s.player_power_amount(PowerKind::NoEnergyGain), 0);
    }
}
#[test]
fn outbreak_triggers_poison_and_haze_applies_both_debuffs() {
    let mut s = board();
    let enemy = s.enemy_uids[0];
    play(&mut s, CardId::OutbreakCard, false);
    assert_eq!(hp(&s, enemy), 491);
    assert_eq!(power(&s, enemy, PowerKind::Poison), 8);
    let other = s.spawn_enemy(sts2_core::MonsterKind::Nibbit);
    card_effects::apply_power_to_enemy(&mut s, other, PowerKind::Artifact, 1);
    play(&mut s, CardId::Haze, true);
    assert_eq!(power(&s, other, PowerKind::Poison), 0);
    assert_eq!(power(&s, other, PowerKind::Weak), 2);
}
#[test]
fn expertise_retains_exactly_drawn_cards_through_suspended_shuffle() {
    let mut s = board();
    let old = mint(&mut s, CardId::StrikeSilent, false);
    s.player.hand.push(old);
    let drawn1 = mint(&mut s, CardId::DefendSilent, false);
    let drawn2 = mint(&mut s, CardId::StrikeSilent, false);
    s.player.discard.extend([drawn1, drawn2]);
    let p = s.player.creature_uid.unwrap();
    card_effects::apply_power_to_creature(&mut s, p, PowerKind::Stratagem, 1);
    let expertise = mint(&mut s, CardId::Expertise, false);
    s.player.hand.insert(0, expertise);
    let _ = sts2_core::apply(
        &mut s,
        Action::PlayCard {
            hand_index: 0,
            target: None,
        },
    );
    for _ in 0..20 {
        if s.pending_choice.is_none() {
            break;
        }
        let choice = sts2_core::legal_actions(&s)
            .into_iter()
            .find(|a| matches!(a, Action::ResolveChoice { .. }))
            .unwrap();
        let _ = sts2_core::apply(&mut s, choice);
    }
    assert!(s.pending_choice.is_none());
    assert!(!s.player.retain_this_turn.contains(&old));
    assert_eq!(
        s.player.retain_this_turn.len(),
        1,
        "Stratagem adds one card directly; only the other is drawn"
    );
    assert!(s
        .player
        .retain_this_turn
        .iter()
        .all(|uid| [drawn1, drawn2].contains(uid)));
    assert!(s.pending_draw_resume.is_none());
}
#[test]
fn scrape_keeps_global_free_skill_and_discards_drawn_paid_attack() {
    let mut s = board();
    let p = s.player.creature_uid.unwrap();
    card_effects::apply_power_to_creature(&mut s, p, PowerKind::FreeSkill, 1);
    let skill = mint(&mut s, CardId::DefendSilent, false);
    let attack = mint(&mut s, CardId::StrikeSilent, false);
    s.player.draw.extend([skill, attack]);
    play(&mut s, CardId::Scrape, false);
    assert!(s.player.hand.contains(&skill));
    assert!(s.player.discard.contains(&attack));
}
#[test]
fn hyperbeam_focus_loss_expires_and_artifact_negates_it() {
    for artifact in [false, true] {
        let mut s = board();
        let p = s.player.creature_uid.unwrap();
        if artifact {
            card_effects::apply_power_to_creature(&mut s, p, PowerKind::Artifact, 1);
        }
        play(&mut s, CardId::Hyperbeam, false);
        assert_eq!(
            s.player_power_amount(PowerKind::Focus),
            if artifact { 0 } else { -3 }
        );
        sts2_core::hooks::dispatch_after_turn_end(
            &mut s,
            &sts2_core::hooks::TurnSideCtx { side: Side::Player },
        );
        assert_eq!(s.player_power_amount(PowerKind::Focus), 0);
        assert_eq!(s.player_power_amount(PowerKind::HyperbeamFocusDown), 0);
    }
}
#[test]
fn rocket_punch_discount_accumulates_until_played() {
    let mut s = board();
    let rocket = mint(&mut s, CardId::RocketPunch, false);
    s.player.hand.push(rocket);
    let status = card_pod_from_id(CardId::Dazed, false).unwrap();
    card_effects::spawn_card_into_hand_with_origin(&mut s, status, true);
    assert_eq!(sts2_core::hooks::effective_card_cost(&s, rocket), 1);
    card_effects::spawn_card_into_hand_with_origin(&mut s, status, true);
    assert_eq!(sts2_core::hooks::effective_card_cost(&s, rocket), 0);
    assert_eq!(
        sts2_core::apply(
            &mut s,
            Action::PlayCard {
                hand_index: 0,
                target: Some(0)
            }
        ),
        StepResult::Advanced
    );
    assert_eq!(sts2_core::hooks::effective_card_cost(&s, rocket), 2);
    assert_eq!(
        s.cards[rocket.0.get() as usize - 1].combat_var(VarKey::EnergyDiscountUntilPlayed),
        0
    );
}
#[test]
fn abundance_offers_upgraded_powers_and_makes_only_selected_card_free() {
    let mut s = board();
    s.player.character = Some(sts2_core::potions::CharacterId::Necrobinder);
    play(&mut s, CardId::Abundance, false);
    let pc = s.pending_choice.as_ref().unwrap();
    assert_eq!(pc.options.len(), 3);
    assert!(pc.options.iter().all(|o| o.upgraded));
    for o in &pc.options {
        assert_eq!(
            card_pod_from_id(o.card_id, true).unwrap().card_type,
            sts2_core::cards::CardType::Power
        );
    }
    let _ = sts2_core::apply(
        &mut s,
        Action::ResolveChoice {
            selection: smallvec::smallvec![1],
        },
    );
    assert_eq!(s.player.hand.len(), 1);
    let c = &s.cards[s.player.hand[0].0.get() as usize - 1];
    assert!(c.upgraded);
    assert_eq!(c.combat_var(VarKey::FreeThisTurn), 1);
}
#[test]
fn dowsing_counts_unknown_map_points_once_and_transforms_after_five() {
    use sts2_core::map::{MapCoord, MapPointType};
    use sts2_core::runstate::{Room, RoomType};
    dispatch::ensure_installed();
    let mut run = sts2_core::RunState::new();
    run.master_deck
        .push(card_pod_from_id(CardId::Dowsing, false).unwrap());
    for row in 0..5 {
        let coord = MapCoord::new(0, row);
        run.enter_room(Room::at(coord, MapPointType::Unknown, RoomType::Monster));
        run.enter_room(Room::at(coord, MapPointType::Unknown, RoomType::Event));
    }
    assert_eq!(run.master_deck[0].card_id, CardId::Abundance);
}
#[test]
fn juggling_copies_original_third_attack_before_nested_autoplay() {
    let mut s = board();
    let p = s.player.creature_uid.unwrap();
    card_effects::apply_power_to_creature(&mut s, p, PowerKind::Juggling, 1);
    for pow in &mut s.powers {
        if pow.kind == PowerKind::Juggling {
            pow.counter = 2;
        }
    }
    let child = mint(&mut s, CardId::StrikeDefect, false);
    s.player.draw.push(child);
    let uproar = mint(&mut s, CardId::Uproar, false);
    s.player.hand.push(uproar);
    let _ = sts2_core::apply(
        &mut s,
        Action::PlayCard {
            hand_index: 0,
            target: Some(0),
        },
    );
    assert!(s
        .player
        .hand
        .iter()
        .any(|u| s.cards[u.0.get() as usize - 1].card_id == CardId::Uproar));
    assert!(!s
        .player
        .hand
        .iter()
        .any(|u| s.cards[u.0.get() as usize - 1].card_id == CardId::StrikeDefect));
}
#[test]
fn replay_does_not_resolve_again_after_last_enemy_dies() {
    let mut s = board();
    let enemy = s.enemy_uids[0];
    s.creatures
        .iter_mut()
        .find(|c| c.id == enemy.0)
        .unwrap()
        .current_hp = 3;
    let strike = mint(&mut s, CardId::StrikeSilent, false);
    s.cards[strike.0.get() as usize - 1].base_replay_count = 3;
    s.player.hand.push(strike);
    let _ = sts2_core::apply(
        &mut s,
        Action::PlayCard {
            hand_index: 0,
            target: Some(0),
        },
    );
    assert_eq!(s.combat_log.card_plays_finished_this_turn, 1);
}
#[test]
fn mad_science_violence_receives_vigor_on_all_three_hits() {
    use sts2_content::cards::colorless::helpers::{set_mad_science_variant, TinkerTimeRider};
    let mut s = board();
    let p = s.player.creature_uid.unwrap();
    card_effects::apply_power_to_creature(&mut s, p, PowerKind::Vigor, 5);
    let card = mint(&mut s, CardId::MadScience, false);
    set_mad_science_variant(
        &mut s.cards[card.0.get() as usize - 1],
        sts2_core::cards::CardType::Attack,
        TinkerTimeRider::Violence,
    );
    s.player.hand.push(card);
    let enemy = s.enemy_uids[0];
    let _ = sts2_core::apply(
        &mut s,
        Action::PlayCard {
            hand_index: 0,
            target: Some(0),
        },
    );
    assert_eq!(hp(&s, enemy), 449);
}

#[test]
fn full_beta_solo_card_constructor_metadata_matches_source_catalog() {
    use sts2_content::cards::{
        spec_index::{card_csharp_entry, spec_for_card_id},
        CardKeywords,
    };
    let catalog: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../docs/beta-migration/beta_card_catalog.json"
    ))
    .unwrap();
    let catalog = catalog.as_object().unwrap();
    let mut mismatches = Vec::new();
    let mut checked = 0;
    for &id in CardId::all() {
        let Some(spec) = spec_for_card_id(id) else {
            continue;
        };
        let entry = if id == CardId::DeprecatedCard {
            "DeprecatedCard".to_owned()
        } else {
            card_csharp_entry(id)
        };
        let Some((name, source)) = catalog
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&entry))
        else {
            continue;
        };
        if source["multiplayer_constraint"] == "MultiplayerOnly" {
            continue;
        }
        checked += 1;
        let cost = if source["x_cost"] == true {
            -1
        } else {
            source["energy_cost"]
                .as_i64()
                .unwrap_or(i64::from(name != "DeprecatedCard"))
        };
        if i64::from(spec.cost) != cost {
            mismatches.push(format!("{name} cost {} vs {cost}", spec.cost));
        }
        if let Some(stars) = source["star_cost"].as_i64() {
            let card = card_pod_from_id(id, false).unwrap();
            let actual = sts2_core::effective_star_cost(&card);
            if i64::from(actual) != stars.max(0) {
                mismatches.push(format!("{name} star cost {actual} vs {stars}"));
            }
        }
        if let Some(vars) = source["vars"].as_array() {
            for var in vars {
                let field = var["type"].as_str().unwrap_or("");
                // These source variables are interpreted in handlers rather
                // than as printed POD attack/block fields.
                if matches!(
                    id,
                    CardId::Burn
                        | CardId::Decay
                        | CardId::Infection
                        | CardId::Toxic
                        | CardId::MadScience
                        | CardId::SecondWind
                        | CardId::BoneShards
                ) {
                    continue;
                }
                if !["Damage", "Block"].contains(&field) {
                    continue;
                }
                let Some(base) = var["base_value"].as_i64() else {
                    continue;
                };
                for up in [false, true] {
                    let card = card_pod_from_id(id, up).unwrap();
                    let expected = if up {
                        var["upgraded_value"].as_i64().unwrap_or(base)
                    } else {
                        base
                    };
                    let actual = if field == "Damage" {
                        card.base_damage
                    } else {
                        card.base_block
                    };
                    if i64::from(actual) != expected {
                        mismatches.push(format!("{name} {field} up={up} {actual} vs {expected}"));
                    }
                }
            }
        }
        let typ = format!("{:?}", spec.card_type);
        if source["type"]
            .as_str()
            .is_some_and(|expected| expected != typ)
        {
            mismatches.push(format!("{name} type {typ} vs {}", source["type"]));
        }
        let rarity = format!("{:?}", spec.rarity);
        if source["rarity"]
            .as_str()
            .is_some_and(|expected| expected != rarity)
        {
            mismatches.push(format!("{name} rarity {rarity} vs {}", source["rarity"]));
        }
        if let Some(target) = source["target"].as_str() {
            let expected = match target {
                "AnyEnemy" => "Enemy",
                "Self" | "AnyAlly" | "AllAllies" => "Self_",
                "RandomEnemy" => "Random",
                other => other,
            };
            let actual = format!("{:?}", spec.target_type);
            if actual != expected {
                mismatches.push(format!("{name} target {actual} vs {expected}"));
            }
        }
        for (label, flag) in [
            ("Exhaust", CardKeywords::EXHAUST),
            ("Ethereal", CardKeywords::ETHEREAL),
            ("Retain", CardKeywords::RETAIN),
            ("Innate", CardKeywords::INNATE),
        ] {
            let expected = source["keywords"]
                .as_array()
                .is_some_and(|v| v.iter().any(|v| v == label));
            if spec.keywords.contains(flag) != expected {
                mismatches.push(format!(
                    "{name} keyword {label}={} vs {expected}",
                    spec.keywords.contains(flag)
                ));
            }
        }
    }
    assert!(checked > 500, "full catalog checked {checked}");
    assert!(
        mismatches.is_empty(),
        "{} constructor differences:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn deprecated_card_removes_only_its_own_master_instance_and_not_generated_copy() {
    dispatch::ensure_installed();
    let mut run = sts2_core::RunState::new();
    run.current_hp = 80;
    run.max_hp = 80;
    let mut a = card_pod_from_id(CardId::DeprecatedCard, false).unwrap();
    a.base_block = 11;
    let mut b = a;
    b.base_block = 22;
    run.master_deck.extend([a, b]);
    let roster = [(
        sts2_core::MonsterKind::Nibbit,
        sts2_content::monster_dispatch::MonsterSpawnArgs::None,
        500,
    )];
    let mut s = sts2_content::run_bridge::enter_combat_from_roster(
        &run,
        &roster,
        sts2_core::runstate::RoomType::Monster,
    );
    let original = s
        .cards
        .iter()
        .find(|c| c.combat_var(VarKey::MasterDeckIndex) == 2)
        .unwrap()
        .uid;
    let copied = *s
        .cards
        .iter()
        .find(|c| c.combat_var(VarKey::MasterDeckIndex) == 1)
        .unwrap();
    let generated = card_effects::spawn_card_into_hand_with_origin(&mut s, copied, true);
    for uid in [generated, original] {
        s.player.play.push(uid);
        let _ = card_effects::dispatch_for_card(&mut s, uid, CardId::DeprecatedCard, None);
        s.player.play.retain(|u| *u != uid);
    }
    sts2_content::run_bridge::exit_combat(&mut run, &s);
    assert_eq!(run.master_deck.len(), 1);
    assert_eq!(run.master_deck[0].base_block, 11);
}
#[test]
fn well_laid_plans_retains_entire_hand_without_a_selector() {
    let mut s = board();
    play(&mut s, CardId::WellLaidPlans, false);
    play(&mut s, CardId::WellLaidPlans, true);
    assert_eq!(s.player_power_amount(PowerKind::WellLaidPlans), 1);
    let one = mint(&mut s, CardId::StrikeSilent, false);
    let two = mint(&mut s, CardId::DefendSilent, false);
    s.player.hand.extend([one, two]);
    let _ = sts2_core::combat_loop::end_player_turn_phase_one(&mut s);
    let _ = sts2_core::combat_loop::end_player_turn_phase_two(&mut s);
    assert!(s.pending_choice.is_none());
    assert!(s.player.hand.contains(&one));
    assert!(s.player.hand.contains(&two));
}
#[test]
fn expertise_obeys_no_draw_and_does_not_retain_existing_hand() {
    let mut s = board();
    let old = mint(&mut s, CardId::StrikeSilent, false);
    s.player.hand.push(old);
    let draw = mint(&mut s, CardId::DefendSilent, false);
    s.player.draw.push(draw);
    let p = s.player.creature_uid.unwrap();
    card_effects::apply_power_to_creature(&mut s, p, PowerKind::NoDraw, 1);
    play(&mut s, CardId::Expertise, true);
    assert!(s.player.retain_this_turn.is_empty());
    assert_eq!(s.player.draw.as_slice(), &[draw]);
}

#[test]
fn tracking_increases_card_attack_damage_by_fifty_percent() {
    let mut s = board();
    let p = s.player.creature_uid.unwrap();
    let enemy = s.enemy_uids[0];
    card_effects::apply_power_to_creature(&mut s, p, PowerKind::Tracking, 50);
    card_effects::apply_power_to_enemy(&mut s, enemy, PowerKind::Weak, 1);
    let strike = mint(&mut s, CardId::StrikeSilent, false);
    s.player.hand.push(strike);
    let _ = sts2_core::apply(
        &mut s,
        Action::PlayCard {
            hand_index: 0,
            target: Some(0),
        },
    );
    assert_eq!(hp(&s, enemy), 491);
}

#[test]
fn sidestep_grants_next_turn_energy_at_both_upgrade_levels() {
    for (up, amount) in [(false, 1), (true, 2)] {
        let mut s = board();
        play(&mut s, CardId::Sidestep, up);
        assert_eq!(s.player_power_amount(PowerKind::EnergyNextTurn), amount);
        assert_eq!(s.player.energy, 20);
    }
}
#[test]
fn entropy_cannot_transform_a_curse_into_ascenders_bane() {
    let mut s = board();
    s.player.character = Some(sts2_core::CharacterId::Necrobinder);
    let curse = mint(&mut s, CardId::Regret, false);
    s.player.hand.push(curse);
    assert!(card_effects::transform_card_to_random(&mut s, curse).is_none());
    assert!(s.player.hand.contains(&curse));
}

#[test]
fn maul_grows_every_combat_copy_and_upgrade_preserves_growth() {
    let mut s = board();
    let first = mint(&mut s, CardId::Maul, false);
    let second = mint(&mut s, CardId::Maul, true);
    s.player.discard.extend([first, second]);
    play(&mut s, CardId::Maul, true);
    assert_eq!(s.cards[first.0.get() as usize - 1].base_damage, 8);
    assert_eq!(s.cards[second.0.get() as usize - 1].base_damage, 9);
    s.cards[first.0.get() as usize - 1].upgrade();
    assert_eq!(s.cards[first.0.get() as usize - 1].base_damage, 9);
}
#[test]
fn supermassive_counts_mad_science_chaos_generation() {
    use sts2_content::cards::colorless::helpers::{set_mad_science_variant, TinkerTimeRider};
    let mut s = board();
    s.player.character = Some(sts2_core::CharacterId::Regent);
    let card = mint(&mut s, CardId::MadScience, false);
    set_mad_science_variant(
        &mut s.cards[card.0.get() as usize - 1],
        sts2_core::cards::CardType::Skill,
        TinkerTimeRider::Chaos,
    );
    s.player.hand.push(card);
    let _ = sts2_core::apply(
        &mut s,
        Action::PlayCard {
            hand_index: 0,
            target: None,
        },
    );
    assert_eq!(s.count_player_generated_cards_this_combat(), 1);
    let enemy = s.enemy_uids[0];
    let old = hp(&s, enemy);
    play(&mut s, CardId::Supermassive, false);
    assert_eq!(old - hp(&s, enemy), 8);
}

#[test]
fn ordinary_generation_does_not_allocate_deprecated_identity_slots() {
    let mut s = board();
    let card = card_pod_from_id(CardId::Thrash, false).unwrap();
    let uid = card_effects::spawn_card_into_hand_with_origin(&mut s, card, true);
    let card = &s.cards[uid.0.get() as usize - 1];
    assert!(!card.combat_vars.iter().any(|(key, _)| matches!(
        key,
        VarKey::MasterDeckIndex | VarKey::RemoveMasterDeckInstance
    )));
}
