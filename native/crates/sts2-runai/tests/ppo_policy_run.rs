// Drives `arena` (`sweeps`) with `PpoPolicy` (`ppo`) — both features required.
#![cfg(all(feature = "ppo", feature = "sweeps"))]
//! `--policy ppo` plays complete runs through the arena harness (Task 8
//! of the 2026-07-07 ppo-rust-inference plan). Skips gracefully when
//! the committed ONNX model isn't present in this checkout, mirroring
//! the fixture-skip idiom used by the other `ppo_*` gate tests.

use sts2_core::potions::CharacterId;
use sts2_runai::arena::{
    run_cell, CellSpec, CombatBudgetKind, PolicyKind, DEFAULT_LIVELOCK_STALL_LIMIT,
};

#[test]
fn policy_kind_ppo_parses() {
    assert_eq!(PolicyKind::parse("ppo").unwrap().token(), "ppo");
}

#[test]
fn ppo_policy_completes_short_runs() {
    if sts2_runai::ppo::net::onnx_model_path().is_none() {
        eprintln!(
            "skip: no ONNX model in this checkout (STS2_RUNAI_PPO unset, no sibling snapshot)"
        );
        return;
    }
    for seed in [0_u64, 1] {
        let spec = CellSpec {
            seed,
            character: CharacterId::Ironclad,
            ascension: 0,
            policy: PolicyKind::Ppo,
            combat: CombatBudgetKind::EvalCheap,
            // Debug-build MCTS at the frozen 1000/4000 budget blows
            // through nextest's 180s terminate-after; shrink the sims
            // via the CellSpec debug overrides (the exact knob the CLI
            // exposes as --sims/--boss-sims). The PPO net still owns
            // every non-combat decision and combat still bridges
            // through `MctsCombatPolicy` — only the search budget
            // shrinks. The frozen eval-cheap budget is exercised at
            // release speed by the Task 8 manual CLI smoke.
            sims_override: Some(24),
            boss_sims_override: Some(48),
            max_steps: 3000,
            livelock_stall_limit: DEFAULT_LIVELOCK_STALL_LIMIT,
            act1: sts2_runai::arena::Act1Choice::default(),
            audit: false,
            ablate: None,
        };
        let (row, _) = run_cell(&spec);
        assert!(
            row.outcome == "won" || row.outcome == "lost",
            "seed {seed}: outcome {} (bin {:?})",
            row.outcome,
            row.noncompletion_bin
        );
    }
}
