//! Pathing surface: exhaustive act-DAG enumeration scored by projected V
//! (plan §2 "pathing"; bead `sts2-rs-40ikw.5`).
//!
//! The act DAG is 7 columns × 15 rows with a pinned boss — distinct
//! source→boss paths are O(50–500), so every path is enumerated (no
//! pruning) and walked through `projection.rs`; each path scores
//! `V(projected state at boss arrival)` + per-room immediate terms.
//! Event rooms are chance nodes (uniform over the remaining pool, §2.5
//! rule 2). The plan replans every floor — projection error compounds —
//! so only the best path's FIRST edge ever commits (`Move { col }`).

use std::collections::BTreeMap;

use sts2_core::map::{MapCoord, MapPointType};

use crate::eval::ranker_features::ContextFeatures;
use crate::eval::run_value::ValueNet;
use crate::eval::{HeuristicV, OptionalityWeights};
use crate::projection::{project_path, score_path_grounded};
use crate::view::{MapView, VisibleRunView};

/// Defensive cap on enumerated paths (the real generator stays well
/// under this; a degenerate graph won't wedge the policy).
const MAX_PATHS: usize = 10_000;

/// All coord-paths from the player's position to the boss (exclusive —
/// the boss room itself is not part of the path). Each path starts at a
/// legal next node (child of `current`, or a start-node child before the
/// first move).
#[must_use]
pub fn enumerate_paths(map: &MapView) -> Vec<Vec<MapCoord>> {
    let roots: Vec<MapCoord> = match map.current {
        Some(cur) => map
            .nodes
            .get(&cur)
            .map(|n| n.children.clone())
            .unwrap_or_default(),
        None => map.start_children.clone(),
    };

    let mut paths = Vec::new();
    let mut stack = Vec::new();
    for root in roots {
        dfs(map, root, &mut stack, &mut paths);
        if paths.len() >= MAX_PATHS {
            break;
        }
    }
    paths
}

fn dfs(map: &MapView, coord: MapCoord, stack: &mut Vec<MapCoord>, paths: &mut Vec<Vec<MapCoord>>) {
    if paths.len() >= MAX_PATHS {
        return;
    }
    // The boss meta-node terminates a path without being part of it.
    if Some(coord) == map.boss_coord {
        paths.push(stack.clone());
        return;
    }
    stack.push(coord);
    match map.nodes.get(&coord) {
        Some(node) if !node.children.is_empty() => {
            for &child in &node.children {
                dfs(map, child, stack, paths);
            }
        }
        // Final-row node (or ghost edge): falls through to the boss.
        _ => paths.push(stack.clone()),
    }
    stack.pop();
}

/// The room type of the node a candidate first-edge `Move { col }`
/// commits to — the child of the current node (or a start node before the
/// first move) sitting at column `col`. Powers the M2.5 `path_delta`
/// escalation (which encounter the edge leads into). `None` when the
/// column isn't a legal next edge or its node is unknown.
#[must_use]
pub fn next_room_for_col(map: &MapView, col: i32) -> Option<MapPointType> {
    let children = match map.current {
        Some(cur) => map.nodes.get(&cur).map(|n| &n.children)?,
        None => &map.start_children,
    };
    let child = children.iter().find(|c| c.col == col)?;
    map.nodes.get(child).map(|n| n.room)
}

/// Room types along a coord path (unknown coords → `Unknown`).
fn rooms_of(map: &MapView, path: &[MapCoord]) -> Vec<MapPointType> {
    path.iter()
        .map(|c| map.nodes.get(c).map_or(MapPointType::Unknown, |n| n.room))
        .collect()
}

/// Optionality summary for one candidate first column, computed over the
/// reachable source→boss sub-DAG that commits to that column. Powers the
/// `elite_skip_bonus` real-option term and the pathing visualizer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColumnOptionality {
    /// Distinct source→boss paths that pass through this first column.
    pub n_paths: usize,
    /// Fewest elites on any path through the column (`0` ⇒ an elite-free
    /// route to the boss exists).
    pub min_elites: usize,
    /// Most elites on any path through the column.
    pub max_elites: usize,
}

impl ColumnOptionality {
    /// The literal "skip an elite if you want" real option: the column
    /// keeps BOTH an elite-free route (`min_elites == 0`) and an
    /// elite-bearing route (`max_elites > 0`) to the boss reachable, so the
    /// committed edge defers the fight-or-route-around decision to a later,
    /// better-informed floor.
    #[must_use]
    pub const fn can_skip_elite(self) -> bool {
        self.min_elites == 0 && self.max_elites > 0
    }
}

