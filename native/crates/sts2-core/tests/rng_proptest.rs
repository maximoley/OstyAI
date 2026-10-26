//! Property-based invariants for the deterministic `.NET`-compatible RNG
//! (`sts2_core::dotnet_rng::Rng`).
//!
//! The RNG is the foundation of engine determinism and C# parity: every
//! shuffle, intent roll, map generation and reward draw flows through it.
//! The scripted parity fixtures pin a handful of concrete sequences against
//! the C# decomp, but they only exercise the seeds they happen to use.
//! These `proptest` cases assert the *contracts* the rest of the engine
//! relies on — range bounds, counter accounting, fast-forward equivalence,
//! determinism, and shuffle being a true permutation — across hundreds of
//! generated seeds and arguments. A regression in any of these would silently
//! desync the Rust engine from the game's RNG stream.
//!
//! Scope note: this is intentionally a self-contained template for the
//! property-testing approach. The RNG is the highest-leverage place to start
//! because its contracts are pure and easy to state; richer engine invariants
//! (state round-trips, command reversibility) can follow the same pattern.

use proptest::prelude::*;
use sts2_core::dotnet_rng::Rng;

proptest! {
    /// `next_int(low, high)` is documented as an INCLUSIVE `[low, high]`
    /// sample. Bounds are kept well inside `INT_MAX` so the range stays on
    /// the small-range sampling path; the inclusive contract holds on both
    /// paths but this keeps the generated args meaningful.
    #[test]
    fn next_int_is_inclusive_in_range(
        seed in any::<i32>(),
        low in -1_000_000i32..1_000_000,
        span in 0i32..2_000_000,
    ) {
        let high = low + span;
        let mut rng = Rng::new(seed);
        for _ in 0..64 {
            let v = rng.next_int(low, high);
            prop_assert!(v >= low && v <= high, "{v} not in [{low}, {high}]");
        }
    }

    /// `next_int_exclusive(low, high)` is half-open `[low, high)`.
    #[test]
    fn next_int_exclusive_is_half_open(
        seed in any::<i32>(),
        low in -1_000_000i32..1_000_000,
        span in 1i32..2_000_000,
    ) {
        let high = low + span;
        let mut rng = Rng::new(seed);
        for _ in 0..64 {
            let v = rng.next_int_exclusive(low, high);
            prop_assert!(v >= low && v < high, "{v} not in [{low}, {high})");
        }
    }

    /// `next_double` is uniform `[0, 1)` and `next_float_range(lo, hi)` stays
    /// within `[lo, hi]` (it returns `lo` when `lo == hi`).
    #[test]
    fn float_samples_stay_in_range(
        seed in any::<i32>(),
        lo in -1_000.0f64..1_000.0,
        width in 0.0f64..1_000.0,
    ) {
        let hi = lo + width;
        let mut rng = Rng::new(seed);
        for _ in 0..64 {
            let d = rng.next_double();
            prop_assert!((0.0..1.0).contains(&d), "next_double {d} not in [0, 1)");
            let f = rng.next_float_range(lo, hi);
            prop_assert!(f >= lo && f <= hi, "next_float_range {f} not in [{lo}, {hi}]");
        }
    }

    /// The public counter ticks exactly once per logical `next_*` call.
    #[test]
    fn counter_tracks_calls(seed in any::<i32>(), n in 0u32..500) {
        let mut rng = Rng::new(seed);
        prop_assert_eq!(rng.counter(), 0);
        for i in 0..n {
            rng.next_int(0, 100);
            prop_assert_eq!(rng.counter(), i + 1);
        }
        prop_assert_eq!(rng.counter(), n);
    }

    /// `with_counter(seed, n)` must land on the same stream position as a
    /// fresh `new(seed)` advanced by `n` single-sample `next_int` calls — the
    /// fast-forward path and the normal sampling path have to consume the
    /// underlying stream identically, or replay-from-counter desyncs.
    #[test]
    fn fast_forward_matches_stepwise(seed in any::<i32>(), n in 0u32..500) {
        let mut stepwise = Rng::new(seed);
        for _ in 0..n {
            // Small range => exactly one internal sample, matching one
            // fast-forward step.
            stepwise.next_int(0, 7);
        }
        let mut jumped = Rng::with_counter(seed, n);

        prop_assert_eq!(stepwise.counter(), jumped.counter());
        // Next draws from both must agree, and so must the counters after.
        prop_assert_eq!(stepwise.next_int(0, 1_000), jumped.next_int(0, 1_000));
        prop_assert_eq!(stepwise.counter(), jumped.counter());
    }

    /// Same seed => identical stream. Cloning mid-stream forks an identical
    /// continuation. This is the determinism guarantee the whole sim leans on.
    #[test]
    fn determinism_and_clone(seed in any::<i32>(), warmup in 0u32..50) {
        let mut a = Rng::new(seed);
        let mut b = Rng::new(seed);
        for _ in 0..warmup {
            prop_assert_eq!(a.next_int(0, 999), b.next_int(0, 999));
        }
        // A clone taken mid-stream equals the original and continues identically.
        let mut cloned = a.clone();
        prop_assert_eq!(&a, &cloned);
        for _ in 0..32 {
            prop_assert_eq!(a.next_double().to_bits(), cloned.next_double().to_bits());
        }
    }

    /// `shuffle` is a permutation (multiset-preserving) and deterministic for
    /// a given seed.
    #[test]
    fn shuffle_is_deterministic_permutation(
        seed in any::<i32>(),
        mut data in prop::collection::vec(any::<i32>(), 0..64),
    ) {
        let original = data.clone();
        let mut other = data.clone();

        let mut rng_a = Rng::new(seed);
        let mut rng_b = Rng::new(seed);
        rng_a.shuffle(&mut data);
        rng_b.shuffle(&mut other);

        // Same seed => identical result.
        prop_assert_eq!(&data, &other);

        // Multiset preserved: sorting both yields the original sorted.
        let mut sorted_in = original;
        let mut sorted_out = data;
        sorted_in.sort_unstable();
        sorted_out.sort_unstable();
        prop_assert_eq!(sorted_out, sorted_in);
    }

    /// `next_gaussian_int` never escapes its `[min, max]` clamp range, and is
    /// deterministic for a fixed seed.
    #[test]
    fn gaussian_int_stays_in_bounds(
        seed in any::<i32>(),
        min in -100i32..100,
        width in 0i32..200,
    ) {
        let max = min + width;
        let mean = f64::from(min + max) / 2.0;
        let mut rng = Rng::new(seed);
        let mut check = Rng::new(seed);
        for _ in 0..32 {
            let v = rng.next_gaussian_int(mean, 5.0, min, max);
            prop_assert!(v >= min && v <= max, "{v} not in [{min}, {max}]");
            // Deterministic for the same seed.
            prop_assert_eq!(v, check.next_gaussian_int(mean, 5.0, min, max));
        }
    }

    /// `fork` advances the parent by exactly one draw and yields a child whose
    /// stream is fully determined by the parent's state at fork time.
    #[test]
    fn fork_is_deterministic(seed in any::<i32>(), warmup in 0u32..20) {
        let mut parent_a = Rng::new(seed);
        let mut parent_b = Rng::new(seed);
        for _ in 0..warmup {
            parent_a.next_int(0, 50);
            parent_b.next_int(0, 50);
        }
        let before = parent_a.counter();
        let mut child_a = parent_a.fork();
        let mut child_b = parent_b.fork();
        // Fork consumes exactly one draw from the parent.
        prop_assert_eq!(parent_a.counter(), before + 1);
        // Children derived from identical parent state are identical.
        for _ in 0..16 {
            prop_assert_eq!(child_a.next_int(0, i32::MAX - 1), child_b.next_int(0, i32::MAX - 1));
        }
    }
}

