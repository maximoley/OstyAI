//! Property tests for [`sts2_core::damage::apply_damage`].
//!
//! Targets the post-call contract documented at `damage.rs:35-46`:
//!
//!   * The return value equals the actual HP loss the target took
//!     (block-absorbed damage is excluded).
//!   * `current_hp` ends in `[0, max_hp]` (the explicit clamp at
//!     `damage.rs:178-181`).
//!   * `block` ends in `[0, prior_block]` — block never *increases*
//!     from an attack absorption, and never goes negative.
//!   * UNBLOCKABLE skips block absorption entirely
//!     (`damage.rs:141-148` mirrors Python `damage.py:135-137`).
//!   * Zero raw damage cannot mutate HP or block (the `unblocked > 0`
//!     short-circuit at `damage.rs:155-171`).
//!
//! The fixture is a one-creature combat state with no powers / no
//! relics — bypassing every listener so the properties are about the
//! primitive absorption + clamp pipeline only. Multi-listener
//! interactions (Vulnerable / Thorns / FlameBarrier) already have
//! example-based coverage in `damage.rs::tests`.

use hegel::generators as gs;
use hegel::TestCase;
use smallvec::SmallVec;
use std::num::NonZeroU32;
use sts2_core::hooks::{CreatureRef, ValueProps};
use sts2_core::{CombatState, Creature, CreatureUid, IntentHistory, MonsterId, MonsterKind};

fn cultist_uid() -> CreatureUid {
    CreatureUid::new(1).expect("non-zero")
}

/// Build a clean one-creature state with given HP / block. No powers,
/// no relics — every listener walk is empty.
fn build_state(current_hp: i32, max_hp: i32, block: i32) -> CombatState {
    let mut state = CombatState::default();
    state.player.current_hp = 50;
    state.player.max_hp = 50;
    state.creatures.push(Creature {
        id: NonZeroU32::new(1).expect("non-zero"),
        current_hp,
        max_hp,
        block,
        powers: SmallVec::new(),
        species: Some(MonsterKind::Nibbit),
        intent: None,
        intent_history: IntentHistory::default(),
        monster_id: MonsterId("Nibbit"),
        ai: None,
    });
    state.enemy_uids.push(cultist_uid());
    state
}

#[hegel::test(test_cases = 500)]
fn hp_stays_in_bounds(tc: TestCase) {
    let max_hp = tc.draw(gs::integers::<i32>().min_value(1).max_value(999));
    let current_hp = tc.draw(gs::integers::<i32>().min_value(0).max_value(max_hp));
    let block = tc.draw(gs::integers::<i32>().min_value(0).max_value(999));
    let raw = tc.draw(gs::integers::<i32>().min_value(0).max_value(9999));
    let unblockable = tc.draw(gs::booleans());

    let mut state = build_state(current_hp, max_hp, block);
    let props = if unblockable {
        ValueProps::MOVE.union(ValueProps::UNBLOCKABLE)
    } else {
        ValueProps::MOVE
    };

    let hp_loss = sts2_core::damage::apply_damage(
        &mut state,
        CreatureRef::Player,
        cultist_uid(),
        raw,
        props,
        None,
    );

    let after = &state.creatures[0];
    tc.note(&format!(
        "raw={raw} unblockable={unblockable} block_in={block} hp_in={current_hp} max={max_hp} \
         -> hp_after={} block_after={} hp_loss={hp_loss}",
        after.current_hp, after.block
    ));

    assert!(after.current_hp >= 0, "current_hp went negative");
    assert!(after.current_hp <= max_hp, "current_hp exceeded max_hp");
    assert!(after.block >= 0, "block went negative");
    state
        .assert_invariants()
        .expect("structural invariants hold");
}

#[hegel::test(test_cases = 500)]
fn block_never_increases_on_attack(tc: TestCase) {
    let max_hp = tc.draw(gs::integers::<i32>().min_value(1).max_value(999));
    let current_hp = tc.draw(gs::integers::<i32>().min_value(1).max_value(max_hp));
    let block_before = tc.draw(gs::integers::<i32>().min_value(0).max_value(999));
    let raw = tc.draw(gs::integers::<i32>().min_value(0).max_value(9999));

    let mut state = build_state(current_hp, max_hp, block_before);
    let _ = sts2_core::damage::apply_damage(
        &mut state,
        CreatureRef::Player,
        cultist_uid(),
        raw,
        ValueProps::MOVE,
        None,
    );
    let block_after = state.creatures[0].block;
    tc.note(&format!(
        "block_before={block_before} raw={raw} -> block_after={block_after}"
    ));
    assert!(block_after <= block_before);
    assert!(block_after >= 0);
}