/// Log-sum-exp soft-max over a column's path scores at temperature `tau`.
/// `tau <= 0` (or non-finite) collapses to the plain `max` — the historical
/// hard-max aggregator, bit-for-bit. Numerically stabilized by factoring out
/// the max. Larger `tau` rewards a column with *many* good paths over one
/// with a single good path and the rest bad (the value of flexibility under
/// per-floor replanning; see [`OptionalityWeights`]).
#[must_use]
fn softmax_aggregate(scores: &[f64], tau: f64) -> f64 {
    let max = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if tau <= 0.0 || !tau.is_finite() || !max.is_finite() {
        return max;
    }
    let sum: f64 = scores.iter().map(|s| ((s - max) / tau).exp()).sum();
    max + tau * sum.ln()
}

/// Count `Elite` rooms on a coord path.
fn count_elites(map: &MapView, path: &[MapCoord]) -> usize {
    path.iter()
        .filter(|c| {
            map.nodes
                .get(c)
                .is_some_and(|n| n.room == MapPointType::Elite)
        })
        .count()
}

/// Per-first-column [`ColumnOptionality`] over the live act DAG, ascending
/// by column. Drives the visualizer and any optionality-aware reporting;
/// independent of the value function. `None` when no map / no paths.
#[must_use]
pub fn column_optionality(view: &VisibleRunView) -> Option<Vec<(i32, ColumnOptionality)>> {
    let map = view.map.as_ref()?;
    let mut by_col: BTreeMap<i32, ColumnOptionality> = BTreeMap::new();
    for p in enumerate_paths(map).iter().filter(|p| !p.is_empty()) {
        let n_elites = count_elites(map, p);
        let e = by_col.entry(p[0].col).or_insert(ColumnOptionality {
            n_paths: 0,
            min_elites: usize::MAX,
            max_elites: 0,
        });
        e.n_paths += 1;
        e.min_elites = e.min_elites.min(n_elites);
        e.max_elites = e.max_elites.max(n_elites);
    }
    if by_col.is_empty() {
        return None;
    }
    Some(by_col.into_iter().collect())
}

/// Score every path and return the best first-step column plus, for each
/// legal first column, its **optionality-aware** score (ascending column
/// order). Powers both the tier-1 pick and the audit `v_scores` / M2.5
/// `path_delta` margin gate. Uses the value function's own
/// [`OptionalityWeights`] (`v.weights.optionality`). `None` when no map /
/// no paths.
#[must_use]
pub fn scored_first_cols(view: &VisibleRunView, v: &HeuristicV) -> Option<(i32, Vec<(i32, f64)>)> {
    scored_first_cols_grounded(view, v, &BTreeMap::new())
}

/// Like [`scored_first_cols`] but each first column may carry a
/// **rollout-grounded** HP loss for its committing room (M2.5
/// `path_delta`; bead `sts2-rs-5jrk9`): when a path's first column is in
/// `grounded_hp_by_col`, its committing combat room is scored with the
/// probe's measured HP loss instead of the flat projection table (see
/// [`crate::projection::score_path_grounded`]). Columns absent from the
/// map fall back to the pure projection score, so an empty map is exactly
/// [`scored_first_cols`].
#[must_use]
pub fn scored_first_cols_grounded(
    view: &VisibleRunView,
    v: &HeuristicV,
    grounded_hp_by_col: &BTreeMap<i32, f64>,
) -> Option<(i32, Vec<(i32, f64)>)> {
    scored_first_cols_opt(view, v, grounded_hp_by_col, &v.weights.optionality)
}

/// The optionality-aware first-column scorer with an **explicit** weights
/// override (the A/B harness pins `OptionalityWeights::hard_max()` for the
/// baseline arm; production callers route through
/// [`scored_first_cols_grounded`] with `v.weights.optionality`).
///
/// Per first column the score is `softmax_tau(path scores through col) +
/// branch_bonus·ln(#paths) + elite_skip_bonus·[can skip an elite]`. With
/// `OptionalityWeights::hard_max()` this is exactly the legacy
/// `max(path scores)`, so the two arms differ only in the flexibility
/// valuation.
#[must_use]
pub fn scored_first_cols_opt(
    view: &VisibleRunView,
    v: &HeuristicV,
    grounded_hp_by_col: &BTreeMap<i32, f64>,
    opt: &OptionalityWeights,
) -> Option<(i32, Vec<(i32, f64)>)> {
    let map = view.map.as_ref()?;
    let mut scores_by_col: BTreeMap<i32, Vec<f64>> = BTreeMap::new();
    let mut elites_by_col: BTreeMap<i32, (usize, usize)> = BTreeMap::new();
    for p in enumerate_paths(map).iter().filter(|p| !p.is_empty()) {
        let col = p[0].col;
        let grounded = grounded_hp_by_col.get(&col).copied();
        let s = score_path_grounded(view, v, &rooms_of(map, p), grounded);
        scores_by_col.entry(col).or_default().push(s);
        let n_elites = count_elites(map, p);
        let e = elites_by_col.entry(col).or_insert((usize::MAX, 0));
        e.0 = e.0.min(n_elites);
        e.1 = e.1.max(n_elites);
    }
    if scores_by_col.is_empty() {
        return None;
    }
    // Death-risk gate for the elite-skip option: elites are a reward farm we
    // normally want, so the "keep an escape open" bonus only matters as HP
    // (hence survival) falls. ~0 when healthy, ramping to full when wounded.
    let hp_frac = f64::from(view.hp) / f64::from(view.max_hp.max(1));
    let skip_danger = opt.elite_skip_danger(hp_frac);
    let cols: Vec<(i32, f64)> = scores_by_col
        .into_iter()
        .map(|(col, scores)| {
            let base = softmax_aggregate(&scores, opt.tau);
            let (min_e, max_e) = elites_by_col.get(&col).copied().unwrap_or((0, 0));
            let can_skip_elite = min_e == 0 && max_e > 0;
            #[allow(clippy::cast_precision_loss)]
            let branch = opt.branch_bonus * (scores.len() as f64).ln();
            let skip = if can_skip_elite {
                opt.elite_skip_bonus * skip_danger
            } else {
                0.0
            };
            (col, base + branch + skip)
        })
        .collect();
    let best = cols
        .iter()
        .copied()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?
        .0;
    Some((best, cols))
}