// ---------------------------------------------------------------------------
// `DotNetRandom` — the legacy `System.Random(int)` port.
//
// `Rng` runs on `MegaRandom` since v0.107.1, so `DotNetRandom` survives only as
// the reference implementation of the pre-v0.107.1 stream (and for replaying
// v0.103.x captures). Its whole value is "byte-identical to `System.Random`",
// which nothing asserted: every golden vector in this crate pins `MegaRandom`.
//
// The vectors below were emitted by a .NET 9 console harness calling the BCL
// directly (`new Random(seed)`), which is the authority for this port — the C#
// game decomp never reimplements `System.Random`, it just constructs one. .NET
// keeps the seeded ctor on the legacy subtractive generator for compatibility
// (only the *seedless* ctor moved to xoshiro in .NET 6), so these values are
// the same ones the pre-v0.107.1 game produced.
// ---------------------------------------------------------------------------

use hegel::generators as gs;
use hegel::TestCase;
use sts2_core::DotNetRandom;

/// `Random.Next()` is `InternalSample()` verbatim. Eight draws per seed across
/// zero, ±small, a large seed and both `i32` extremes.
#[test]
fn dotnet_random_internal_sample_matches_system_random_oracle() {
    let cases: [(i32, [i32; 8]); 7] = [
        (
            0,
            [
                1_559_595_546,
                1_755_192_844,
                1_649_316_166,
                1_198_642_031,
                442_452_829,
                1_200_195_957,
                1_945_678_308,
                949_569_752,
            ],
        ),
        (
            1,
            [
                534_011_718,
                237_820_880,
                1_002_897_798,
                1_657_007_234,
                1_412_011_072,
                929_393_559,
                760_389_092,
                2_026_928_803,
            ],
        ),
        (
            42,
            [
                1_434_747_710,
                302_596_119,
                269_548_474,
                1_122_627_734,
                361_709_742,
                563_913_476,
                1_555_655_117,
                1_101_493_307,
            ],
        ),
        (
            -1,
            [
                534_011_718,
                237_820_880,
                1_002_897_798,
                1_657_007_234,
                1_412_011_072,
                929_393_559,
                760_389_092,
                2_026_928_803,
            ],
        ),
        (
            2026,
            [
                343_446_667,
                608_692_637,
                1_970_727_268,
                2_133_607_805,
                1_967_399_789,
                162_867_594,
                1_436_444_038,
                1_835_621_726,
            ],
        ),
        (
            i32::MAX,
            [
                1_559_595_546,
                1_755_192_844,
                1_649_316_172,
                1_198_642_031,
                442_452_829,
                1_200_195_955,
                1_945_678_308,
                949_569_752,
            ],
        ),
        (
            i32::MIN,
            [
                1_559_595_546,
                1_755_192_844,
                1_649_316_172,
                1_198_642_031,
                442_452_829,
                1_200_195_955,
                1_945_678_308,
                949_569_752,
            ],
        ),
    ];
    for (seed, expected) in cases {
        let mut rng = DotNetRandom::new(seed);
        let got: Vec<i32> = (0..8).map(|_| rng.internal_sample()).collect();
        assert_eq!(got, expected, "seed {seed}");
    }
}