#[hegel::test(test_cases = 500)]
fn return_value_equals_hp_delta(tc: TestCase) {
    // For a survivor (hp_after > 0): the returned hp_loss MUST equal
    // hp_before - hp_after. Without any modify_hp_lost listeners
    // installed this is the load-bearing contract callers (e.g. damage
    // reflection) rely on.
    let max_hp = tc.draw(gs::integers::<i32>().min_value(2).max_value(999));
    // Force hp_before >= 1 so we can make survival deterministic by
    // bounding raw damage below hp_before.
    let hp_before = tc.draw(gs::integers::<i32>().min_value(1).max_value(max_hp));
    let block = tc.draw(gs::integers::<i32>().min_value(0).max_value(99));
    // Cap raw damage so the survivor branch is reachable. We still
    // exercise overkill in the separate hp_stays_in_bounds property.
    let raw = tc.draw(gs::integers::<i32>().min_value(0).max_value(hp_before - 1));

    let mut state = build_state(hp_before, max_hp, block);
    let hp_loss = sts2_core::damage::apply_damage(
        &mut state,
        CreatureRef::Player,
        cultist_uid(),
        raw,
        ValueProps::MOVE,
        None,
    );
    let hp_after = state.creatures[0].current_hp;
    tc.note(&format!(
        "raw={raw} block={block} hp_before={hp_before} hp_after={hp_after} hp_loss={hp_loss}"
    ));
    assert!(
        hp_after > 0,
        "fixture should keep target alive (raw < hp_before)"
    );
    assert_eq!(hp_loss, hp_before - hp_after, "return ≠ HP delta");
}

#[hegel::test(test_cases = 500)]
fn unblockable_never_touches_block(tc: TestCase) {
    // damage.rs:141-148: UNBLOCKABLE absorbs zero block; block must
    // stay exactly equal to prior_block.
    let max_hp = tc.draw(gs::integers::<i32>().min_value(1).max_value(999));
    let current_hp = tc.draw(gs::integers::<i32>().min_value(0).max_value(max_hp));
    let block_before = tc.draw(gs::integers::<i32>().min_value(0).max_value(999));
    let raw = tc.draw(gs::integers::<i32>().min_value(0).max_value(9999));

    let mut state = build_state(current_hp, max_hp, block_before);
    let _ = sts2_core::damage::apply_damage(
        &mut state,
        CreatureRef::Player,
        cultist_uid(),
        raw,
        ValueProps::MOVE.union(ValueProps::UNBLOCKABLE),
        None,
    );
    let block_after = state.creatures[0].block;
    tc.note(&format!(
        "raw={raw} block_before={block_before} block_after={block_after}"
    ));
    assert_eq!(
        block_after, block_before,
        "UNBLOCKABLE must not consume block"
    );
}

#[hegel::test(test_cases = 500)]
fn zero_raw_damage_is_noop(tc: TestCase) {
    // Zero raw damage with no listeners: nothing should change on the
    // target. The `unblocked > 0` short-circuit at damage.rs:155-171
    // prevents the modify_hp_lost walk; the block-absorption math
    // produces `absorbed = min(0, block) = 0` so block is untouched.
    let max_hp = tc.draw(gs::integers::<i32>().min_value(1).max_value(999));
    let current_hp = tc.draw(gs::integers::<i32>().min_value(0).max_value(max_hp));
    let block_before = tc.draw(gs::integers::<i32>().min_value(0).max_value(999));

    let mut state = build_state(current_hp, max_hp, block_before);
    let hp_loss = sts2_core::damage::apply_damage(
        &mut state,
        CreatureRef::Player,
        cultist_uid(),
        0,
        ValueProps::MOVE,
        None,
    );
    let after = &state.creatures[0];
    tc.note(&format!(
        "block_before={block_before} hp_before={current_hp} -> \
         hp_after={} block_after={} hp_loss={hp_loss}",
        after.current_hp, after.block
    ));
    assert_eq!(hp_loss, 0);
    assert_eq!(after.current_hp, current_hp);
    assert_eq!(after.block, block_before);
}