/// Best first-step column only (the M1 tier-1 pick). Thin wrapper over
/// [`scored_first_cols`].
#[must_use]
pub fn best_first_col(view: &VisibleRunView, v: &HeuristicV) -> Option<i32> {
    scored_first_cols(view, v).map(|(col, _)| col)
}

/// The full route the optionality-aware planner would commit from the
/// player's current position to the boss, under *mean* (no-noise)
/// projection — i.e. "the plan from here". Each floor it commits the best
/// first edge ([`scored_first_cols`]), decays projected HP by that room's
/// expected cost (so near-boss picks see the realistic HP it will arrive
/// with), advances, and repeats until a final-row node (whose only exit is
/// the boss, never part of the route). The boss node itself is excluded.
///
/// This is the route the workbench overlays on the map (`path_plan` PyO3
/// surface). Empty when there is no map / no legal first move.
#[must_use]
pub fn planned_route(view: &VisibleRunView, v: &HeuristicV) -> Vec<MapCoord> {
    planned_route_from(view, v, None)
}

/// [`planned_route`], but with the FIRST hop forced to `first_col` when
/// given (and that column is actually a legal child); every hop after it is
/// planned normally.
///
/// This exists because the workbench paints one route on the map while a
/// *different* brain may be the one driving: with the PPO run policy
/// selected, the net picks the next room and this surface picked the rest,
/// so the pencil line could start at a node the run was never going to
/// take. Seeding the committed first move makes the drawn route the
/// continuation of the move that will actually happen instead of a second,
/// contradictory opinion about it.
///
/// An unknown/illegal `first_col` falls back to the planner's own pick, so
/// a stale recommendation can only cost the seeding, never the route.
#[must_use]
pub fn planned_route_from(
    view: &VisibleRunView,
    v: &HeuristicV,
    first_col: Option<i32>,
) -> Vec<MapCoord> {
    use crate::projection::{project_room, Projected};

    let mut view = view.clone();
    let act = usize::from(view.act_index);
    let mut route: Vec<MapCoord> = Vec::new();
    let mut forced = first_col;

    // The DAG is acyclic and bounded; the loop cap is purely defensive.
    for _ in 0..64 {
        let planned = scored_first_cols(&view, v).map(|(col, _)| col);
        // The forced column applies to the first hop only, and only while it
        // names a real child (checked below through the same `find`).
        let Some(best_col) = forced.take().or(planned) else {
            break;
        };
        let map = view.map.as_ref().expect("scored_first_cols needs a map");
        let children = match map.current {
            Some(cur) => map.nodes.get(&cur).map(|n| n.children.clone()),
            None => Some(map.start_children.clone()),
        };
        let Some(children) = children.filter(|c| !c.is_empty()) else {
            break;
        };
        let Some(coord) = children
            .iter()
            .find(|c| c.col == best_col)
            .or_else(|| children.iter().find(|c| Some(c.col) == planned))
            .copied()
        else {
            break;
        };
        let room = map
            .nodes
            .get(&coord)
            .map_or(MapPointType::Unknown, |n| n.room);
        route.push(coord);

        // Decay projected HP by the room's expected cost so the next
        // floor's replanning evaluates HP at the level it would really
        // arrive with (rest sites heal, combat rooms cost).
        let mut p = Projected::start(&view);
        project_room(&mut p, room, act, v, &view);
        #[allow(clippy::cast_possible_truncation)]
        let next_hp = p.hp.clamp(0.0, f64::from(view.max_hp)) as i32;

        let has_next = map
            .nodes
            .get(&coord)
            .is_some_and(|n| !n.children.is_empty());
        if let Some(m) = view.map.as_mut() {
            m.current = Some(coord);
        }
        view.hp = next_hp;
        if !has_next {
            break;
        }
    }
    route
}

