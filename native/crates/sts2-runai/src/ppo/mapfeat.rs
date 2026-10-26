//! R5b map representation — Rust port of `sts2-pycli/python/ppo/mapfeat.py`
//! (porting authority; never modify that file).
//!
//! From a decision's `full_map` obs + current coord + offered move
//! options, compute per-node and per-option features that turn "what
//! does this path lead to?" into a lookup:
//!
//! - per node: room type, ego-relative `(dx, dy) = node - current`,
//!   forward-reachable flag, and the `[0, 1]`-scaled reverse-topo DAG
//!   aggregates (min/max elites-ahead, distance-to-rest);
//! - per move option: destination room + the destination node's
//!   forward-cone aggregate;
//! - for the act: the identity of the boss the map terminates in
//!   ([`MapFeatures::boss_idx`]), which the encoder puts on the state
//!   token of every decision.
//!
//! All aggregates are scaled to `[0, 1]` by [`MAP_AGG_CAP`] (unscaled
//! magnitudes drown the additive token sum — see `mapfeat.py`'s
//! module docstring for the original finding).
//!
//! **Correspondence traps** (get these wrong and every downstream
//! feature silently shifts):
//! - Python's node dict is INSERTION-ORDERED: rows ascending, nodes in
//!   a row's array order, synthetic boss node appended LAST. This
//!   module uses an order-preserving [`OrderedNodes`], never a
//!   `BTreeMap`/`HashMap` for node order.
//! - `_dag_aggregates` walks reverse-topo by DESCENDING row using a
//!   STABLE sort over the insertion order (ties keep their original
//!   relative order) — [`dag_aggregates`] uses `Vec::sort_by_key`
//!   (stable) for the same reason.
//! - Top-row nodes with empty children get a synthetic edge to the
//!   boss node (the UI schema omits that edge).
//! - `seeds` = offered destinations ∩ nodes, ∪ current node;
//!   `reachable` = ALL nodes when `seeds` is empty, else the forward
//!   flood from `seeds`.
//! - `(cx, cy) = (-1, -1)` when there is no current coordinate (act
//!   start); `dest_row = cur.1 + 1` when a current coord exists, else
//!   `0`.

use std::collections::{BTreeMap, BTreeSet};

use super::obs::FullMapObs;
use super::vocab;

/// Dist-to-rest cap and the `[0, 1]` scale divisor (mirrors
/// `mapfeat.py::MAP_AGG_CAP`).
pub const MAP_AGG_CAP: i64 = 15;

/// Per-map-node R5b features (ego-relative position + reverse-topo DAG
/// aggregates). Mirrors `mapfeat.py::NodeFeat`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeFeat {
    pub coord: (i64, i64),
    pub room_idx: usize,
    pub dx: f64,
    pub dy: f64,
    /// 0.0/1.0 forward-reachability flag.
    pub reachable: f64,
    pub min_elites: f64,
    pub max_elites: f64,
    pub dist_rest: f64,
    /// Boss-registry index on the terminal boss node; the pad row
    /// (`vocab::bosses().size`) on every other node. See
    /// [`MapFeatures::boss_idx`].
    pub boss_idx: usize,
}

/// Per-move-option forward-cone features: same aggregates as
/// [`NodeFeat`], minus `coord`/`reachable`, plus `dest_room_idx`.
/// Mirrors `mapfeat.py::OptionConeFeat`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OptionConeFeat {
    pub dest_room_idx: usize,
    pub dx: f64,
    pub dy: f64,
    pub min_elites: f64,
    pub max_elites: f64,
    pub dist_rest: f64,
}

/// One decision's full R5b feature set. `options` is aligned 1:1 with
/// a map `Decision`'s `Move{col}` candidates in action order (empty
/// for non-path decisions). Mirrors `mapfeat.py::MapFeatures`.
#[derive(Debug, Clone, PartialEq)]
pub struct MapFeatures {
    pub nodes: Vec<NodeFeat>,
    pub options: Vec<OptionConeFeat>,
    /// Boss-registry index for the boss at the top of THIS act's map,
    /// or the pad row when the engine could not resolve one. The
    /// encoder writes it on the state token of every decision, which is
    /// what lets a card-removal or upgrade choice be made with the
    /// upcoming boss in view.
    pub boss_idx: usize,
}

