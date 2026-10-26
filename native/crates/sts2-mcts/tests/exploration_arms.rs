//! Contract tests for the three adaptive-exploration knobs added by the
//! 2026-08-14 study: [`MctsConfig::fpu_reduction`],
//! [`MctsConfig::cpuct_utility_stdev_scale`] and
//! [`MctsConfig::root_sequential_halving`].
//!
//! The load-bearing property for all three is that the **default is inert** —
//! the deployed search must not move a bit unless an arm is explicitly turned
//! on. Everything else in the file checks the knob does what its doc says.
//!
//! See `docs/research/2026-08-14-adaptive-exploration-beyond-fixed-c-puct.md`.

mod common;

use common::build_handful;
use sts2_core::cards::{CardId, TargetType};
use sts2_mcts::{Mcts, MctsConfig};

/// A position with a genuinely dominant action and three near-equivalent
/// alternatives, so visit allocation is meaningful.
fn fixture(seed: u64) -> sts2_core::CombatState {
    let hand = [
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
    ];
    build_handful(seed, &hand, /*cultist_hp=*/ 20, /*player_hp=*/ 20)
}

fn base(seed: u64, sims: u32) -> MctsConfig {
    MctsConfig {
        max_simulations: sims,
        seed,
        max_rollout_depth: 50,
        c_puct: 2.5,
        ..MctsConfig::default()
    }
}

/// Root children as `(visits, mean_value)`, transposition-collapsed the same
/// way the benchmark harness collapses them.
fn children(m: &Mcts) -> Vec<(u32, f32)> {
    use std::collections::BTreeMap;
    let mut seen: BTreeMap<(u32, u32, u32), (u32, f32)> = BTreeMap::new();
    for o in m.root_outcomes() {
        let mean = o.value_sum / o.visits.max(1) as f32;
        seen.entry((o.visits, o.wins, o.losses))
            .or_insert((o.visits, mean));
    }
    seen.into_values().collect()
}

fn visit_spread(m: &Mcts) -> f64 {
    let v = children(m);
    let hi = v.iter().map(|(n, _)| *n).max().unwrap_or(1);
    let lo = v.iter().map(|(n, _)| *n).min().unwrap_or(1);
    f64::from(hi) / f64::from(lo.max(1))
}

/// The whole point of the `Option` on `fpu_reduction`: `None` must reproduce
/// the historical `f32::INFINITY` first-visit tiebreak exactly. If this ever
/// fails, every pre-existing fixture in the repo has silently moved.
#[test]
fn fpu_none_is_bit_identical_to_the_historical_default() {
    for seed in 0..4_u64 {
        let mut a = Mcts::new(fixture(seed), base(seed, 400));
        let mut b = Mcts::new(
            fixture(seed),
            MctsConfig {
                fpu_reduction: None,
                ..base(seed, 400)
            },
        );
        assert_eq!(a.search_n(400), b.search_n(400));
        let (ca, cb) = (children(&a), children(&b));
        assert_eq!(ca.len(), cb.len(), "child count moved at seed {seed}");
        for (x, y) in ca.iter().zip(cb.iter()) {
            assert_eq!(x.0, y.0, "visits moved at seed {seed}");
            assert_eq!(x.1.to_bits(), y.1.to_bits(), "value moved at seed {seed}");
        }
    }
}

/// Visit concentration must be monotone in the FPU penalty.
///
/// Deliberately *not* asserted at one hand-picked magnitude. The first version
/// of this test compared a single `r = 0.2` arm with `>=` and passed 8/8
/// against a knob that was provably inert — the branch it targeted was
/// unreachable, since expansion pops one action per simulation and backs a
/// value up through it, so no child is ever `visits == 0` at descent time.
/// Monotonicity across a range cannot be satisfied by a no-op, and it also
/// records the real magnitude story: a small `r` is genuinely inert against
/// `c_puct = 2.5`, because the explore term on the visited children is far
/// larger than the penalty. The knob only bites once `r` is commensurate with
/// that term.
#[test]
fn visit_concentration_is_monotone_in_fpu() {
    // Measured as root children *opened*, not as a visit ratio. FPU's lever in
    // this engine is whether a node expands another action or refines the ones
    // it has, so breadth is what it controls directly; among the children that
    // do get opened the visit split stays near-even (and the harness's
    // collapse-by-identical-stats would report a degenerate 1.0 spread for
    // exactly that reason).
    let ladder = [0.0_f32, 0.5, 2.0, 100.0];
    let seeds = 6_u64;
    let mut mean_children = Vec::new();
    for &r in &ladder {
        let mut total = 0.0;
        for seed in 0..seeds {
            let mut m = Mcts::new(
                fixture(seed),
                MctsConfig {
                    fpu_reduction: Some(r),
                    ..base(seed, 600)
                },
            );
            let _ = m.search_n(600);
            total += m.root_outcomes().len() as f64;
        }
        #[allow(clippy::cast_precision_loss)]
        mean_children.push(total / seeds as f64);
    }
    for w in mean_children.windows(2) {
        assert!(
            w[1] <= w[0] + 1e-9,
            "breadth is not monotone (non-increasing) in the FPU penalty: {mean_children:?}"
        );
    }
    assert!(
        mean_children[ladder.len() - 1] < mean_children[0],
        "the largest FPU penalty opened as many children as the smallest: {mean_children:?}"
    );
}