/// Best first-step column by the **learned M4 value net** (plan §5/M4
/// "slot V into pathing EV"; bead `sts2-rs-40ikw.8`). Each path's economy
/// is projected to boss arrival (`projection::project_path`), and the V
/// scores the resulting state — current deck/relics (unchanged pre-commit)
/// with the path's projected HP/gold — so paths are ranked by the learned
/// value of where they land, not the heuristic `hp_term`/`gold_term`. The
/// projection `v` is still used to walk the rooms (HP-loss tables etc.).
/// `None` when no map / no paths.
#[must_use]
pub fn best_first_col_by_value(
    view: &VisibleRunView,
    v: &HeuristicV,
    net: &ValueNet,
) -> Option<i32> {
    scored_first_cols_by_value(view, v, net)?
        .into_iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(col, _)| col)
}

/// Per-column learned-V score (the `by_col` build behind
/// [`best_first_col_by_value`], exposed for the hybrid map-policy blend —
/// AZ M5b task 8). Each entry is the best (max) `net.value(...)` over the
/// paths whose first edge lands in that column. `None` when there's no map
/// / no paths (same gate as `best_first_col_by_value`).
#[must_use]
pub fn scored_first_cols_by_value(
    view: &VisibleRunView,
    v: &HeuristicV,
    net: &ValueNet,
) -> Option<Vec<(i32, f64)>> {
    let map = view.map.as_ref()?;
    let mut by_col: BTreeMap<i32, f64> = BTreeMap::new();
    for p in enumerate_paths(map).iter().filter(|p| !p.is_empty()) {
        let proj = project_path(view, v, &rooms_of(map, p));
        let projected = view
            .with_hp(proj.hp.max(1.0) as i32)
            .with_gold_delta((proj.gold.max(0.0) as i32) - view.gold);
        let value = net.value(&ContextFeatures::from_view(&projected));
        by_col
            .entry(p[0].col)
            .and_modify(|m| {
                if value > *m {
                    *m = value;
                }
            })
            .or_insert(value);
    }
    Some(by_col.into_iter().collect())
}

// ── Sampled-replanning expected-value pathing (learned, knob-free) ───────
//
// `best_first_col_by_value` (above) scores each first move by the learned
// net at the *single deterministic* projected boss-arrival state — the same
// collapse-to-a-point that the hand-coded soft-max worked around. The EV
// pather instead scores a first move by the **expected** net value over
// SAMPLED, replanned futures: it commits the edge, then walks to the boss
// re-deciding each floor by the net under *realized* (noisy) fight costs,
// and averages the net's value of where it lands. A first move that keeps
// good fallback routes reachable lands better on average (the agent reacts
// to bad luck by replanning onto a safe line), so optionality CAN emerge from
// the learned value function + uncertainty with no `tau` / `elite_skip_bonus`
// — the closed-loop object the soft-max only approximated in closed form.
//
// CAVEAT (measured — `examples/pathing_optionality_ab.rs --ev` + the
// 2026-06-23 experiment doc): the EV only *bites* when the value function is
// shaped so `E[V(outcome)] != V(E[outcome])`. The deployed `value_human_a0`
// net predicts EXPECTED ACT and is ~monotone-linear in HP with no
// map/optionality features, so its Jensen gap ≈ 0 and EV ≈ deterministic.
// Making optionality genuinely emerge from learning is the "features later"
// half: retrain with a risk/survival-shaped target and/or map-topology
// features. This function is the substrate that pays off once that lands.

/// Deterministic per-decision RNG (splitmix64) for the EV sampler — keeps
/// the EV reproducible given `(seed, col, sample, floor)`.
fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Lognormal multiplier (median 1, log-sigma `sigma`) from a u64 key.
fn lognormal_mult(key: u64, sigma: f64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let u = |k: u64| (splitmix(k) >> 11) as f64 / (1u64 << 53) as f64;
    let (u1, u2) = (u(key).max(1e-12), u(key ^ 0x5DEE_CE66));
    let z = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    (sigma * z).exp()
}

/// Mean projected HP after entering one `room` from `view` (rest heals,
/// combat costs) — the lookahead the replanner decides on, before luck.
fn mean_hp_after_room(view: &VisibleRunView, v: &HeuristicV, room: MapPointType) -> f64 {
    use crate::projection::{project_room, Projected};
    let mut p = Projected::start(view);
    project_room(&mut p, room, usize::from(view.act_index), v, view);
    p.hp
}