/// One converted map node: snake-case room type + child coords.
/// Mirrors the Python `{"type": snake, "children": [(col,row), ...]}`
/// inner dict `nodes_from_full_map` builds.
struct NodeInfo {
    room_type: String,
    children: Vec<(i64, i64)>,
    /// The boss encounter id on the terminal boss node, empty on every
    /// other node. Mirrors the Python inner dict's `"boss_id"` key.
    boss_id: String,
}

/// Insertion-ordered `(coord -> NodeInfo)` table — the Rust stand-in
/// for Python's insertion-ordered `dict[tuple[int,int], dict]`.
/// `order` is the ground truth for iteration order; `info` is the
/// companion `BTreeMap` index for lookups (a map's node count is
/// small/bounded — well under a hundred — so a `BTreeMap` keeps this
/// off the `HashMap`-cardinality carve-out without costing anything
/// observable). Re-inserting an existing coord updates its value
/// WITHOUT moving its position, exactly like a Python dict
/// re-assignment (`nodes[coord] = ...` never reorders an existing
/// key).
struct OrderedNodes {
    order: Vec<(i64, i64)>,
    info: BTreeMap<(i64, i64), NodeInfo>,
}

impl OrderedNodes {
    fn new() -> Self {
        Self {
            order: Vec::new(),
            info: BTreeMap::new(),
        }
    }

    fn insert(&mut self, coord: (i64, i64), info: NodeInfo) {
        if !self.info.contains_key(&coord) {
            self.order.push(coord);
        }
        self.info.insert(coord, info);
    }

    fn contains(&self, coord: &(i64, i64)) -> bool {
        self.info.contains_key(coord)
    }

    fn get(&self, coord: &(i64, i64)) -> Option<&NodeInfo> {
        self.info.get(coord)
    }

    fn get_mut(&mut self, coord: &(i64, i64)) -> Option<&mut NodeInfo> {
        self.info.get_mut(coord)
    }

    fn iter(&self) -> impl Iterator<Item = (&(i64, i64), &NodeInfo)> {
        self.order.iter().map(move |c| (c, &self.info[c]))
    }

    fn len(&self) -> usize {
        self.order.len()
    }
}

/// `full_map()`'s UI PascalCase room type -> vocab snake form
/// (`"RestSite"` -> `"rest_site"`). Mirrors `mapfeat.py::_pascal_to_snake`
/// EXACTLY (deliberately simpler than `vocab::canonical`'s boundary
/// rule — no digit/no run-of-caps handling, just "uppercase and not
/// the first char").
fn pascal_to_snake(t: &str) -> String {
    let mut out = String::with_capacity(t.len() + 4);
    for (i, ch) in t.chars().enumerate() {
        if ch.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(ch.to_lowercase());
    }
    out
}

/// Convert `full_map`'s nested-rows schema into the order-preserving
/// node table the DAG DP consumes. Includes the terminal boss node (no
/// children); top-row nodes with empty children get a synthetic edge
/// to it. Mirrors `mapfeat.py::nodes_from_full_map`.
///
/// The boss node keeps its `boss_id`. It used to be dropped here — the
/// node was rewritten as a bare `{"type": "boss"}` — which is why the
/// identity of the act's boss never reached the policy despite the
/// engine supplying it on every observation.
fn nodes_from_full_map(full_map: Option<&FullMapObs>) -> OrderedNodes {
    let mut nodes = OrderedNodes::new();
    let Some(fm) = full_map else {
        return nodes;
    };
    for row in &fm.rows {
        for n in row {
            let coord = (n.col, n.row);
            let children = n.children.iter().map(|c| (c.col, c.row)).collect();
            nodes.insert(
                coord,
                NodeInfo {
                    room_type: pascal_to_snake(&n.room_type),
                    children,
                    boss_id: String::new(),
                },
            );
        }
    }
    if let Some(boss) = &fm.boss {
        let bcoord = (boss.col, boss.row);
        // top_y computed over the pre-boss node set, exactly like
        // Python's `max((c[1] for c in nodes), default=-1)` evaluated
        // before `nodes[bcoord] = ...` is assigned. Note that at A10 the
        // second-boss node (one row ABOVE the boss) is an ordinary row
        // node here, so it — not the top room row — is `top_y`, and the
        // synthetic edge it receives points DOWN at the boss. That
        // backwards edge is dropped by `dag_aggregates`, matching
        // `mapfeat.py`.
        let top_row = nodes.order.iter().map(|c| c.1).max();
        nodes.insert(
            bcoord,
            NodeInfo {
                room_type: "boss".to_string(),
                children: Vec::new(),
                boss_id: boss_id_of(full_map),
            },
        );
        if let Some(top_row) = top_row {
            let candidates: Vec<(i64, i64)> = nodes
                .order
                .iter()
                .copied()
                .filter(|c| c.1 == top_row && *c != bcoord)
                .collect();
            for c in candidates {
                if let Some(info) = nodes.get_mut(&c) {
                    if info.children.is_empty() {
                        info.children = vec![bcoord];
                    }
                }
            }
        }
    }
    nodes
}

