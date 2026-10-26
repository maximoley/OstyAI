use rand::{rngs::StdRng, RngCore, SeedableRng};

use super::splitmix64;
use crate::deck_archetype::Archetype;

// ── Per-sample parameter derivation ──────────────────────────────────────────

/// Hyperparameter set derived deterministically per `(run_seed, sample_seed)`
/// pair. Used by the deckbuilding oracle to diversify sampled card-pick
/// policies without any shared mutable state.
///
/// - `temperature` — softmax temperature for card-pick decisions (> 0).
/// - `skip_bias`   — additive score offset applied to "skip" options.
/// - `archetype`   — coarse target archetype for bias regression.
/// - `bias_strength` — weight applied to archetype-alignment bonuses.
///   Fixed at `0.5` across all samples (a sensible middle ground between
///   no archetype pressure and overriding all other signals).
#[derive(Clone, Debug, PartialEq)]
pub struct SampleParams {
    pub temperature: f64,
    pub skip_bias: f64,
    pub archetype: Archetype,
    pub bias_strength: f64,
}

/// Fixed archetype-bias weight. Kept as a named constant so callers don't
/// need magic numbers and it's trivially adjustable if a future A/B warrants
/// it. See [`SampleParams::bias_strength`].
pub const DEFAULT_BIAS_STRENGTH: f64 = 0.5;

/// Salt constants — distinct primes mixed before each `splitmix64` call to
/// derive three independent 64-bit streams from a single `(run_seed, sample_seed)`
/// pair. Must never collide; documented here so reviewers can verify independence.
const SALT_TEMPERATURE: u64 = 0x1111_1111_1111_1111;
const SALT_SKIP_BIAS: u64 = 0x2222_2222_2222_2222;
const SALT_ARCHETYPE: u64 = 0x3333_3333_3333_3333;
const SALT_RNG: u64 = 0xAAAA_AAAA_AAAA_AAAA;

