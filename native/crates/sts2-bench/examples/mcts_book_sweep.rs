//! `mcts_book_sweep` — sweep one search knob against the position book.
//!
//! A8 FINDINGS §4: the median root Q *spread* is 0.034 while the UCB
//! explore term at the top child is 0.36, so the exploration bonus is an
//! order of magnitude larger than the entire signal it trades against.
//! Visit allocation comes out near-uniform (top child 1.07x the uniform
//! share, `vmax/vmin` 1.19) — every child gets N/k samples and none is
//! refined.
//!
//! ## Prerequisite: the fixture pool
//!
//! Every entry stores a *path* into `sts2-rs/data/run-fixtures/`, which is
//! git-ignored, so it is absent on a fresh clone and after an agent
//! worktree that produced it is cleaned up. Regenerate it with
//! **`just harvest-fixtures`** (~44 s for all five characters, deterministic
//! — the frozen `state_hash` still matches across hundreds of commits) or
//! `logs/2026-08-12-fixture-pool/harvest.sh` directly.
//! ## Why this sweeps `c_puct` and not the value scale
//!
//! The issue asks for a joint rescale-and-sweep. For **selection** they
//! are the same knob: UCB picks `argmax(Q + c·sqrt(ln N / n))`, so
//! scaling every value by `a` and scaling `c_puct` by `1/a` produce
//! identical orderings. A uniform linear rescale of the value therefore
//! explores nothing this sweep does not.
//!
//! What a real rescale would additionally change is everything keyed to
//! an *absolute* value threshold, none of which scales with it:
//! `tie_eps` (0.02), the loss-gradient band `[-1.45, -1.05)`, the win
//! reservation at `>= +1.0`, and the depth-cut floor at `> -1.0`. Those
//! are semantic changes, not selection changes, and they belong in a
//! separate arm — pinning down the selection optimum first says how much
//! of §4 is even a selection problem.
//!
//! The nonlinear knob is `IN_PROGRESS_DIVISOR` (the `tanh` argument),
//! which changes relative spacing rather than gain. It is out of scope
//! here for the same reason: only worth moving if the linear family
//! plateaus below the achievable agreement.
//!
//! ## Grid
//!
//! Centred on where the arithmetic says the optimum should be, not on
//! the deployed value. Matching the explore term to the measured spread
//! implies `c_puct ~ 2.5 * (0.034 / 0.36) ~ 0.24`, an order of magnitude
//! below the deployed 2.5 and below `mcts_c_puct_sweep`'s 0.7 floor —
//! which is why that harness could not have found it.
//!
//! ## Metrics
//!
//! - **`gold_agr`** — fraction of positions where the arm picks the
//!   book's gold action. The quality metric; judge on this.
//! - **`vmax_vmin` / `top_share`** — visit concentration, the mechanism
//!   §4 predicts should move. An arm that improves agreement without
//!   concentrating visits is not working the way the diagnosis says.
//! - **`top_gap`** — separation the arm achieves between its top two
//!   children.
//! - **`stability`** — fraction of seeds agreeing with the arm's own
//!   modal pick. Self-consistency, not quality: an arm that always picks
//!   the same wrong action scores 1.0. Reported because a greedier
//!   `c_puct` could buy agreement by locking onto noise early, and this
//!   is what would expose it.
//! - **`resolved` / `med z`** — per-ply resolution (GH #446 item 3). For
//!   each search, `z = (Q₁ − Q₂) / sqrt(SE₁² + SE₂²)` over the top two
//!   root children, with each child's standard error from its own return
//!   variance (`RootOutcome::mean_stderr`). `resolved` is the fraction of
//!   searches with `z ≥ --z` (default 1.96). Agreement says whether the
//!   pick was right; resolution says whether the search *knew*. A change
//!   that raises resolution without moving agreement is still progress,
//!   and one that moves agreement without touching resolution is probably
//!   seed luck. It is also what makes the book's blind spot visible: the
//!   gold filter keeps only positions two 50k-sim oracles resolve
//!   unanimously, so a low `resolved` at 1000 sims on those same positions
//!   is the search being short of budget, not the position being a tie.
//! - **`regret`** — `Q_oracle(gold) − Q_oracle(pick)`, mean over cells, in
//!   the oracle's per-rollout value units (GH #494). Needs a book with an
//!   `oracle_q` table (`combat_position_book oracle-q`); `-` otherwise.
//!   Agreement counts misses, regret prices them: the 2026-09-06 budget
//!   panel showed an arm can trade misses one-for-one on the book and
//!   still lose 6pp of win rate because the misses it *added* were the
//!   expensive kind. A pick the oracle never expanded has unknown regret
//!   and is left out, not scored as zero. **Read it as a visit-count
//!   statistic, not a value** (GH #494 step 2,
//!   `logs/2026-09-07-blunder-diagnosis/FINDINGS.md`): mean backup gives
//!   the 5–30×-visited gold child a Q near its best line while siblings
//!   average exploration and heuristic rollouts, so the difference tracks
//!   the visit gap (r = −0.10 against playout regret). The gate prints it
//!   as `oracle_dq (visit-shaded)`; `book_position_probe playout-regret`
//!   is the value metric. A `combat_position_book hard` book has no oracle
//!   table and its `gold_aliases` (second copy of the gold card) count as
//!   agreement.
//!
//! ## Per-motif tables
//!
//! When the book carries motif tags (`combat_position_book annotate`
//! writes them; see `sts2_cli::replay::motif`), the deployed and best arms
//! are also broken down per tag, so a regression names *which* decision
//! class moved. `proven_pair` positions are tagged and scored, not
//! excluded — they are the only class whose gold answer can be audited
//! against an engine proof.
//!
//! ```sh
//! cargo run --release -p sts2-bench --example mcts_book_sweep -- \
//!     [--dump rows.json] [--z 1.96] [--budget eval-cheap|elite-focus|motif-focus] \
//!     [book_json] [pool_root] [seeds] [sims] [knob] [values]
//! ```
//!
//! `--dump` writes one JSON row per (arm, position, seed) — pick, gold,
//! agreement, z, regret, sims, and (playout-gold books only) the label's
//! `margin_pp` — which is what
//! `scripts/decision_quality_gate.py` pairs across two git SHAs for the
//! standing gate (item 4).
//!
//! `--budget <kind>` replaces the flat `sims` with what the live
//! `sts2_runai::policy::CombatBudget` of that name would spend on each
//! position (boss/elite by the entry's tier, motif by the realized root),
//! so a budget-reallocation arm is scored on decisions before a win-rate
//! panel. The header line then reports the mean sims per position, and the
//! gate prints the base/branch compute ratio — read the agreement delta
//! against it, since the book's tier and motif mix is not the live one.

