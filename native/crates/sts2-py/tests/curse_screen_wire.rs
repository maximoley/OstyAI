//! Wire-surface guard for the mid-enemy-turn KnowledgeDemon curse screen.
//!
//! C# `KnowledgeDemon.CurseOfKnowledge` (`KnowledgeDemon.cs:149-189`) `await`s
//! a blocking card screen INSIDE the enemy turn, so the engine now suspends
//! `Action::EndTurn` mid-`EnemyTurnExec` with a `MonsterCurseSelect` open. That
//! is a state shape `state_to_dict` had never been handed before (previously
//! the only suspended-with-a-choice states were on the player's side of the
//! turn boundary), so this locks that the exporter survives it — and pins the
//! phase/side it reports, which the mirror driver has to reconcile against the
//! world's own `PlayerPlayPhase` / `Player` at the same decision.

#![allow(clippy::missing_panics_doc)]

use pyo3::prelude::*;

use sts2_content::monsters::act2_hive::create_knowledge_demon_ai;
use sts2_core::{
    apply, cards::CardId, powers::PowerKind, Action, ChoiceKind, CombatPhase, CombatState, Intent,
    MonsterKind, Side, StepResult,
};
use sts2_rs::state_dict::state_to_dict;

/// Drive a real `EndTurn` into the suspended-on-the-curse-screen state.
fn suspended_on_curse_screen() -> CombatState {
    let mut state = CombatState::default();
    state.player.current_hp = 200;
    state.player.max_hp = 200;
    let _ = state.init_player_creature();
    let kd = state.spawn_enemy(MonsterKind::KnowledgeDemon);
    state.set_creature_ai(kd, create_knowledge_demon_ai());
    if let Some(c) = state.creatures.iter_mut().find(|c| c.id == kd.0) {
        c.intent = Some(Intent::Debuff {
            kind: PowerKind::Weak,
            amount: 0,
        });
    }
    state.round = 1;
    state.side = Side::Player;
    state.phase = CombatPhase::PlayerPlayPhase;
    assert_eq!(
        apply(&mut state, Action::EndTurn),
        StepResult::PendingChoice,
        "EndTurn must suspend on the curse screen",
    );
    state
}

#[test]
fn state_to_dict_survives_a_mid_enemy_turn_curse_screen() {
    let state = suspended_on_curse_screen();
    let pc = state.pending_choice.as_ref().expect("screen open");
    assert_eq!(pc.kind, ChoiceKind::MonsterCurseSelect);
    assert_eq!(
        pc.options
            .iter()
            .map(|o| o.card_id)
            .collect::<Vec<CardId>>(),
        vec![CardId::Disintegration, CardId::MindRot],
    );

    Python::with_gil(|py| {
        let d = state_to_dict(py, &state).expect("exporter must not fail on a suspended state");
        // The suspended state reports the phase it is genuinely in. The live
        // game's own wire at this same decision says `PlayerPlayPhase` /
        // `Player` (it is a player card screen), so a mirror comparing phases
        // verbatim will see a difference here — reconciling that is the
        // driver's job, not the engine's, and the ROUND (the field that was
        // a turn ahead before this retiming) now agrees.
        assert_eq!(
            d.get_item("phase")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "EnemyPhase",
        );
        assert_eq!(
            d.get_item("side")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "Enemy",
        );
        assert_eq!(
            d.get_item("round")
                .unwrap()
                .unwrap()
                .extract::<i32>()
                .unwrap(),
            1,
            "the round has NOT advanced past the decision",
        );
        // `state_to_dict` does not emit the pending choice at all (the wire
        // schema treats mid-resolution slots as out of band), so the parked
        // enemy turn cannot survive a to_dict/from_dict round-trip either —
        // the same pre-existing limitation every `pending_*_resume` marker has.
        assert!(d.get_item("pending_choice").unwrap().is_none());
    });
}
