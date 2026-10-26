//! E3/E4/E5 calibration harnesses (full-run-AI plan §6; bead
//! `sts2-rs-40ikw.6`).
//!
//! Collects card-shaped decision points from self-play, then re-scores
//! every option under multiple probe configurations against a reference
//! configuration:
//!
//! - **E3 (budget fidelity):** Kendall τ of per-point option rankings
//!   under cheap sim budgets vs a strong full-fight reference.
//! - **E4 (label noise vs K):** flip rate of the preferred option at
//!   K ∈ {…} vs a high-K reference.
//! - **E5 (truncation validity):** Kendall τ for horizon T ∈ {…} vs a
//!   full-fight reference.
//!
//! All knobs are caller-tunable so a smoke run is cheap and the full
//! calibration is a flag away (`run_arena calibrate`).

use rayon::prelude::*;
use serde::Serialize;

use sts2_core::full_run::{drive_full_run, FullRunConfig, RunPolicy};
use sts2_core::potions::CharacterId;
use sts2_core::run_manager::{RunAction, RunManager};

use crate::policy::{CombatBudget, HeuristicPolicy, MctsCombatPolicy};
use crate::rollout::{
    candidates_for_surface, detect_card_surface, fight_delta, CandidateMutation, EncounterPlan,
    FightCache, ProbeBudget,
};
use crate::view::VisibleRunView;

/// One frozen decision point: the cloned manager + the shared
/// encounter plan + the option list.
#[derive(Debug)]
pub struct DecisionPoint {
    pub rm: RunManager,
    pub plan: EncounterPlan,
    pub candidates: Vec<CandidateMutation>,
    pub seed: u64,
    pub step: u64,
}

/// Policy wrapper that snapshots card-shaped decisions while the inner
/// policy drives.
struct CollectingPolicy<P> {
    inner: P,
    seed: u64,
    step: u64,
    out: Vec<DecisionPoint>,
}

impl<P: RunPolicy> RunPolicy for CollectingPolicy<P> {
    fn prepare(&mut self, rm: &mut RunManager) {
        // Forward the per-decision hook (see LabelingPolicy for rationale);
        // no-op for inner policies that don't override `prepare`.
        self.inner.prepare(rm);
    }

    fn choose(&mut self, rm: &RunManager, actions: &[RunAction]) -> Option<usize> {
        let step = self.step;
        self.step += 1;
        if let Some(surface) = detect_card_surface(rm, actions) {
            let view = VisibleRunView::from_run_manager(rm);
            let candidates = candidates_for_surface(rm, &view, surface, actions);
            if candidates.len() >= 2 {
                let key = step ^ (rm.seed.rotate_left(24));
                let plan = EncounterPlan::for_decision(rm, key);
                if plan.elite.is_some() || plan.boss.is_some() {
                    self.out.push(DecisionPoint {
                        rm: rm.clone(),
                        plan,
                        candidates: candidates.into_iter().map(|(c, _)| c).collect(),
                        seed: self.seed,
                        step,
                    });
                }
            }
        }
        self.inner.choose(rm, actions)
    }
}