#[path = "common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sts2_cli::replay::motif::Motif;
use sts2_core::{legal_actions, CombatPhase, CombatState};
use sts2_mcts::{BackupRule, HiddenInfoMode, Mcts, MctsConfig, RolloutPolicyKind};

/// Deployed value is 2.5. The grid brackets it and reaches down an order
/// of magnitude to where matching the explore term to the measured Q
/// spread puts the optimum.
const C_PUCT_GRID: [f32; 8] = [0.05, 0.1, 0.2, 0.4, 0.8, 1.41, 2.5, 4.0];

/// Default `z` at which a search counts as having resolved its top-two
/// choice: the two-sided 5% normal quantile.
const DEFAULT_Z_RESOLVED: f64 = 1.96;

/// `z` reported when the top two children have identical returns on every
/// rollout (zero variance) — a proven separation, not an infinite one.
const Z_CAP: f64 = 99.0;

#[derive(Debug, Clone, Deserialize)]
struct Book {
    oracle_sims: u32,
    #[serde(default)]
    max_warmup_plies: u32,
    chance_agreement: f64,
    entries: Vec<BookEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct BookEntry {
    /// v1 books predate `book_id`; fall back to the entry's index so a
    /// dump row still has a stable join key.
    #[serde(default)]
    book_id: u32,
    character: String,
    tier: String,
    fixture_rel_path: String,
    seed: u64,
    state_hash: u64,
    gold_action: String,
    /// Other debug renderings of the *same* move — a second copy of the
    /// gold card in another hand slot. Written by `combat_position_book
    /// hard`, which scores moves by card identity; empty on oracle books,
    /// whose gold is one `hand_index`. A pick matching an alias agrees.
    #[serde(default)]
    gold_aliases: Vec<String>,
    /// Motif tags written by `combat_position_book`. Empty on an
    /// un-annotated book, in which case the per-motif tables are skipped.
    #[serde(default)]
    motifs: Vec<String>,
    /// The oracle's root table (`combat_position_book oracle-q`). Empty on
    /// a book without it, in which case `regret` is reported as `-`.
    #[serde(default)]
    oracle_q: Vec<OracleChild>,
    /// The playout table written by `combat_position_book hard`. Absent on
    /// oracle books. Only the margin is mirrored — see [`EntryPlayout`].
    #[serde(default)]
    playout: Option<EntryPlayout>,
}

/// The one field of the `hard` builder's playout table the sweep carries
/// into its dump: how many percentage points of playout win rate separate
/// the gold move from the runner-up.
///
/// A playout-gold book's answers are not equally decisive — most hard
/// positions really are near-ties (median margin ~3pp) and a few are not —
/// so a gate that treats every cell alike measures mostly coin flips. The
/// dump carries the margin so the gate can bucket by it.
#[derive(Debug, Clone, Deserialize)]
struct EntryPlayout {
    margin_pp: f64,
}

/// One row of the oracle's root table: the pooled 50 000-sim mean value of
/// taking `action`. Only the two fields the sweep reads are mirrored.
#[derive(Debug, Clone, Deserialize)]
struct OracleChild {
    action: String,
    q: f32,
}

/// What the search left on the table, in the oracle's own value units:
/// `Q_oracle(gold) − Q_oracle(pick)`. Zero when it agreed; `None` when the
/// book carries no table, or the pick is an action the oracle never
/// expanded (its Q is unknown, not zero). Slightly negative values are
/// real: gold is the oracle's most-*visited* child, and on a near-tie the
/// runner-up can carry a marginally higher mean.
fn regret_of(oracle_q: &[OracleChild], gold: &str, pick: &str) -> Option<f64> {
    if oracle_q.is_empty() {
        return None;
    }
    if pick == gold {
        return Some(0.0);
    }
    let q = |a: &str| {
        oracle_q
            .iter()
            .find(|c| c.action == a)
            .map(|c| f64::from(c.q))
    };
    Some(q(gold)? - q(pick)?)
}

/// One (arm, position, seed) row of the `--dump` file. The gate pairs two
/// dumps on `(book_id, seed)`, so both are stable across git SHAs; the rest
/// is carried so the gate can break the pairing down without re-reading
/// the book.
#[derive(Debug, Clone, Serialize)]
struct DumpRow<'a> {
    knob: &'a str,
    value: f32,
    sims: u32,
    book_id: u32,
    character: &'a str,
    tier: &'a str,
    motifs: &'a [String],
    seed: u64,
    pick: &'a str,
    gold: &'a str,
    agree: bool,
    z: f64,
    /// `Q_oracle(gold) − Q_oracle(pick)`; `null` when the book has no
    /// oracle table for this position. See [`regret_of`].
    regret: Option<f64>,
    /// Playout win-rate gap between gold and the runner-up, in percentage
    /// points; `null` on a book with no playout table. A property of the
    /// *label*, identical in both arms, so the gate uses it to bucket
    /// cells rather than to compare them.
    margin_pp: Option<f64>,
}