/// The child coord the net would commit from `view`'s current position —
/// the SAME full-path-to-boss lookahead as [`best_first_col_by_value`]
/// (enumerate every continuation, project each deterministically, keep the
/// per-first-coord best net landing), returning the chosen coord rather than
/// just its column. Using the identical inner policy as the deterministic
/// pather is what makes the EV vs det comparison fair: both replan with the
/// same strength, so the only difference is point-estimate vs expectation.
fn net_next_coord(view: &VisibleRunView, v: &HeuristicV, net: &ValueNet) -> Option<MapCoord> {
    let map = view.map.as_ref()?;
    let mut by_coord: BTreeMap<MapCoord, f64> = BTreeMap::new();
    for p in enumerate_paths(map).iter().filter(|p| !p.is_empty()) {
        let proj = project_path(view, v, &rooms_of(map, p));
        let projected = view
            .with_hp(proj.hp.max(1.0) as i32)
            .with_gold_delta((proj.gold.max(0.0) as i32) - view.gold);
        let value = net.value(&ContextFeatures::from_view(&projected));
        by_coord
            .entry(p[0])
            .and_modify(|m| {
                if value > *m {
                    *m = value;
                }
            })
            .or_insert(value);
    }
    by_coord
        .into_iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(c, _)| c)
}

/// Walk one sampled future from `start` (already committed) to the boss,
/// replanning each floor by the net under realized (noisy) combat costs, and
/// return the net value of the landed state. `key` seeds the cost draws.
fn sampled_future_value(
    view: &VisibleRunView,
    v: &HeuristicV,
    net: &ValueNet,
    start: MapCoord,
    key: u64,
    sigma: f64,
) -> f64 {
    let mut sub = view.clone();
    let mut coord = start;
    let mut floor = 0u64;
    loop {
        // Realize this room's HP under noise (combat rooms only; rest/other
        // keep their deterministic mean).
        let map = sub.map.as_ref().expect("ev walk needs a map");
        let room = map
            .nodes
            .get(&coord)
            .map_or(MapPointType::Unknown, |n| n.room);
        let mean_after = mean_hp_after_room(&sub, v, room);
        let loss = f64::from(sub.hp) - mean_after;
        let realized_after = if room.is_combat() && loss > 0.0 {
            f64::from(sub.hp) - loss * lognormal_mult(key ^ splitmix(floor), sigma)
        } else {
            mean_after
        };
        #[allow(clippy::cast_possible_truncation)]
        let hp = realized_after.clamp(0.0, f64::from(sub.max_hp)) as i32;
        sub = sub.with_hp(hp.max(0));
        let has_next = sub
            .map
            .as_ref()
            .and_then(|m| m.nodes.get(&coord))
            .is_some_and(|n| !n.children.is_empty());
        if let Some(m) = sub.map.as_mut() {
            m.current = Some(coord);
        }
        if sub.hp <= 0 || !has_next {
            break;
        }
        let Some(next) = net_next_coord(&sub, v, net) else {
            break;
        };
        coord = next;
        floor += 1;
        if floor > 64 {
            break;
        }
    }
    net.value(&ContextFeatures::from_view(&sub))
}

