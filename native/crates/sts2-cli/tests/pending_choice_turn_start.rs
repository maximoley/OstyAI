//! Bead `sts2-rs-l2nz`: regression test for the
//! "pending_choice at turn-start" panic that fired on random-Ironclad
//! seeds {2, 9, 11, 21, 23} after `sts2-rs-2zkw` added the per-round
//! action cap.
//!
//! Pre-fix, MCTS-driven combat on these seeds panicked at
//! `crates/sts2-core/src/combat_loop.rs:344`. Post-fix,
//! `start_player_turn` defers the rest of turn-setup when a
//! `before_side_turn_start` listener opens a `pending_choice`,
//! returning [`StepResult::PendingChoice`]; the simulator drives
//! `Action::ResolveChoice` and the deferred body resumes via
//! `continue_player_turn_setup`.
//!
//! GH issue #31: seeds 9 and 23 previously surfaced separate
//! `TearAsunder: Enemy-targeted` / `Uppercut: Enemy-targeted` panics
//! through Havoc's `auto_play_card_from_draw_top`. Fixed by routing
//! the auto-play target resolution through `get_random_enemy` and
//! skipping dispatch when no alive enemy is available.

use std::path::PathBuf;
use std::process::Command;

fn binary_path() -> PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_sts2-cli") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target/debug/sts2-cli")
}

#[test]
fn random_ironclad_seeds_no_turn_start_panic_l2nz() {
    let seeds = [2u64, 9, 11, 21, 23];

    for seed in seeds {
        let output = Command::new(binary_path())
            .args([
                "combat",
                "--deck-preset",
                "random",
                "--random-deck-character",
                "ironclad",
                "--seed",
                &seed.to_string(),
                "--agent",
                "mcts",
                "--mcts-hint-default",
                "200",
                "--mcts-rollout-depth",
                "10",
            ])
            .output()
            .expect("spawn sts2-cli");

        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            !stderr.contains("pending_choice at turn-start")
                && !stdout.contains("pending_choice at turn-start"),
            "seed={seed}: l2nz panic regressed.\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        );
        assert!(
            output.status.success(),
            "seed={seed}: sts2-cli exited non-zero: {:?}\nstderr: {stderr}",
            output.status
        );
    }
}