/// Which knob the sweep values apply to. Everything else stays at the
/// deployed dialect, so an arm differs from production in exactly one
/// number — the property that makes the result attributable.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Knob {
    CPuct,
    TieEps,
    /// GitHub #492: power-mean backup order `p`, globally shifted by
    /// `MAX_LEAF_VALUE_ABS`. Value `0` means `BackupRule::Mean` — the
    /// deployed rule — so a grid starting at `0` carries its own control.
    BackupPm,
    /// As [`Self::BackupPm`] but per-node min-max normalized. Same `0 =
    /// Mean` convention.
    BackupPmn,
    /// GitHub #446: the priority the heuristic rollout gives `EndTurn`
    /// ([`MctsConfig::rollout_end_turn_priority`]). The deployed `10_000`
    /// collapses its softmax weight to `+0.0`, so the rollout empties the
    /// hand every turn and cannot represent a line whose merit is holding a
    /// card. A finite value inside the card range makes stopping early a
    /// sampled option. `10_000` is the control, so a grid containing it
    /// carries its own baseline like the backup knobs do.
    EndTurnPrio,
    /// GitHub #720: which rollout policy runs the playout at all
    /// ([`MctsConfig::rollout_policy`]). `0` is the deployed
    /// [`RolloutPolicyKind::Heuristic`] and `1` is
    /// [`RolloutPolicyKind::Uniform`], so a grid of `0,1` carries its own
    /// control like the backup knobs do.
    ///
    /// This is the one knob that changes the estimator's *family* rather
    /// than a number inside it. The coverage gate (`COVERAGE_THRESHOLD`,
    /// bead `sts2-rs-j0f6`) has only ever measured
    /// Heuristic-with-*low*-coverage against Uniform; since PR #719 gave
    /// every playable card a priority, nothing falls back any more, and
    /// Heuristic-at-*full*-coverage versus Uniform is newly askable.
    RolloutPolicy,
}

impl Knob {
    fn parse(s: &str) -> Self {
        match s {
            "c_puct" | "cpuct" => Self::CPuct,
            "tie_eps" | "tieeps" => Self::TieEps,
            "backup_pm" | "pm" => Self::BackupPm,
            "backup_pmn" | "pmn" => Self::BackupPmn,
            "end_turn_prio" | "endturn" => Self::EndTurnPrio,
            "rollout_policy" | "rollout" => Self::RolloutPolicy,
            other => panic!(
                "unknown knob {other:?} (want c_puct | tie_eps | backup_pm | \
                 backup_pmn | end_turn_prio | rollout_policy)"
            ),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::CPuct => "c_puct",
            Self::TieEps => "tie_eps",
            Self::BackupPm => "backup_pm_p",
            Self::BackupPmn => "backup_pmn_p",
            Self::EndTurnPrio => "end_turn_prio",
            Self::RolloutPolicy => "rollout_policy",
        }
    }
    /// The deployed value, marked in the output so a sweep always shows
    /// what it is being compared against.
    fn deployed(self) -> f32 {
        match self {
            Self::CPuct => 2.5,
            Self::TieEps => 0.02,
            // `0` is the sentinel for `BackupRule::Mean`, i.e. production.
            Self::BackupPm | Self::BackupPmn => 0.0,
            Self::EndTurnPrio => f32::from(sts2_mcts::expert::END_TURN_PRIORITY),
            // `0` is the sentinel for `RolloutPolicyKind::Heuristic`.
            Self::RolloutPolicy => 0.0,
        }
    }
}

/// Decode a [`Knob::RolloutPolicy`] grid value. Panics on anything else,
/// so a typo in the grid fails loudly instead of silently running the
/// control twice and reporting a flat sweep.
#[allow(clippy::cast_possible_truncation)]
fn rollout_policy_kind(value: f32) -> RolloutPolicyKind {
    match value.round() as i32 {
        0 => RolloutPolicyKind::Heuristic,
        1 => RolloutPolicyKind::Uniform,
        other => panic!("rollout_policy takes 0 (Heuristic, deployed) or 1 (Uniform); got {other}"),
    }
}

