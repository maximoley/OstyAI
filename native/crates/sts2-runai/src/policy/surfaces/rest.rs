//! Rest-site surface: heal-vs-smith V comparison (plan §2 "rest";
//! bead `sts2-rs-40ikw.5`).
//!
//! Heal value = ΔV of the engine's `floor(max_hp * 0.3)` heal through
//! the HP term; smith value = best upgrade ΔV over the non-upgraded
//! deck ([`picks::best_upgrade`]). Low HP ⇒ the steep HP term wins
//! (heal); high HP ⇒ the heal is mostly wasted and the smith wins.

use crate::eval::HeuristicV;
use crate::policy::surfaces::picks;
use crate::view::VisibleRunView;

/// Rest-site option indices in the engine's base option list
/// (`[Rest, Smith]` — see `RunManager::get_available_actions`).
pub const REST_IDX: usize = 0;
pub const SMITH_IDX: usize = 1;

/// Pick rest (heal) vs smith (upgrade). Returns the option index plus
/// the `[heal_dv, smith_dv]` scores for the audit log.
#[must_use]
pub fn best_rest_option(view: &VisibleRunView, v: &HeuristicV) -> (usize, Vec<f64>) {
    let heal_amount = (f64::from(view.max_hp) * v.weights.projection.rest_heal_frac).floor() as i32;
    let healed = view.with_hp(view.hp + heal_amount);
    let heal_dv = v.score(&healed) - v.score(view);

    let eligible = picks::non_upgraded_deck_indices(view);
    let smith_dv =
        picks::best_upgrade(view, v, &eligible).map_or(f64::NEG_INFINITY, |(_, dv, _)| dv);

    if smith_dv > heal_dv && !eligible.is_empty() {
        (SMITH_IDX, vec![heal_dv, smith_dv])
    } else {
        (REST_IDX, vec![heal_dv, smith_dv])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sts2_content::run_callbacks::new_run_manager_for_character;
    use sts2_core::potions::CharacterId;

    fn fresh_view() -> VisibleRunView {
        let rm = new_run_manager_for_character(37, CharacterId::Ironclad);
        crate::view::VisibleRunView::from_run_manager(&rm)
    }

    /// M1 acceptance: heal at low HP, smith at full HP.
    #[test]
    fn heals_low_smiths_high() {
        let v = HeuristicV::default();
        let low = fresh_view().with_hp(10);
        assert_eq!(best_rest_option(&low, &v).0, REST_IDX);

        let mut high = fresh_view();
        high.hp = high.max_hp;
        assert_eq!(best_rest_option(&high, &v).0, SMITH_IDX);
    }

    /// Fully-upgraded deck never picks smith.
    #[test]
    fn all_upgraded_deck_rests() {
        let v = HeuristicV::default();
        let mut view = fresh_view();
        view.hp = view.max_hp;
        for c in &mut view.deck {
            c.upgraded = true;
        }
        assert_eq!(best_rest_option(&view, &v).0, REST_IDX);
    }
}