/// Reverse-topo DP over rows: `(min_elites, max_elites, dist_rest)`
/// per node. `elites-ahead = (1 if this node is an elite) + min/max
/// over live children`; `dist_rest = 0` at a rest node else `1 + min
/// over children`, capped at [`MAP_AGG_CAP`]. Processes rows
/// DESCENDING (children before parents) via a STABLE sort over
/// insertion order — ties (equal row) keep their original relative
/// order, mirroring Python's `sorted(nodes, key=lambda c: -c[1])`
/// (stable) over a dict whose iteration order IS insertion order.
/// Mirrors `mapfeat.py::_dag_aggregates`.
///
/// Children are restricted to those ALREADY aggregated (`agg`), not
/// merely present in `nodes` — mirroring the same guard in
/// `mapfeat.py::_dag_aggregates`. A child on the parent's own row, or
/// any edge the descending-row order has not reached yet, would
/// otherwise index an absent key. That is not hypothetical: at A10 the
/// DoubleBoss second-boss node (`ActMap::attach_second_boss`) sits one
/// row above the boss and `nodes_from_full_map` hands it a synthetic
/// edge pointing back DOWN at the boss, so every A10 run that reached
/// the final act's map panicked here with "no entry found for key".
/// Below A10 every edge points strictly upward, so the guard is inert
/// and the features are unchanged.
fn dag_aggregates(nodes: &OrderedNodes) -> BTreeMap<(i64, i64), (i64, i64, i64)> {
    let mut order = nodes.order.clone();
    order.sort_by_key(|c| std::cmp::Reverse(c.1));

    let mut agg: BTreeMap<(i64, i64), (i64, i64, i64)> = BTreeMap::new();
    for coord in order {
        let info = nodes
            .get(&coord)
            .expect("coord sourced from nodes.order is always present");
        let is_elite = i64::from(info.room_type == "elite");
        let is_rest = info.room_type == "rest_site";
        let children: Vec<(i64, i64)> = info
            .children
            .iter()
            .copied()
            .filter(|c| agg.contains_key(c))
            .collect();
        let (mn, mx, dr) = if children.is_empty() {
            (is_elite, is_elite, if is_rest { 0 } else { MAP_AGG_CAP })
        } else {
            let cmin = children
                .iter()
                .map(|c| agg[c].0)
                .min()
                .expect("non-empty children");
            let cmax = children
                .iter()
                .map(|c| agg[c].1)
                .max()
                .expect("non-empty children");
            let cdr = children
                .iter()
                .map(|c| agg[c].2)
                .min()
                .expect("non-empty children");
            let dr = if is_rest { 0 } else { MAP_AGG_CAP.min(1 + cdr) };
            (is_elite + cmin, is_elite + cmax, dr)
        };
        agg.insert(coord, (mn, mx, dr));
    }
    agg
}

