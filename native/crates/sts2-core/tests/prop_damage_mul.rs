//! Property tests for [`sts2_core::hooks::DamageMul`].
//!
//! Targets the pure rational-multiplier type documented in
//! `crates/sts2-core/src/hooks.rs:79-167`. Properties exercised:
//!
//!   * `ONE` is left/right identity of `compose` and of `apply`.
//!   * `ZERO.apply(x) == 0` for all in-range x.
//!   * `compose` is associative.
//!   * Order invariance after a single floor: `a.compose(b).apply(x)
//!     == b.compose(a).apply(x)` (load-bearing claim from the doc-
//!     comment that listener iteration order does not affect the final
//!     integer result).
//!   * `apply` matches the Python reference `math.floor(x * num / den)`
//!     bit-for-bit (computed in i128 here as ground truth).
//!   * For non-negative `num` and non-negative `x`, `apply(x) >= 0`.
//!   * Monotonicity in `x` when `num >= 0`.
//!
//! Generator bounds are chosen so that composed intermediate products
//! stay well inside `i64`: all Python listener multipliers documented
//! in `docs/multiplier-rounding-audit.md` have `num,den <= ~1e3`, and
//! damage values cap around `~10_000`. We stretch slightly past the
//! audit envelope to flush overflow/saturation edges without going
//! anywhere near `i64` overflow on the i128 reference path.

use hegel::generators as gs;
use hegel::TestCase;
use sts2_core::hooks::DamageMul;

/// Draw a `DamageMul` with `num ∈ [-2000, 2000]`, `den ∈ [1, 2000]`.
/// Matches the audit envelope (Vulnerable 3/2, Weak 3/4, Double 2/1,
/// Triple 3/1, etc.) with a healthy margin.
#[hegel::composite]
fn arb_mul(tc: TestCase) -> DamageMul {
    let num = tc.draw(gs::integers::<i64>().min_value(-2000).max_value(2000));
    let den = tc.draw(gs::integers::<i64>().min_value(1).max_value(2000));
    DamageMul::new(num, den)
}

/// Damage-range input. Real callers pass `i32` damage that practically
/// caps at a few thousand; we cover `[-10_000, 10_000]` to exercise
/// negative-input flooring (Python `math.floor` rounds toward `-inf`,
/// which `i64::div_euclid` matches).
fn arb_x(tc: &TestCase) -> i32 {
    tc.draw(gs::integers::<i32>().min_value(-10_000).max_value(10_000))
}

/// Reference flooring computed in `i128` — matches Python's
/// `math.floor(x * num / den)` exactly (for `den > 0`).
fn floor_reference(x: i32, num: i64, den: i64) -> i128 {
    let prod = i128::from(x) * i128::from(num);
    let d = i128::from(den);
    prod.div_euclid(d)
}

#[hegel::test]
fn one_is_apply_identity(tc: TestCase) {
    let x = arb_x(&tc);
    assert_eq!(DamageMul::ONE.apply(x), x);
}

#[hegel::test]
fn zero_apply_is_zero(tc: TestCase) {
    let x = arb_x(&tc);
    assert_eq!(DamageMul::ZERO.apply(x), 0);
}

#[hegel::test]
fn one_is_compose_left_identity(tc: TestCase) {
    let m = tc.draw(arb_mul());
    let x = arb_x(&tc);
    let composed = DamageMul::ONE.compose(m);
    tc.note(&format!("m={m:?} composed={composed:?} x={x}"));
    assert_eq!(composed.apply(x), m.apply(x));
}

#[hegel::test]
fn one_is_compose_right_identity(tc: TestCase) {
    let m = tc.draw(arb_mul());
    let x = arb_x(&tc);
    let composed = m.compose(DamageMul::ONE);
    tc.note(&format!("m={m:?} composed={composed:?} x={x}"));
    assert_eq!(composed.apply(x), m.apply(x));
}