fn arm_config(knob: Knob, value: f32, sims: u32, seed: u64) -> MctsConfig {
    let base = MctsConfig {
        max_simulations: sims,
        seed,
        max_rollout_depth: 200,
        hidden_info: HiddenInfoMode::BeliefDraw,
        c_puct: 2.5,
        draw_widening_c: 2.0,
        ..MctsConfig::default()
    };
    match knob {
        Knob::CPuct => MctsConfig {
            c_puct: value,
            ..base
        },
        Knob::EndTurnPrio => MctsConfig {
            // Priorities are `i16`; the grid is given in the same units.
            #[allow(clippy::cast_possible_truncation)]
            rollout_end_turn_priority: value.round().clamp(-32768.0, 32767.0) as i16,
            ..base
        },
        Knob::RolloutPolicy => MctsConfig {
            rollout_policy: rollout_policy_kind(value),
            ..base
        },
        Knob::TieEps => MctsConfig {
            tie_eps: value,
            ..base
        },
        Knob::BackupPm | Knob::BackupPmn => {
            if value <= 0.0 {
                base
            } else {
                MctsConfig {
                    backup_rule: if knob == Knob::BackupPm {
                        BackupRule::PowerMean
                    } else {
                        BackupRule::PowerMeanNorm
                    },
                    backup_power_p: value,
                    ..base
                }
            }
        }
    }
}

fn trial_seed(t: u32) -> u64 {
    u64::from(t)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(0x51ED_2701)
}

/// Realize a book entry, and verify it is the state the gold answer was
/// derived from. A hash mismatch means the fixture or the engine moved
/// under the book; comparing against a stale answer would silently score
/// noise, so it is a hard error rather than a skip.
/// Why an entry produced no scorable state. The distinction is the whole
/// point: "the fixture file is not on this machine" and "the engine moved
/// under the book" have opposite fixes, and conflating them sent GH #446
/// after a rebuild of two suites that turned out to be fine — the pool was
/// simply absent (it is git-ignored, and the copy that existed had been
/// inside a since-deleted agent worktree).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Skip {
    /// The fixture did not load: absent pool, or an unparseable TOML.
    Missing,
    /// It loaded, but warm-up left no decision to score (terminal, or not
    /// in the play phase). A property of the position, not of the pool.
    NoDecision,
    /// It loaded and realized, but to a different state than the book's
    /// frozen `state_hash`. This is the one that means "stale".
    HashMiss,
}

fn realize_verified(pool_root: &PathBuf, e: &BookEntry, warmup: u32) -> Result<CombatState, Skip> {
    let path = pool_root.join(&e.fixture_rel_path);
    let mut state = common::realize_fixture(&path, e.seed).map_err(|_| Skip::Missing)?;
    // Must mirror the book builder exactly, or every warmed-up entry
    // reconstructs to a different state and fails the hash guard below.
    // Both call the same `common::warmup_plies` so they cannot drift.
    common::warmup_plies(&mut state, e.seed, warmup);
    if state.is_over || state.phase != CombatPhase::PlayerPlayPhase {
        return Err(Skip::NoDecision);
    }
    if sts2_core::state_hash(&state) != e.state_hash {
        eprintln!(
            "[sweep] STATE HASH MISMATCH for {} — this fixture realizes to a \
             different state than the book froze. Rebuild the book \
             (combat_position_book) before trusting any number here.",
            e.fixture_rel_path
        );
        return Err(Skip::HashMiss);
    }
    Ok(state)
}

/// One search's outcome on one position: what it picked and how sure it
/// was. Kept per seed so the dump can be paired seed-by-seed across SHAs.
struct SeedRow {
    pick: String,
    agree: bool,
    /// Top-two separation in standard errors; [`Z_CAP`] when both
    /// children had zero return variance and a positive gap.
    z: f64,
    /// See [`regret_of`].
    regret: Option<f64>,
}

/// One arm's measurements on one position, averaged over seeds.
struct Cell {
    agree: f64,
    /// Fraction of seeds that ended the turn while a card play was still
    /// legal. This is the blunder shape #445 targets, measured directly
    /// rather than inferred from agreement.
    endturn_with_play: f64,
    vmax_vmin: f64,
    /// Same: part of the recorded cell, not an input to the summary below.
    #[allow(dead_code)]
    top_share: f64,
    top_gap: f64,
    stability: f64,
    /// Fraction of seeds whose top-two `z` reached the resolution
    /// threshold.
    resolved: f64,
    /// Median per-seed `z`. A median because a single zero-variance seed
    /// pins the mean at [`Z_CAP`].
    z_median: f64,
    /// Mean per-seed regret over the seeds that had one; `None` when the
    /// book carries no oracle table for this position.
    regret: Option<f64>,
    rows: Vec<SeedRow>,
}

/// Top-two separation in standard errors. `children` are `(visits, mean,
/// stderr)` sorted by mean, best first. Zero combined standard error with a
/// positive gap is a proven separation ([`Z_CAP`]); with a zero gap it is
/// an exact tie (`0.0`).
fn top_two_z(children: &[(u32, f32, f32)]) -> f64 {
    if children.len() < 2 {
        return 0.0;
    }
    let gap = f64::from(children[0].1 - children[1].1);
    let se = (f64::from(children[0].2).powi(2) + f64::from(children[1].2).powi(2)).sqrt();
    if se <= f64::EPSILON {
        if gap > 0.0 {
            Z_CAP
        } else {
            0.0
        }
    } else {
        (gap / se).min(Z_CAP)
    }
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 2]
}

