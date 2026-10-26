//! Throwaway diagnostic: trace run-level HP/deck/gold at every non-combat
//! decision for one seed under heuristic meta + MCTS combat. Used to
//! root-cause the 2026-06-12 all-act-1-deaths anomaly (sts2-rs-40ikw.5).

use sts2_core::full_run::{drive_full_run, FullRunConfig, RunPolicy};
use sts2_core::run_manager::{Phase, RunAction, RunManager};
use sts2_core::CharacterId;
use sts2_runai::policy::heuristic::HeuristicPolicy;
use sts2_runai::policy::{CombatBudget, MctsCombatPolicy};

struct Tracing<P: RunPolicy>(P);

impl<P: RunPolicy> RunPolicy for Tracing<P> {
    fn prepare(&mut self, rm: &mut RunManager) {
        // Forward the per-decision hook; no-op unless the inner policy
        // overrides `prepare` (e.g. PpoPolicy's event materialization).
        self.0.prepare(rm);
    }

    fn choose(&mut self, rm: &RunManager, actions: &[RunAction]) -> Option<usize> {
        let idx = self.0.choose(rm, actions);
        if rm.phase() != Phase::Combat {
            let in_combat_hp = rm.combat.as_ref().map(|c| c.player.current_hp);
            println!(
                "floor={:2} act={} phase={:?} hp={}/{} (combat_hp={:?}) gold={} deck={} chose={:?}",
                rm.run_state.total_floor,
                rm.run_state.act_index,
                rm.phase(),
                rm.run_state.current_hp,
                rm.run_state.max_hp,
                in_combat_hp,
                rm.run_state.gold,
                rm.run_state.master_deck.len(),
                idx.map(|i| &actions[i]),
            );
        }
        idx
    }
}

fn main() {
    let seed: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut rm =
        sts2_content::run_callbacks::new_run_manager_for_character(seed, CharacterId::Ironclad);
    let meta = HeuristicPolicy::new();
    let mut policy = Tracing(MctsCombatPolicy::new(
        meta,
        CombatBudget::eval_cheap(),
        seed,
    ));
    let report = drive_full_run(&mut rm, &mut policy, &FullRunConfig::default());
    println!(
        "outcome={:?} steps={} hp={} act={}",
        report.outcome, report.steps, report.final_hp, report.act_index
    );
}