#[hegel::test(test_cases = 2000)]
fn compose_apply_matches_python_floor(tc: TestCase) {
    let m = tc.draw(arb_mul());
    let x = arb_x(&tc);
    let got = m.apply(x);
    let want = floor_reference(x, m.num, m.den);
    tc.note(&format!("m={m:?} x={x} got={got} want={want}"));
    // Skip cases that overflow i32 — apply() saturates and the doc
    // explicitly calls this out; ground truth is i128, so we only
    // check equality when ground truth itself is representable.
    if let Ok(want_i32) = i32::try_from(want) {
        assert_eq!(i64::from(got), i64::from(want_i32));
    }
}

#[hegel::test(test_cases = 2000)]
fn compose_is_order_invariant_after_single_floor(tc: TestCase) {
    // Doc claim (hooks.rs:131-134): "Composition is commutative +
    // associative on rationals, so listener iteration order does not
    // affect the final integer result (after the single floor at end)."
    let a = tc.draw(arb_mul());
    let b = tc.draw(arb_mul());
    let x = arb_x(&tc);

    let ab = a.compose(b);
    let ba = b.compose(a);

    // Filter out cases where either composed product overflows i32 —
    // apply() saturates and the saturation direction may differ.
    let want_ab = floor_reference(x, ab.num, ab.den);
    let want_ba = floor_reference(x, ba.num, ba.den);
    if i32::try_from(want_ab).is_err() || i32::try_from(want_ba).is_err() {
        return;
    }

    let got_ab = ab.apply(x);
    let got_ba = ba.apply(x);
    tc.note(&format!("a={a:?} b={b:?} x={x} ab={got_ab} ba={got_ba}"));
    assert_eq!(got_ab, got_ba, "order of compose changed result");
}

#[hegel::test(test_cases = 2000)]
fn compose_is_associative(tc: TestCase) {
    let a = tc.draw(arb_mul());
    let b = tc.draw(arb_mul());
    let c = tc.draw(arb_mul());
    let x = arb_x(&tc);

    let left = a.compose(b).compose(c);
    let right = a.compose(b.compose(c));

    let want_left = floor_reference(x, left.num, left.den);
    let want_right = floor_reference(x, right.num, right.den);
    if i32::try_from(want_left).is_err() || i32::try_from(want_right).is_err() {
        return;
    }

    let got_left = left.apply(x);
    let got_right = right.apply(x);
    tc.note(&format!(
        "a={a:?} b={b:?} c={c:?} x={x} left={got_left} right={got_right}"
    ));
    assert_eq!(got_left, got_right);
}

#[hegel::test]
fn non_negative_inputs_non_negative_output(tc: TestCase) {
    // For non-negative multiplier (num >= 0, den > 0) and x >= 0,
    // apply() must return >= 0. This is the property real damage-
    // pipeline callers rely on (the final clamp at 0 in damage.rs:148
    // assumes the pipeline can't go negative for non-negative inputs).
    let num = tc.draw(gs::integers::<i64>().min_value(0).max_value(2000));
    let den = tc.draw(gs::integers::<i64>().min_value(1).max_value(2000));
    let x = tc.draw(gs::integers::<i32>().min_value(0).max_value(10_000));
    let m = DamageMul::new(num, den);
    let got = m.apply(x);
    tc.note(&format!("m={m:?} x={x} got={got}"));
    assert!(got >= 0, "non-negative input produced negative output");
}

#[hegel::test]
fn monotone_in_x_when_num_non_negative(tc: TestCase) {
    let num = tc.draw(gs::integers::<i64>().min_value(0).max_value(2000));
    let den = tc.draw(gs::integers::<i64>().min_value(1).max_value(2000));
    let x = tc.draw(gs::integers::<i32>().min_value(-10_000).max_value(10_000));
    let delta = tc.draw(gs::integers::<i32>().min_value(0).max_value(1000));
    let Some(y) = x.checked_add(delta) else {
        return;
    };
    let m = DamageMul::new(num, den);
    let fx = m.apply(x);
    let fy = m.apply(y);
    tc.note(&format!("m={m:?} x={x} y={y} fx={fx} fy={fy}"));

    // Only assert when both outputs are representable (no saturation).
    let ref_x = floor_reference(x, num, den);
    let ref_y = floor_reference(y, num, den);
    if i32::try_from(ref_x).is_err() || i32::try_from(ref_y).is_err() {
        return;
    }
    assert!(fx <= fy, "f({x})={fx} > f({y})={fy} for non-neg multiplier");
}