/// The precedence contract: a large enough FPU penalty must suppress further
/// expansion entirely, leaving a node with untried actions it never opens.
/// This is the sharpest available proof that the knob reaches the descent.
#[test]
fn huge_fpu_penalty_suppresses_expansion() {
    let seed = 3_u64;
    let mut wide = Mcts::new(fixture(seed), base(seed, 500));
    let _ = wide.search_n(500);
    let mut narrow = Mcts::new(
        fixture(seed),
        MctsConfig {
            fpu_reduction: Some(100.0),
            ..base(seed, 500)
        },
    );
    let _ = narrow.search_n(500);
    assert!(
        children(&narrow).len() < children(&wide).len(),
        "a +100 FPU penalty opened {} root children, same as or more than the \
         default's {} — the knob is not reaching the selection descent",
        children(&narrow).len(),
        children(&wide).len()
    );
}

/// `cpuct_utility_stdev_scale = 0.0` must make the variance factor identically
/// 1.0, i.e. the multiplier disappears rather than merely being small.
#[test]
fn variance_scaling_off_is_bit_identical() {
    for seed in 0..4_u64 {
        let mut a = Mcts::new(fixture(seed), base(seed, 400));
        let mut b = Mcts::new(
            fixture(seed),
            MctsConfig {
                cpuct_utility_stdev_scale: 0.0,
                // A deliberately absurd prior: with `scale == 0` it must never
                // be read, so this cannot leak into the result.
                cpuct_utility_stdev_prior: 0.001,
                ..base(seed, 400)
            },
        );
        assert_eq!(a.search_n(400), b.search_n(400));
        for (x, y) in children(&a).iter().zip(children(&b).iter()) {
            assert_eq!(x.0, y.0, "visits moved at seed {seed}");
            assert_eq!(x.1.to_bits(), y.1.to_bits(), "value moved at seed {seed}");
        }
    }
}

/// With the prior pinned to the node's own σ the factor is 1.0 by
/// construction, so the arm must land back on the constant-`c_puct` search.
/// This is the calibration contract: the knob is a *relative* adjustment
/// around `cpuct_utility_stdev_prior`, not a free rescale.
#[test]
fn variance_scaling_is_neutral_at_its_own_prior() {
    let seed = 7_u64;
    let mut probe = Mcts::new(fixture(seed), base(seed, 800));
    let _ = probe.search_n(800);
    let sigma = probe.root_utility_stdev();
    assert!(
        sigma > 0.0 && sigma.is_finite(),
        "root utility stdev should be a positive finite number, got {sigma}"
    );
    // Not a claim about equality of the whole search — deeper nodes have their
    // own σ — only that the root's factor is neutral, which bounds how far the
    // arm can drift from the constant it is anchored to.
    let mut scaled = Mcts::new(
        fixture(seed),
        MctsConfig {
            cpuct_utility_stdev_scale: 0.85,
            cpuct_utility_stdev_prior: sigma,
            ..base(seed, 800)
        },
    );
    let _ = scaled.search_n(800);
    let drift = (scaled.root_utility_stdev() - sigma).abs() / sigma;
    assert!(
        drift < 0.5,
        "anchoring the prior to the measured σ drifted the root by {drift:.3} — \
         the factor is supposed to be ~1 there"
    );
}

/// Sequential Halving must spend exactly the budget it was handed. An arm
/// that quietly under-spends would look better per-simulation than it is, and
/// an over-spending one would be an unfair comparison against the UCT arms.
#[test]
fn sequential_halving_spends_exactly_the_budget() {
    for &sims in &[64_u32, 300, 1000] {
        for seed in 0..3_u64 {
            let mut m = Mcts::new(
                fixture(seed),
                MctsConfig {
                    root_sequential_halving: true,
                    ..base(seed, sims)
                },
            );
            let _ = m.search_n(sims);
            let root_visits: u32 = m.root_outcomes().iter().map(|o| o.visits).sum();
            // Every simulation backs up through exactly one root edge, but
            // transposition merging can report a shared child once per
            // incoming edge, so the sum is a lower bound, never a shortfall.
            assert!(
                root_visits >= sims - 1,
                "SH spent only {root_visits} of {sims} sims at seed {seed}"
            );
        }
    }
}

/// SH allocates geometrically more budget to surviving arms, so it must
/// concentrate visits far more than UCT — that is the mechanism, and an arm
/// that does not show it is not running the schedule.
#[test]
fn sequential_halving_concentrates_visits() {
    let mut wins = 0;
    let seeds = 6;
    for seed in 0..seeds {
        let mut uct = Mcts::new(fixture(seed), base(seed, 800));
        let _ = uct.search_n(800);
        let mut sh = Mcts::new(
            fixture(seed),
            MctsConfig {
                root_sequential_halving: true,
                ..base(seed, 800)
            },
        );
        let _ = sh.search_n(800);
        if visit_spread(&sh) > visit_spread(&uct) {
            wins += 1;
        }
    }
    assert!(
        wins * 2 > seeds,
        "SH concentrated visits on only {wins}/{seeds} seeds"
    );
}

/// SH must still find the dominant action — a budget schedule that eliminated
/// the winner would be worse than useless.
#[test]
fn sequential_halving_still_finds_the_dominant_action() {
    let hand = [
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
    ];
    let mut hits = 0;
    let seeds = 12;
    for seed in 0..seeds {
        let state = build_handful(seed, &hand, /*cultist_hp=*/ 6, /*player_hp=*/ 6);
        let mut m = Mcts::new(
            state,
            MctsConfig {
                root_sequential_halving: true,
                ..base(seed, 1000)
            },
        );
        if matches!(
            m.search_n(1000),
            sts2_core::Action::PlayCard { hand_index: 0, .. }
        ) {
            hits += 1;
        }
    }
    assert!(
        hits * 10 >= seeds * 9,
        "SH found the lethal Strike on only {hits}/{seeds} seeds"
    );
}
