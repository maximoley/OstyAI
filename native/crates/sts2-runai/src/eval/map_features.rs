//! AZ M5b map-candidate encoder. One legal next column → a fixed
//! `[f32; N_MAP_CAND_FEAT]` (node-type one-hot + normalized column). The
//! run-MCTS carries the lookahead; this encoding only lets the map head
//! distill the visit distribution, so it is deliberately minimal.

use serde::{Deserialize, Serialize};
use sts2_core::map::MapPointType;
use sts2_core::run_manager::RunAction;

use crate::policy::surfaces::pathing::next_room_for_col;
use crate::view::VisibleRunView;

/// 8-way node-type one-hot + 1 normalized column.
pub const N_MAP_CAND_FEAT: usize = 9;

/// Bump on ANY change to the map candidate encoding (added feature,
/// reordered one-hot, changed normalization). The unified net file
/// validates this on load so a stale map head refuses to run.
const MAP_SCHEMA: &str = "sts2-runai-map-v1:8onehot+col";

#[must_use]
pub fn map_feature_schema_id() -> &'static str {
    MAP_SCHEMA
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapCandFeatures {
    pub room: MapPointType,
    pub col: i32,
    pub n_cols: u8,
}

impl MapCandFeatures {
    #[must_use]
    pub fn feat_vec(&self) -> [f32; N_MAP_CAND_FEAT] {
        let mut v = [0.0f32; N_MAP_CAND_FEAT];
        // One-hot slot order is FROZEN (schema-guarded).
        let slot = match self.room {
            MapPointType::Monster => 0,
            MapPointType::Elite => 1,
            MapPointType::RestSite => 2,
            MapPointType::Shop => 3,
            MapPointType::Treasure => 4,
            MapPointType::Ancient => 5,
            MapPointType::Boss => 6,
            // Unknown / Unassigned / anything else.
            _ => 7,
        };
        v[slot] = 1.0;
        let denom = f32::from(self.n_cols.saturating_sub(1)).max(1.0);
        v[8] = self.col as f32 / denom;
        v
    }
}

/// Encode every legal `Move { col }` in `actions` (in `actions` order) as a
/// `MapCandFeatures`. `None` off the map surface (no view map, or < 2 moves).
#[must_use]
pub fn map_candidates_for_actions(
    view: &VisibleRunView,
    actions: &[RunAction],
) -> Option<Vec<(usize, MapCandFeatures)>> {
    let map = view.map.as_ref()?;
    let n_cols =
        u8::try_from(map.nodes.keys().map(|c| c.col).max().unwrap_or(0) + 1).unwrap_or(u8::MAX);
    let out: Vec<(usize, MapCandFeatures)> = actions
        .iter()
        .enumerate()
        .filter_map(|(i, a)| match a {
            RunAction::Move { col } => {
                let room = next_room_for_col(map, *col).unwrap_or(MapPointType::Unknown);
                Some((
                    i,
                    MapCandFeatures {
                        room,
                        col: *col,
                        n_cols,
                    },
                ))
            }
            _ => None,
        })
        .collect();
    if out.len() < 2 {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onehot_and_column_norm() {
        let f = MapCandFeatures {
            room: MapPointType::Elite,
            col: 3,
            n_cols: 7,
        };
        let v = f.feat_vec();
        assert!((v[1] - 1.0).abs() < f32::EPSILON, "Elite → slot 1");
        assert!(
            (v.iter().take(8).sum::<f32>() - 1.0).abs() < f32::EPSILON,
            "exactly one hot"
        );
        assert!(
            (v[8] - 3.0 / 6.0).abs() < 1e-6,
            "col normalized by n_cols-1"
        );
    }

    #[test]
    fn unassigned_falls_into_unknown_slot() {
        let v = MapCandFeatures {
            room: MapPointType::Unassigned,
            col: 0,
            n_cols: 1,
        }
        .feat_vec();
        assert!(
            (v[7] - 1.0).abs() < f32::EPSILON,
            "Unassigned → Unknown slot 7"
        );
        assert!((v[8]).abs() < f32::EPSILON, "col 0 / max(1,denom) = 0");
    }

    #[test]
    fn map_candidates_encode_the_legal_moves() {
        use sts2_core::run_manager::{Phase, RunAction};
        crate::rollout::ensure_content_installed();
        let mut rm = sts2_content::run_callbacks::new_run_manager_for_character_with_ascension(
            7,
            sts2_core::potions::CharacterId::Ironclad,
            0,
        )
        .with_neow_start();
        sts2_content::run_callbacks::initialize_run_setup(&mut rm);
        // Step via first legal action until the first map choice (bounded).
        for _ in 0..50 {
            let actions = rm.get_available_actions();
            if rm.phase() == Phase::MapChoice {
                let view = VisibleRunView::from_run_manager(&rm);
                let cands = map_candidates_for_actions(&view, &actions)
                    .expect("map choice offers >=2 moves");
                let n_moves = actions
                    .iter()
                    .filter(|a| matches!(a, RunAction::Move { .. }))
                    .count();
                assert_eq!(cands.len(), n_moves, "one candidate per legal Move");
                for (i, _) in &cands {
                    assert!(
                        matches!(actions[*i], RunAction::Move { .. }),
                        "action_idx points at a Move"
                    );
                }
                return;
            }
            if actions.is_empty() {
                break;
            }
            rm.take_action(actions[0]);
        }
        panic!("did not reach a MapChoice within 50 steps");
    }
}
