//! AlphaZero M2 planning-transition API: step a run while fast-forwarding
//! combat through the combat surrogate. See
//! `docs/plans/2026-07-02-alphazero-m2-planning-transition-design.md`.

use crate::eval::ranker_features::ContextFeatures;
use crate::eval::surrogate::{CombatOutcome, SurrogateNet};
use sts2_core::run_manager::{Phase, RunAction, RunManager};
use sts2_core::run_rng_set::{outcome_stream_seed_u64, splitmix64};
use sts2_core::runstate::RunState;

/// Salt for the combat-survival Bernoulli draw (disjoint from the RNG-twin
/// salts used elsewhere in the run-RNG-set).
const SURVIVAL_SALT: u64 = 0x50FA_1123_7C0D_E5A1;

/// A combat outcome oracle for the planner. Implemented by [`SurrogateNet`]
/// (real net) and by test stubs (forced outcomes).
pub trait CombatSurrogate {
    /// Predict `(win_prob, hp_frac_lost)` for entering `encounter_idx` from
    /// the pre-combat state encoded in `ctx`.
    fn predict_outcome(&self, ctx: &ContextFeatures, encounter_idx: usize) -> CombatOutcome;
}

impl CombatSurrogate for SurrogateNet {
    fn predict_outcome(&self, ctx: &ContextFeatures, encounter_idx: usize) -> CombatOutcome {
        self.predict(ctx, encounter_idx)
    }
}

/// Result of one planning step. `terminal = Some(player_won)` iff the run
/// reached `Phase::RunOver`.
#[derive(Debug, Clone, Copy)]
pub struct StepOutcome {
    pub stochastic: bool,
    pub terminal: Option<bool>,
}

/// A cloneable run planner: steps `RunManager` at run-level decisions,
/// fast-forwarding combat and stochastic draws as chance transitions.
#[derive(Clone)]
pub struct PlanningRun<'a> {
    rm: RunManager,
    surrogate: &'a dyn CombatSurrogate,
    base: u64,
}

