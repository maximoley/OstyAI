//! UCT-over-a-4-action-toy-state convergence test (bead sts2-rs-69p
//! acceptance criterion).
//!
//! Constructs a hand with one clearly dominant action (a one-shot
//! 50-damage Strike that finishes the 10-HP Cultist) plus three
//! suboptimal alternatives (no-damage Defends and a low-damage Strike).
//! All four are legal under the energy budget. Within 1000 simulations
//! MCTS must pick the killing Strike on at least 95% of 20 seeds.

mod common;

use common::build_handful;
use sts2_core::cards::{CardId, TargetType};
use sts2_core::Action;
use sts2_mcts::{Mcts, MctsConfig, RolloutPolicyKind};

#[test]
fn uct_converges_to_dominant_action() {
    // Hand: idx0 = Strike (6 dmg — the real dispatch deals 6), idx1/2/3 =
    // Defends. Cultist@6 HP with Attack(6) intent + player@6 HP means
    // Strike kills + wins, anything else lets the cultist kill the player
    // on the next enemy turn. Four hand actions + EndTurn = 5 root edges,
    // matching the "4-action toy state" bead wording (the EndTurn edge is
    // also dominated by Strike).
    let hand = [
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
    ];

    let mut hits = 0;
    let seeds: Vec<u64> = (0..20).collect();
    for &seed in &seeds {
        let state = build_handful(seed, &hand, /*cultist_hp=*/ 6, /*player_hp=*/ 6);
        // Pinned to the pre-`sts2-rs-dfv1` Uniform rollout policy: this
        // test's convergence expectations were calibrated against it.
        let cfg = MctsConfig {
            max_simulations: 1000,
            seed,
            rollout_policy: RolloutPolicyKind::Uniform,
            ..MctsConfig::default()
        };
        let mut mcts = Mcts::new(state, cfg);
        let best = mcts.search();
        // The dominant choice is "play hand index 0 targeting enemy 0".
        if matches!(
            best,
            Action::PlayCard {
                hand_index: 0,
                target: Some(0),
            }
        ) {
            hits += 1;
        }
    }

    assert!(
        hits >= 19,
        "MCTS must pick dominant action ≥95% of 20 seeds; got {hits}/20"
    );
}

#[test]
fn root_visits_concentrates_on_dominant() {
    let hand = [
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
    ];
    let state = build_handful(0, &hand, 6, 6);
    // Pinned to Uniform rollout — see test header above.
    let cfg = MctsConfig {
        max_simulations: 1000,
        seed: 0,
        rollout_policy: RolloutPolicyKind::Uniform,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state, cfg);
    let _ = mcts.search();
    let visits = mcts.root_visits();
    // The killing-Strike child should have collected the plurality of visits.
    let max_v = visits.iter().map(|(_, v, _)| *v).max().unwrap_or(0);
    let total: u32 = visits.iter().map(|(_, v, _)| *v).sum();
    assert!(
        max_v * 2 >= total,
        "most-visited action should hold ≥50% of root visits; max={max_v} total={total}",
    );
}

/// The killing-Strike child should accumulate real wins (not just visits)
/// once rollouts start landing on player-victory terminals. Sanity check
/// for the bucket-counter wiring.
#[test]
fn dominant_action_collects_wins() {
    let hand = [
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
    ];
    let state = build_handful(0, &hand, 6, 6);
    // Pinned to Uniform rollout — see test header above.
    let cfg = MctsConfig {
        max_simulations: 1000,
        seed: 0,
        rollout_policy: RolloutPolicyKind::Uniform,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state, cfg);
    let _ = mcts.search();
    let outcomes = mcts.root_outcomes();
    let dominant = outcomes
        .iter()
        .find(|o| {
            matches!(
                o.action,
                Action::PlayCard {
                    hand_index: 0,
                    target: Some(0),
                }
            )
        })
        .expect("dominant Strike action should be a root child");
    assert_eq!(
        dominant.wins + dominant.losses + dominant.ties,
        dominant.visits,
    );
    assert!(
        dominant.wins > 0,
        "dominant action should record at least one win; got wins={} losses={} ties={}",
        dominant.wins,
        dominant.losses,
        dominant.ties,
    );
    // Half-loose: dominant action's win rate must clear simple majority.
    assert!(
        dominant.wins * 2 > dominant.visits,
        "dominant action should win majority of its rollouts; got {}/{}",
        dominant.wins,
        dominant.visits,
    );
}

/// Bead `sts2-rs-nkf9`: with the Heuristic rollout policy the dominant
/// action's `mean_value` should converge to a stable Q across seeds at
/// a low simulation budget. The Uniform-policy baseline above needs
/// ~1000 sims to find the killer Strike — with priority-weighted
/// rollouts the Strike dominates within a few hundred sims, so its Q
/// should be tight across seeds.
///
/// We assert two things:
///   * the most-visited root child is the dominant Strike on every
///     seed (no flakes at sims=300 under Heuristic);
///   * the standard deviation of the dominant child's `mean_value`
///     across seeds is below a generous threshold — this guards
///     against a future tuning pass that destabilises the Q signal
///     (e.g. by widening softmax temperature or relaxing dedup).
#[test]
fn heuristic_low_sims_q_stability() {
    use sts2_mcts::RootOutcome;

    let hand = [
        (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
        (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
    ];

    let seeds: [u64; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
    let mut q_dominant: Vec<f32> = Vec::with_capacity(seeds.len());
    let mut hits = 0u32;

    for &seed in &seeds {
        let state = build_handful(seed, &hand, /*cultist_hp=*/ 6, /*player_hp=*/ 6);
        let cfg = MctsConfig {
            max_simulations: 300,
            seed,
            rollout_policy: RolloutPolicyKind::Heuristic,
            ..MctsConfig::default()
        };
        let mut mcts = Mcts::new(state, cfg);
        let best = mcts.search();
        if matches!(
            best,
            Action::PlayCard {
                hand_index: 0,
                target: Some(0),
            }
        ) {
            hits += 1;
        }
        let outs = mcts.root_outcomes();
        let dominant: &RootOutcome = outs
            .iter()
            .find(|o| {
                matches!(
                    o.action,
                    Action::PlayCard {
                        hand_index: 0,
                        target: Some(0),
                    }
                )
            })
            .expect("dominant Strike must be a root child");
        q_dominant.push(dominant.mean_value);
    }

    assert!(
        hits == seeds.len() as u32,
        "Heuristic@300 must pick dominant Strike on every seed; got {hits}/{}",
        seeds.len()
    );

    // Sample mean / stddev of dominant Q across seeds.
    #[allow(clippy::cast_precision_loss)]
    let n = q_dominant.len() as f32;
    let mean: f32 = q_dominant.iter().sum::<f32>() / n;
    let var: f32 = q_dominant.iter().map(|q| (q - mean).powi(2)).sum::<f32>() / n;
    let stddev = var.sqrt();
    // The dominant Strike wins outright in this hand-rigged scenario,
    // so its mean_value should sit near +1.0 with tiny variance under
    // a sharp heuristic policy. Threshold loose enough to absorb
    // softmax-tau retunes while still flagging a regression to the
    // pre-dfv1 ~0.3 stddev band.
    assert!(
        stddev < 0.10,
        "Heuristic dominant-Q stddev across seeds must be <0.10; got mean={mean:.3} stddev={stddev:.3} samples={q_dominant:?}"
    );
}
