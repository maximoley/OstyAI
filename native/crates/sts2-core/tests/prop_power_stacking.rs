//! Property tests for [`sts2_core::card_effects::apply_power_to_creature`].
//!
//! Documented contract at `card_effects.rs:232-308`:
//!
//!   * `amount <= 0` → early-out, no mutation.
//!   * Target with `current_hp <= 0` → no-op.
//!   * Existing power of same kind → `amount = saturating_add(prior, n)`.
//!   * No prior power → allocate fresh PowerUid with `amount = n`.
//!
//! Vanilla fixture: one Cultist-shape enemy, no relics, no other
//! powers, no listeners. Targeting Strength keeps every modify-chain
//! listener out of the picture (no PowerKind installs a
//! modify-power-amount arm against Strength at this pin).

use hegel::generators as gs;
use hegel::TestCase;
use smallvec::SmallVec;
use std::num::NonZeroU32;
use sts2_core::card_effects::apply_power_to_creature;
use sts2_core::{
    CombatState, Creature, CreatureUid, IntentHistory, MonsterId, MonsterKind, PowerKind,
};

fn target_uid() -> CreatureUid {
    CreatureUid::new(1).expect("non-zero")
}

fn build_state(target_hp: i32) -> CombatState {
    let mut state = CombatState::default();
    state.player.current_hp = 50;
    state.player.max_hp = 50;
    state.creatures.push(Creature {
        id: NonZeroU32::new(1).expect("non-zero"),
        current_hp: target_hp,
        max_hp: 48.max(target_hp),
        block: 0,
        powers: SmallVec::new(),
        species: Some(MonsterKind::Nibbit),
        intent: None,
        intent_history: IntentHistory::default(),
        monster_id: MonsterId("Nibbit"),
        ai: None,
    });
    state.enemy_uids.push(target_uid());
    state
}

/// Total amount of a given PowerKind on the target. Sums across all
/// instances (there should only ever be one per kind, but we
/// belt-and-suspenders sum to assert that explicitly elsewhere).
fn total_amount(state: &CombatState, kind: PowerKind) -> i32 {
    let creature = &state.creatures[0];
    creature
        .powers
        .iter()
        .filter_map(|uid| state.powers.get_by_id(uid.0))
        .filter(|p| p.kind == kind)
        .map(|p| p.amount)
        .sum()
}

fn instance_count(state: &CombatState, kind: PowerKind) -> usize {
    let creature = &state.creatures[0];
    creature
        .powers
        .iter()
        .filter_map(|uid| state.powers.get_by_id(uid.0))
        .filter(|p| p.kind == kind)
        .count()
}

#[hegel::test(test_cases = 300)]
fn two_applies_sum(tc: TestCase) {
    let n = tc.draw(gs::integers::<i32>().min_value(1).max_value(50));
    let m = tc.draw(gs::integers::<i32>().min_value(1).max_value(50));
    let mut state = build_state(48);

    apply_power_to_creature(&mut state, target_uid(), PowerKind::Strength, n);
    apply_power_to_creature(&mut state, target_uid(), PowerKind::Strength, m);

    assert_eq!(
        total_amount(&state, PowerKind::Strength),
        n + m,
        "two +N applies should sum"
    );
    assert_eq!(
        instance_count(&state, PowerKind::Strength),
        1,
        "stacking same kind should not allocate a new instance"
    );
    state.assert_invariants().expect("invariants hold");
}

#[hegel::test(test_cases = 300)]
fn non_positive_is_noop(tc: TestCase) {
    let n = tc.draw(gs::integers::<i32>().min_value(-10_000).max_value(0));
    let mut state = build_state(48);

    apply_power_to_creature(&mut state, target_uid(), PowerKind::Strength, n);

    assert_eq!(total_amount(&state, PowerKind::Strength), 0);
    assert_eq!(instance_count(&state, PowerKind::Strength), 0);
    state.assert_invariants().expect("invariants hold");
}

#[hegel::test(test_cases = 200)]
fn dead_target_is_noop(tc: TestCase) {
    // Target with current_hp <= 0 must be ignored. The position lookup
    // at card_effects.rs:241-244 explicitly filters on `current_hp > 0`.
    let n = tc.draw(gs::integers::<i32>().min_value(1).max_value(50));
    let mut state = build_state(0);

    apply_power_to_creature(&mut state, target_uid(), PowerKind::Strength, n);

    assert_eq!(total_amount(&state, PowerKind::Strength), 0);
    assert_eq!(instance_count(&state, PowerKind::Strength), 0);
}

#[hegel::test(test_cases = 200)]
fn missing_target_is_noop(tc: TestCase) {
    // A target uid that doesn't resolve to any creature must be ignored.
    let n = tc.draw(gs::integers::<i32>().min_value(1).max_value(50));
    let mut state = build_state(48);

    let phantom = CreatureUid::new(999).expect("non-zero");
    apply_power_to_creature(&mut state, phantom, PowerKind::Strength, n);

    assert_eq!(total_amount(&state, PowerKind::Strength), 0);
    state.assert_invariants().expect("invariants hold");
}

#[hegel::test(test_cases = 300)]
fn n_applies_sum(tc: TestCase) {
    // Generalization of `two_applies_sum`: applying +a_1, +a_2, ... +a_k
    // with each a_i > 0 sums to Σa_i. Catches any pathological
    // saturating_add path that would clamp prematurely (we stay well
    // below i32::MAX with these bounds).
    let k = tc.draw(gs::integers::<u32>().min_value(1).max_value(8)) as usize;
    let mut expected = 0i32;
    let mut state = build_state(48);
    for _ in 0..k {
        let a = tc.draw(gs::integers::<i32>().min_value(1).max_value(1000));
        apply_power_to_creature(&mut state, target_uid(), PowerKind::Strength, a);
        expected += a;
    }
    assert_eq!(total_amount(&state, PowerKind::Strength), expected);
    assert_eq!(instance_count(&state, PowerKind::Strength), 1);
}