impl std::fmt::Debug for PlanningRun<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlanningRun")
            .field("rm", &self.rm)
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl<'a> PlanningRun<'a> {
    #[must_use]
    pub fn new(rm: RunManager, surrogate: &'a dyn CombatSurrogate, search_seed: u64) -> Self {
        Self {
            rm,
            surrogate,
            base: search_seed,
        }
    }

    /// Run-level legal actions. Never surfaces combat micro-actions — the
    /// invariant `phase() != Phase::Combat` holds after `new` (fresh runs
    /// start out of combat) and after every `apply` (a future task resolves
    /// combat internally before returning).
    #[must_use]
    pub fn legal_actions(&self) -> Vec<RunAction> {
        self.rm.get_available_actions()
    }

    #[must_use]
    pub fn run_state(&self) -> &RunState {
        &self.rm.run_state
    }

    #[must_use]
    pub fn phase(&self) -> Phase {
        self.rm.phase()
    }

    #[must_use]
    pub fn randomness_base(&self) -> u64 {
        self.base
    }

    /// Override the outcome-resampling base for this run. Used by the
    /// outer-loop MCTS (`planning_mcts::chance_widen_or_sample`) to thread a
    /// per-chance-node decorrelated base into `apply`'s reseed + survival
    /// draw before widening — mirrors combat MCTS threading its node's
    /// `randomness_base` into `reseed_combat_streams` (`sts2-mcts/src/lib.rs`).
    /// Without this, every chance node's widens would reseed off the
    /// tree-global search seed (`self.base` as set by `new`), making
    /// distinct chance nodes' outcomes correlated.
    pub(crate) fn rebase(&mut self, base: u64) {
        self.base = base;
    }

    /// Extract [`ContextFeatures`] for the run state `self` currently
    /// holds (§2.5 player-visible discipline: goes through
    /// [`crate::view::VisibleRunView`], never touches `rm` directly).
    /// The leaf-evaluation seam for the run-level MCTS (M3 Tasks 4-6):
    /// [`crate::planning_mcts::SurrogateLeafEval`] calls this to score a
    /// leaf node.
    #[must_use]
    pub fn context_features(&self) -> ContextFeatures {
        let view = crate::view::VisibleRunView::from_run_manager(&self.rm);
        ContextFeatures::from_view(&view)
    }

    /// Cursor-SENSITIVE structural hash of the run this node currently
    /// holds. Used by [`crate::planning_mcts`] as the chance-node BASE
    /// parent-hash input — two decision nodes that are otherwise
    /// identical but reached via different RNG draws must still get
    /// different chance-node randomness bases, which `run_hash`'s
    /// RNG-cursor sensitivity guarantees.
    #[must_use]
    pub fn run_hash(&self) -> u64 {
        sts2_core::hashing::run_hash(&self.rm)
    }

    /// Cursor-INSENSITIVE structural hash of the run this node currently
    /// holds. Used by [`crate::planning_mcts`] to dedup chance-node
    /// outcomes (and to populate each tree node's `search_hash`): two
    /// outcomes that reached the same observable state via different
    /// draws must merge for search purposes.
    #[must_use]
    pub fn run_search_hash(&self) -> u64 {
        sts2_core::hashing::run_search_hash(&self.rm)
    }

    /// Apply one run-level action. `outcome_n = 0` is the real seed
    /// realization; `n ≥ 1` samples a counterfactual (only meaningful for a
    /// stochastic transition — the caller/M3 passes `n ≥ 1` only when
    /// widening a detected chance node). Any combat entered by the action is
    /// fast-forwarded through the surrogate before returning, so the
    /// resulting phase is a run-level decision or `RunOver`.
    pub fn apply(&mut self, action: RunAction, outcome_n: u32) -> StepOutcome {
        // n >= 1: sample a counterfactual draw by reseeding the run-draw
        // streams to (base, n) BEFORE the action (matches the combat-MCTS
        // reseed-then-reapply pattern). n == 0 keeps the real engine
        // timeline.
        if outcome_n >= 1 {
            self.rm
                .run_state
                .rng
                .reseed_run_draw_streams(self.base, outcome_n);
        }
        let fp_before = self.rm.run_state.rng.run_draw_counter_fingerprint();
        let _ = self.rm.take_action(action);
        let mut combat_happened = false;
        // Fast-forward every combat the action lands in (usually one; a loop
        // covers event→combat chains).
        while self.rm.phase() == Phase::Combat {
            self.fast_forward_current_combat(outcome_n);
            combat_happened = true;
        }
        let fp_after = self.rm.run_state.rng.run_draw_counter_fingerprint();
        let drew = fp_after != fp_before;
        let terminal = if self.rm.phase() == Phase::RunOver {
            Some(self.rm.run_state.player_won)
        } else {
            None
        };
        StepOutcome {
            stochastic: combat_happened || drew,
            terminal,
        }
    }

    /// Clone the inner [`RunManager`] for a raw (non-`PlanningRun`)
    /// comparison. Test-only escape hatch: production callers never need a
    /// bare `RunManager` clone, only `PlanningRun` state.
    #[cfg(test)]
    pub(crate) fn clone_rm_for_test(&self) -> RunManager {
        self.rm.clone()
    }

    fn fast_forward_current_combat(&mut self, outcome_n: u32) {
        let _ = fast_forward_combat(&mut self.rm, self.surrogate, self.base, outcome_n);
    }
}

