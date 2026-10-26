//! Map-path inference utility — given an [`ActMap`] and a scraped
//! per-row `(map_point_type)` sequence, recover the column sequence
//! the player visited.
//!
//! Bead `sts2-rs-ed5nw` (R-Act1-Replay W2 — umbrella `sts2-rs-i6h9i`).
//!
//! ## Why
//!
//! `.run` scraped corpus dumps record each visited room as
//! `{map_point_type, rooms[0].model_id}` but NOT the column. To drive
//! the engine's [`crate::run_manager::RunManager`] through `Move{col}`
//! actions for replay, we need to recover the per-row column sequence.
//!
//! ## Algorithm
//!
//! Pure structural DFS over the start→boss subgraph: enumerate every
//! path through the generated map, filter to those whose visited
//! nodes' [`MapPointType`] matches the scraped sequence row-by-row,
//! return the unique survivor (or apply a "straightest path" tie-break
//! when room_type alone leaves multiple candidates).
//!
//! ### Model-id refinement (deferred)
//!
//! When two candidate paths share an identical room_type sequence but
//! the scraped corpus has distinct `model_id` strings, forward-
//! simulating each candidate's `encounter_rng` (W1, `sts2-rs-lakuh`)
//! would disambiguate them. That refinement is intentionally omitted
//! here because the current `sts2-content` Act-1 encounter pools have
//! a single entry (e.g. `act1_normal_pool()` = `[nibbit_solo]`), so
//! every Monster room produces the same model_id regardless of path.
//! Until the pool expands, model_id has zero discriminating power and
//! adds only RNG-state plumbing complexity. Filed as a follow-up
//! consideration in the bead notes; revisit when the Act-1 pool grows
//! past ~3 entries.

use crate::map::{ActMap, MapPointIdx, MapPointType};

/// Outcome of a successful path inference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceResult {
    /// One column per row, indexed 0-based by `row_index = row - 1`
    /// (i.e. `visited_cols[0]` is the column visited on row 1,
    /// `visited_cols[map_length - 1]` is the boss column). Length
    /// equals `map.map_length`.
    pub visited_cols: Vec<i32>,
    /// Count of alternate paths that ALSO matched the scraped sequence
    /// but were discarded by the tie-breaker. `0` = unambiguous match.
    /// Callers running corpus-level diagnostics should track this
    /// ratio; the W2 bead targets ≥95% unambiguous inference.
    pub ambiguous_alternates: usize,
}

/// Failure modes for [`infer_visited_cols`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceError {
    /// `scraped_room_types.len() != map.map_length`. Both must agree
    /// on the total non-entrance row count (boss row inclusive).
    LengthMismatch { scraped: usize, map_length: usize },
    /// The map has no `start` or `boss` set — caller passed a partial
    /// (un-generated) `ActMap`.
    UnboundMap,
    /// Zero start→boss paths matched the scraped room_type sequence.
    NoMatch,
}

/// Infer the column-per-row sequence corresponding to a scraped
/// `(map_point_type)` walk.
///
/// `scraped_room_types[i]` is the room type at row `i + 1` (rows are
/// 1-indexed in [`ActMap`]; row 0 is the entrance). The slice MUST
/// have length `map.map_length` — i.e. it covers every room row plus
/// the boss row.
///
/// Returns the unique matching path, or — if multiple paths share the
/// same room_type sequence — the "straightest" path measured by
/// minimum sum of absolute per-row column deltas. This mirrors the
/// "prefer column closest to last-visited col" heuristic noted in the
/// W2 bead and matches typical STS player behavior (humans don't
/// zig-zag for free).
///
/// # Errors
///
/// - [`InferenceError::LengthMismatch`] — `scraped_room_types` length
///   != `map.map_length`.
/// - [`InferenceError::UnboundMap`] — `map.start` or `map.boss` is
///   `None`.
/// - [`InferenceError::NoMatch`] — no start→boss path in the map has
///   a row-by-row room_type sequence equal to `scraped_room_types`.
pub fn infer_visited_cols(
    map: &ActMap,
    scraped_room_types: &[MapPointType],
) -> Result<InferenceResult, InferenceError> {
    let expected_len = map.map_length as usize;
    if scraped_room_types.len() != expected_len {
        return Err(InferenceError::LengthMismatch {
            scraped: scraped_room_types.len(),
            map_length: expected_len,
        });
    }
    let start = map.start.ok_or(InferenceError::UnboundMap)?;
    let boss = map.boss.ok_or(InferenceError::UnboundMap)?;

    let mut candidates: Vec<Vec<i32>> = Vec::new();
    let mut current: Vec<i32> = Vec::with_capacity(expected_len);
    dfs(
        map,
        start,
        boss,
        scraped_room_types,
        &mut current,
        &mut candidates,
    );

    match candidates.len() {
        0 => Err(InferenceError::NoMatch),
        1 => Ok(InferenceResult {
            visited_cols: candidates.into_iter().next().expect("len == 1"),
            ambiguous_alternates: 0,
        }),
        n => {
            // Tie-break: pick the path with minimum total |Δcol|. On
            // further tie, prefer lexicographically smallest col
            // sequence (deterministic; callers can compare across runs).
            candidates.sort_by(|a, b| {
                let da = total_col_delta(a);
                let db = total_col_delta(b);
                da.cmp(&db).then_with(|| a.cmp(b))
            });
            Ok(InferenceResult {
                visited_cols: candidates.into_iter().next().expect("n >= 2"),
                ambiguous_alternates: n - 1,
            })
        }
    }
}

