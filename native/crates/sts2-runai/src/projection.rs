//! Per-room-class state projection for pathing (plan §2 "pathing", §5/M1).
//!
//! `g(state, room_type)` — walk a candidate path's rooms through cheap
//! expected deltas (HP loss per encounter class × act, reward ΔV, gold,
//! rest/shop effects) so the pathing surface can score
//! `V(projected state at boss arrival)` per path. Without this model,
//! "expectimax with node values from V" is undefined: V at a future node
//! depends on how HP/gold/deck evolve along the path.
//!
//! Tables start heuristic (`[projection]` in `data/runai/heuristic_v.toml`)
//! and get refit from self-play audit-log statistics after the M2
//! campaign. Event rooms are chance nodes: expectation over the
//! uniform-over-remaining event pool (§2.5 rule 2 approximation —
//! E10 revisits if calibration flags it).
//!
//! §2.5: consumes only the [`VisibleRunView`] (+ its map room types).

use sts2_core::map::MapPointType;

use crate::eval::HeuristicV;
use crate::policy::surfaces::events::event_pool_expectation;
use crate::view::VisibleRunView;

/// Running projected state along a path walk.
#[derive(Clone, Copy, Debug)]
pub struct Projected {
    pub hp: f64,
    pub max_hp: f64,
    pub gold: f64,
    /// Accumulated reward / immediate ΔV terms (cards, relics, smiths…)
    /// not expressible as HP/gold.
    pub bonus_v: f64,
    /// `true` once projected HP hit <= 0 anywhere along the walk.
    pub died: bool,
}

impl Projected {
    #[must_use]
    pub fn start(view: &VisibleRunView) -> Self {
        Self {
            hp: f64::from(view.hp),
            max_hp: f64::from(view.max_hp),
            gold: f64::from(view.gold),
            bonus_v: 0.0,
            died: false,
        }
    }

    fn lose_hp(&mut self, amount: f64) {
        self.hp -= amount;
        if self.hp <= 0.0 {
            // Keep the (negative) deficit — the death penalty scales
            // with overkill so a 4-elite suicide line stays strictly
            // worse than a 1-fight death.
            self.died = true;
        }
    }
}

/// Apply one room's expected deltas to `p`. `act` is the 0-based act
/// index (clamps into the per-act tables). Boss rooms are deliberately
/// NOT applied — pathing evaluates V *at boss arrival*, the boss fight
/// itself is the same for every path.
pub fn project_room(
    p: &mut Projected,
    room: MapPointType,
    act: usize,
    v: &HeuristicV,
    view: &VisibleRunView,
) {
    let t = &v.weights.projection;
    let act = act.min(2);
    match room {
        MapPointType::Monster => {
            p.lose_hp(t.monster_hp_loss[act]);
            p.gold += t.monster_gold;
            p.bonus_v += t.monster_reward_dv;
        }
        MapPointType::Elite => {
            p.lose_hp(t.elite_hp_loss[act]);
            p.gold += t.elite_gold;
            p.bonus_v += t.elite_reward_dv;
        }
        MapPointType::RestSite => {
            // The agent will pick the better of heal vs smith there;
            // project that same max.
            let heal = (t.rest_heal_frac * p.max_hp).floor().min(p.max_hp - p.hp);
            let progress = view.act_progress();
            let heal_dv = v.hp_term((p.hp + heal) as i32, p.max_hp as i32, progress)
                - v.hp_term(p.hp as i32, p.max_hp as i32, progress);
            if heal_dv > t.smith_dv {
                p.hp = (p.hp + heal).min(p.max_hp);
            } else {
                p.bonus_v += t.smith_dv;
            }
        }
        MapPointType::Shop => p.bonus_v += t.shop_dv,
        MapPointType::Treasure => p.bonus_v += t.treasure_dv,
        MapPointType::Ancient => {
            // Chance node: uniform over the remaining (unseen) event
            // pool, per-event priors from the events surface table.
            let (hp_delta, dv) = event_pool_expectation(view, t);
            p.lose_hp(-hp_delta); // hp_delta is signed (negative = loss)
            p.bonus_v += dv + t.event_dv;
        }
        MapPointType::Unknown => {
            p.lose_hp(t.unknown_hp_loss);
            p.bonus_v += t.unknown_dv;
        }
        MapPointType::Boss | MapPointType::Unassigned => {}
    }
}

/// Apply a *grounded* combat room: the M2.5 `path_delta` probe measured
/// the committing room's HP loss directly, so use it instead of the flat
/// `*_hp_loss[act]` table value — but keep the same gold + reward ΔV terms
/// `project_room` would add. Only meaningful for `Monster` / `Elite`.
fn project_combat_room_grounded(
    p: &mut Projected,
    room: MapPointType,
    v: &HeuristicV,
    hp_loss: f64,
) {
    let t = &v.weights.projection;
    p.lose_hp(hp_loss);
    match room {
        MapPointType::Monster => {
            p.gold += t.monster_gold;
            p.bonus_v += t.monster_reward_dv;
        }
        MapPointType::Elite => {
            p.gold += t.elite_gold;
            p.bonus_v += t.elite_reward_dv;
        }
        _ => {}
    }
}

