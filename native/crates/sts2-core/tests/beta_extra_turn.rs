//! Beta v0.111.0 Models.Powers/AmbergrisPower.cs and Relics/PaelsEye.cs.
use std::num::NonZeroU32;
use sts2_core::{
    CombatPhase, CombatState, Creature, CreatureUid, Intent, IntentHistory, MonsterId, MonsterKind,
    PowerKind, Relic, RelicKind, StepResult,
};

fn state_with_extra_turns(stacks: i32, paels_eye: bool) -> CombatState {
    let mut state = CombatState {
        phase: CombatPhase::PlayerPlayPhase,
        round: 1,
        ..CombatState::default()
    };
    state.player.current_hp = 50;
    state.player.max_hp = 50;
    state.player.max_energy = 3;
    let id = NonZeroU32::new(1).unwrap();
    state.creatures.push(Creature {
        id,
        current_hp: 48,
        max_hp: 48,
        block: 0,
        powers: smallvec::SmallVec::new(),
        species: Some(MonsterKind::ShrinkerBeetle),
        intent: Some(Intent::Buff {
            kind: PowerKind::Strength,
            amount: 0,
        }),
        intent_history: IntentHistory::default(),
        monster_id: MonsterId("ShrinkerBeetle"),
        ai: None,
    });
    state.enemy_uids.push(CreatureUid(id));
    state.init_player_creature();
    let player = state.player.creature_uid.unwrap();
    sts2_core::card_effects::apply_power_to_creature(
        &mut state,
        player,
        PowerKind::Ambergris,
        stacks,
    );
    if paels_eye {
        state.relics.push(Relic {
            id: NonZeroU32::new(1).unwrap(),
            kind: RelicKind::PaelsEye,
            counter: 0,
            is_wax: false,
            is_melted: false,
        });
    }
    state
}

#[test]
fn beta_ambergris_stacks_grant_exactly_that_many_extra_turns() {
    let mut state = state_with_extra_turns(2, false);
    for remaining in [1, 0] {
        assert_eq!(
            sts2_core::combat_loop::end_turn_dispatch(&mut state),
            StepResult::Advanced
        );
        assert_eq!(state.round, 1, "enemy turn must be skipped");
        assert_eq!(state.player_power_amount(PowerKind::Ambergris), remaining);
    }
    assert!(!state.powers.iter().any(|p| p.kind == PowerKind::Ambergris));
    assert_eq!(
        sts2_core::combat_loop::end_turn_dispatch(&mut state),
        StepResult::Advanced
    );
    assert_eq!(state.round, 2, "third turn returns to the enemy");
}

#[test]
fn beta_ambergris_and_paels_eye_share_one_extra_turn() {
    let mut state = state_with_extra_turns(1, true);
    assert_eq!(
        sts2_core::combat_loop::end_turn_dispatch(&mut state),
        StepResult::Advanced
    );
    assert_eq!(state.round, 1);
    assert_eq!(state.player_power_amount(PowerKind::Ambergris), 0);
    assert_ne!(state.relics[0].counter & 1, 0);
    assert_eq!(
        sts2_core::combat_loop::end_turn_dispatch(&mut state),
        StepResult::Advanced
    );
    assert_eq!(
        state.round, 2,
        "both effects were consumed by the same extra turn"
    );
}