/// Forward flood-fill of nodes reachable from `seeds` (inclusive).
/// Mirrors `mapfeat.py::_reachable` (a stack-based DFS; the result is
/// a set, so pop/push order doesn't affect the outcome).
fn reachable(nodes: &OrderedNodes, seeds: &BTreeSet<(i64, i64)>) -> BTreeSet<(i64, i64)> {
    let mut seen: BTreeSet<(i64, i64)> = BTreeSet::new();
    let mut stack: Vec<(i64, i64)> = seeds
        .iter()
        .copied()
        .filter(|s| nodes.contains(s))
        .collect();
    while let Some(n) = stack.pop() {
        if seen.contains(&n) {
            continue;
        }
        seen.insert(n);
        if let Some(info) = nodes.get(&n) {
            for c in &info.children {
                if nodes.contains(c) && !seen.contains(c) {
                    stack.push(*c);
                }
            }
        }
    }
    seen
}

/// The current act's boss encounter id (`"THE_KIN_BOSS"`), or `""`.
///
/// `full_map()`'s boss `id` is the engine's own resolution chain
/// (pinned boss > generated roll > single-boss-pool fallback), so it is
/// present from the first floor of every act — the boss is decided at
/// map-generation time, long before the player walks to it. Empty means
/// the engine could not resolve one, which callers turn into the
/// registry's pad row rather than guessing. Mirrors
/// `mapfeat.py::boss_id_of`.
#[must_use]
pub fn boss_id_of(full_map: Option<&FullMapObs>) -> String {
    full_map
        .and_then(|fm| fm.boss.as_ref())
        .and_then(|b| b.id.clone())
        .unwrap_or_default()
}

/// Clamp `v / MAP_AGG_CAP` to `[0, 1]`. Mirrors `mapfeat.py::_scale`.
fn scale(v: i64) -> f64 {
    (v as f64 / MAP_AGG_CAP as f64).clamp(0.0, 1.0)
}

