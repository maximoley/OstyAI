//! BETA-121: completed effects and the opening-hand AutoPrePlay boundary.
use std::num::NonZeroU32;
use sts2_content::cards::spec_index::card_pod_from_id;
use sts2_core::cards::CardId;
use sts2_core::{
    Action, CardUid, CombatPhase, CombatState, PowerKind, Relic, RelicKind, StepResult,
};

fn board() -> CombatState {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    let mut state = CombatState {
        phase: CombatPhase::PlayerPlayPhase,
        ..CombatState::default()
    };
    state.player.current_hp = 80;
    state.player.max_hp = 80;
    state.init_player_creature();
    let enemy = state.spawn_enemy(sts2_core::MonsterKind::Nibbit);
    let creature = state
        .creatures
        .iter_mut()
        .find(|c| c.id == enemy.0)
        .unwrap();
    creature.current_hp = 1000;
    creature.max_hp = 1000;
    state.relics.push(Relic {
        id: NonZeroU32::new(1).unwrap(),
        kind: RelicKind::UnceasingTop,
        counter: 0,
        is_wax: false,
        is_melted: false,
    });
    state
}

fn mint(state: &mut CombatState, id: CardId) -> CardUid {
    let mut card = card_pod_from_id(id, false).unwrap();
    card.uid = CardUid::new(u32::try_from(state.cards.len() + 1).unwrap()).unwrap();
    let uid = card.uid;
    state.cards.push(card);
    uid
}

#[test]
fn beta_top_draws_after_hellraiser_autoplays_entire_opening_hand() {
    let mut state = board();
    let owner = state.player.creature_uid.unwrap();
    sts2_core::card_effects::apply_power_to_creature(&mut state, owner, PowerKind::Hellraiser, 1);
    let strikes: Vec<_> = (0..5)
        .map(|_| mint(&mut state, CardId::StrikeIronclad))
        .collect();
    let defend = mint(&mut state, CardId::DefendIronclad);
    state.player.draw.extend(strikes.iter().copied());
    state.player.draw.push(defend);
    state.phase = CombatPhase::PlayerTurnStart;
    assert_eq!(
        sts2_core::combat_loop::start_player_turn(&mut state),
        StepResult::Advanced
    );
    assert_eq!(state.phase, CombatPhase::PlayerPlayPhase);
    assert_eq!(state.player.hand.as_slice(), &[defend]);
    assert_eq!(state.player.discard.as_slice(), strikes.as_slice());
    assert_eq!(state.combat_log.player_card_plays_this_combat, 5);
}

#[test]
fn beta_top_draws_after_last_manual_card_completes() {
    let mut state = board();
    let strike = mint(&mut state, CardId::StrikeIronclad);
    let defend = mint(&mut state, CardId::DefendIronclad);
    state.player.hand.push(strike);
    state.player.draw.push(defend);
    state.player.energy = 3;
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
    assert_eq!(state.player.hand.as_slice(), &[defend]);
    assert_eq!(state.player.discard.as_slice(), &[strike]);
}

#[test]
fn beta_top_waits_for_whole_distilled_chaos_effect_before_drawing() {
    let mut state = board();
    let strikes: Vec<_> = (0..3)
        .map(|_| mint(&mut state, CardId::StrikeIronclad))
        .collect();
    let defend = mint(&mut state, CardId::DefendIronclad);
    state.player.draw.extend(strikes.iter().copied());
    state.player.draw.push(defend);
    state
        .player
        .potions
        .push(sts2_core::PotionKind::DistilledChaos);
    assert_eq!(
        sts2_core::apply(
            &mut state,
            Action::DrinkPotion {
                potion_slot: 0,
                target: None
            }
        ),
        StepResult::Advanced
    );
    assert_eq!(state.player.hand.as_slice(), &[defend]);
    assert_eq!(state.player.discard.as_slice(), strikes.as_slice());
    assert_eq!(state.combat_log.player_card_plays_this_combat, 3);
}