/// `Random.NextDouble()` == `InternalSample() * (1.0 / int.MaxValue)`. Compared
/// on raw IEEE-754 bits so a last-ulp divergence in the scaling constant can't
/// hide behind an epsilon.
#[test]
fn dotnet_random_sample_matches_system_random_oracle() {
    let cases: [(i32, [u64; 4]); 2] = [
        (
            0,
            [
                0x3FE7_3D62_86AE_7AC5,
                0x3FEA_2787_8334_4F0F,
                0x3FE8_93A4_51B1_2749,
                0x3FE1_DC74_DBE3_B8EA,
            ],
        ),
        (
            42,
            [
                0x3FE5_6120_CFAA_C242,
                0x3FC2_0940_1724_1280,
                0x3FC0_10FB_BA20_21F7,
                0x3FE0_BA7C_25A1_74F8,
            ],
        ),
    ];
    for (seed, expected) in cases {
        let mut rng = DotNetRandom::new(seed);
        let got: Vec<u64> = (0..4).map(|_| rng.sample().to_bits()).collect();
        assert_eq!(got, expected, "seed {seed}");
    }
}

/// The large-range path. `Random.Next(min, max)` switches to
/// `GetSampleForLargeRange()` once `(long)max - min > int.MaxValue`, then folds
/// it back with `(int)((long)(sample * range) + min)`. Reproducing that fold
/// here pins the sign-flip branch and the `2*int.MaxValue - 1` divisor, which
/// no in-crate caller exercises any more.
#[test]
fn dotnet_random_large_range_matches_system_random_oracle() {
    const RANGE: f64 = (i32::MAX as i64 - i32::MIN as i64) as f64;
    let cases: [(i32, [i32; 6]); 3] = [
        (
            0,
            [
                -1_559_595_548,
                1_649_316_165,
                442_452_828,
                -1_945_678_310,
                2_099_272_108,
                626_863_972,
            ],
        ),
        (
            42,
            [
                1_434_747_709,
                -269_548_476,
                -361_709_744,
                1_555_655_116,
                -372_913_051,
                -503_774_880,
            ],
        ),
        (
            2026,
            [
                343_446_666,
                1_970_727_267,
                -1_967_399_791,
                -1_436_444_040,
                -1_347_728_080,
                1_949_820_434,
            ],
        ),
    ];
    for (seed, expected) in cases {
        let mut rng = DotNetRandom::new(seed);
        let got: Vec<i32> = (0..6)
            .map(|_| {
                let sample = rng.get_sample_for_large_range();
                ((sample * RANGE) as i64 + i64::from(i32::MIN)) as i32
            })
            .collect();
        assert_eq!(got, expected, "seed {seed}");
    }
}

