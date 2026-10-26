//! Quiescence pass: drive a `CombatState` through any sole-legal-action
//! transitions before scoring it with a leaf eval.
//!
//! Bead `sts2-rs-8hs96` (Caissawary Tier 2). When the engine reports
//! exactly one legal action, applying it is forced — no policy choice
//! is being skipped. Driving the state through these forced transitions
//! lets the leaf eval observe a "quiet" position, which avoids scoring
//! states that are about to take a known, mandatory transition (end of
//! turn auto-passes, single-card-in-hand draw-only states, etc.).
//!
//! Conservative by design: we only apply moves the engine itself
//! declares to be the sole legal action, so false positives are
//! impossible. Integration site is gated behind
//! [`crate::MctsConfig::use_quiescence`] (default `false`) so existing
//! fixtures remain byte-stable.

use sts2_core::{apply as core_apply, is_terminal, legal_actions, CombatState};

/// Apply at most one forced action. Returns `true` if a transition
/// was applied (i.e. the state had exactly one legal action and it was
/// not already terminal), `false` otherwise.
pub fn quiescence_step(state: &mut CombatState) -> bool {
    if is_terminal(state) {
        return false;
    }
    let acts = legal_actions(state);
    if acts.len() != 1 {
        return false;
    }
    let _ = core_apply(state, acts[0].clone());
    true
}

/// Repeatedly apply [`quiescence_step`] until either the state has more
/// than one legal action, the state becomes terminal, or `max_steps`
/// steps have been applied. Returns the number of steps applied.
pub fn drive_to_quiescence(state: &mut CombatState, max_steps: u32) -> u32 {
    let mut steps = 0u32;
    while steps < max_steps && quiescence_step(state) {
        steps += 1;
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_zero_applies_nothing() {
        let mut state = CombatState::default();
        let applied = drive_to_quiescence(&mut state, 0);
        assert_eq!(applied, 0);
    }

    #[test]
    fn cap_respected() {
        let mut state = CombatState::default();
        let applied = drive_to_quiescence(&mut state, 3);
        assert!(applied <= 3);
    }

    #[test]
    fn post_drive_invariant() {
        // After driving to quiescence with a generous cap, either the
        // state is terminal, has != 1 legal action, OR the cap was
        // exhausted. Only the first two cases are "quiescent"; the cap
        // is a safety net for pathological chains (which shouldn't
        // exist at the pin but the bound matters for future-proofing).
        let mut state = CombatState::default();
        let steps = drive_to_quiescence(&mut state, 32);
        if steps < 32 && !is_terminal(&state) {
            let acts = legal_actions(&state);
            assert_ne!(
                acts.len(),
                1,
                "drive halted before cap but state still has 1 legal action"
            );
        }
    }
}