/// V-delta at projected boss arrival for a finished projection walk
/// (`hp_term(projected) − hp_term(now) + gold_term(projected) −
/// gold_term(now) + Σ bonus ΔV − death penalty`). Common terms (deck,
/// relics, …) cancel in the per-path argmax and are omitted.
fn finish_score(view: &VisibleRunView, v: &HeuristicV, p: &Projected) -> f64 {
    // HP importance evaluated at boss-arrival progress (end of this act).
    // GH #63: `act_boss_progress` reads the now-0-based view act_index.
    let boss_progress = view.act_boss_progress();
    let hp_now = v.hp_term(view.hp, view.max_hp, boss_progress);
    let hp_proj = v.hp_term(p.hp.max(1.0) as i32, view.max_hp, boss_progress);
    let gold_now = v.gold_term(view.gold);
    let gold_proj = v.gold_term(p.gold.max(0.0) as i32);

    let death = if p.died {
        // Continuous, dominated-by-construction penalty for paths whose
        // expectation runs out of HP.
        v.weights.projection.death_penalty
            * (1.0 + (-p.hp).max(0.0) / f64::from(view.max_hp.max(1)))
    } else {
        0.0
    };

    (hp_proj - hp_now) + (gold_proj - gold_now) + p.bonus_v - death
}

/// Score a whole path (room types in walk order, boss excluded). Thin
/// wrapper over [`score_path_grounded`] with no rollout grounding.
#[must_use]
pub fn score_path(view: &VisibleRunView, v: &HeuristicV, rooms: &[MapPointType]) -> f64 {
    score_path_grounded(view, v, rooms, None)
}

/// Score a path with an optional **rollout-grounded** HP loss for the
/// committing (first) room (M2.5 `path_delta`; bead `sts2-rs-5jrk9`).
/// When `grounded_first_hp` is `Some` and the first room is a combat room
/// (`Monster`/`Elite`), that room's HP loss comes from the probe instead
/// of the flat projection table; everything else (gold, reward ΔV, the
/// whole tail) is projected as usual. This keeps the path score in
/// V-units (no win-rate-vs-V scale clash) and stays tail-aware — the fix
/// for Addendum 7's broken additive path blend.
#[must_use]
pub fn score_path_grounded(
    view: &VisibleRunView,
    v: &HeuristicV,
    rooms: &[MapPointType],
    grounded_first_hp: Option<f64>,
) -> f64 {
    finish_score(
        view,
        v,
        &project_path_grounded(view, v, rooms, grounded_first_hp),
    )
}

/// Walk a path's rooms and return the **projected state at boss arrival**
/// (HP / gold / bonus_v / died) without collapsing it to a scalar — the
/// economy the M4 learned V scores a path by (deck/relics are unchanged
/// pre-commit, so V differentiates paths through their projected HP/gold;
/// bead `sts2-rs-40ikw.8`). `score_path*` is `finish_score(project_path*)`.
#[must_use]
pub fn project_path(view: &VisibleRunView, v: &HeuristicV, rooms: &[MapPointType]) -> Projected {
    project_path_grounded(view, v, rooms, None)
}

/// Like [`project_path`] but with the M2.5 rollout-grounded first-room HP
/// loss (`path_delta`) when `grounded_first_hp` is `Some`.
#[must_use]
pub fn project_path_grounded(
    view: &VisibleRunView,
    v: &HeuristicV,
    rooms: &[MapPointType],
    grounded_first_hp: Option<f64>,
) -> Projected {
    let mut p = Projected::start(view);
    // `view.act_index` is 0-based (0..=2; GH #63 normalized it at the view
    // chokepoint), which is exactly the index space of the per-act
    // `*_hp_loss` tables (`[act1, act2, act3]`). No conversion needed.
    let act = usize::from(view.act_index);
    for (i, &room) in rooms.iter().enumerate() {
        match grounded_first_hp {
            Some(hp_loss)
                if i == 0 && matches!(room, MapPointType::Monster | MapPointType::Elite) =>
            {
                project_combat_room_grounded(&mut p, room, v, hp_loss);
            }
            _ => project_room(&mut p, room, act, v, view),
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use sts2_content::run_callbacks::new_run_manager_for_character;
    use sts2_core::potions::CharacterId;

    fn fresh_view() -> VisibleRunView {
        let rm = new_run_manager_for_character(7, CharacterId::Ironclad);
        VisibleRunView::from_run_manager(&rm)
    }

    #[test]
    fn elite_path_costs_more_hp_than_monster_path() {
        let view = fresh_view();
        let v = HeuristicV::default();
        let mut monsters = Projected::start(&view);
        let mut elites = Projected::start(&view);
        for _ in 0..3 {
            project_room(&mut monsters, MapPointType::Monster, 0, &v, &view);
            project_room(&mut elites, MapPointType::Elite, 0, &v, &view);
        }
        assert!(elites.hp < monsters.hp);
        assert!(elites.bonus_v > monsters.bonus_v, "elites pay more reward");
    }

    #[test]
    fn rest_site_heals_at_low_hp() {
        let view = fresh_view().with_hp(10);
        let v = HeuristicV::default();
        let mut p = Projected::start(&view);
        project_room(&mut p, MapPointType::RestSite, 0, &v, &view);
        assert!(p.hp > 10.0, "low-HP rest projects as a heal");
    }

    #[test]
    fn lethal_projection_is_dominated() {
        let view = fresh_view().with_hp(5);
        let v = HeuristicV::default();
        let deadly = vec![MapPointType::Elite; 4];
        let safe = vec![MapPointType::Monster; 1];
        assert!(score_path(&view, &v, &deadly) < score_path(&view, &v, &safe));
    }
}
