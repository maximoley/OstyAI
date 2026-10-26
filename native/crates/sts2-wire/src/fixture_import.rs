//! Checked combat-start import for training and fixed validation fixtures.
//! The historical Python producer has three obsolete opening moves. Normalize
//! only those explicit first-player-turn starts to the installed native content. This
//! is not a mid-combat move alias and must never reset an arbitrary live state.
use crate::{parse_json, state_from_dict_impl, unsupported, WireResult};
use sts2_content::monster_dispatch::{build_ai_with_ascension, MonsterSpawnArgs, SpawnPosition};
use sts2_core::{CombatState, MonsterKind};

pub fn state_from_fixture_json(value: &serde_json::Value) -> WireResult<CombatState> {
    let mut parsed = parse_json(value)?;
    for (i, enemy) in parsed.encounter.iter_mut().enumerate() {
        let obsolete_start = matches!((enemy.kind, enemy.move_state.as_deref()),
            (MonsterKind::SkulkingColony, Some("SMASH_MOVE")) |
            (MonsterKind::HauntedShip, Some("RAMMING_SPEED_MOVE")) |
            (MonsterKind::Axebot, Some("SHARPEN_MOVE")));
        // Dead enemies can retain historical move labels, including obsolete
        // openings. Their next move is never executed; leave corpse metadata
        // untouched and keep the strict migration gate for living enemies.
        if obsolete_start && (enemy.is_dead || enemy.hp <= 0) {
            continue;
        }
        if obsolete_start {
            if parsed.round != 1 || parsed.turn != 1 || parsed.phase != "PlayerPlayPhase"
                || enemy.hp <= 0 || enemy.is_dead || enemy.is_escaped || !enemy.intent_history.is_empty()
                || parsed.intent_amounts != crate::state_dict::IntentAmounts::Base {
                return Err(unsupported(format!("$.encounter[{i}].move_state"),
                    "obsolete opening move is only migratable in a first-player-turn base-intent fixture before any enemy move"));
            }
            let ai = build_ai_with_ascension(enemy.kind, MonsterSpawnArgs::default(),
                SpawnPosition::default(), parsed.ascension);
            enemy.move_state = Some(ai.current_move().id.to_owned());
            enemy.intent = ai.current_move().intents.first().copied();
            enemy.intent_history.clear();
        }
    }
    let state = state_from_dict_impl(&parsed)?;
    for uid in state.active_enemy_uids() {
        let c = sts2_core::creature_by_id(&state.creatures, uid.0).expect("enemy UID exists");
        if c.current_hp > 0 && c.ai.is_none() {
            return Err(unsupported("$.encounter", &format!(
                "live enemy {:?} has no restored AI; refusing legacy Attack(5) fallback", c.species)));
        }
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sts2_core::{Action, Intent};
    fn wire(kind: &str, move_id: &str, ascension: u8) -> serde_json::Value {
        let mut w: serde_json::Value = serde_json::from_str(include_str!("fixture_import_test.json")).unwrap();
        w["player"]["hp"]=json!(500);w["player"]["max_hp"]=json!(500);
        w["rng"]["ascension"]=json!(ascension);
        w["encounter"][0]["kind"]=json!(kind);
        w["encounter"][0]["move_state"]=json!(move_id);
        w
    }
    #[test]
    fn colony_old_start_runs_a10_rotation_and_strength() {
        let mut s=state_from_fixture_json(&wire("SkulkingColony","SMASH_MOVE",10)).unwrap();
        let uid=s.active_enemy_uids()[0];
        let expected=[("ZOOM_MOVE",16),("ZOOM_MOVE_2",16),("INERTIA_MOVE",11),("PIERCING_STABS_MOVE",8),("ZOOM_MOVE",16)];
        for (name,damage) in expected {
            let c=sts2_core::creature_by_id(&s.creatures,uid.0).unwrap();
            assert_eq!(c.ai.as_ref().unwrap().current_move().id,name);
            match c.intent.unwrap() {Intent::Attack{damage:d}|Intent::MultiAttack{damage:d,..}=>assert_eq!(d,damage),x=>panic!("{x:?}")};
            sts2_core::apply(&mut s,Action::EndTurn);
        }
        assert!(s.powers.iter().any(|p|p.kind==sts2_core::PowerKind::Strength && p.amount==4));
    }
    #[test]
    fn known_obsolete_starts_are_migrated_and_pet_is_not_an_enemy() {
        for (kind,old,new) in [("SkulkingColony","SMASH_MOVE","ZOOM_MOVE"),("HauntedShip","RAMMING_SPEED_MOVE","HAUNT_MOVE"),("Axebot","SHARPEN_MOVE","HAMMER_UPPERCUT")] {
            let mut w=wire(kind,old,10);w["player"]["osty"]=json!({"hp":1,"max_hp":1,"block":0,"alive":true});
            let s=state_from_fixture_json(&w).unwrap();
            let c=sts2_core::creature_by_id(&s.creatures,s.active_enemy_uids()[0].0).unwrap();
            assert_eq!(c.ai.as_ref().unwrap().current_move().id,new);
        }
    }
    #[test]
    fn unknown_missing_and_midcombat_legacy_moves_fail_closed() {
        for name in ["TYPO_MOVE", ""] {
            let mut w=wire("SkulkingColony",name,10);
            if name.is_empty(){w["encounter"][0].as_object_mut().unwrap().remove("move_state");}
            assert!(state_from_fixture_json(&w).is_err());
        }
        let mut w=wire("SkulkingColony","SMASH_MOVE",10);w["round"]=json!(2);
        assert!(state_from_fixture_json(&w).is_err());
        w["round"]=json!(1);w["encounter"][0]["hp"]=json!(79);
        let s=state_from_fixture_json(&w).unwrap();
        assert_eq!(sts2_core::creature_by_id(&s.creatures,s.active_enemy_uids()[0].0).unwrap().current_hp,79);
        // Opening relic damage is retained, not mistaken for a later turn.
        w["encounter"][0]["intent_history"]=json!([0]);
        assert!(state_from_fixture_json(&w).is_err());
    }
    #[test]
    fn dead_obsolete_moves_preserve_corpses_and_live_ai() {
        let mut w=wire("Axebot","SHARPEN_MOVE",10);
        w["encounter"][0]["hp"]=json!(0);
        w["encounter"][0]["is_dead"]=json!(true);
        let mut live=w["encounter"][0].clone();
        live["hp"]=json!(80);live["is_dead"]=json!(false);
        live["move_state"]=json!("BOOT_UP");
        w["encounter"].as_array_mut().unwrap().push(live);
        let before=state_from_dict_impl(&parse_json(&w).unwrap()).unwrap();
        let s=state_from_fixture_json(&w).unwrap();
        assert_eq!(crate::emit::combat::run_rng_states(&s),crate::emit::combat::run_rng_states(&before));
        let corpse=sts2_core::creature_by_id(&s.creatures,s.enemy_uids[0].0).unwrap();
        assert_eq!(corpse.current_hp,0);
        let live=sts2_core::creature_by_id(&s.creatures,s.enemy_uids[1].0).unwrap();
        assert_eq!(live.current_hp,80);
        assert_eq!(live.ai.as_ref().unwrap().current_move().id,"BOOT_UP");
        // A corpse's old move is inert even in a later snapshot; a live old
        // opening remains prohibited outside the narrowly defined start.
        w["round"]=json!(2);
        assert!(state_from_fixture_json(&w).is_ok());
        w["encounter"][0]["hp"]=json!(80);
        w["encounter"][0]["is_dead"]=json!(false);
        assert!(state_from_fixture_json(&w).is_err());
    }
    #[test]
    fn before_turn_setup_autoplay_choice_finishes_before_natural_draw() {
        sts2_content::cards::dispatch::install_default_registry();
        for draw_count in [1, 2] {
            for atomic in [false, true] {
                let mut w: serde_json::Value = serde_json::from_str(include_str!("turn_start_autoplay_test.json")).unwrap();
                w["player"]["powers"][0]["name"]=json!(sts2_core::PowerKind::Hellraiser.python_name());
                let mut s=state_from_json_for_test(&w);
                s.phase=sts2_core::CombatPhase::PlayerTurnStart;
                // A before-side-turn-start draw (e.g. Centennial Puzzle after
                // start-turn damage) auto-plays Sculpting Strike via Hellraiser.
                s.draw_into_hand(draw_count);
                assert!(s.pending_choice.is_some());
                assert!(s.pending_autoplay_resume.is_some());
                assert!(s.pending_draw_resume.is_some());
                s.pending_turn_setup_resume=true;
                if atomic {
                    sts2_core::resolve_choice_atomic_apply(&mut s,&[1]);
                } else {
                    sts2_core::apply(&mut s,Action::ResolveChoice{selection:smallvec::smallvec![1]});
                }
                assert_eq!(s.phase,sts2_core::CombatPhase::PlayerPlayPhase);
                assert!(s.pending_choice.is_none());
                assert!(s.pending_autoplay_resume.is_none());
                assert!(s.pending_draw_resume.is_none());
                assert!(!s.pending_turn_setup_resume);
                assert_eq!(s.player.hand.len(),(6+draw_count) as usize);
                assert_eq!(s.player.discard.len(),1);
                assert!(s.player.play.is_empty());
                assert_eq!(s.combat_log.card_plays_finished_this_turn,1);
            }
        }
    }
    fn turn_start_choice_after_boulder(kind: sts2_core::PowerKind) {
        sts2_content::cards::dispatch::install_default_registry();
        for lethal in [true, false] {
            let mut w: serde_json::Value = serde_json::from_str(include_str!("turn_start_autoplay_test.json")).unwrap();
            w["player"]["powers"]=json!([
                {"name":sts2_core::PowerKind::RollingBoulder.python_name(),"amount":if lethal {1000} else {1}},
                {"name":kind.python_name(),"amount":1}
            ]);
            let mut s=state_from_fixture_json(&w).unwrap();
            let result=sts2_core::apply(&mut s,Action::EndTurn);
            if lethal {
                assert_eq!(result,sts2_core::StepResult::CombatWon,
                    "a suppressed choice must not suspend combat-end processing");
                assert!(s.is_over && s.player_won);
                assert!(s.pending_choice.is_none() && s.pending_cluster_resume.is_none());
                assert!(s.player.exhaust.is_empty(),"suppressed choice must not exhaust a card");
            } else {
                assert_eq!(result,sts2_core::StepResult::PendingChoice);
                assert!(!s.is_over);
                assert!(s.pending_choice.is_some() && s.pending_cluster_resume.is_some());
                sts2_core::resolve_choice_atomic_apply(&mut s,&[0]);
                assert_eq!(s.phase,sts2_core::CombatPhase::PlayerPlayPhase);
                assert!(s.pending_choice.is_none() && s.pending_cluster_resume.is_none());
            }
        }
    }
    #[test]
    fn lethal_boulder_does_not_park_suppressed_tyranny_choice() {
        turn_start_choice_after_boulder(sts2_core::PowerKind::Tyranny);
    }
    #[test]
    fn lethal_boulder_does_not_park_suppressed_tools_choice() {
        turn_start_choice_after_boulder(sts2_core::PowerKind::ToolsOfTheTrade);
    }
    fn state_from_json_for_test(w: &serde_json::Value) -> CombatState {
        crate::state_from_json(w).unwrap()
    }
    #[test]
    fn ordinary_round_trip_keeps_a10_ai_and_rng() {
        let s=state_from_fixture_json(&wire("SkulkingColony","SMASH_MOVE",10)).unwrap();
        let emitted=crate::emit::combat::combat_state_to_value(&s).unwrap();
        let again=crate::state_from_json(&emitted).unwrap();
        let uid=again.active_enemy_uids()[0];
        let c=sts2_core::creature_by_id(&again.creatures,uid.0).unwrap();
        assert!(matches!(c.ai.as_ref().unwrap().current_move().intents[0],Intent::Attack{damage:16}));
        assert_eq!(crate::emit::combat::run_rng_states(&s),crate::emit::combat::run_rng_states(&again));
    }
}