/// `GetSampleForLargeRange` burns exactly two `InternalSample()` calls (value +
/// sign coin). State equality after the fact is the strongest form of that
/// claim available without exposing the seed array.
#[test]
fn dotnet_random_large_range_consumes_two_internal_samples() {
    let mut via_large = DotNetRandom::new(7);
    let _ = via_large.get_sample_for_large_range();

    let mut via_raw = DotNetRandom::new(7);
    let _ = via_raw.internal_sample();
    let _ = via_raw.internal_sample();

    assert_eq!(via_large, via_raw);
}

/// .NET seeds from `abs(seed)`, with `int.MinValue` folded to `int.MaxValue`
/// (its negation doesn't fit). Oracle-confirmed: `Random(-42)` and `Random(42)`
/// emit the same stream, as do `Random(int.MinValue)` and `Random(int.MaxValue)`
/// — while `Random(0)` stays distinct from the extremes.
#[test]
fn dotnet_random_seed_is_folded_by_abs() {
    assert_eq!(DotNetRandom::new(-42), DotNetRandom::new(42));
    assert_eq!(DotNetRandom::new(i32::MIN), DotNetRandom::new(i32::MAX));
    assert_ne!(DotNetRandom::new(0), DotNetRandom::new(i32::MAX));
}

/// `placeholder()` skips `initialize`, so its seed array is all zeroes and the
/// subtractive step can only ever yield `0 - 0`. Pinning the degenerate stream
/// documents *why* the doc comment says it must never be sampled: it doesn't
/// panic or drift, it silently returns a constant.
#[test]
fn dotnet_random_placeholder_yields_a_constant_zero_stream() {
    let mut rng = DotNetRandom::placeholder();
    for _ in 0..64 {
        assert_eq!(rng.internal_sample(), 0);
    }
    assert!((DotNetRandom::placeholder().sample() - 0.0).abs() < f64::EPSILON);
    // Sign coin sees 0 (even) => negate 0 => the midpoint of the mapped range.
    let mid = DotNetRandom::placeholder().get_sample_for_large_range();
    assert!((mid - 0.5).abs() < 1e-9, "got {mid}");
}

/// The constructor must uphold the .NET seed-array invariant (`0 <= x <=
/// int.MaxValue`), which is what keeps `internal_sample` inside `[0,
/// int.MaxValue)`. `int.MaxValue` itself is unreachable — the port decrements
/// it explicitly, matching .NET's "never return MBIG" rule.
#[hegel::test(test_cases = 200)]
fn dotnet_random_internal_sample_stays_in_31_bit_range(tc: TestCase) {
    let seed = tc.draw(gs::integers::<i32>());
    let mut rng = DotNetRandom::new(seed);
    for step in 0..96 {
        let v = rng.internal_sample();
        assert!(
            (0..i32::MAX).contains(&v),
            "seed={seed} step={step} value={v} escaped [0, i32::MAX)"
        );
    }
}

/// Both `[0, 1)` samplers stay in the unit interval on arbitrary seeds. The
/// large-range one is the interesting case: its sign flip makes the numerator
/// swing negative before the `+ (int.MaxValue - 1)` re-centres it.
#[hegel::test(test_cases = 200)]
fn dotnet_random_unit_interval_samplers_stay_in_range(tc: TestCase) {
    let seed = tc.draw(gs::integers::<i32>());
    let mut rng = DotNetRandom::new(seed);
    for _ in 0..32 {
        let s = rng.sample();
        assert!(
            (0.0..1.0).contains(&s),
            "sample {s} escaped [0, 1) (seed {seed})"
        );
        let l = rng.get_sample_for_large_range();
        assert!(
            (0.0..1.0).contains(&l),
            "large-range sample {l} escaped [0, 1) (seed {seed})"
        );
    }
}

/// Same seed, same stream. This is the contract every
/// replay/determinization caller leans on.
#[hegel::test(test_cases = 200)]
fn dotnet_random_is_deterministic_per_seed(tc: TestCase) {
    let seed = tc.draw(gs::integers::<i32>());
    let mut a = DotNetRandom::new(seed);
    let mut b = DotNetRandom::new(seed);
    for _ in 0..48 {
        assert_eq!(a.internal_sample(), b.internal_sample());
    }
    assert_eq!(a, b);
}

/// The abs-fold as a property: negating a seed never changes the stream.
/// `i32::MIN` is the one input whose negation overflows, and the port routes it
/// through an explicit branch to `int.MaxValue` rather than `saturating_abs`.
#[hegel::test(test_cases = 200)]
fn dotnet_random_negated_seed_gives_identical_stream(tc: TestCase) {
    let seed = tc.draw(gs::integers::<i32>().min_value(i32::MIN + 1));
    assert_eq!(DotNetRandom::new(seed), DotNetRandom::new(-seed));
}
