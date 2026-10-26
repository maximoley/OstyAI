//! Property tests for [`sts2_core::hooks::ValueProps`].
//!
//! Mirrors Python `ValueProp` IntFlag at
//! `sts2-rl-agent@01c094d:sts2_env/core/enums.py:54-62`. Every powered
//! listener arm (Strength / Vulnerable / Weak / Frail / Dexterity)
//! gates on `is_powered()`, so the bitflag laws here are load-bearing.

use hegel::generators as gs;
use hegel::TestCase;
use sts2_core::hooks::ValueProps;

/// Draw an arbitrary ValueProps value over the documented 5-bit
/// domain (bit 0 is intentionally unused — see Python IntFlag gap
/// preserved at hooks.rs:228-229). We allow `u32` raw values so the
/// laws hold even for hypothetically wider future tags.
fn arb_props(tc: &TestCase) -> ValueProps {
    let bits = tc.draw(gs::integers::<u32>().min_value(0).max_value(0xff));
    // Reconstruct via the public NONE/union API to honor the type
    // boundary (constructor is `pub` but only through the named
    // constants, so we compose).
    let mut acc = ValueProps::NONE;
    if bits & 2 != 0 {
        acc = acc.union(ValueProps::UNBLOCKABLE);
    }
    if bits & 4 != 0 {
        acc = acc.union(ValueProps::UNPOWERED);
    }
    if bits & 8 != 0 {
        acc = acc.union(ValueProps::MOVE);
    }
    if bits & 16 != 0 {
        acc = acc.union(ValueProps::SKIP_HURT_ANIM);
    }
    acc
}

#[hegel::test]
fn union_is_commutative(tc: TestCase) {
    let a = arb_props(&tc);
    let b = arb_props(&tc);
    assert_eq!(a.union(b), b.union(a));
}

#[hegel::test]
fn union_is_associative(tc: TestCase) {
    let a = arb_props(&tc);
    let b = arb_props(&tc);
    let c = arb_props(&tc);
    assert_eq!(a.union(b).union(c), a.union(b.union(c)));
}

#[hegel::test]
fn union_is_idempotent(tc: TestCase) {
    let a = arb_props(&tc);
    assert_eq!(a.union(a), a);
}

#[hegel::test]
fn none_is_union_identity(tc: TestCase) {
    let a = arb_props(&tc);
    assert_eq!(a.union(ValueProps::NONE), a);
    assert_eq!(ValueProps::NONE.union(a), a);
}

#[hegel::test]
fn contains_self_always(tc: TestCase) {
    let a = arb_props(&tc);
    assert!(a.contains(a));
}

#[hegel::test]
fn contains_none_always(tc: TestCase) {
    let a = arb_props(&tc);
    assert!(a.contains(ValueProps::NONE));
}

#[hegel::test]
fn union_contains_both_operands(tc: TestCase) {
    let a = arb_props(&tc);
    let b = arb_props(&tc);
    let ab = a.union(b);
    assert!(ab.contains(a), "a ∪ b should contain a");
    assert!(ab.contains(b), "a ∪ b should contain b");
}

#[hegel::test]
fn is_powered_matches_python_definition(tc: TestCase) {
    // Python: is_powered = (self & MOVE) and not (self & UNPOWERED)
    let a = arb_props(&tc);
    let want = a.contains(ValueProps::MOVE) && !a.contains(ValueProps::UNPOWERED);
    assert_eq!(a.is_powered(), want);
}

#[hegel::test]
fn unpowered_dominates_move(tc: TestCase) {
    // Adding UNPOWERED to any set kills is_powered, no matter what else
    // is set. Mirrors how Thorns / FlameBarrier reflections terminate
    // recursion by setting UNPOWERED on the reflective hit
    // (damage.rs:48 + hooks.rs:506 comment chain).
    let a = arb_props(&tc);
    let with_unpowered = a.union(ValueProps::UNPOWERED);
    assert!(!with_unpowered.is_powered());
}

#[test]
fn canonical_powered_constants() {
    // Deterministic anchors for the load-bearing constants — the
    // recorder default is MOVE (record_fixture.py:322).
    assert!(ValueProps::MOVE.is_powered());
    assert!(!ValueProps::NONE.is_powered());
    assert!(!ValueProps::UNPOWERED.is_powered());
    assert!(!ValueProps::UNBLOCKABLE.is_powered());
}
