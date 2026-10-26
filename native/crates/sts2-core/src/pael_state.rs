//! Shared trackers for the Pael relic cluster.
//!
//! The Pael cluster (`PaelsBlood`, `PaelsClaw`, `PaelsEye`, `PaelsFlesh`,
//! `PaelsGrowth`, `PaelsHorn`, `PaelsLegion`, `PaelsTears`, `PaelsTooth`,
//! `PaelsWing` — see
//! `sts2-rl-agent@01c094d:sts2_env/relics/shop_event.py:2558-2846`)
//! shares run-level state that several relics read and mutate jointly:
//!
//! - **Pet level** — incremented by feed-the-pet effects, read by
//!   relics whose damage / block scale with pet maturity.
//! - **Sacrificed count** — used by [`crate::RelicId::PaelsWing`]'s
//!   `sacrifice_card_reward` flow ("every N sacrifices, obtain a
//!   random relic"). Python tracks this as a per-relic
//!   `_rewards_sacrificed: int`; the Rust port lifts it to a single
//!   run-side counter so multiple Pael relics can share the same
//!   sacrifice ledger if a future Pael relic also needs it.
//! - **Fed count** — total times the pet has been fed across the
//!   run. Separate from `level` so a future relic can branch on raw
//!   feeds (e.g. "every 5th feed grants…") independent of any
//!   level-up curve.
//!
//! Bead `sts2-rs-70anj.3.20` ships the **fields and accessors only** —
//! per-relic listeners (`PaelsWing`, `PaelsTears`, `PaelsTooth`, …)
//! land in their dedicated relic-port beads downstream and call into
//! these accessors instead of carrying private state.

/// Cluster-wide trackers for Pael relics. Lives on
/// [`crate::runstate::RunState::pael_state`].
///
/// Defaults to zero on every counter — a fresh run starts with no
/// Pael interactions, the pet at level 0, never fed, no rewards
/// sacrificed. Pael relics check / mutate these counters via the
/// run-state accessor methods.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PaelState {
    /// Pet maturity level. Mirrors a future `pael_pet_level: int` slot.
    /// Incremented by [`crate::runstate::RunState::pael_level_up`];
    /// decremented only by death-revert paths (none today).
    pub level: u32,
    /// Cumulative count of card rewards sacrificed to the Pael
    /// cluster (read by `PaelsWing`'s "every N sacrifices, obtain
    /// relic" branch). Pure-monotonic in this pin.
    pub sacrificed_count: u32,
    /// Cumulative count of feed-the-pet interactions across the
    /// entire run. Distinct from [`Self::level`] so caller relics
    /// can branch on raw feeds independent of any level curve.
    pub fed_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_zero() {
        let s = PaelState::default();
        assert_eq!(s.level, 0);
        assert_eq!(s.sacrificed_count, 0);
        assert_eq!(s.fed_count, 0);
    }

    #[test]
    fn copy_semantics() {
        // Cluster-shared trackers should be cheap to snapshot in
        // listeners — confirm Copy is derived (compile-time check).
        let s = PaelState {
            level: 3,
            sacrificed_count: 7,
            fed_count: 11,
        };
        let t = s; // Copy
        assert_eq!(s.level, t.level);
        assert_eq!(s.sacrificed_count, t.sacrificed_count);
        assert_eq!(s.fed_count, t.fed_count);
    }

    #[test]
    fn eq_and_hash_work_for_set_membership() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        seen.insert(PaelState::default());
        assert!(seen.contains(&PaelState::default()));
        assert!(!seen.contains(&PaelState {
            level: 1,
            sacrificed_count: 0,
            fed_count: 0
        }));
    }
}