#[test]
fn beta_top_waits_for_last_cards_selection_then_draws_selected_top() {
    let mut state = board();
    let headbutt = mint(&mut state, CardId::Headbutt);
    let defend = mint(&mut state, CardId::DefendIronclad);
    state.player.hand.push(headbutt);
    state.player.discard.push(defend);
    let strike = mint(&mut state, CardId::StrikeIronclad);
    state.player.discard.push(strike);
    state.player.energy = 3;
    assert_eq!(
        sts2_core::apply(
            &mut state,
            Action::PlayCard {
                hand_index: 0,
                target: Some(0)
            }
        ),
        StepResult::PendingChoice
    );
    assert!(state.player.hand.is_empty());
    assert!(state.player.play.contains(&headbutt));
    let choice = sts2_core::legal_actions(&state)
        .into_iter()
        .find(|action| matches!(action, Action::ResolveChoice { .. }))
        .unwrap();
    assert_eq!(sts2_core::apply(&mut state, choice), StepResult::Advanced);
    assert_eq!(state.player.hand.as_slice(), &[defend]);
    assert_eq!(state.player.discard.as_slice(), &[strike, headbutt]);
}

#[test]
fn beta_top_opening_draw_resumes_after_stratagem_without_replaying_turn_start() {
    let mut state = board();
    let owner = state.player.creature_uid.unwrap();
    for power in [PowerKind::Hellraiser, PowerKind::Stratagem] {
        sts2_core::card_effects::apply_power_to_creature(&mut state, owner, power, 1);
    }
    let strikes: Vec<_> = (0..5)
        .map(|_| mint(&mut state, CardId::StrikeIronclad))
        .collect();
    let defend = mint(&mut state, CardId::DefendIronclad);
    state.player.draw.extend(strikes.iter().copied());
    state.player.discard.push(defend);
    state.phase = CombatPhase::PlayerTurnStart;
    assert_eq!(
        sts2_core::combat_loop::start_player_turn(&mut state),
        StepResult::PendingChoice
    );
    assert_eq!(state.combat_log.player_card_plays_this_combat, 5);
    let choice = state.pending_choice.as_ref().unwrap();
    let index = choice
        .options
        .iter()
        .position(|option| option.card_uid == Some(defend))
        .unwrap();
    let action = Action::ResolveChoice {
        selection: smallvec::smallvec![u8::try_from(index).unwrap()],
    };
    assert_eq!(sts2_core::apply(&mut state, action), StepResult::Advanced);
    assert_eq!(state.phase, CombatPhase::PlayerPlayPhase);
    assert_eq!(state.player.hand.as_slice(), &[defend]);
    // Stratagem adds Defend directly; the suspended Top draw still draws one Strike.
    assert_eq!(state.combat_log.player_card_plays_this_combat, 6);
    assert!(state.pending_cluster_resume.is_none());
}

#[test]
fn beta_top_ignores_unstarted_batch_siblings_after_each_mayhem_play() {
    let mut state = board();
    let owner = state.player.creature_uid.unwrap();
    sts2_core::card_effects::apply_power_to_creature(&mut state, owner, PowerKind::Mayhem, 2);
    let opening: Vec<_> = (0..5)
        .map(|_| mint(&mut state, CardId::DefendIronclad))
        .collect();
    let fire = mint(&mut state, CardId::FiendFire);
    let grit = mint(&mut state, CardId::TrueGrit);
    let top_draw = mint(&mut state, CardId::DefendIronclad);
    let final_draw = mint(&mut state, CardId::Decay);
    state.player.draw.extend(opening.iter().copied());
    state.player.draw.extend([fire, grit, top_draw, final_draw]);
    state.phase = CombatPhase::PlayerTurnStart;
    assert_eq!(
        sts2_core::combat_loop::start_player_turn(&mut state),
        StepResult::Advanced
    );
    // Fiend Fire empties the hand; Top draws Defend before True Grit plays.
    // True Grit exhausts that Defend; the next Top trigger draws Decay.
    assert!(
        state.player.exhaust.contains(&top_draw),
        "hand={:?} draw={:?} play={:?} exhaust={:?} plays={}",
        state.player.hand,
        state.player.draw,
        state.player.play,
        state.player.exhaust,
        state.combat_log.player_card_plays_this_combat
    );
    assert_eq!(state.player.hand.as_slice(), &[final_draw]);
    assert_eq!(state.combat_log.player_card_plays_this_combat, 2);
}