fn run_cell(
    state: &CombatState,
    entry: &BookEntry,
    knob: Knob,
    value: f32,
    sims: u32,
    seeds: u32,
    z_resolved: f64,
) -> Cell {
    let mut agree = 0.0;
    let mut vmax_vmin = 0.0;
    let mut top_share = 0.0;
    let mut top_gap = 0.0;
    let mut picks: BTreeMap<String, u32> = BTreeMap::new();
    let mut endturn_with_play = 0.0;
    let mut resolved = 0.0;
    let mut zs: Vec<f64> = Vec::with_capacity(seeds as usize);
    let mut rows: Vec<SeedRow> = Vec::with_capacity(seeds as usize);
    let (gold, oracle_q) = (entry.gold_action.as_str(), entry.oracle_q.as_slice());
    // A card play being legal at the root is a property of the position,
    // not of the arm, so it is computed once outside the seed loop.
    let play_available = legal_actions(state)
        .iter()
        .any(|a| matches!(a, sts2_core::Action::PlayCard { .. }));

    for t in 0..seeds {
        let mut m = Mcts::new(state.clone(), arm_config(knob, value, sims, trial_seed(t)));
        let action = m.search_n(sims);
        let pick = format!("{action:?}");
        *picks.entry(pick.clone()).or_default() += 1;
        let hit = pick == gold || entry.gold_aliases.contains(&pick);
        if hit {
            agree += 1.0;
        }
        if play_available && matches!(action, sts2_core::Action::EndTurn) {
            endturn_with_play += 1.0;
        }

        // Collapse transposition-merged children before reading the visit
        // distribution: `root_outcomes()` reports one merged node once per
        // incoming root edge, so raw counts overstate both the child count
        // and the spread of visits (POSTFIX-RESULTS correction, 2026-08-11).
        let mut seen: BTreeMap<(u32, u32, u32), (u32, f32, f32)> = BTreeMap::new();
        for o in m.root_outcomes() {
            seen.entry((o.visits, o.wins, o.losses)).or_insert((
                o.visits,
                o.mean_value,
                o.mean_stderr(),
            ));
        }
        let mut vals: Vec<(u32, f32, f32)> = seen.into_values().collect();
        vals.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let z = top_two_z(&vals);
        zs.push(z);
        if z >= z_resolved {
            resolved += 1.0;
        }
        rows.push(SeedRow {
            regret: regret_of(oracle_q, gold, &pick),
            pick,
            agree: hit,
            z,
        });
        if vals.len() < 2 {
            continue;
        }
        let visits: Vec<u32> = vals.iter().map(|(v, _, _)| *v).collect();
        let total: u32 = visits.iter().sum();
        let k = vals.len();
        #[allow(clippy::cast_precision_loss)]
        {
            vmax_vmin += f64::from(*visits.iter().max().unwrap())
                / f64::from(*visits.iter().min().unwrap()).max(1.0);
            top_share += (f64::from(*visits.iter().max().unwrap()) / f64::from(total.max(1)))
                / (1.0 / k as f64);
            top_gap += f64::from(vals[0].1 - vals[1].1);
        }
    }

    let n = f64::from(seeds.max(1));
    let modal = picks.values().max().copied().unwrap_or(0);
    let regrets: Vec<f64> = rows.iter().filter_map(|r| r.regret).collect();
    #[allow(clippy::cast_precision_loss)]
    let regret = (!regrets.is_empty()).then(|| regrets.iter().sum::<f64>() / regrets.len() as f64);
    Cell {
        regret,
        agree: agree / n,
        endturn_with_play: endturn_with_play / n,
        vmax_vmin: vmax_vmin / n,
        top_share: top_share / n,
        top_gap: top_gap / n,
        stability: f64::from(modal) / n,
        resolved: resolved / n,
        z_median: median(&mut zs),
        rows,
    }
}