/// Collect up to `max_points` card-shaped decision points from
/// heuristic self-play over `seeds` (combat budget optional — `None`
/// keeps collection cheap; points then skew early-run, which is fine
/// for plumbing smokes but the full calibration should pass a real
/// budget).
#[must_use]
pub fn collect_decision_points(
    seeds: &[u64],
    character: CharacterId,
    ascension: u8,
    combat: Option<CombatBudget>,
    max_points: usize,
) -> Vec<DecisionPoint> {
    let mut points = Vec::new();
    for &seed in seeds {
        if points.len() >= max_points {
            break;
        }
        let mut rm = sts2_content::run_callbacks::new_run_manager_for_character_with_ascension(
            seed, character, ascension,
        )
        .with_neow_start();
        // C#-faithful run setup (relic bags + room generation), matching the
        // arena so calibration sees the same run-state shape (Addendum 23).
        sts2_content::run_callbacks::initialize_run_setup(&mut rm);
        let cfg = FullRunConfig::default();
        if let Some(budget) = combat {
            let mut policy = CollectingPolicy {
                inner: MctsCombatPolicy::new(HeuristicPolicy::new(), budget, seed),
                seed,
                step: 0,
                out: Vec::new(),
            };
            let _ = drive_full_run(&mut rm, &mut policy, &cfg);
            points.extend(policy.out);
        } else {
            let mut policy = CollectingPolicy {
                inner: HeuristicPolicy::new(),
                seed,
                step: 0,
                out: Vec::new(),
            };
            let _ = drive_full_run(&mut rm, &mut policy, &cfg);
            points.extend(policy.out);
        }
    }
    points.truncate(max_points);
    points
}

/// Score every candidate of a point under `budget` (shared cache —
/// keys carry the budget, so arms can't collide).
fn score_point(point: &DecisionPoint, budget: &ProbeBudget, cache: &FightCache) -> Vec<f64> {
    point
        .candidates
        .iter()
        .map(|c| fight_delta(&point.rm, c, &point.plan, budget, cache).score())
        .collect()
}

/// Kendall τ-b between two equal-length score vectors. `None` when a
/// τ is undefined (fewer than 2 items, or all ties on either side).
#[must_use]
pub fn kendall_tau(a: &[f64], b: &[f64]) -> Option<f64> {
    let n = a.len().min(b.len());
    if n < 2 {
        return None;
    }
    let mut concordant = 0i64;
    let mut discordant = 0i64;
    let mut ties_a = 0i64;
    let mut ties_b = 0i64;
    for i in 0..n {
        for j in (i + 1)..n {
            let da = a[i] - a[j];
            let db = b[i] - b[j];
            if da == 0.0 && db == 0.0 {
                ties_a += 1;
                ties_b += 1;
            } else if da == 0.0 {
                ties_a += 1;
            } else if db == 0.0 {
                ties_b += 1;
            } else if (da > 0.0) == (db > 0.0) {
                concordant += 1;
            } else {
                discordant += 1;
            }
        }
    }
    let n0 = (n * (n - 1) / 2) as i64;
    let denom = (((n0 - ties_a) as f64) * ((n0 - ties_b) as f64)).sqrt();
    if denom == 0.0 {
        return None;
    }
    Some((concordant - discordant) as f64 / denom)
}

fn argmax(scores: &[f64]) -> Option<usize> {
    crate::policy::surfaces::picks::argmax(scores)
}

/// One arm of a calibration table.
#[derive(Clone, Debug, Serialize)]
pub struct ArmReport {
    /// Human-readable arm label (e.g. `"sims=100"`, `"K=16"`, `"T=5"`).
    pub arm: String,
    /// Mean Kendall τ-b vs the reference, over points where τ is
    /// defined.
    pub mean_tau: f64,
    /// Points where τ was defined.
    pub tau_points: usize,
    /// Fraction of points whose preferred option differs from the
    /// reference's preferred option.
    pub flip_rate: f64,
    /// Points compared for the flip rate.
    pub flip_points: usize,
}

/// Full calibration output (serialized to JSON by the CLI).
#[derive(Clone, Debug, Serialize)]
pub struct CalibrationReport {
    pub points: usize,
    pub e3_budgets: Vec<ArmReport>,
    pub e4_ks: Vec<ArmReport>,
    pub e5_ts: Vec<ArmReport>,
}