/// Fast-forward the combat `rm` is currently in through `surrogate`, and
/// report what the surrogate predicted.
///
/// Free function rather than a [`PlanningRun`] method because a second caller
/// needs it on a bare [`RunManager`]: the PPO screening env
/// (`sts2-py::run_manager::surrogate_resolve_combat`) swaps its combat MCTS
/// for this, so a training run resolves a fight in one forward pass instead of
/// a search per decision. Both callers MUST share this body — the survival
/// draw's seeding is the part that makes a fast-forwarded fight reproducible
/// per `(seed, node)`, and a second copy of it would drift silently.
///
/// Precondition: `rm.phase() == Phase::Combat`. `outcome_n = 0` is the real
/// seed realization; `n >= 1` samples a counterfactual (outer-loop widening).
pub fn fast_forward_combat(
    rm: &mut RunManager,
    surrogate: &dyn CombatSurrogate,
    base: u64,
    outcome_n: u32,
) -> CombatOutcome {
    let enc_idx = rm.run_state.current_encounter_idx.unwrap_or(usize::MAX);
    let view = crate::view::VisibleRunView::from_run_manager(rm);
    let ctx = ContextFeatures::from_view(&view);
    let outcome = surrogate.predict_outcome(&ctx, enc_idx);
    // Bernoulli(win_prob) seeded by (base, n): deterministic per (node, n).
    let bits = splitmix64(outcome_stream_seed_u64(base, outcome_n, SURVIVAL_SALT));
    #[allow(clippy::cast_precision_loss)]
    let u = (bits >> 11) as f64 / (1u64 << 53) as f64;
    let survived = (u as f32) < outcome.win_prob;
    let _ = rm.resolve_combat_fast(outcome.hp_frac_lost, survived);
    outcome
}

#[cfg(test)]
// Exact float comparison is intentional here: the stub returns the exact
// literal it was constructed with, without arithmetic.
#[allow(clippy::float_cmp)]
// `pub(crate)` so `planning_mcts.rs`'s tests can reuse `StubSurrogate` /
// `fresh_ironclad_run` instead of duplicating the M2 fixture machinery.
pub(crate) mod tests {
    use super::*;
    use crate::eval::ranker_features::ContextFeatures;
    use crate::eval::surrogate::CombatOutcome;
    use sts2_core::cards::CardId;
    use sts2_core::run_manager::RunAction;
    use sts2_core::run_manager::RunManager;

    /// A fixed-outcome stub for driving PlanningRun deterministically in tests.
    pub(crate) struct StubSurrogate {
        pub win_prob: f32,
        pub hp_frac_lost: f32,
    }
    impl CombatSurrogate for StubSurrogate {
        fn predict_outcome(&self, _ctx: &ContextFeatures, _enc: usize) -> CombatOutcome {
            CombatOutcome {
                win_prob: self.win_prob,
                hp_frac_lost: self.hp_frac_lost,
            }
        }
    }

    #[test]
    fn stub_surrogate_returns_fixed_outcome() {
        let s = StubSurrogate {
            win_prob: 1.0,
            hp_frac_lost: 0.25,
        };
        // ContextFeatures has no `Default`; the stub ignores `ctx`, so the
        // minimal empty value (all fields are `pub`) is sufficient here.
        let ctx = ContextFeatures {
            deck: Vec::new(),
            relics: Vec::new(),
            scalars: Vec::new(),
        };
        let o = s.predict_outcome(&ctx, 0);
        assert_eq!(o.win_prob, 1.0);
        assert_eq!(o.hp_frac_lost, 0.25);
    }

    /// Build a fresh Ironclad run through the real content callbacks +
    /// Neow start, mirroring `feature_export::run_export_cell`
    /// (`crate::feature_export.rs:175-184`) exactly so Tasks 5-7 can reuse
    /// this same construction path.
    pub(crate) fn fresh_ironclad_run(seed: u64) -> RunManager {
        crate::rollout::ensure_content_installed();
        let mut rm = sts2_content::run_callbacks::new_run_manager_for_character_with_ascension(
            seed,
            sts2_core::potions::CharacterId::Ironclad,
            0,
        )
        .with_neow_start();
        sts2_content::run_callbacks::initialize_run_setup(&mut rm);
        rm
    }