/// **Sampled-replanning expected-value** first move, scored by the learned
/// `net` (plan: "EV now, features later"). For each legal first column,
/// average the net value of `samples` sampled futures (each replanned to the
/// boss by the net under noisy fight costs); pick the column with the
/// highest mean. `sigma` is the per-fight log-noise (e.g. `0.35`). Returns
/// the best column plus every column's EV (ascending). `None` when no map /
/// no first move. Knob-free w.r.t. optionality — flexibility is *priced by
/// the net through the realized states the replanner reaches*, not by any
/// `OptionalityWeights` term.
#[must_use]
pub fn best_first_col_by_value_ev(
    view: &VisibleRunView,
    v: &HeuristicV,
    net: &ValueNet,
    samples: u32,
    sigma: f64,
    seed: u64,
) -> Option<(i32, Vec<(i32, f64)>)> {
    let map = view.map.as_ref()?;
    let roots: Vec<MapCoord> = match map.current {
        Some(cur) => map.nodes.get(&cur).map(|n| n.children.clone())?,
        None => map.start_children.clone(),
    };
    if roots.is_empty() {
        return None;
    }
    // Aggregate per column (several first nodes can share a column only
    // across acts; within one act a column maps to one child, but keep the
    // max to mirror the other scorers).
    let mut by_col: BTreeMap<i32, f64> = BTreeMap::new();
    for root in roots {
        let n = samples.max(1);
        let mut acc = 0.0;
        for s in 0..n {
            let key = seed
                ^ splitmix(u64::from(s))
                ^ splitmix((u64::from(root.col as u32) << 20) ^ u64::from(root.row as u32));
            acc += sampled_future_value(view, v, net, root, key, sigma);
        }
        let ev = acc / f64::from(n);
        by_col
            .entry(root.col)
            .and_modify(|m| {
                if ev > *m {
                    *m = ev;
                }
            })
            .or_insert(ev);
    }
    let cols: Vec<(i32, f64)> = by_col.into_iter().collect();
    let best = cols
        .iter()
        .copied()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?
        .0;
    Some((best, cols))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sts2_content::run_callbacks::new_run_manager_for_character;
    use sts2_core::potions::CharacterId;
    use sts2_core::run_manager::{Phase, RunAction};

    /// On a real generated act map: the DAG enumerates fully, every
    /// path is non-empty, and the chosen first column is one of the
    /// legal `Move` actions (M1 acceptance test).
    #[test]
    fn pathing_enumerates_real_dag_and_returns_legal_move() {
        let rm = new_run_manager_for_character(17, CharacterId::Ironclad);
        assert_eq!(rm.phase(), Phase::MapChoice);
        let view = crate::view::VisibleRunView::from_run_manager(&rm);
        let map = view.map.as_ref().expect("act map");

        let paths = enumerate_paths(map);
        assert!(!paths.is_empty(), "act DAG must have paths");
        assert!(paths.len() < MAX_PATHS, "path count must be bounded");
        for p in &paths {
            assert!(!p.is_empty());
        }

        let col = best_first_col(&view, &HeuristicV::default()).expect("a best path");
        let legal: Vec<i32> = rm
            .get_available_actions()
            .into_iter()
            .filter_map(|a| match a {
                RunAction::Move { col } => Some(col),
                _ => None,
            })
            .collect();
        assert!(
            legal.contains(&col),
            "chosen col {col} must be in legal moves {legal:?}"
        );
    }

    /// A seeded first hop is honoured, the rest of the route is still
    /// planned, and an illegal seed degrades to the planner's own pick.
    ///
    /// This is what keeps the map's pencil line and the run policy from
    /// naming different next rooms: the workbench seeds the column the
    /// driving brain (PPO) actually chose.
    #[test]
    fn planned_route_honours_a_seeded_first_hop() {
        let rm = new_run_manager_for_character(17, CharacterId::Ironclad);
        assert_eq!(rm.phase(), Phase::MapChoice);
        let view = crate::view::VisibleRunView::from_run_manager(&rm);
        let v = HeuristicV::default();

        let unseeded = planned_route(&view, &v);
        assert!(!unseeded.is_empty(), "the planner must produce a route");

        let legal: Vec<i32> = rm
            .get_available_actions()
            .into_iter()
            .filter_map(|a| match a {
                RunAction::Move { col } => Some(col),
                _ => None,
            })
            .collect();
        assert!(legal.len() > 1, "seed 17 must offer a real choice to seed");

        for col in &legal {
            let seeded = planned_route_from(&view, &v, Some(*col));
            assert_eq!(seeded[0].col, *col, "seeded first hop must commit");
            assert_eq!(
                seeded.len(),
                unseeded.len(),
                "only the first hop is seeded; the rest is still planned to the boss"
            );
        }

        // A stale/bogus seed must cost the seeding, never the route.
        let bogus = planned_route_from(&view, &v, Some(99));
        assert_eq!(bogus, unseeded);
    }

    /// `OptionalityWeights::hard_max()` reproduces the legacy max
    /// aggregator bit-for-bit: per column, the optionality score equals the
    /// single best path's score.
    #[test]
    fn hard_max_reproduces_legacy_max() {
        let rm = new_run_manager_for_character(17, CharacterId::Ironclad);
        let view = crate::view::VisibleRunView::from_run_manager(&rm);
        let v = HeuristicV::default();
        let (_, cols) =
            scored_first_cols_opt(&view, &v, &BTreeMap::new(), &OptionalityWeights::hard_max())
                .expect("scored cols");

        // Recompute the plain per-column max independently.
        let map = view.map.as_ref().unwrap();
        let mut want: BTreeMap<i32, f64> = BTreeMap::new();
        for p in enumerate_paths(map).iter().filter(|p| !p.is_empty()) {
            let s = crate::projection::score_path(&view, &v, &rooms_of(map, p));
            want.entry(p[0].col)
                .and_modify(|m| *m = f64::max(*m, s))
                .or_insert(s);
        }
        for (col, score) in cols {
            assert!(
                (score - want[&col]).abs() < 1e-9,
                "hard-max col {col} must equal legacy max"
            );
        }
    }

    /// Soft-max (tau > 0) never scores a column *below* its hard-max, and
    /// strictly exceeds it whenever the column has more than one path —
    /// the value-of-flexibility increment.
    #[test]
    fn softmax_dominates_hard_max_with_multiple_paths() {
        let rm = new_run_manager_for_character(17, CharacterId::Ironclad);
        let view = crate::view::VisibleRunView::from_run_manager(&rm);
        let v = HeuristicV::default();
        let mut soft = OptionalityWeights::hard_max();
        soft.tau = 0.2;

        let hard =
            scored_first_cols_opt(&view, &v, &BTreeMap::new(), &OptionalityWeights::hard_max())
                .unwrap()
                .1;
        let softs = scored_first_cols_opt(&view, &v, &BTreeMap::new(), &soft)
            .unwrap()
            .1;
        let opt = column_optionality(&view).unwrap();
        for ((c, hs), (cs, ss)) in hard.iter().zip(&softs) {
            assert_eq!(c, cs);
            assert!(*ss >= *hs - 1e-9, "softmax col {c} below hard-max");
            let n = opt.iter().find(|(oc, _)| oc == c).unwrap().1.n_paths;
            if n > 1 {
                assert!(*ss > *hs, "softmax col {c} with {n} paths must exceed max");
            }
        }
    }

    /// The `elite_skip_bonus` flips the pick toward an elite-skippable
    /// column when the value scores are otherwise tied. A synthetic 2-branch
    /// map: column 0 funnels through a forced elite; column 6 keeps both an
    /// elite-free and an elite-bearing route.
    #[test]
    fn elite_skip_bonus_breaks_a_tie() {
        use crate::view::{MapNode, MapView};
        let m = |col, row, room, children: &[(i32, i32)]| {
            (
                MapCoord::new(col, row),
                MapNode {
                    room,
                    children: children.iter().map(|&(c, r)| MapCoord::new(c, r)).collect(),
                },
            )
        };
        let boss = MapCoord::new(3, 3);
        // Left spine (col 0): start→Treasure→boss  (no elite at all — the
        //   highest-value route, but zero elite optionality).
        // Right spine (col 6): start→Monster→{Elite, Monster}→boss
        //   (can fight the elite for reward OR route around it).
        let nodes: BTreeMap<MapCoord, MapNode> = [
            m(0, 1, MapPointType::Treasure, &[(3, 3)]),
            m(6, 1, MapPointType::Monster, &[(5, 2), (6, 2)]),
            m(5, 2, MapPointType::Elite, &[(3, 3)]),
            m(6, 2, MapPointType::Monster, &[(3, 3)]),
        ]
        .into_iter()
        .collect();
        let mut view = crate::view::VisibleRunView::from_run_manager(
            &new_run_manager_for_character(1, CharacterId::Ironclad),
        );
        view.map = Some(MapView {
            nodes,
            current: None,
            start_children: vec![MapCoord::new(0, 1), MapCoord::new(6, 1)],
            boss_coord: Some(boss),
            boss_row: 3,
        });
        // Wounded: the elite-skip option is gated on death risk, so it only
        // bites below the HP threshold. Pin HP to 20% of max.
        view.hp = (f64::from(view.max_hp) * 0.2).round() as i32;
        let v = HeuristicV::default();

        let opt = column_optionality(&view).unwrap();
        let lo = opt.iter().find(|(c, _)| *c == 0).unwrap().1;
        let hi = opt.iter().find(|(c, _)| *c == 6).unwrap().1;
        assert!(!lo.can_skip_elite(), "col 0 has no elite to skip");
        assert!(hi.can_skip_elite(), "col 6 keeps the skip option");

        // A pure-value scorer (hard-max, no optionality) prefers col 0: the
        // treasure route takes zero HP loss and still pays a reward ΔV,
        // beating col 6's best route (which must walk a Monster and either
        // an Elite or a second Monster). Only the optionality scorer prices
        // in col 6's elite-skip flexibility — and only because we're wounded.
        let hard =
            scored_first_cols_opt(&view, &v, &BTreeMap::new(), &OptionalityWeights::hard_max())
                .unwrap()
                .0;
        let mut big = OptionalityWeights::hard_max();
        big.elite_skip_bonus = 5.0; // dominate any value gap
        big.elite_skip_hp_threshold = 0.5; // open the death-risk gate
        let opt_col = scored_first_cols_opt(&view, &v, &BTreeMap::new(), &big)
            .unwrap()
            .0;
        assert_eq!(hard, 0, "value-only pick is the safe treasure spine");
        assert_eq!(
            opt_col, 6,
            "elite-skip bonus reroutes to the optional spine"
        );

        // ...but when HEALTHY the death-risk gate zeroes the same bonus, so
        // the agent keeps farming the high-value line (no reroute): elites
        // are a reward you want unless dying is the bigger risk.
        let mut healthy = view.clone();
        healthy.hp = healthy.max_hp; // hp_frac 1.0 >= threshold → danger 0
        let healthy_col = scored_first_cols_opt(&healthy, &v, &BTreeMap::new(), &big)
            .unwrap()
            .0;
        assert_eq!(
            healthy_col, 0,
            "at full HP the gated skip bonus is dormant; keep the value pick"
        );
    }

    /// The death-risk gate ramps the elite-skip weight from 0 (healthy) to 1
    /// (0 HP), and is fully off when the threshold is non-positive.
    #[test]
    fn elite_skip_danger_ramps_with_hp() {
        let w = OptionalityWeights::default(); // threshold 0.5
        assert!(
            (w.elite_skip_danger(1.0) - 0.0).abs() < 1e-9,
            "full HP: no danger"
        );
        assert!(
            (w.elite_skip_danger(0.5) - 0.0).abs() < 1e-9,
            "at threshold: no danger"
        );
        assert!(
            (w.elite_skip_danger(0.25) - 0.5).abs() < 1e-9,
            "halfway below: 0.5"
        );
        assert!(
            (w.elite_skip_danger(0.0) - 1.0).abs() < 1e-9,
            "0 HP: full danger"
        );
        assert!(
            OptionalityWeights::hard_max().elite_skip_danger(0.0).abs() < 1e-9,
            "disabled gate (threshold 0) never fires"
        );
    }

    /// Load the deployed value net from the repo data dir, or `None` when
    /// it isn't present (keeps the test environment-tolerant).
    fn deployed_value_net() -> Option<ValueNet> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/runai/nets/value_human_a0.json");
        ValueNet::from_weights_path(&path).ok()
    }

    /// `scored_first_cols_by_value`'s max-by-value column must equal
    /// `best_first_col_by_value`'s pick — the refactor that factored the
    /// per-col scores out of the argmax must not change behavior (AZ M5b
    /// task 8). (Skips when the deployed net isn't checked out.)
    #[test]
    fn scored_first_cols_by_value_argmax_matches_best_first_col() {
        let Some(net) = deployed_value_net() else {
            eprintln!("value_human_a0.json absent — skipping scored-cols parity test");
            return;
        };
        let rm = new_run_manager_for_character(17, CharacterId::Ironclad);
        let view = crate::view::VisibleRunView::from_run_manager(&rm);
        let v = HeuristicV::default();

        let scored = scored_first_cols_by_value(&view, &v, &net).expect("scored cols");
        assert!(!scored.is_empty(), "a real act map has candidate columns");
        let want = scored
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|&(col, _)| col);

        let got = best_first_col_by_value(&view, &v, &net);
        assert_eq!(
            got, want,
            "best_first_col_by_value must match scored-cols argmax"
        );
    }

    /// The sampled-EV pather returns a legal first column, is deterministic
    /// given the seed, and prices every column finitely. (Skips when the
    /// deployed net isn't checked out.)
    #[test]
    fn value_ev_pather_is_legal_and_deterministic() {
        let Some(net) = deployed_value_net() else {
            eprintln!("value_human_a0.json absent — skipping EV pather test");
            return;
        };
        let rm = new_run_manager_for_character(17, CharacterId::Ironclad);
        let mut view = crate::view::VisibleRunView::from_run_manager(&rm);
        view.hp = (f64::from(view.max_hp) * 0.4).round() as i32; // wounded → reactive
        let v = HeuristicV::default();

        let (best, cols) =
            best_first_col_by_value_ev(&view, &v, &net, 8, 0.35, 42).expect("an EV pick");
        assert!(cols.iter().all(|(_, ev)| ev.is_finite()), "finite EVs");

        let legal: Vec<i32> = view
            .map
            .as_ref()
            .unwrap()
            .start_children
            .iter()
            .map(|c| c.col)
            .collect();
        assert!(
            legal.contains(&best),
            "EV pick {best} must be a legal first move"
        );

        // Same seed ⇒ same pick (reproducible EV).
        let (best2, _) = best_first_col_by_value_ev(&view, &v, &net, 8, 0.35, 42).unwrap();
        assert_eq!(best, best2, "EV is deterministic given the seed");
    }

    /// The planned route is a contiguous parent→child chain from a legal
    /// first move to a final-row node, and its first coord matches the
    /// optionality pick.
    #[test]
    fn planned_route_is_contiguous_and_reaches_final_row() {
        let rm = new_run_manager_for_character(5, CharacterId::Ironclad);
        let view = crate::view::VisibleRunView::from_run_manager(&rm);
        let v = HeuristicV::default();
        let route = planned_route(&view, &v);
        assert!(!route.is_empty(), "a plan exists from the start row");

        let map = view.map.as_ref().unwrap();
        assert!(
            map.start_children.contains(&route[0]),
            "route starts at a legal first move"
        );
        assert_eq!(
            route[0].col,
            best_first_col(&view, &v).unwrap(),
            "route head is the optionality first pick"
        );
        // Each step is a child of the previous node.
        for w in route.windows(2) {
            let kids = &map.nodes.get(&w[0]).unwrap().children;
            assert!(
                kids.contains(&w[1]),
                "{:?}->{:?} must be an edge",
                w[0],
                w[1]
            );
        }
        // The last node is on the final row (its only exit is the boss).
        let last = *route.last().unwrap();
        assert!(
            map.nodes.get(&last).is_some_and(|n| n.children.is_empty())
                || map
                    .nodes
                    .get(&last)
                    .is_some_and(|n| n.children.iter().all(|c| Some(*c) == map.boss_coord)),
            "route ends adjacent to the boss"
        );
    }

    /// Replanning works mid-map: every path roots at a child of the
    /// current node.
    #[test]
    fn paths_root_at_current_children() {
        let rm = new_run_manager_for_character(23, CharacterId::Ironclad);
        let view = crate::view::VisibleRunView::from_run_manager(&rm);
        let map = view.map.as_ref().expect("act map");
        let paths = enumerate_paths(map);
        for p in &paths {
            assert!(
                map.start_children.contains(&p[0]),
                "pre-first-move roots must be start children"
            );
        }
    }
}