/// Compare `arm` scores against `reference` scores across points.
fn compare_arm(label: String, arm: &[Vec<f64>], reference: &[Vec<f64>]) -> ArmReport {
    let mut taus = Vec::new();
    let mut flips = 0usize;
    let mut flip_points = 0usize;
    for (a, r) in arm.iter().zip(reference) {
        if let Some(t) = kendall_tau(a, r) {
            taus.push(t);
        }
        if let (Some(pa), Some(pr)) = (argmax(a), argmax(r)) {
            flip_points += 1;
            if pa != pr {
                flips += 1;
            }
        }
    }
    ArmReport {
        arm: label,
        mean_tau: if taus.is_empty() {
            f64::NAN
        } else {
            taus.iter().sum::<f64>() / taus.len() as f64
        },
        tau_points: taus.len(),
        flip_rate: if flip_points == 0 {
            f64::NAN
        } else {
            flips as f64 / flip_points as f64
        },
        flip_points,
    }
}

/// Calibration knobs (every default is the plan's full-run value; the
/// CLI scales them down for smokes).
#[derive(Clone, Debug)]
pub struct CalibrationConfig {
    /// Base runtime config the arms perturb one axis of.
    pub base: ProbeBudget,
    /// E3 sim-budget arms (each at `base.k` / `base.t_turns`).
    pub e3_sims: Vec<u32>,
    /// E3 reference: full-fight strong search.
    pub e3_ref_sims: u32,
    pub e3_ref_k: u32,
    /// E4 K arms (at `base.sims` / `base.t_turns`).
    pub e4_ks: Vec<u32>,
    /// E4 reference K.
    pub e4_ref_k: u32,
    /// E5 T arms (at `base.sims` / `base.k`; `0` = full fight).
    pub e5_ts: Vec<u32>,
}

impl Default for CalibrationConfig {
    fn default() -> Self {
        Self {
            base: ProbeBudget::default(),
            e3_sims: vec![1, 100, 1000],
            e3_ref_sims: 5000,
            e3_ref_k: 32,
            e4_ks: vec![8, 16, 32, 64],
            e4_ref_k: 160,
            e5_ts: vec![3, 5, 8, 0],
        }
    }
}