/// Combine `run_seed` (u64) and `sample_seed` (u64) into a single 64-bit
/// value using the same XOR-mixing convention as `MctsCombatPolicy::mcts_pick`
/// (mod.rs ~:202): `seed_bits ^ (sample_seed+1) · φ64`.
fn base_mix(run_seed: u64, sample_seed: u64) -> u64 {
    run_seed
        ^ sample_seed
            .wrapping_add(1)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// Deterministically derive a [`SampleParams`] from `(run_seed, sample_seed)`.
///
/// Each field is drawn from an independent splitmix64 stream seeded with a
/// distinct salt:
///
/// | Field       | Range         | Stream salt |
/// |-------------|---------------|-------------|
/// | temperature | [0.3, 2.0]   | `SALT_TEMPERATURE` |
/// | skip_bias   | [-0.2, +0.4] | `SALT_SKIP_BIAS`   |
/// | archetype   | uniform over `Archetype::ALL` | `SALT_ARCHETYPE` |
/// | bias_strength | constant `DEFAULT_BIAS_STRENGTH` | — |
#[must_use]
pub fn sample_params_for(run_seed: u64, sample_seed: u64) -> SampleParams {
    let base = base_mix(run_seed, sample_seed);

    // Temperature: map a u64 to [0.0, 1.0) then scale to [0.3, 2.0].
    let t_bits = splitmix64(base ^ SALT_TEMPERATURE);
    let t_unit = (t_bits >> 11) as f64 * (1.0 / (1u64 << 53) as f64);
    let temperature = 0.3 + t_unit * (2.0 - 0.3);

    // skip_bias: map to [-0.2, +0.4] (width 0.6).
    let b_bits = splitmix64(base ^ SALT_SKIP_BIAS);
    let b_unit = (b_bits >> 11) as f64 * (1.0 / (1u64 << 53) as f64);
    let skip_bias = -0.2 + b_unit * 0.6;

    // Archetype: uniform index into Archetype::ALL.
    let a_bits = splitmix64(base ^ SALT_ARCHETYPE);
    let archetype = Archetype::ALL[(a_bits as usize) % Archetype::ALL.len()];

    SampleParams {
        temperature,
        skip_bias,
        archetype,
        bias_strength: DEFAULT_BIAS_STRENGTH,
    }
}

/// Construct a deterministic [`StdRng`] seeded from `(run_seed, sample_seed)`.
///
/// The seed is derived via `splitmix64(base_mix(...) ^ SALT_RNG)` so the RNG
/// stream is independent of the parameter streams produced by
/// [`sample_params_for`] for the same inputs.
#[must_use]
pub fn policy_rng(run_seed: u64, sample_seed: u64) -> StdRng {
    let seed = splitmix64(base_mix(run_seed, sample_seed) ^ SALT_RNG);
    StdRng::seed_from_u64(seed)
}

// ── Softmax sampling ──────────────────────────────────────────────────────────

/// Sample an index from `scores` using a softmax distribution at the given
/// `temperature`.
///
/// - `temperature <= 0.0`: deterministic argmax; ties broken to the **lowest**
///   index (matches [`crate::policy::surfaces::picks::argmax`] semantics).
/// - Non-finite scores (`NEG_INFINITY`, `NaN`) are assigned weight 0 and are
///   never selected.
/// - Numerically stable: subtracts the max finite score before exponentiating.
/// - If all scores are non-finite (no valid choice), returns index 0 as a safe
///   fallback instead of panicking. Callers should ensure the dispatch is
///   correct; this guard prevents full-run crashes from mis-routed preempts.
///
/// # Panics
///
/// Panics only if `scores` is empty.
pub fn softmax_sample(scores: &[f64], temperature: f64, rng: &mut impl RngCore) -> usize {
    assert!(!scores.is_empty(), "softmax_sample: empty scores");

    // temperature <= 0 → deterministic argmax (lowest index wins ties)
    if temperature <= 0.0 {
        let mut best: Option<(usize, f64)> = None;
        for (i, &s) in scores.iter().enumerate() {
            if !s.is_finite() {
                continue;
            }
            let better = match best {
                None => true,
                Some((_, b)) => s > b,
            };
            if better {
                best = Some((i, s));
            }
        }
        // All-non-finite at temperature=0: safe fallback to index 0.
        return best.map_or(0, |(i, _)| i);
    }

    // Find max finite score for numerical stability
    let max_finite = scores
        .iter()
        .copied()
        .filter(|s| s.is_finite())
        .fold(f64::NEG_INFINITY, f64::max);
    // All-non-finite: no valid choice — return index 0 as a safe fallback.
    // (The dispatch fix in SampledDeckPolicy::choose should prevent this for
    // all known preempt surfaces, but this guards any future sampler arm.)
    if !max_finite.is_finite() {
        return 0;
    }

    // Compute unnormalized weights: exp((score - max) / temperature)
    // Non-finite scores get weight 0.
    let weights: Vec<f64> = scores
        .iter()
        .map(|&s| {
            if s.is_finite() {
                ((s - max_finite) / temperature).exp()
            } else {
                0.0
            }
        })
        .collect();

    let total: f64 = weights.iter().sum();
    assert!(total > 0.0, "softmax_sample: all weights are zero");

    // Inverse-CDF sampling via cumulative sum
    // Draw u in [0, total)
    let u = {
        // rand 0.8: next_u64 gives uniform u64; convert to f64 in [0,1)
        let bits = rng.next_u64();
        let unit = (bits >> 11) as f64 * (1.0 / (1u64 << 53) as f64);
        unit * total
    };

    let mut cumsum = 0.0;
    for (i, &w) in weights.iter().enumerate() {
        cumsum += w;
        if u < cumsum {
            return i;
        }
    }
    // Fallback: floating-point edge case — return last non-zero weight index
    weights
        .iter()
        .rposition(|&w| w > 0.0)
        .expect("softmax_sample: no positive weight")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, RngCore, SeedableRng};

    // ── Task 1.2 tests (write RED first, then implement) ─────────────────

    #[test]
    fn beta_sampling_mix_keeps_high_seed_bits() {
        let high = 0xfedc_ba98_7654_3210_u64;
        assert_ne!(base_mix(high, 0), base_mix(u64::from(high as u32), 0));
        assert_eq!(base_mix(high, u64::MAX), high);
    }

    /// Same `(run_seed, sample_seed)` must produce identical `SampleParams`.
    ///
    /// This test asserts bit-exact reproducibility of deterministic derivation,
    /// not approximate floating-point equality — hence the `float_cmp` allow.
    #[allow(clippy::float_cmp)]
    #[test]
    fn sample_params_deterministic() {
        let p1 = sample_params_for(42, 7);
        let p2 = sample_params_for(42, 7);
        assert!(
            p1.temperature == p2.temperature,
            "temperature not bit-exact: {} vs {}",
            p1.temperature,
            p2.temperature
        );
        assert!(
            p1.skip_bias == p2.skip_bias,
            "skip_bias not bit-exact: {} vs {}",
            p1.skip_bias,
            p2.skip_bias
        );
        assert_eq!(p1.archetype, p2.archetype);
        assert!(
            p1.bias_strength == p2.bias_strength,
            "bias_strength not bit-exact"
        );
    }

    /// Different `sample_seed` must yield different params for most seeds.
    /// We test 20 pairs and require at least 15 differ (extreme coincidence
    /// would be a distribution bug, not bad luck).
    #[test]
    fn sample_params_diverse_across_seeds() {
        let run_seed = 1;
        let params: Vec<SampleParams> =
            (0u64..20).map(|s| sample_params_for(run_seed, s)).collect();
        let different = params
            .windows(2)
            .filter(|w| {
                (w[0].temperature - w[1].temperature).abs() > 1e-12
                    || (w[0].skip_bias - w[1].skip_bias).abs() > 1e-12
                    || w[0].archetype != w[1].archetype
            })
            .count();
        assert!(
            different >= 15,
            "only {different}/19 consecutive pairs differed — params not diverse"
        );
    }

    /// Temperature must lie in [0.3, 2.0] for all sample seeds.
    #[test]
    fn sample_params_temperature_in_range() {
        for s in 0u64..50 {
            let p = sample_params_for(99, s);
            assert!(
                p.temperature >= 0.3 && p.temperature <= 2.0,
                "temperature {t} out of [0.3,2.0] for sample_seed={s}",
                t = p.temperature
            );
        }
    }

    /// skip_bias must lie in [-0.2, +0.4] for all sample seeds.
    #[test]
    fn sample_params_skip_bias_in_range() {
        for s in 0u64..50 {
            let p = sample_params_for(99, s);
            assert!(
                p.skip_bias >= -0.2 && p.skip_bias <= 0.4,
                "skip_bias {b} out of [-0.2,0.4] for sample_seed={s}",
                b = p.skip_bias
            );
        }
    }

    /// `policy_rng` is deterministic: same inputs → same first 5 draws.
    #[test]
    fn policy_rng_deterministic() {
        let mut rng1 = policy_rng(10, 3);
        let mut rng2 = policy_rng(10, 3);
        for _ in 0..5 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    /// Different `sample_seed` → different RNG streams (checked on first draw).
    #[test]
    fn policy_rng_differs_by_sample_seed() {
        let mut rng_a = policy_rng(10, 0);
        let mut rng_b = policy_rng(10, 1);
        let draws_a: Vec<u64> = (0..5).map(|_| rng_a.next_u64()).collect();
        let draws_b: Vec<u64> = (0..5).map(|_| rng_b.next_u64()).collect();
        assert_ne!(
            draws_a, draws_b,
            "different sample_seed must yield different RNG stream"
        );
    }

    // ── Existing softmax_sample tests ─────────────────────────────────────

    #[test]
    fn zero_temperature_is_argmax() {
        let mut rng = StdRng::seed_from_u64(1);
        assert_eq!(softmax_sample(&[0.1, 0.9, 0.3], 0.0, &mut rng), 1);
    }

    #[test]
    fn high_temperature_eventually_picks_each() {
        let mut rng = StdRng::seed_from_u64(7);
        let mut seen = [false; 3];
        for _ in 0..1000 {
            seen[softmax_sample(&[0.1, 0.2, 0.15], 5.0, &mut rng)] = true;
        }
        assert!(seen.iter().all(|&b| b));
    }

    #[test]
    fn non_finite_never_sampled() {
        let mut rng = StdRng::seed_from_u64(3);
        for _ in 0..200 {
            assert_ne!(softmax_sample(&[f64::NEG_INFINITY, 1.0], 2.0, &mut rng), 0);
        }
    }

    #[test]
    fn equal_scores_break_to_lowest_index() {
        // Tied scores at temperature 0 must take the argmax branch and
        // break ties to the LOWEST index (picks::argmax semantics). A naive
        // argmax using `>=` instead of `>` would return the highest index (1).
        let mut rng = StdRng::seed_from_u64(11);
        assert_eq!(softmax_sample(&[0.5, 0.5], 0.0, &mut rng), 0);
    }

    #[test]
    fn negative_temperature_is_argmax() {
        // temperature < 0 must take the argmax branch (the guard is `<= 0.0`,
        // not `== 0.0`). A naive `if temperature == 0.0` guard would fall
        // through to the softmax path and divide by a negative temperature,
        // inverting the distribution and not deterministically returning 1.
        let mut rng = StdRng::seed_from_u64(13);
        assert_eq!(softmax_sample(&[0.1, 0.9, 0.3], -1.0, &mut rng), 1);
    }

    /// All-non-finite scores return index 0 (safe fallback, never panics).
    /// This guards the defensive choke-point fix: any future sampler arm that
    /// builds an all-NEG_INFINITY score vec (e.g. a mis-routed preempt) must
    /// return 0 rather than crashing the full-run engine.
    #[test]
    fn all_non_finite_returns_zero() {
        let mut rng = StdRng::seed_from_u64(99);
        assert_eq!(
            softmax_sample(&[f64::NEG_INFINITY, f64::NEG_INFINITY], 1.0, &mut rng),
            0,
            "all-non-finite scores must return index 0 (safe fallback)"
        );
        // Also at temperature 0.
        assert_eq!(
            softmax_sample(&[f64::NEG_INFINITY, f64::NEG_INFINITY], 0.0, &mut rng),
            0,
            "all-non-finite at temperature=0 must return index 0"
        );
    }
}
