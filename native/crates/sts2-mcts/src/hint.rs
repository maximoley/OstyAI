//! Hint surface helpers — labels and constants shared by the Python
//! `mcts_recommend` wrapper (bead `sts2-rs-cosb`, hint-T4) and any
//! Rust-side caller that wants to render rollout outcomes as
//! human-friendly buckets.
//!
//! The Python adapter (`sts2-cli/python/sts2_hint/`) consumes
//! [`WIN_PROB_BUCKETS`] to attach an ordinal label ("likely-win" /
//! "even" / "likely-loss") to per-action win-probability scores. The
//! bucket edges match the cutoffs used by the existing CLI hint
//! formatter (`sts2-cli/src/agent/human.rs`) so Python-side hints and
//! REPL hints render the same bucket for the same numbers.

/// Win-probability bucket labels keyed by an inclusive **lower-bound**
/// on the win probability (in `[0.0, 1.0]`).
///
/// Lookup rule: walk top-to-bottom and pick the first row whose lower
/// bound is `<= p_win`. Rows are ordered from highest threshold to
/// lowest so the linear scan terminates early on the common case
/// (rollouts agree the action wins).
///
/// Cutoffs:
///   * `>= 0.66` → `"likely-win"`
///   * `>= 0.34` → `"even"`
///   * `>= 0.00` → `"likely-loss"`
///
/// These are the same cutoffs the CLI hint formatter uses for its
/// Q-bucket annotation. Kept in `sts2-mcts` (rather than `sts2-py`)
/// so both Rust and Python callers can pull from one source of truth.
pub const WIN_PROB_BUCKETS: [(f32, &str); 3] =
    [(0.66, "likely-win"), (0.34, "even"), (0.00, "likely-loss")];

/// Resolve `p_win` (in `[0.0, 1.0]`) to its bucket label via the
/// [`WIN_PROB_BUCKETS`] table. Values outside `[0, 1]` are clamped.
#[must_use]
pub fn win_prob_bucket(p_win: f32) -> &'static str {
    let p = p_win.clamp(0.0, 1.0);
    for (lo, label) in WIN_PROB_BUCKETS {
        if p >= lo {
            return label;
        }
    }
    // Unreachable in practice — the last row's lower bound is 0.0 and
    // `p` was clamped to `[0, 1]`. Fall back to the lowest bucket.
    WIN_PROB_BUCKETS[WIN_PROB_BUCKETS.len() - 1].1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_cover_full_range() {
        // Edge cases.
        assert_eq!(win_prob_bucket(0.0), "likely-loss");
        assert_eq!(win_prob_bucket(1.0), "likely-win");
        // Middle band.
        assert_eq!(win_prob_bucket(0.5), "even");
        // Just below / above each cutoff.
        assert_eq!(win_prob_bucket(0.65), "even");
        assert_eq!(win_prob_bucket(0.66), "likely-win");
        assert_eq!(win_prob_bucket(0.33), "likely-loss");
        assert_eq!(win_prob_bucket(0.34), "even");
    }

    #[test]
    fn buckets_clamp_out_of_range() {
        assert_eq!(win_prob_bucket(-0.5), "likely-loss");
        assert_eq!(win_prob_bucket(1.5), "likely-win");
    }

    #[test]
    fn bucket_table_is_three_rows() {
        // Stability check — Python adapter parses these by index.
        assert_eq!(WIN_PROB_BUCKETS.len(), 3);
        assert_eq!(WIN_PROB_BUCKETS[0].1, "likely-win");
        assert_eq!(WIN_PROB_BUCKETS[1].1, "even");
        assert_eq!(WIN_PROB_BUCKETS[2].1, "likely-loss");
    }
}