fn main() -> std::io::Result<()> {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    // Flags first, then the positional list the pre-#446 invocation used,
    // so every existing command line keeps working unchanged.
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut args: Vec<String> = Vec::with_capacity(raw.len());
    let mut dump: Option<PathBuf> = None;
    let mut z_resolved = DEFAULT_Z_RESOLVED;
    // `--budget <kind>`: score a live `CombatBudget` instead of a flat sims
    // count. Each position then runs at the sims the deployed agent would
    // give it — by tier from the book entry, by root motif from the state —
    // so the GH #446 reallocation arms (`elite-focus`, `motif-focus`) are
    // judged on decisions before any win-rate run. The positional `sims`
    // argument is ignored when this is set.
    let mut budget: Option<sts2_runai::policy::CombatBudget> = None;
    let mut ri = raw.into_iter();
    while let Some(a) = ri.next() {
        match a.as_str() {
            "--dump" => dump = ri.next().map(PathBuf::from),
            "--z" => {
                z_resolved = ri
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(DEFAULT_Z_RESOLVED);
            }
            "--budget" => {
                let kind = ri.next().unwrap_or_default();
                let parsed = sts2_runai::arena::CombatBudgetKind::parse(&kind)
                    .unwrap_or_else(|e| panic!("--budget: {e}"));
                budget = Some(
                    parsed
                        .budget(None, None)
                        .unwrap_or_else(|| panic!("--budget none has no sims to score")),
                );
            }
            _ => args.push(a),
        }
    }
    let mut it = args.iter();
    let book_path = PathBuf::from(
        it.next()
            .cloned()
            .unwrap_or_else(|| "data/combat_position_book.json".to_string()),
    );
    let pool_root = PathBuf::from(
        it.next()
            .cloned()
            .unwrap_or_else(|| "data/run-fixtures".to_string()),
    );
    let seeds: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(4);
    let sims: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(1000);
    // Which knob to sweep; defaults to `c_puct` so the pre-rename invocation
    // keeps working.
    let knob = it.next().map_or(Knob::CPuct, |s| Knob::parse(s));
    // Optional comma-separated value list. Restricting to one value turns
    // this into a sims ladder: run it once per budget and read the single row,
    // instead of paying the whole grid at 8000 sims to learn one number.
    let grid: Vec<f32> = it.next().map_or_else(
        || match knob {
            Knob::CPuct => C_PUCT_GRID.to_vec(),
            // 0.0 restores strict-equality tie-breaking; 0.02 is deployed;
            // 0.075 is the per-child reproducibility the A8 audit measured
            // across search seeds, i.e. where the band arguably should sit
            // if it is meant to cover the estimator's own noise.
            Knob::TieEps => vec![0.0, 0.01, 0.02, 0.05, 0.075, 0.15],
            // `0` = deployed `BackupRule::Mean`; the rest is the ladder the
            // dominance suite (`dominance_pairs`) found signal on.
            Knob::BackupPm | Knob::BackupPmn => vec![0.0, 1.0, 1.5, 2.0, 3.0, 4.0],
            // `10_000` = deployed (the rollout never ends a turn early).
            // The rest walks `EndTurn` down through the card-priority range
            // — `NEUTRAL_PRIORITY` is 50, Strike 55, Bash 25 — so the arms
            // span "ends the turn only when nothing decent is left" (300)
            // to "ends it readily" (30).
            Knob::EndTurnPrio => vec![10_000.0, 300.0, 150.0, 90.0, 60.0, 30.0],
            // `0` = deployed Heuristic, `1` = Uniform. Two arms is the
            // whole grid: the knob is categorical, not a ladder.
            Knob::RolloutPolicy => vec![0.0, 1.0],
        },
        |s| {
            s.split(',')
                .filter_map(|p| p.trim().parse::<f32>().ok())
                .collect()
        },
    );
    assert!(!grid.is_empty(), "c_puct list parsed to nothing");

    let book: Book =
        serde_json::from_slice(&std::fs::read(&book_path)?).expect("position book parses");
    eprintln!(
        "[sweep] book: {} positions (oracle {} sims), chance {:.3}; \
         knob={} grid {:?} x {seeds} seeds x {sims} sims",
        book.entries.len(),
        book.oracle_sims,
        book.chance_agreement,
        knob.label(),
        grid
    );

    std::panic::set_hook(Box::new(|_| {}));
    let outcomes: Vec<(BookEntry, Result<CombatState, Skip>)> = book
        .entries
        .par_iter()
        .map(|e| {
            let r =
                std::panic::catch_unwind(|| realize_verified(&pool_root, e, book.max_warmup_plies))
                    .unwrap_or(Err(Skip::NoDecision));
            (e.clone(), r)
        })
        .collect();
    let _ = std::panic::take_hook();
    let count = |want: Skip| {
        outcomes
            .iter()
            .filter(|(_, r)| r.as_ref().err() == Some(&want))
            .count()
    };
    let (missing, no_decision, hash_miss) = (
        count(Skip::Missing),
        count(Skip::NoDecision),
        count(Skip::HashMiss),
    );
    let realized: Vec<(BookEntry, CombatState)> = outcomes
        .into_iter()
        .filter_map(|(e, r)| r.ok().map(|s| (e, s)))
        .collect();
    eprintln!(
        "[sweep] {} of {} positions verified against the book's state hash \
         (skipped: {missing} fixture missing, {no_decision} no decision, \
         {hash_miss} hash mismatch)",
        realized.len(),
        book.entries.len()
    );
    // Refuse to report on a partial corpus. Splitting the reason matters:
    // "the pool is not on this machine" is a 44-second fix and says nothing
    // about the book, whereas a hash mismatch is the book actually being
    // stale. Reporting the first as the second cost a wrong conclusion once
    // already (GH #446).
    assert!(
        realized.len() * 2 >= book.entries.len(),
        "only {}/{} book positions verified — refusing to report a number scored \
         on a partial corpus.\n  {missing} fixtures could not be loaded from {}. \
         The pool is git-ignored and regenerable: run `just harvest-fixtures` \
         (~44 s, deterministic) or logs/2026-08-12-fixture-pool/harvest.sh, and \
         make sure CHARACTERS covers every character the book references.\n  \
         {hash_miss} realized to a different state than the book froze — THAT is \
         a stale book, and it needs a combat_position_book rebuild.",
        realized.len(),
        book.entries.len(),
        pool_root.display()
    );

    // Simulations per position: the flat `sims` argument, or what the live
    // `--budget` would give this position (tier from the book entry, motif
    // from the realized root).
    let pos_sims: Vec<u32> = realized
        .iter()
        .map(|(e, state)| {
            budget.as_ref().map_or(sims, |b| {
                use sts2_runai::policy::BudgetTier;
                let tier = match e.tier.as_str() {
                    "boss" => BudgetTier::Boss,
                    "elite" => BudgetTier::Elite,
                    _ => BudgetTier::Common,
                };
                b.sims_for(tier, state)
            })
        })
        .collect();
    let sims_label = budget.as_ref().map_or_else(
        || sims.to_string(),
        |b| {
            #[allow(clippy::cast_precision_loss)]
            let mean =
                pos_sims.iter().map(|&s| f64::from(s)).sum::<f64>() / pos_sims.len().max(1) as f64;
            format!(
                "budget(common={} elite={} boss={} motif={}; mean/position={mean:.0})",
                b.sims,
                b.elite_sims.unwrap_or(b.sims),
                b.boss_sims,
                b.motif_sims
                    .map_or_else(|| "-".to_string(), |m| m.to_string()),
            )
        },
    );

    // Every (arm, position) cell is independent; flatten so one rayon pool
    // covers the whole grid rather than parallelising per arm.
    let cells: Vec<(usize, usize, Cell)> = grid
        .iter()
        .enumerate()
        .flat_map(|(ci, c)| {
            realized
                .iter()
                .enumerate()
                .map(move |(pi, r)| (ci, pi, *c, r))
        })
        .collect::<Vec<_>>()
        .into_par_iter()
        .map(|(ci, pi, c, (entry, state))| {
            (
                ci,
                pi,
                run_cell(state, entry, knob, c, pos_sims[pi], seeds, z_resolved),
            )
        })
        .collect();

    let mut by_arm: Vec<Vec<(usize, Cell)>> = (0..grid.len()).map(|_| Vec::new()).collect();
    for (ci, pi, cell) in cells {
        by_arm[ci].push((pi, cell));
    }

    println!("\n=== {} sweep on the position book ===", knob.label());
    println!(
        "positions={} seeds={seeds} sims={sims_label} chance_agreement={:.3} z_resolved={z_resolved}",
        realized.len(),
        book.chance_agreement
    );
    println!(
        "\n{:>7}  {:>9}  {:>8}  {:>9}  {:>7}  {:>12}  {:>10}  {:>9}  {:>9}",
        knob.label(),
        "gold_agr",
        "regret",
        "resolved",
        "med z",
        "endturn/play",
        "med v/v",
        "med gap",
        "stability"
    );
    // Mean regret over the cells that carry an oracle table; `-` when the
    // book has none. Same units as the oracle's Q (per-rollout value).
    let regret_col = |rows: &[(usize, Cell)]| -> String {
        let v: Vec<f64> = rows.iter().filter_map(|(_, c)| c.regret).collect();
        if v.is_empty() {
            format!("{:>8}", "-")
        } else {
            #[allow(clippy::cast_precision_loss)]
            let m = v.iter().sum::<f64>() / v.len() as f64;
            format!("{m:>8.4}")
        }
    };
    let mut best = (0usize, -1.0_f64);
    for (ci, c) in grid.iter().enumerate() {
        let rows = &by_arm[ci];
        if rows.is_empty() {
            continue;
        }
        #[allow(clippy::cast_precision_loss)]
        let n = rows.len() as f64;
        let mean = |f: fn(&Cell) -> f64| rows.iter().map(|(_, c)| f(c)).sum::<f64>() / n;
        // Concentration is reported as a MEDIAN: `vmax/vmin` is heavy-tailed
        // (one child with a single visit sends it to hundreds), so a mean is
        // dominated by outliers and is not comparable to the median figures
        // A8 FINDINGS and `logs/2026-08-12-q-landscape/` quote.
        let med = |f: fn(&Cell) -> f64| {
            let mut v: Vec<f64> = rows.iter().map(|(_, c)| f(c)).collect();
            median(&mut v)
        };
        let agr = mean(|c| c.agree);
        if agr > best.1 {
            best = (ci, agr);
        }
        let deployed = if (*c - knob.deployed()).abs() < 1e-6 {
            "  <- deployed"
        } else {
            ""
        };
        println!(
            "{c:>7.2}  {agr:>9.3}  {}  {:>9.3}  {:>7.2}  {:>12.3}  {:>10.2}  {:>9.4}  {:>9.3}{deployed}",
            regret_col(rows),
            mean(|c| c.resolved),
            med(|c| c.z_median),
            mean(|c| c.endturn_with_play),
            med(|c| c.vmax_vmin),
            med(|c| c.top_gap),
            mean(|c| c.stability),
        );
    }
    println!(
        "\nbest arm: {}={:.2} at gold_agr={:.3}",
        knob.label(),
        grid[best.0],
        best.1
    );

    // Per-tier and per-motif for the winning arm and the deployed arm: §4
    // says boss positions are where resolution is worst, so that is where
    // a selection fix should show up first; the motif split says which
    // decision class moved.
    let deployed_idx = grid
        .iter()
        .position(|c| (*c - knob.deployed()).abs() < 1e-6);
    let has_motifs = realized.iter().any(|(e, _)| !e.motifs.is_empty());
    // A single-arm run (the gate's shape) has best == deployed; print the
    // breakdown once.
    let best_idx = (deployed_idx != Some(best.0)).then_some(best.0);
    for (label, idx) in [("best", best_idx), ("deployed", deployed_idx)] {
        let Some(idx) = idx else { continue };
        // (agree sum, resolved sum, n, regret sum, regret n)
        #[derive(Default)]
        struct Acc(f64, f64, u32, f64, u32);
        impl Acc {
            fn add(&mut self, cell: &Cell) {
                self.0 += cell.agree;
                self.1 += cell.resolved;
                self.2 += 1;
                if let Some(r) = cell.regret {
                    self.3 += r;
                    self.4 += 1;
                }
            }
            fn row(&self) -> String {
                let regret = if self.4 == 0 {
                    format!("{:>8}", "-")
                } else {
                    format!("{:>8.4}", self.3 / f64::from(self.4))
                };
                format!(
                    "{:>4}  {:>8.3}  {regret}  {:>8.3}",
                    self.2,
                    self.0 / f64::from(self.2),
                    self.1 / f64::from(self.2)
                )
            }
        }
        let mut per_tier: BTreeMap<&str, Acc> = BTreeMap::new();
        let mut per_motif: BTreeMap<&str, Acc> = BTreeMap::new();
        for (pi, cell) in &by_arm[idx] {
            let e = &realized[*pi].0;
            per_tier.entry(e.tier.as_str()).or_default().add(cell);
            for m in &e.motifs {
                per_motif.entry(m.as_str()).or_default().add(cell);
            }
        }
        println!(
            "\n{label} ({}={:.2}) by tier:   {:<20} {:>4}  {:>8}  {:>8}  {:>8}",
            knob.label(),
            grid[idx],
            "tier",
            "n",
            "gold_agr",
            "regret",
            "resolved"
        );
        for (tier, acc) in &per_tier {
            println!("{:>31} {tier:<20} {}", "", acc.row());
        }
        if has_motifs {
            println!(
                "{label} ({}={:.2}) by motif:  {:<20} {:>4}  {:>8}  {:>8}  {:>8}",
                knob.label(),
                grid[idx],
                "motif",
                "n",
                "gold_agr",
                "regret",
                "resolved"
            );
            // Report order is the enum's, so the table reads the same way
            // every run; tags the binary does not know come last.
            let known: Vec<&str> = Motif::ALL.iter().map(|m| m.as_str()).collect();
            let mut order: Vec<&str> = known
                .iter()
                .copied()
                .filter(|k| per_motif.contains_key(k))
                .collect();
            order.extend(per_motif.keys().copied().filter(|k| !known.contains(k)));
            for m in order {
                println!("{:>31} {m:<20} {}", "", per_motif[m].row());
            }
        }
    }
    if !has_motifs {
        println!(
            "\n(no motif tags on this book — run `combat_position_book annotate` to add them)"
        );
    }

    if let Some(path) = dump {
        let mut out: Vec<DumpRow<'_>> = Vec::new();
        for (ci, arm) in by_arm.iter().enumerate() {
            for (pi, cell) in arm {
                let e = &realized[*pi].0;
                for (t, row) in cell.rows.iter().enumerate() {
                    out.push(DumpRow {
                        knob: knob.label(),
                        value: grid[ci],
                        sims: pos_sims[*pi],
                        book_id: e.book_id,
                        character: &e.character,
                        tier: &e.tier,
                        motifs: &e.motifs,
                        seed: t as u64,
                        pick: &row.pick,
                        gold: &e.gold_action,
                        agree: row.agree,
                        z: row.z,
                        regret: row.regret,
                        margin_pp: e.playout.as_ref().map(|p| p.margin_pp),
                    });
                }
            }
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&out)?)?;
        eprintln!("[sweep] wrote {} rows to {}", out.len(), path.display());
    }
    let _ = legal_actions;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // exact literals in, exact literals out