/// Build R5b features from a decision's `full_map` obs, current
/// coord, and offered `Move{col}` destination columns (in action
/// order; empty for non-path decisions). `dest_row = cur.1 + 1` when
/// `map_coord` is `Some`, else `0` (act-start `MapChoice` semantics).
/// Mirrors `mapfeat.py::compute`.
#[must_use]
pub fn compute(
    full_map: Option<&FullMapObs>,
    map_coord: Option<(i64, i64)>,
    move_cols: &[i64],
) -> MapFeatures {
    let nodes = nodes_from_full_map(full_map);
    let boss_pad = vocab::bosses().size;
    let raw_boss = boss_id_of(full_map);
    let boss_idx = if raw_boss.is_empty() {
        boss_pad
    } else {
        vocab::bosses().index(&raw_boss)
    };
    let cur = map_coord;
    let agg = dag_aggregates(&nodes);

    let dest_row = cur.map_or(0, |c| c.1 + 1);
    let opt_dests: Vec<(i64, i64)> = move_cols.iter().map(|&c| (c, dest_row)).collect();

    let mut seeds: BTreeSet<(i64, i64)> = opt_dests
        .iter()
        .copied()
        .filter(|d| nodes.contains(d))
        .collect();
    if let Some(c) = cur {
        if nodes.contains(&c) {
            seeds.insert(c);
        }
    }
    let reachable_set: BTreeSet<(i64, i64)> = if seeds.is_empty() {
        nodes.order.iter().copied().collect()
    } else {
        reachable(&nodes, &seeds)
    };

    let (cx, cy) = cur.unwrap_or((-1, -1));

    let mut node_feats = Vec::with_capacity(nodes.len());
    for (coord, info) in nodes.iter() {
        let (mn, mx, dr) = agg[coord];
        node_feats.push(NodeFeat {
            coord: *coord,
            room_idx: vocab::room_index(&info.room_type),
            dx: (coord.0 - cx) as f64,
            dy: (coord.1 - cy) as f64,
            reachable: if reachable_set.contains(coord) {
                1.0
            } else {
                0.0
            },
            min_elites: scale(mn),
            max_elites: scale(mx),
            dist_rest: scale(dr),
            boss_idx: if info.boss_id.is_empty() {
                boss_pad
            } else {
                vocab::bosses().index(&info.boss_id)
            },
        });
    }

    let mut opt_feats = Vec::with_capacity(opt_dests.len());
    for d in &opt_dests {
        let (mn, mx, dr) = agg.get(d).copied().unwrap_or((0, 0, MAP_AGG_CAP));
        let dest_room = nodes
            .get(d)
            .map_or_else(|| "unknown".to_string(), |i| i.room_type.clone());
        opt_feats.push(OptionConeFeat {
            dest_room_idx: vocab::room_index(&dest_room),
            dx: (d.0 - cx) as f64,
            dy: (d.1 - cy) as f64,
            min_elites: scale(mn),
            max_elites: scale(mx),
            dist_rest: scale(dr),
        });
    }

    MapFeatures {
        nodes: node_feats,
        options: opt_feats,
        boss_idx,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ppo::obs::{BossNodeObs, Coord, FullMapObs, MapNodeObs};

    fn coord(col: i64, row: i64) -> Coord {
        Coord { col, row }
    }

    fn node(col: i64, row: i64, room_type: &str, children: Vec<Coord>) -> MapNodeObs {
        MapNodeObs {
            col,
            row,
            room_type: room_type.to_string(),
            children,
            visited: false,
            current: false,
        }
    }

    /// row0: (0,0) monster -> (0,1); (1,0) elite -> (1,1)
    /// row1: (0,1) rest_site; (1,1) monster   (top row, children empty -> boss edge)
    /// boss at (0,2)
    fn mini_map() -> FullMapObs {
        FullMapObs {
            rows: vec![
                vec![
                    node(0, 0, "Monster", vec![coord(0, 1)]),
                    node(1, 0, "Elite", vec![coord(1, 1)]),
                ],
                vec![
                    node(0, 1, "RestSite", vec![]),
                    node(1, 1, "Monster", vec![]),
                ],
            ],
            boss: Some(BossNodeObs {
                col: 0,
                row: 2,
                room_type: "Boss".to_string(),
                kind: None,
                id: None,
            }),
            // Below A10 — this fixture is a plain single-boss act.
            second_boss: None,
            current_coord: None,
        }
    }

    #[test]
    fn dag_aggregates_and_boss_edge() {
        let m = mini_map();
        let f = compute(Some(&m), Some((0, 0)), &[0, 1]);
        // node order: row0 left..right, row1 left..right, boss LAST
        assert_eq!(f.nodes.len(), 5);
        assert_eq!(f.nodes[4].room_idx, crate::ppo::vocab::room_index("boss"));
        // elite child of (1,0): min=max=1 elite ahead from (1,0)'s cone? verify by hand:
        // (1,0) is elite, children [(1,1)] -> min=max=1+0=1 -> scaled 1/15
        let e = &f.nodes[1];
        assert!((e.min_elites - 1.0 / 15.0).abs() < 1e-12);
        // rest node dist_rest == 0
        let r = &f.nodes[2];
        assert!(r.dist_rest.abs() < 1e-12);
        // option cones align with move_cols order
        assert_eq!(f.options.len(), 2);
        assert_eq!(
            f.options[0].dest_room_idx,
            crate::ppo::vocab::room_index("rest_site")
        );
    }

    /// A10 DoubleBoss final-act shape: a second Boss-typed node one row
    /// ABOVE the boss, surfaced as an ordinary row node by
    /// `full_map_view` (only `ActMap::boss` is grid-exempt). Before the
    /// row filter in `nodes_from_full_map` this made the second boss the
    /// "top row", gave it a synthetic edge DOWN to the boss, and
    /// panicked `dag_aggregates` with "no entry found for key" — killing
    /// every A10 run that reached the act-3 map.
    fn double_boss_map() -> FullMapObs {
        let mut m = mini_map();
        m.rows.push(vec![node(0, 3, "Boss", vec![])]);
        m
    }

    #[test]
    fn a10_second_boss_node_does_not_panic_the_dag() {
        let m = double_boss_map();
        let f = compute(Some(&m), Some((0, 0)), &[0, 1]);
        // 4 room nodes + second boss (row order) + synthetic boss LAST.
        assert_eq!(f.nodes.len(), 6);
        assert_eq!(
            f.nodes[5].room_idx,
            crate::ppo::vocab::room_index("boss"),
            "the synthetic boss node is still appended last"
        );
    }

    /// Values transcribed from running `mapfeat.py::compute` (the
    /// porting authority) on this exact map — the A10 shape is where
    /// the two implementations used to disagree, so pin it numerically
    /// rather than by reasoning about it.
    #[test]
    fn a10_second_boss_aggregates_match_the_python_authority() {
        let f = compute(Some(&double_boss_map()), Some((0, 0)), &[0, 1]);
        let cap = MAP_AGG_CAP as f64;
        // (col, row, min_elites, max_elites, dist_rest) — unscaled.
        let expect: [(i64, i64, f64, f64, f64); 6] = [
            (0, 0, 0.0, 0.0, 1.0), // (0,0) monster -> rest at (0,1)
            (1, 0, 1.0, 1.0, cap), // (1,0) elite -> (1,1), no rest ahead
            (0, 1, 0.0, 0.0, 0.0), // rest site
            (1, 1, 0.0, 0.0, cap), // top ROOM row: no boss edge at A10
            (0, 3, 0.0, 0.0, cap), // second boss: its down-edge is dropped
            (0, 2, 0.0, 0.0, cap), // synthetic boss node, appended last
        ];
        for (i, (col, row, mn, mx, dr)) in expect.into_iter().enumerate() {
            let n = &f.nodes[i];
            assert_eq!(n.coord, (col, row), "node {i} coord");
            assert!(
                (n.min_elites - mn / cap).abs() < 1e-12,
                "node {i} min_elites"
            );
            assert!(
                (n.max_elites - mx / cap).abs() < 1e-12,
                "node {i} max_elites"
            );
            assert!((n.dist_rest - dr / cap).abs() < 1e-12, "node {i} dist_rest");
        }
    }

    /// The act's boss identity reaches [`MapFeatures::boss_idx`] and the
    /// terminal boss node, and NOTHING else. Before this, the boss node was
    /// rewritten as a bare `{"type": "boss"}` and the id was dropped on the
    /// floor even though `full_map()` supplies it on every observation.
    #[test]
    fn the_boss_id_reaches_the_features_and_only_the_boss_node() {
        let mut m = mini_map();
        m.boss.as_mut().expect("mini_map has a boss").id = Some("THE_KIN_BOSS".to_string());
        let f = compute(Some(&m), Some((0, 0)), &[0, 1]);

        let row = vocab::bosses().index("THE_KIN_BOSS");
        assert!(row < vocab::bosses().oov, "THE_KIN_BOSS must not be OOV");
        assert_eq!(f.boss_idx, row);
        // The synthetic boss node is appended last; every other node pads.
        let pad = vocab::bosses().size;
        let named: Vec<usize> = f
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.boss_idx != pad)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(named, vec![f.nodes.len() - 1]);
        assert_eq!(f.nodes[f.nodes.len() - 1].boss_idx, row);
    }

    /// A map whose boss the engine could not name costs the feature, never
    /// the decision — it must land on the PAD row (which contributes zero
    /// through `padding_idx`), not on OOV, which is a real trainable row
    /// meaning "a boss I do not recognize".
    #[test]
    fn an_unresolved_boss_pads_rather_than_landing_on_oov() {
        let f = compute(Some(&mini_map()), Some((0, 0)), &[0, 1]);
        assert_eq!(f.boss_idx, vocab::bosses().size);
        assert!(f.nodes.iter().all(|n| n.boss_idx == vocab::bosses().size));
    }

    #[test]
    fn act_start_has_no_current_coord() {
        let m = mini_map();
        let f = compute(Some(&m), None, &[0]);
        // dest_row = 0, (cx,cy) = (-1,-1)
        assert!((f.options[0].dx - 1.0).abs() < 1e-12); // 0 - (-1)
    }

    #[test]
    fn empty_full_map_yields_no_nodes() {
        let f = compute(None, None, &[]);
        assert!(f.nodes.is_empty());
        assert!(f.options.is_empty());
    }

    #[test]
    fn empty_seeds_makes_every_node_reachable() {
        // move_cols pointing off-map (no offered dests land in `nodes`)
        // and no current coord -> seeds is empty -> every node reachable.
        let m = mini_map();
        let f = compute(Some(&m), None, &[99]);
        assert!(f.nodes.iter().all(|n| (n.reachable - 1.0).abs() < 1e-12));
    }
}