    /// Probe whether taking `action` on a clone of `rm` would land the run
    /// in `Phase::Combat`. Used to find the map move that enters the first
    /// fight without disturbing the caller's `RunManager` / `PlanningRun` —
    /// `PlanningRun::apply` always fast-forwards any combat it enters away,
    /// so detecting entry requires driving a throwaway clone with the raw
    /// `RunManager::take_action` instead (mirrors the M1 harvest's
    /// `HarvestPolicy::choose`, `combat_surrogate_data.rs:474-485`, which
    /// detects entry by observing `phase()` after the fact; probing ahead is
    /// the equivalent here since `apply` never surfaces `Phase::Combat`).
    fn enters_combat(rm: &RunManager, action: RunAction) -> bool {
        let mut probe = rm.clone();
        let _ = probe.take_action(action);
        probe.phase() == Phase::Combat
    }

    /// Drive a fresh Ironclad run (seeded `seed`) forward via
    /// `PlanningRun::apply` — which auto-resolves any intervening combat
    /// through `stub` — until `pr` is positioned at a run-level decision
    /// whose legal actions include one that would enter combat. The
    /// returned `PlanningRun` is NOT yet in combat; callers pair this with
    /// `action_that_enters_combat` + one more `apply` call to observe that
    /// specific combat's fast-forward.
    pub(crate) fn run_at_first_combat_entry(
        seed: u64,
        stub: StubSurrogate,
    ) -> PlanningRun<'static> {
        // `apply` borrows the surrogate for the `PlanningRun`'s lifetime;
        // leaking a `Box` gives it `'static` so this helper can return an
        // owned `PlanningRun` (acceptable in tests — bounded, per-test leak).
        let stub: &'static StubSurrogate = Box::leak(Box::new(stub));
        let rm = fresh_ironclad_run(seed);
        let mut pr = PlanningRun::new(rm, stub, 0x5EED_5EED);
        loop {
            let actions = pr.legal_actions();
            assert!(
                !actions.is_empty(),
                "ran out of legal actions before reaching the first combat"
            );
            if actions.iter().any(|&a| enters_combat(&pr.rm, a)) {
                return pr;
            }
            let out = pr.apply(actions[0], 0);
            assert_ne!(
                pr.phase(),
                Phase::RunOver,
                "run ended before reaching the first combat"
            );
            assert_ne!(
                out.terminal,
                Some(false),
                "run died before reaching the first combat"
            );
        }
    }

    /// A controlled finite-support transition for dedup testing. The real
    /// content sampler has distinct full card offers, gold, and potion outcomes;
    /// nine widens need not collide once those future payloads are hashed.
    pub(crate) fn with_constant_combat_outcome(
        mut pr: PlanningRun<'static>,
    ) -> PlanningRun<'static> {
        fn build(
            rs: &mut RunState,
            _: u8,
            room: sts2_core::runstate::RoomType,
        ) -> Option<sts2_core::CombatState> {
            if !matches!(
                room,
                sts2_core::RoomType::Monster
                    | sts2_core::RoomType::Elite
                    | sts2_core::RoomType::Boss
            ) {
                return None;
            }
            let mut combat = sts2_core::CombatState::default();
            combat.player.current_hp = rs.current_hp;
            combat.player.max_hp = rs.max_hp;
            combat.gold = rs.gold;
            combat.run_rng = rs.rng.clone();
            Some(combat)
        }
        fn exit(rs: &mut RunState, combat: &sts2_core::CombatState) {
            rs.current_hp = combat.player.current_hp;
            rs.max_hp = combat.player.max_hp;
            rs.gold = combat.gold;
            rs.rng = combat.run_rng.clone();
        }
        fn cards(_: &mut RunState) -> Vec<sts2_core::cards::CardId> {
            vec![sts2_core::cards::CardId::Bash]
        }
        pr.rm.run_state.relics.clear();
        pr.rm = pr
            .rm
            .with_content_callbacks(sts2_core::run_manager::ContentCallbacks {
                build_combat_for_room: Some(build),
                exit_combat: Some(exit),
                generate_card_reward_options: Some(cards),
                ..Default::default()
            });
        pr
    }

    /// Return the legal action from `pr` that enters combat — the action
    /// `run_at_first_combat_entry` positioned `pr` in front of.
    pub(crate) fn action_that_enters_combat(pr: &PlanningRun<'_>) -> RunAction {
        pr.legal_actions()
            .into_iter()
            .find(|&a| enters_combat(&pr.rm, a))
            .expect("run_at_first_combat_entry must position pr before a combat-entering action")
    }

    /// Drive a fresh run (seeded `seed`) to the map move that enters the
    /// first combat, returning `(pr, action)` where `pr` is positioned
    /// immediately BEFORE that move. `pr.apply(action, n)` fast-forwards
    /// the won combat (per `stub`) and draws the post-combat card reward —
    /// the transition Task 6's fidelity/counterfactual tests target.
    ///
    /// The survival Bernoulli draw inside `fast_forward_current_combat` is
    /// sampled from the disjoint `SURVIVAL_SALT` stream (never the
    /// `rewards`/`shops`/`transformations` run-draw streams), so a won
    /// fast-forwarded combat draws the card reward through the exact same
    /// `rewards`-stream call path a played-out combat would use.
    pub(crate) fn run_before_card_reward_draw(
        seed: u64,
        stub: StubSurrogate,
    ) -> (PlanningRun<'static>, RunAction) {
        let pr = run_at_first_combat_entry(seed, stub);
        let action = action_that_enters_combat(&pr);
        (pr, action)
    }

    /// Read the currently-offered card-reward ids off `pr`'s inner
    /// `RunManager` (the `Phase::CardReward` fan-out populated by
    /// `RunManager::generate_card_reward_options`).
    pub(crate) fn offered_card_reward_ids(pr: &PlanningRun<'_>) -> Vec<CardId> {
        pr.rm.card_reward_options().to_vec()
    }

    #[test]
    fn forced_win_advances_and_applies_mean_hp_loss() {
        // Drive a fresh run to the first combat entry, then apply the map
        // move that enters it with a stub forced to win_prob=1.0,
        // hp_frac_lost=0.2.
        let mut pr = run_at_first_combat_entry(
            1,
            StubSurrogate {
                win_prob: 1.0,
                hp_frac_lost: 0.2,
            },
        );
        let max_hp = pr.run_state().max_hp;
        let entry_hp = pr.run_state().current_hp;
        let action = action_that_enters_combat(&pr);
        let out = pr.apply(action, 0);
        assert_ne!(
            pr.phase(),
            Phase::Combat,
            "combat must be resolved, not surfaced"
        );
        assert!(
            out.terminal != Some(false),
            "a forced win must not end the run in a loss"
        );
        let expected = (entry_hp - ((0.2f32 * max_hp as f32).round() as i32)).max(1);
        assert_eq!(pr.run_state().current_hp, expected);
        assert!(out.stochastic, "combat fast-forward is stochastic");
    }

    #[test]
    fn forced_loss_ends_run() {
        let mut pr = run_at_first_combat_entry(
            1,
            StubSurrogate {
                win_prob: 0.0,
                hp_frac_lost: 0.9,
            },
        );
        let action = action_that_enters_combat(&pr);
        let out = pr.apply(action, 0);
        assert_eq!(out.terminal, Some(false));
        assert_eq!(pr.phase(), Phase::RunOver);
    }

    #[test]
    fn apply_is_deterministic_for_same_seed_and_n() {
        let mk = || {
            run_at_first_combat_entry(
                1,
                StubSurrogate {
                    win_prob: 0.5,
                    hp_frac_lost: 0.3,
                },
            )
        };
        let (mut a, mut b) = (mk(), mk());
        let act_a = action_that_enters_combat(&a);
        let act_b = action_that_enters_combat(&b);
        a.apply(act_a, 4);
        b.apply(act_b, 4);
        assert_eq!(a.run_state().current_hp, b.run_state().current_hp);
        assert_eq!(a.phase(), b.phase());
    }

    #[test]
    fn n0_reproduces_real_engine_draw() {
        // Prove: PlanningRun::apply(action, 0) does not perturb the
        // deterministic run-draw sequence relative to the real engine.
        //
        // ADAPTED from the plan's literal draft (which compared against a
        // bare `raw.take_action(action)`): `take_action` on a
        // combat-entering action only enters `Phase::Combat` — it does not
        // auto-resolve combat, so it never reaches the post-combat reward
        // draw, making a naive raw-vs-apply fingerprint comparison after a
        // single `take_action` vacuous (both sides would report "no draw
        // happened yet").
        //
        // Instead this drives the SAME two engine calls `apply` performs
        // internally (`take_action` to enter combat, then
        // `resolve_combat_fast` to fast-forward the win) directly on a raw
        // `RunManager` clone, and compares the resulting run-draw
        // fingerprint against `PlanningRun::apply`'s. Because
        // `fast_forward_current_combat`'s survival Bernoulli draw comes
        // from the disjoint `SURVIVAL_SALT` stream (never `rewards` /
        // `shops` / `transformations`), passing the stub's forced
        // `(hp_frac_lost, survived=true)` straight to `resolve_combat_fast`
        // reproduces exactly what `apply` computes at `outcome_n = 0` —
        // this is the real engine's post-combat reward draw, not a
        // fast-forward-only artifact.
        let stub = StubSurrogate {
            win_prob: 1.0,
            hp_frac_lost: 0.1,
        };
        let (mut pr, action) = run_before_card_reward_draw(7, stub);

        let mut raw = pr.clone_rm_for_test();
        let raw_fp_before = raw.run_state.rng.run_draw_counter_fingerprint();
        let _ = raw.take_action(action);
        assert_eq!(
            raw.run_state.rng.run_draw_counter_fingerprint(),
            raw_fp_before,
            "entering combat alone must not draw any run-draw RNG"
        );
        let _ = raw.resolve_combat_fast(0.1, true);
        let raw_fp_after = raw.run_state.rng.run_draw_counter_fingerprint();
        assert_ne!(
            raw_fp_after, raw_fp_before,
            "the won combat must draw a card reward (the transition under test must actually draw)"
        );

        pr.apply(action, 0);
        assert_eq!(
            pr.run_state().rng.run_draw_counter_fingerprint(),
            raw_fp_after,
            "n=0 apply must reproduce the real engine's run-draw fingerprint"
        );
    }

    #[test]
    fn n_ge_1_yields_counterfactual_card_reward() {
        // Same reward transition; n=1 vs n=2 must offer different cards.
        let mk = |n| {
            let (mut pr, action) = run_before_card_reward_draw(
                7,
                StubSurrogate {
                    win_prob: 1.0,
                    hp_frac_lost: 0.1,
                },
            );
            pr.apply(action, n);
            offered_card_reward_ids(&pr)
        };
        assert_ne!(
            mk(1),
            mk(2),
            "different outcome ordinals must offer different cards"
        );
    }

    #[test]
    fn planning_run_exposes_legal_actions_and_base() {
        let rm = fresh_ironclad_run(1);
        let stub = StubSurrogate {
            win_prob: 1.0,
            hp_frac_lost: 0.1,
        };
        let pr = PlanningRun::new(rm, &stub, 0xDEAD_BEEF);
        assert_eq!(pr.randomness_base(), 0xDEAD_BEEF);
        let actions = pr.legal_actions();
        assert!(!actions.is_empty(), "a fresh run must have legal actions");
        // Combat micro-actions must never surface (PlanningRun resolves
        // combat internally).
        assert!(
            actions.iter().all(|a| !matches!(
                a,
                RunAction::PlayCard { .. }
                    | RunAction::EndTurn
                    | RunAction::CombatResolveChoice { .. }
            )),
            "combat micro-actions leaked into planning legal_actions"
        );
    }
}