/// Run the E3/E4/E5 calibration over `points` (rayon across points).
#[must_use]
pub fn calibrate(points: &[DecisionPoint], cfg: &CalibrationConfig) -> CalibrationReport {
    let cache = FightCache::with_capacity(65_536);
    let score_all = |budget: &ProbeBudget| -> Vec<Vec<f64>> {
        points
            .par_iter()
            .map(|p| score_point(p, budget, &cache))
            .collect()
    };

    // E3: cheap budgets vs full-fight strong reference.
    let e3_ref = score_all(&ProbeBudget {
        sims: cfg.e3_ref_sims,
        k: cfg.e3_ref_k,
        t_turns: 0,
        ..cfg.base
    });
    let e3_budgets = cfg
        .e3_sims
        .iter()
        .map(|&sims| {
            let arm = score_all(&ProbeBudget { sims, ..cfg.base });
            compare_arm(format!("sims={sims}"), &arm, &e3_ref)
        })
        .collect();

    // E4: K sweep vs high-K reference at the base sims/T.
    let e4_ref = score_all(&ProbeBudget {
        k: cfg.e4_ref_k,
        ..cfg.base
    });
    let e4_ks = cfg
        .e4_ks
        .iter()
        .map(|&k| {
            let arm = score_all(&ProbeBudget { k, ..cfg.base });
            compare_arm(format!("K={k}"), &arm, &e4_ref)
        })
        .collect();

    // E5: truncation sweep vs full-fight reference at base sims/K.
    let e5_ref = score_all(&ProbeBudget {
        t_turns: 0,
        ..cfg.base
    });
    let e5_ts = cfg
        .e5_ts
        .iter()
        .map(|&t| {
            let arm = score_all(&ProbeBudget {
                t_turns: t,
                ..cfg.base
            });
            let label = if t == 0 {
                "T=full".to_string()
            } else {
                format!("T={t}")
            };
            compare_arm(label, &arm, &e5_ref)
        })
        .collect();

    CalibrationReport {
        points: points.len(),
        e3_budgets,
        e4_ks,
        e5_ts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kendall_tau_basics() {
        // Perfect agreement.
        assert!((kendall_tau(&[1.0, 2.0, 3.0], &[10.0, 20.0, 30.0]).unwrap() - 1.0).abs() < 1e-12);
        // Perfect inversion.
        assert!((kendall_tau(&[1.0, 2.0, 3.0], &[3.0, 2.0, 1.0]).unwrap() + 1.0).abs() < 1e-12);
        // Undefined: all ties on one side.
        assert!(kendall_tau(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]).is_none());
        // Undefined: too short.
        assert!(kendall_tau(&[1.0], &[2.0]).is_none());
        // Mixed: one swapped pair out of three items.
        let t = kendall_tau(&[1.0, 2.0, 3.0], &[1.0, 3.0, 2.0]).unwrap();
        assert!((t - (1.0 / 3.0)).abs() < 1e-12, "{t}");
    }

    /// Plumbing smoke: a tiny collection + calibration produces a
    /// table with the right arm labels and sane values (τ in [-1, 1]).
    #[test]
    fn calibrate_smoke_tiny() {
        let points = collect_decision_points(&[3, 5, 7, 11], CharacterId::Ironclad, 0, None, 2);
        assert!(
            !points.is_empty(),
            "self-play over 4 seeds must hit at least one card-shaped decision"
        );
        let cfg = CalibrationConfig {
            base: ProbeBudget {
                sims: 2,
                k: 2,
                t_turns: 1,
                ..ProbeBudget::default()
            },
            e3_sims: vec![1, 2],
            e3_ref_sims: 4,
            e3_ref_k: 2,
            e4_ks: vec![1, 2],
            e4_ref_k: 4,
            e5_ts: vec![1, 0],
        };
        let report = calibrate(&points, &cfg);
        assert_eq!(report.points, points.len());
        assert_eq!(report.e3_budgets.len(), 2);
        assert_eq!(report.e4_ks.len(), 2);
        assert_eq!(report.e5_ts.len(), 2);
        for arm in report
            .e3_budgets
            .iter()
            .chain(&report.e4_ks)
            .chain(&report.e5_ts)
        {
            if !arm.mean_tau.is_nan() {
                assert!((-1.0..=1.0).contains(&arm.mean_tau), "{arm:?}");
            }
            if !arm.flip_rate.is_nan() {
                assert!((0.0..=1.0).contains(&arm.flip_rate), "{arm:?}");
            }
        }
    }

    /// Regression guard for the generic-wrapper `prepare`-forwarding
    /// invariant: every `RunPolicy` wrapper (`CollectingPolicy`,
    /// `LabelingPolicy`, `FeatureExportPolicy`, `ProxyStudyPolicy`,
    /// `CapturePolicy`, `Tracing`) must forward `prepare` to its inner
    /// policy, or a wrapped `PpoPolicy` silently skips event
    /// materialization. `CollectingPolicy` is the representative here
    /// (all six forwards are identical one-liners).
    #[test]
    fn collecting_policy_forwards_prepare_to_inner() {
        struct SpyPolicy {
            prepared: bool,
        }
        impl RunPolicy for SpyPolicy {
            fn prepare(&mut self, _rm: &mut RunManager) {
                self.prepared = true;
            }
            fn choose(&mut self, _rm: &RunManager, actions: &[RunAction]) -> Option<usize> {
                (!actions.is_empty()).then_some(0)
            }
        }

        let mut rm = sts2_content::run_callbacks::new_run_manager_for_character_with_ascension(
            0,
            CharacterId::Ironclad,
            0,
        );
        let mut policy = CollectingPolicy {
            inner: SpyPolicy { prepared: false },
            seed: 0,
            step: 0,
            out: Vec::new(),
        };
        policy.prepare(&mut rm);
        assert!(
            policy.inner.prepared,
            "CollectingPolicy::prepare must forward to the inner policy"
        );
    }
}
