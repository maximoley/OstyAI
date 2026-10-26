//! Event surface (plan §2 "events"; bead `sts2-rs-40ikw.5`).
//!
//! M1: event-option *outcomes* are not visible at choice time, so
//! options are scored from a small hand-written per-event-option prior
//! table (default 0 ΔV — ties resolve to option 0). M2 replaces these
//! priors with outcome-distribution EV computed over the visible pool
//! (plan §2: "Events: enumerate options; outcome distributions computed
//! over the visible pool; score = E[V(outcome)]").
//!
//! The same table feeds the pathing chance node: an un-entered event
//! room projects as the *expectation over the uniform-over-remaining
//! event pool* (§2.5 rule 2; E10 revisits the uniform approximation).

use sts2_core::events::EventId;

use crate::eval::ProjectionTables;
use crate::view::VisibleRunView;

/// Prior ΔV for picking option `idx` of `event`. Hand priors for a few
/// ported events whose options are structurally known; 0.0 otherwise
/// (→ first option on ties). TODO(M2): replace with outcome-distribution
/// EV through V.
#[must_use]
pub fn event_option_prior(event: EventId, idx: usize) -> f64 {
    match (event, idx) {
        // LostWisp: option 0 claims the LOST_WISP relic + Decay curse,
        // option 1 searches (~60 gold). The curse roughly cancels the
        // relic for a tier-1 deck; prefer the gold slightly.
        (EventId::LostWisp, 0) => 0.0,
        (EventId::LostWisp, 1) => 0.05,
        _ => 0.0,
    }
}

/// Expected `(hp_delta, ΔV)` of *arriving at* `event` (entry effects +
/// a typical choice), used by the pathing chance node. `hp_delta` is
/// signed (negative = expected HP loss). Default `(0, 0)` — the generic
/// `event_hp_loss` table entry covers the unknown mass.
#[must_use]
pub fn event_arrival_prior(event: EventId, tables: &ProjectionTables) -> (f64, f64) {
    // No per-event arrival data yet — the generic table entry covers the
    // whole pool. Per-event rows land with M2's audit-log calibration.
    let _ = event;
    (-tables.event_hp_loss, 0.0)
}

/// Chance-node expectation for an un-entered event room: uniform over
/// the remaining (not-yet-seen) event pool (§2.5 rule 2 approximation —
/// IsAllowed gates shift the shared cursor, but uniform-over-remaining
/// is the M1 model; E10 calibrates). Returns `(hp_delta, ΔV)`.
#[must_use]
pub fn event_pool_expectation(view: &VisibleRunView, tables: &ProjectionTables) -> (f64, f64) {
    let remaining: Vec<EventId> = EventId::all()
        .iter()
        .copied()
        .filter(|e| !view.seen_events.contains(e))
        .collect();
    if remaining.is_empty() {
        return (-tables.event_hp_loss, 0.0);
    }
    let n = remaining.len() as f64;
    let (mut hp, mut dv) = (0.0, 0.0);
    for e in remaining {
        let (h, d) = event_arrival_prior(e, tables);
        hp += h;
        dv += d;
    }
    (hp / n, dv / n)
}

/// Pick the best option index among `n` surfaced `EventChoice` options:
/// argmax of the prior table (ties → lowest index).
#[must_use]
pub fn best_event_option(view: &VisibleRunView, n: usize) -> (usize, Vec<f64>) {
    let scores: Vec<f64> = (0..n)
        .map(|idx| {
            view.current_event
                .map_or(0.0, |e| event_option_prior(e, idx))
        })
        .collect();
    let best = crate::policy::surfaces::picks::argmax(&scores).unwrap_or(0);
    (best, scores)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Weights;
    use sts2_content::run_callbacks::new_run_manager_for_character;
    use sts2_core::potions::CharacterId;

    #[test]
    fn lost_wisp_prefers_search() {
        let rm = new_run_manager_for_character(3, CharacterId::Ironclad);
        let mut view = crate::view::VisibleRunView::from_run_manager(&rm);
        view.current_event = Some(EventId::LostWisp);
        let (best, scores) = best_event_option(&view, 2);
        assert_eq!(best, 1);
        assert_eq!(scores.len(), 2);
    }

    #[test]
    fn unknown_event_defaults_to_first_option() {
        let rm = new_run_manager_for_character(3, CharacterId::Ironclad);
        let view = crate::view::VisibleRunView::from_run_manager(&rm);
        let (best, _) = best_event_option(&view, 3);
        assert_eq!(best, 0);
    }

    #[test]
    fn pool_expectation_is_finite_and_lossy() {
        let rm = new_run_manager_for_character(3, CharacterId::Ironclad);
        let view = crate::view::VisibleRunView::from_run_manager(&rm);
        let w = Weights::embedded();
        let (hp, dv) = event_pool_expectation(&view, &w.projection);
        assert!(hp <= 0.0, "expected HP delta is a loss on average");
        assert!(hp.is_finite() && dv.is_finite());
    }
}