/// Recurse from `node` toward `boss`, extending `current` (the col
/// sequence of rows visited so far) and pushing every complete match
/// into `out`.
fn dfs(
    map: &ActMap,
    node: MapPointIdx,
    boss: MapPointIdx,
    expected: &[MapPointType],
    current: &mut Vec<i32>,
    out: &mut Vec<Vec<i32>>,
) {
    let p = map.point(node);
    // Walk children sorted by col for deterministic enumeration order.
    let mut children = p.children.clone();
    children.sort_by_key(|&idx| map.point(idx).col());
    for child in children {
        let cp = map.point(child);
        let row_idx = (cp.row() - 1) as usize;
        // Defensive: an out-of-range row (e.g. a child below row 1 from
        // the entrance) would index `expected` past its bounds.
        if row_idx >= expected.len() {
            continue;
        }
        if cp.point_type != expected[row_idx] {
            continue;
        }
        current.push(cp.col());
        if child == boss {
            // Path complete.
            out.push(current.clone());
        } else {
            dfs(map, child, boss, expected, current, out);
        }
        current.pop();
    }
}

/// Sum of absolute column deltas along a path. Lower = straighter.
/// Used purely as a tie-break key when multiple paths match the
/// scraped room_type sequence.
fn total_col_delta(cols: &[i32]) -> i32 {
    cols.windows(2).map(|w| (w[1] - w[0]).abs()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{ActMap, MapCoord};

    /// Hand-built tiny map:
    ///
    /// ```text
    /// row 3:                boss(3,3)
    ///                     /          \
    /// row 2:        M(2,2)            M(4,2)
    ///               |   \           /     |
    /// row 1:        M(2,1)          M(4,1)
    ///                \   \         /     /
    /// row 0:            start(3,0)
    /// ```
    ///
    /// Two start→boss paths exist:
    /// - `[2, 2, 3]` via the left chain
    /// - `[4, 4, 3]` via the right chain
    ///
    /// Both have identical `(Monster, Monster, Boss)` room-type
    /// sequences, so inference returns the straightest (both are
    /// equally straight; the lex-smallest survives).
    fn build_diamond() -> ActMap {
        let mut m = ActMap::new(2); // map_length = 3
        let start = m.get_or_create(3, 0);
        m.start = Some(start);
        let boss = m.get_or_create(3, 3);
        m.boss = Some(boss);
        let l1 = m.get_or_create(2, 1);
        let l2 = m.get_or_create(2, 2);
        let r1 = m.get_or_create(4, 1);
        let r2 = m.get_or_create(4, 2);
        m.point_mut(l1).point_type = MapPointType::Monster;
        m.point_mut(l2).point_type = MapPointType::Monster;
        m.point_mut(r1).point_type = MapPointType::Monster;
        m.point_mut(r2).point_type = MapPointType::Monster;
        m.point_mut(boss).point_type = MapPointType::Boss;
        m.add_child(start, l1);
        m.add_child(start, r1);
        m.add_child(l1, l2);
        m.add_child(r1, r2);
        m.add_child(l2, boss);
        m.add_child(r2, boss);
        m
    }

    #[test]
    fn unique_path_recovered() {
        let mut m = build_diamond();
        // Remove the right side entirely so only the left chain remains.
        let r1_idx = m.grid[&MapCoord::new(4, 1)];
        let r2_idx = m.grid[&MapCoord::new(4, 2)];
        let start = m.start.unwrap();
        let boss = m.boss.unwrap();
        m.point_mut(start).children.retain(|&c| c != r1_idx);
        m.point_mut(r2_idx).children.retain(|&c| c != boss);
        // (Don't bother cleaning the orphaned r1/r2 nodes; DFS gates
        // on edge-reachability so they're inert.)

        let scraped = vec![
            MapPointType::Monster,
            MapPointType::Monster,
            MapPointType::Boss,
        ];
        let r = infer_visited_cols(&m, &scraped).unwrap();
        assert_eq!(r.visited_cols, vec![2, 2, 3]);
        assert_eq!(r.ambiguous_alternates, 0);
    }

    #[test]
    fn ambiguous_paths_resolved_by_straightness() {
        let m = build_diamond();
        let scraped = vec![
            MapPointType::Monster,
            MapPointType::Monster,
            MapPointType::Boss,
        ];
        let r = infer_visited_cols(&m, &scraped).unwrap();
        // Both [2,2,3] and [4,4,3] have total_col_delta = 1+1 = 2;
        // lex tie-break picks [2,2,3].
        assert_eq!(r.visited_cols, vec![2, 2, 3]);
        assert_eq!(r.ambiguous_alternates, 1);
    }

    #[test]
    fn no_match_when_room_types_diverge() {
        let mut m = build_diamond();
        // Force the left chain to Elite so it can't match a (M, M, Boss)
        // scrape; the right is still (M, M, Boss).
        let l1 = m.grid[&MapCoord::new(2, 1)];
        m.point_mut(l1).point_type = MapPointType::Elite;

        // Scrape says (Elite, Monster, Boss) — left chain row 2 is
        // Monster, so it fails too.
        let scraped = vec![MapPointType::Elite, MapPointType::Elite, MapPointType::Boss];
        assert_eq!(
            infer_visited_cols(&m, &scraped),
            Err(InferenceError::NoMatch)
        );
    }

    #[test]
    fn length_mismatch_reported() {
        let m = build_diamond();
        let scraped = vec![MapPointType::Monster, MapPointType::Boss]; // len 2 != map_length 3
        assert_eq!(
            infer_visited_cols(&m, &scraped),
            Err(InferenceError::LengthMismatch {
                scraped: 2,
                map_length: 3,
            })
        );
    }

    #[test]
    fn unbound_map_reported() {
        let m = ActMap::new(2);
        let scraped = vec![
            MapPointType::Monster,
            MapPointType::Monster,
            MapPointType::Boss,
        ];
        assert_eq!(
            infer_visited_cols(&m, &scraped),
            Err(InferenceError::UnboundMap)
        );
    }

    /// Round-trip on a real generated Act-1 map: walk any random
    /// start→boss path, record its room-type sequence, hand to
    /// `infer_visited_cols`, expect the original column sequence back
    /// (modulo ambiguous tie-breaks).
    #[test]
    fn generated_map_roundtrip() {
        use crate::map_generator::generate_act_map;
        use crate::DotNetRng as Rng;

        let mut rng = Rng::new(0xCAFEBABE_u32);
        let map = generate_act_map(15, &mut rng, 0, false, 0, None);
        let start = map.start.unwrap();
        let boss = map.boss.unwrap();

        // Walk left-most child at each step to construct a concrete
        // ground-truth path.
        let mut ground_truth = Vec::new();
        let mut cur = start;
        while cur != boss {
            let p = map.point(cur);
            let mut kids = p.children.clone();
            kids.sort_by_key(|&i| map.point(i).col());
            cur = *kids.first().expect("non-boss should have ≥1 child");
            ground_truth.push(map.point(cur).col());
        }
        assert_eq!(ground_truth.len(), map.map_length as usize);

        // Build the scraped room_type sequence from the ground-truth
        // path.
        let scraped: Vec<MapPointType> = ground_truth
            .iter()
            .enumerate()
            .map(|(i, &col)| {
                let row = (i as i32) + 1;
                map.get_point(MapCoord::new(col, row)).unwrap().point_type
            })
            .collect();

        let r = infer_visited_cols(&map, &scraped).expect("inference succeeds");
        // The inferred path must itself have the same room_type
        // sequence as the ground-truth (i.e. inference is consistent;
        // it might tie-break to a different col sequence when sibs
        // share types, which is expected behavior).
        for (i, &col) in r.visited_cols.iter().enumerate() {
            let row = (i as i32) + 1;
            let pt = map.get_point(MapCoord::new(col, row)).unwrap().point_type;
            assert_eq!(pt, scraped[i], "row {row}: inferred type mismatch");
        }
    }
}