mod tests {
    use super::*;

    #[test]
    fn z_is_gap_over_combined_stderr() {
        // gap 0.10, se = sqrt(0.03² + 0.04²) = 0.05 → z = 2.0
        let z = top_two_z(&[(100, 0.60, 0.03), (100, 0.50, 0.04)]);
        assert!((z - 2.0).abs() < 1e-6, "{z}");
    }

    #[test]
    fn zero_variance_is_proven_or_tied_not_infinite() {
        assert_eq!(top_two_z(&[(10, 1.0, 0.0), (10, 0.5, 0.0)]), Z_CAP);
        assert_eq!(top_two_z(&[(10, 0.5, 0.0), (10, 0.5, 0.0)]), 0.0);
        assert_eq!(top_two_z(&[(10, 0.5, 0.1)]), 0.0);
    }

    #[test]
    fn regret_is_the_oracle_gap_and_unknown_without_a_table() {
        let table = vec![
            OracleChild {
                action: "EndTurn".into(),
                q: 0.30,
            },
            OracleChild {
                action: "PlayCard { hand_index: 0, target: None }".into(),
                q: 0.10,
            },
        ];
        assert_eq!(regret_of(&table, "EndTurn", "EndTurn"), Some(0.0));
        let r = regret_of(
            &table,
            "EndTurn",
            "PlayCard { hand_index: 0, target: None }",
        )
        .unwrap();
        assert!((r - 0.2).abs() < 1e-6, "{r}");
        // A pick the oracle never expanded has an unknown Q, not a zero one.
        assert_eq!(regret_of(&table, "EndTurn", "UsePotion { slot: 0 }"), None);
        // No table at all: unknown even on a hit, so a book without
        // `oracle_q` reports `-` rather than a spurious 0.
        assert_eq!(regret_of(&[], "EndTurn", "EndTurn"), None);
    }

    #[test]
    fn median_is_the_upper_middle_element() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&mut [4.0, 1.0, 3.0, 2.0]), 3.0);
        assert_eq!(median(&mut []), 0.0);
    }
}
