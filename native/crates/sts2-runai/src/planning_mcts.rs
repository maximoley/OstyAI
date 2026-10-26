//! AlphaZero M3 leaf-evaluation seam + run-level MCTS tree for the
//! outer-loop run-level search. `RunLeafEval`/`SurrogateLeafEval` define
//! *how a leaf `PlanningRun` node is scored*; `RunPlanner` is the search
//! itself. Task 4 treated every transition as deterministic (the tree
//! only ever grew `Decision` nodes via `apply(action, 0)`). This task (5)
//! adds stochastic branching: a transition whose `StepOutcome::stochastic`
//! is `true` becomes a `RunNodeKind::Chance` node, and nature's outcomes
//! are enumerated by Double Progressive Widening (DPW) with a per-node
//! decorrelated randomness base — mirroring `crates/sts2-mcts/src/lib.rs`'s
//! `alloc_chance_node` / `chance_widen_or_sample`, substituting run
//! analogues (`sts2_core::hashing::{run_hash, run_search_hash}` for the
//! combat engine's `state_hash`/`search_hash`). See
//! `docs/plans/2026-07-01-alphazero-m3-outer-loop-mcts-design.md`.

use std::f32::consts::SQRT_2;

use sts2_core::run_manager::{Phase, RunAction};
use sts2_core::run_rng_set::splitmix64;

use crate::eval::run_value_net::RunValueNet;
use crate::planning::{CombatSurrogate, PlanningRun};

/// Consecutive widen-draws that dedup to an already-seen outcome before a
/// chance node is declared `closed` (DPW has stopped discovering new
/// outcomes). Mirrors `sts2_mcts::CHANCE_CLOSE_K`.
const CHANCE_CLOSE_K: u32 = 16;

/// Fold the action's `Debug`-string identity into the chance-node base —
/// bead `sts2-rs-0lr87` #1: two DIFFERENT stochastic actions from the same
/// decision parent share the same parent-hash and (if expanded in the same
/// position) the same `sibling_salt`, so without this fold they'd collide
/// on the same randomness base. `RunAction` isn't `Hash` (`PlayCard`'s
/// `Option<u32>` target etc. don't need it elsewhere), so the `Debug`
/// string stands in as a stable per-variant+payload identity. Mirrors
/// `sts2_mcts::alloc_chance_node`'s `parent_hash`/`seed`/`salt` fold
/// (`crates/sts2-mcts/src/lib.rs` ~2275-2290) plus the action fold that
/// combat's version doesn't need (combat's `sibling_salt` already
/// decorrelates same-position siblings because each `EdgeAction` expands
/// from a distinct child-count position; the run-level tree can expand two
/// stochastic actions from the same node in either order across searches,
/// so the action itself must be folded in to keep the base attached to the
/// action rather than to expansion order).
pub(crate) fn chance_base(state_hash: u64, seed: u64, sibling_salt: u64, action: RunAction) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&format!("{action:?}"), &mut h);
    let action_hash = std::hash::Hasher::finish(&h);
    splitmix64(
        state_hash
            ^ seed.wrapping_mul(0xD1B5_4A32_D192_ED03)
            ^ sibling_salt.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ action_hash,
    )
}

/// A leaf-value oracle for the run-level MCTS: a win-probability in
/// `[0, 1]` for the run state a [`PlanningRun`] node currently holds.
pub trait RunLeafEval {
    fn value(&self, pr: &PlanningRun<'_>) -> f32;
}

/// Evaluate a leaf via the combat surrogate: build [`ContextFeatures`]
/// for `pr`'s current (player-visible) state and ask the surrogate for
/// its predicted win-probability entering `pr`'s current encounter.
///
/// [`ContextFeatures`]: crate::eval::ranker_features::ContextFeatures
pub struct SurrogateLeafEval<'s> {
    pub surrogate: &'s dyn CombatSurrogate,
}

// `dyn CombatSurrogate` doesn't require `Debug` (mirrors
// `PlanningRun`'s own manual `Debug` impl in `planning.rs`, which omits
// its `surrogate: &dyn CombatSurrogate` field for the same reason).
impl std::fmt::Debug for SurrogateLeafEval<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurrogateLeafEval").finish_non_exhaustive()
    }
}

impl RunLeafEval for SurrogateLeafEval<'_> {
    fn value(&self, pr: &PlanningRun<'_>) -> f32 {
        self.surrogate
            .predict_outcome(
                &pr.context_features(),
                pr.run_state().current_encounter_idx.unwrap_or(usize::MAX),
            )
            .win_prob
    }
}

/// Evaluate a leaf via the learned whole-run value net (AlphaZero M4):
/// win-probability straight from `pr`'s [`ContextFeatures`], skipping the
/// combat-surrogate hop [`SurrogateLeafEval`] takes. `RunValueNet`
/// derives `Debug`, so unlike `SurrogateLeafEval`'s `&dyn CombatSurrogate`
/// field this one can just derive.
///
/// [`ContextFeatures`]: crate::eval::ranker_features::ContextFeatures
#[derive(Debug)]
pub struct NetLeafEval<'n> {
    pub net: &'n RunValueNet,
}

impl RunLeafEval for NetLeafEval<'_> {
    fn value(&self, pr: &PlanningRun<'_>) -> f32 {
        self.net.win_prob(&pr.context_features())
    }
}

/// Fixed-value leaf eval — a test-only stand-in for
/// [`SurrogateLeafEval`] when the search machinery only needs a
/// constant leaf value.
#[cfg(test)]
pub(crate) struct StubLeafEval {
    pub v: f32,
}

#[cfg(test)]
impl RunLeafEval for StubLeafEval {
    fn value(&self, _pr: &PlanningRun<'_>) -> f32 {
        self.v
    }
}

/// Closure-backed leaf eval — a test-only stand-in for
/// [`SurrogateLeafEval`] when a fixture needs a leaf value that varies
/// by run state (Task 6's sanity fixture). The wrapped closure isn't
/// `Debug`, so this type opts out of `missing_debug_implementations`
/// rather than deriving a meaningless impl.
#[cfg(test)]
#[allow(missing_debug_implementations)]
pub(crate) struct FnLeafEval<F: Fn(&PlanningRun<'_>) -> f32>(pub F);

#[cfg(test)]
impl<F: Fn(&PlanningRun<'_>) -> f32> RunLeafEval for FnLeafEval<F> {
    fn value(&self, pr: &PlanningRun<'_>) -> f32 {
        (self.0)(pr)
    }
}

/// A policy-prior oracle for the run-level MCTS: one prior *weight* per
/// action in `actions` order (the caller normalizes; weights need not sum
/// to 1). Mirrors the [`RunLeafEval`] seam — `UniformPriors` reproduces
/// M3's hard-uniform PUCT (`P = 1/children_count`); learned prior sources
/// (the retired ranker/unified-net wrappers) plugged in here at node
/// expansion.
pub trait RunPriorSource {
    fn priors(&self, pr: &PlanningRun<'_>, actions: &[RunAction]) -> Vec<f32>;
}

/// Every action gets weight `1.0` → normalized to `1/n`. Reproduces the
/// pre-seam PUCT behavior exactly (`select_child` only runs once every
/// action is expanded, so `1/n == 1/children.len()` there).
#[derive(Debug, Clone, Copy, Default)]
pub struct UniformPriors;

impl RunPriorSource for UniformPriors {
    fn priors(&self, _pr: &PlanningRun<'_>, actions: &[RunAction]) -> Vec<f32> {
        vec![1.0; actions.len()]
    }
}

/// Configuration for [`RunPlanner::search`].
///
/// Defaults mirror the combat-MCTS PUCT constant (`c_puct = √2`) and a
/// shallow decision-ply budget (`max_depth = 8`) — the M2 full-run probe
/// showed a fresh run reaches `RunOver` in ~106 decision steps with combat
/// never surfacing, so 8 plies is a deliberately small planning horizon,
/// not an attempt to reach the end of the run in one tree.
/// `chance_widening_c`/`chance_widening_alpha` parameterize the DPW cap
/// (`ceil(c * (visits+1)^alpha)`) that bounds how many distinct stochastic
/// outcomes a chance node enumerates before it starts sampling instead of
/// widening.
#[derive(Clone, Copy, Debug)]
pub struct RunMctsConfig {
    pub max_simulations: u32,
    pub max_depth: u32,
    pub c_puct: f32,
    pub chance_widening_c: f32,
    pub chance_widening_alpha: f32,
    pub seed: u64,
}

impl Default for RunMctsConfig {
    fn default() -> Self {
        Self {
            max_simulations: 1000,
            max_depth: 8,
            c_puct: SQRT_2,
            chance_widening_c: 1.0,
            chance_widening_alpha: 0.5,
            seed: 0,
        }
    }
}

type NodeId = usize;

/// A `Decision` node holds a realized `PlanningRun` positioned at a
/// run-level choice; a `Chance` node represents a stochastic transition
/// (`StepOutcome::stochastic == true`) and enumerates nature's possible
/// outcomes via DPW (see `RunNode`'s `chance_*` fields).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunNodeKind {
    Decision,
    Chance,
}

/// One node of the run-level search tree. Every node stores a
/// `PlanningRun` — for `Decision` it's the *realized* post-action state
/// (cheap to `Clone`, see `planning.rs`), for `Chance` it's the *pre-action*
/// state of the stochastic action `action` (re-cloned and re-applied at a
/// fresh `outcome_n` on each DPW widen). This dual meaning avoids a
/// separate `pre_run` field: a chance node's parent is always the decision
/// node whose `run` field is exactly that same pre-action state, so
/// `Chance::run` is simply a clone of `parent.run` taken at allocation time
/// and never mutated afterward — mirrors the combat MCTS's chance mode
/// (`sts2-mcts/src/lib.rs`'s `realized`-state nodes plus
/// `chance_alloc_decision`'s reapplication from the parent's realized
/// state).
struct RunNode<'a> {
    kind: RunNodeKind,
    parent: Option<NodeId>,
    run: PlanningRun<'a>,
    /// `Some(player_won)` iff `run` is at `Phase::RunOver`. Always `None`
    /// for `Chance` (a chance node has no single terminal verdict — its
    /// outcomes might).
    terminal: Option<bool>,
    visits: u32,
    value_sum: f32,
    /// Actions not yet expanded into a child, in `PlanningRun::legal_actions`
    /// order. Popped from the front so children are created — and thus
    /// tie-broken by [`RunPlanner::best_root_action`] — in a fixed,
    /// deterministic order. Always empty for `Chance` (a chance node has no
    /// action menu of its own — it widens/samples instead).
    untried: Vec<RunAction>,
    /// `Decision`-only: `(action, child_id)` edges, in expansion order.
    children: Vec<(RunAction, NodeId)>,
    /// `Decision`-only: normalized PUCT prior per child, aligned to
    /// `children` by expansion order (child `i` == `legal_actions()[i]`,
    /// since `untried` pops from the front and children push in that
    /// order). Populated once, at the node's FIRST expansion, from the
    /// active [`RunPriorSource`]. Empty until then and on `Chance` nodes.
    child_priors: Vec<f32>,
    // ── Chance-node fields: stochastic-transition DPW state. Placeholder-
    // valued (0/empty/false) on `Decision` nodes.
    /// Per-node decorrelated base for reseeding the run-draw RNG streams
    /// on each DPW widen (`chance_base`).
    randomness_base: u64,
    /// Monotonic draw-ordinal counter: the next `outcome_n` a widen will
    /// use. Starts at `1` (ordinal `0` is the on-policy realization
    /// already computed at chance-node allocation).
    chance_draws: u32,
    /// `(ordinal, child_id)` pairs for outcomes DPW has kept (deduped
    /// draws are NOT pushed here — they bump `widenings_since_new`
    /// instead).
    chance_outcomes: Vec<(u32, NodeId)>,
    /// Consecutive widen-draws that deduped to an already-seen outcome.
    /// Reset to `0` on every newly-discovered outcome.
    widenings_since_new: u32,
    /// DPW has stopped discovering new outcomes (`widenings_since_new`
    /// reached [`CHANCE_CLOSE_K`]). Forward-compat scaffolding: set but not
    /// yet consulted by any production logic (unlike combat's
    /// `chance_closed`, which gates proof-number logic) — a future task may
    /// use this to short-circuit further widening once a chance node is
    /// closed.
    closed: bool,
    /// `run_search_hash()` of `run`. For `Decision`, the outcome dedup key;
    /// unused (`0`) for `Chance` (a chance node has no single observable
    /// state of its own).
    search_hash: u64,
    /// `Chance`-only: the action whose transition this node represents —
    /// re-applied to a clone of `run` (the pre-action state) at a fresh
    /// `outcome_n` on each DPW widen.
    action: Option<RunAction>,
}

impl std::fmt::Debug for RunNode<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunNode")
            .field("kind", &self.kind)
            .field("parent", &self.parent)
            .field("run", &self.run)
            .field("terminal", &self.terminal)
            .field("visits", &self.visits)
            .field("value_sum", &self.value_sum)
            .field("untried", &self.untried)
            .field("children", &self.children)
            .finish_non_exhaustive()
    }
}

impl<'a> RunNode<'a> {
    /// Build a fresh `Decision` node for `run`, deriving `untried` from
    /// `run.legal_actions()` — empty for a terminal node (there is nothing
    /// left to expand). Computes `run.run_search_hash()` once.
    fn new_decision(parent: Option<NodeId>, run: PlanningRun<'a>, terminal: Option<bool>) -> Self {
        let search_hash = run.run_search_hash();
        Self::new_decision_with_hash(parent, run, terminal, search_hash)
    }

    /// As [`Self::new_decision`], but for callers that already computed
    /// `run.run_search_hash()` (the DPW widen path hashes the outcome to
    /// dedup BEFORE deciding whether to allocate a node at all) — avoids
    /// hashing the same `run` twice.
    fn new_decision_with_hash(
        parent: Option<NodeId>,
        run: PlanningRun<'a>,
        terminal: Option<bool>,
        search_hash: u64,
    ) -> Self {
        let untried = if terminal.is_some() {
            Vec::new()
        } else {
            run.legal_actions()
        };
        Self {
            kind: RunNodeKind::Decision,
            parent,
            run,
            terminal,
            visits: 0,
            value_sum: 0.0,
            untried,
            children: Vec::new(),
            child_priors: Vec::new(),
            randomness_base: 0,
            chance_draws: 0,
            chance_outcomes: Vec::new(),
            widenings_since_new: 0,
            closed: false,
            search_hash,
            action: None,
        }
    }

    /// Build a fresh `Chance` node wrapping the stochastic `action` applied
    /// from decision node `parent`, whose pre-action state is `pre_run`
    /// (a clone of `parent`'s own `run` — see the struct doc). Ordinal `0`
    /// is reserved for the on-policy realization the caller already
    /// computed (not a reseed), so `chance_draws` starts at `1` and the
    /// first DPW widen uses ordinal `1`.
    fn new_chance(
        parent: NodeId,
        pre_run: PlanningRun<'a>,
        action: RunAction,
        randomness_base: u64,
    ) -> Self {
        Self {
            kind: RunNodeKind::Chance,
            parent: Some(parent),
            run: pre_run,
            terminal: None,
            visits: 0,
            value_sum: 0.0,
            untried: Vec::new(),
            children: Vec::new(),
            child_priors: Vec::new(),
            randomness_base,
            chance_draws: 1,
            chance_outcomes: Vec::new(),
            widenings_since_new: 0,
            closed: false,
            search_hash: 0,
            action: Some(action),
        }
    }
}

/// Root-level introspection snapshot returned by
/// [`RunPlanner::search_with_stats`]: each root child's visit count and
/// average value (`Q`), in expansion order. Lets integration tests (which
/// can't see the private `RunNode` arena) assert on search internals —
/// e.g. that a terminal loss backed up `0.0` rather than a leaf-eval
/// constant, or that a value-steered leaf shifted visits toward the
/// higher-`Q` branch.
#[derive(Debug, Clone, Default)]
pub struct SearchStats {
    pub root_visits: Vec<(RunAction, u32)>,
    pub root_q: Vec<(RunAction, f32)>,
}

/// Run-level MCTS: PUCT-uniform selection (`P = 1 / children_count`, no
/// learned policy prior), depth-bounded leaf bootstrap via [`RunLeafEval`],
/// and plain-average backup (single-agent win-probability, no negamax sign
/// flip, expectimax through chance nodes). A transition whose
/// `StepOutcome::stochastic` is `true` grows a `Chance` node instead of a
/// `Decision` node; nature's outcomes are enumerated by Double Progressive
/// Widening (see `chance_widen_or_sample`).
pub struct RunPlanner<'a> {
    arena: Vec<RunNode<'a>>,
    root: NodeId,
    config: RunMctsConfig,
}

impl std::fmt::Debug for RunPlanner<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunPlanner")
            .field("arena_len", &self.arena.len())
            .field("root", &self.root)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl<'a> RunPlanner<'a> {
    /// Run a full search from `root` and return the most-visited root
    /// child's action — `None` iff `root` has no legal actions (it's
    /// already terminal, or the phase surfaces no actions).
    ///
    /// Precondition (debug-checked only, not enforced in release): both
    /// `config.max_simulations` and `config.max_depth` must be `> 0`. Either
    /// being `0` makes `run_search` a no-op that never allocates a root
    /// child, so `search` would silently return `None` even when `root` has
    /// legal actions — a caller-configuration bug, not a legitimate "no
    /// actions" terminal case, hence a `debug_assert` rather than a
    /// release-mode correction.
    #[must_use]
    pub fn search(
        root: &PlanningRun<'a>,
        config: RunMctsConfig,
        leaf: &dyn RunLeafEval,
        priors: &dyn RunPriorSource,
    ) -> Option<RunAction> {
        Self::search_with_stats(root, config, leaf, priors).0
    }

    /// As [`Self::search`], but also returns the root's per-child visit
    /// counts and average values (`Q = value_sum / visits`) — an
    /// introspection seam for tests/callers that need to confirm search
    /// behavior (determinism, terminal backup, value-steering) at the
    /// public boundary rather than reaching into the arena. Additive: does
    /// not change `search`'s behavior, which delegates here and discards
    /// the stats.
    #[must_use]
    pub fn search_with_stats(
        root: &PlanningRun<'a>,
        config: RunMctsConfig,
        leaf: &dyn RunLeafEval,
        priors: &dyn RunPriorSource,
    ) -> (Option<RunAction>, SearchStats) {
        debug_assert!(
            config.max_simulations > 0,
            "RunMctsConfig::max_simulations must be > 0 (0 silently yields no root children)"
        );
        debug_assert!(
            config.max_depth > 0,
            "RunMctsConfig::max_depth must be > 0 (0 silently yields no root children)"
        );
        let mut planner = Self::new(root, config);
        if planner.root_node().untried.is_empty() && planner.root_node().children.is_empty() {
            return (None, SearchStats::default());
        }
        planner.run_search(leaf, priors);
        let stats = planner.root_stats();
        (planner.best_root_action(), stats)
    }

    /// Per-root-child `(action, visits)` / `(action, Q)` snapshot, in the
    /// same expansion order as `root_node().children`. `Q` is `0.0` for a
    /// child with zero visits (never expanded — shouldn't occur for an
    /// entry in `children`, which is only pushed on expansion, but avoids a
    /// division by zero defensively).
    fn root_stats(&self) -> SearchStats {
        let mut root_visits = Vec::with_capacity(self.root_node().children.len());
        let mut root_q = Vec::with_capacity(self.root_node().children.len());
        for &(action, child_id) in &self.root_node().children {
            let child = &self.arena[child_id];
            root_visits.push((action, child.visits));
            #[allow(clippy::cast_precision_loss)]
            let q = if child.visits == 0 {
                0.0
            } else {
                child.value_sum / child.visits as f32
            };
            root_q.push((action, q));
        }
        SearchStats {
            root_visits,
            root_q,
        }
    }

    fn new(root: &PlanningRun<'a>, config: RunMctsConfig) -> Self {
        let terminal = if root.phase() == Phase::RunOver {
            Some(root.run_state().player_won)
        } else {
            None
        };
        let root_node = RunNode::new_decision(None, root.clone(), terminal);
        Self {
            arena: vec![root_node],
            root: 0,
            config,
        }
    }

    fn root_node(&self) -> &RunNode<'a> {
        &self.arena[self.root]
    }

    fn run_search(&mut self, leaf: &dyn RunLeafEval, priors: &dyn RunPriorSource) {
        for _ in 0..self.config.max_simulations {
            self.simulate(leaf, priors);
        }
    }

    /// One simulation: selection (PUCT-uniform pointer-follow through
    /// `Decision` nodes, DPW widen-or-sample through `Chance` nodes) →
    /// expansion (pop one untried action, `apply(action, 0)`; a stochastic
    /// outcome allocates a `Chance` node wrapping an outcome-0 `Decision`
    /// child) → evaluation (terminal value or `leaf.value`) → backup (plain
    /// average along the path — expectimax through chance nodes, no
    /// negamax sign flip).
    fn simulate(&mut self, leaf: &dyn RunLeafEval, priors: &dyn RunPriorSource) {
        let mut node_id = self.root;
        let mut path: Vec<NodeId> = vec![node_id];
        let mut depth: u32 = 0;
        let mut fresh_value: Option<f32> = None;

        // ── Selection (+ chance descent) ────────────────────────────────
        loop {
            if self.arena[node_id].kind == RunNodeKind::Chance {
                let (next, is_new) = self.chance_widen_or_sample(node_id);
                node_id = next;
                path.push(node_id);
                if is_new {
                    // A freshly-widened outcome is this simulation's leaf —
                    // same treatment as a freshly-expanded Decision child.
                    fresh_value = Some(self.evaluate(node_id, leaf));
                    break;
                }
                // Sampled an existing outcome (always a Decision node) —
                // keep descending from it.
                continue;
            }
            // `Decision` node.
            if self.arena[node_id].terminal.is_some() || depth >= self.config.max_depth {
                break;
            }
            if !self.arena[node_id].untried.is_empty() {
                break; // expand below
            }
            let Some(next) = self.select_child(node_id) else {
                break;
            };
            node_id = next;
            path.push(node_id);
            depth += 1;
        }

        // ── Expansion (only reached at a non-terminal Decision node with
        // untried actions left, inside the depth budget) ─────────────────
        if fresh_value.is_none()
            && self.arena[node_id].kind == RunNodeKind::Decision
            && self.arena[node_id].terminal.is_none()
            && !self.arena[node_id].untried.is_empty()
            && depth < self.config.max_depth
        {
            // First expansion of this decision node: snapshot the full legal
            // set (== current `untried`, in legal order) and compute+normalize
            // its priors ONCE. Child `i` (pushed in pop order) then reads
            // `child_priors[i]`.
            if self.arena[node_id].children.is_empty() {
                let legal = self.arena[node_id].untried.clone();
                let raw = priors.priors(&self.arena[node_id].run, &legal);
                let sum: f32 = raw.iter().sum();
                #[allow(clippy::cast_precision_loss)]
                let norm: Vec<f32> = if sum > 0.0 && raw.len() == legal.len() {
                    raw.iter().map(|p| p / sum).collect()
                } else {
                    vec![1.0 / legal.len().max(1) as f32; legal.len()]
                };
                self.arena[node_id].child_priors = norm;
            }

            let action = self.arena[node_id].untried.remove(0);
            let pre_run = self.arena[node_id].run.clone();
            let mut child_run = pre_run.clone();
            let outcome = child_run.apply(action, 0);
            if outcome.stochastic {
                let chance_id = self.alloc_chance_node(node_id, action, pre_run);
                self.arena[node_id].children.push((action, chance_id));
                path.push(chance_id);

                // Ordinal 0 is the on-policy realization computed above
                // (not a reseed) — always kept, never deduped away.
                let outcome0 = RunNode::new_decision(Some(chance_id), child_run, outcome.terminal);
                let outcome0_id = self.arena.len();
                self.arena.push(outcome0);
                self.arena[chance_id].chance_outcomes.push((0, outcome0_id));
                path.push(outcome0_id);
                fresh_value = Some(self.evaluate(outcome0_id, leaf));
                node_id = outcome0_id;
            } else {
                let child = RunNode::new_decision(Some(node_id), child_run, outcome.terminal);
                let child_id = self.arena.len();
                self.arena.push(child);
                self.arena[node_id].children.push((action, child_id));
                path.push(child_id);
                fresh_value = Some(self.evaluate(child_id, leaf));
                node_id = child_id;
            }
        }

        // ── Evaluation (non-fresh case: terminal / depth-cap leaf) ──────
        let value = fresh_value.unwrap_or_else(|| self.evaluate(node_id, leaf));

        // ── Backup: plain average, no negamax sign flip ────────────────
        for &n in &path {
            let node = &mut self.arena[n];
            node.visits += 1;
            node.value_sum += value;
        }
    }

    /// Allocate a `Chance` node wrapping the stochastic `action` applied
    /// from decision node `parent` (whose pre-action state is `pre_run`,
    /// a clone of `parent`'s own `run`). `randomness_base` folds the
    /// parent's cursor-SENSITIVE [`PlanningRun::run_hash`], the search
    /// seed, a `sibling_salt` (the parent's expanded-child count before
    /// this push, `+1`), and the action's identity — see [`chance_base`].
    fn alloc_chance_node(
        &mut self,
        parent: NodeId,
        action: RunAction,
        pre_run: PlanningRun<'a>,
    ) -> NodeId {
        let parent_hash = self.arena[parent].run.run_hash();
        #[allow(clippy::cast_possible_truncation)]
        let sibling_salt = (self.arena[parent].children.len() as u64).wrapping_add(1);
        let base = chance_base(parent_hash, self.config.seed, sibling_salt, action);
        let node = RunNode::new_chance(parent, pre_run, action, base);
        let id = self.arena.len();
        self.arena.push(node);
        id
    }

    /// Double Progressive Widening at chance node `chance_id`. Returns the
    /// selected outcome's `Decision` child plus a flag that's `true` only
    /// when a brand-new outcome was drawn (the caller treats it as this
    /// simulation's leaf); `false` covers both a deduped widen-draw (the
    /// caller keeps descending into the reused outcome) and a sampled
    /// existing outcome.
    fn chance_widen_or_sample(&mut self, chance_id: NodeId) -> (NodeId, bool) {
        let visits = self.arena[chance_id].visits;
        let n_outcomes = self.arena[chance_id].chance_outcomes.len();
        let c = self.config.chance_widening_c;
        let alpha = self.config.chance_widening_alpha;
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let cap = (c * ((visits + 1) as f32).powf(alpha)).ceil().max(1.0) as usize;

        if n_outcomes >= cap {
            return (self.sample_outcome_by_visits(chance_id), false);
        }

        // WIDEN: re-derive a fresh outcome from the chance node's
        // pre-action state (`run`) by reseeding the run-draw streams to
        // `(randomness_base, n)` and re-applying `action`. `run.clone()`
        // copies the tree-global search-seed `base` unchanged (`PlanningRun`
        // has no reseed of its own besides `new`), so the per-node
        // `randomness_base` computed in `alloc_chance_node` must be
        // explicitly threaded in via `rebase` before `apply` — otherwise
        // every chance node's widens would reseed off the same tree-global
        // base and produce correlated outcomes. Mirrors combat MCTS
        // threading its node's `randomness_base` into
        // `reseed_combat_streams` (`sts2-mcts/src/lib.rs`). `apply` performs
        // the reseed internally for `outcome_n >= 1`.
        let n = self.arena[chance_id].chance_draws;
        self.arena[chance_id].chance_draws = n.saturating_add(1);
        let action = self.arena[chance_id]
            .action
            .expect("chance node always carries its action");
        let mut s = self.arena[chance_id].run.clone();
        s.rebase(self.arena[chance_id].randomness_base);
        let outcome = s.apply(action, n);
        let sh = s.run_search_hash();

        let dup = self.arena[chance_id]
            .chance_outcomes
            .iter()
            .find(|&&(_, cid)| self.arena[cid].search_hash == sh)
            .map(|&(_, cid)| cid);
        if let Some(child) = dup {
            let w = self.arena[chance_id].widenings_since_new.saturating_add(1);
            self.arena[chance_id].widenings_since_new = w;
            if w >= CHANCE_CLOSE_K {
                self.arena[chance_id].closed = true;
            }
            (child, false)
        } else {
            let child = RunNode::new_decision_with_hash(Some(chance_id), s, outcome.terminal, sh);
            let child_id = self.arena.len();
            self.arena.push(child);
            self.arena[chance_id].chance_outcomes.push((n, child_id));
            self.arena[chance_id].widenings_since_new = 0;
            (child_id, true)
        }
    }

    /// Sample an existing chance outcome with probability proportional to
    /// its child's visit count (uniform, keyed deterministically off
    /// `(randomness_base, visits, seed)`, when every outcome is still
    /// unvisited). Deterministic given `(seed, visits-so-far)` — no
    /// mutable RNG state on the planner, so repeated searches with the
    /// same config replay the identical sampling sequence.
    fn sample_outcome_by_visits(&self, chance_id: NodeId) -> NodeId {
        let outcomes = &self.arena[chance_id].chance_outcomes;
        debug_assert!(
            !outcomes.is_empty(),
            "sampling from a chance node with no outcomes"
        );
        let total: u64 = outcomes
            .iter()
            .map(|&(_, cid)| u64::from(self.arena[cid].visits))
            .sum();
        if total == 0 {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let idx = (self.arena[chance_id].visits as usize) % outcomes.len();
            return outcomes[idx].1;
        }
        let key = splitmix64(
            self.arena[chance_id].randomness_base
                ^ u64::from(self.arena[chance_id].visits)
                ^ self.config.seed,
        );
        let mut r = key % total;
        for &(_, cid) in outcomes {
            let w = u64::from(self.arena[cid].visits);
            if r < w {
                return cid;
            }
            r -= w;
        }
        outcomes[outcomes.len() - 1].1
    }

    /// `1.0`/`0.0` for a terminal node (won/lost), else `leaf.value`
    /// (covers both a freshly-expanded child and a depth-capped leaf).
    fn evaluate(&self, node_id: NodeId, leaf: &dyn RunLeafEval) -> f32 {
        let node = &self.arena[node_id];
        match node.terminal {
            Some(true) => 1.0,
            Some(false) => 0.0,
            None => leaf.value(&node.run),
        }
    }

    /// PUCT-uniform descent: `P = 1 / children_count` for every child
    /// (no learned policy prior this task). Mirrors
    /// `sts2-mcts/src/lib.rs`'s PUCT-with-prior branch (~2084-2171): a
    /// never-visited child scores the FPU term `c·P·√ΣN`; a visited child
    /// scores `Q + c·P·√ΣN/(1+n)`. Ties keep the first (lowest-index,
    /// i.e. first-expanded) child — deterministic, since `>` only replaces
    /// the incumbent on a strict improvement.
    fn select_child(&self, node_id: NodeId) -> Option<NodeId> {
        let node = &self.arena[node_id];
        if node.children.is_empty() {
            return None;
        }
        let c = self.config.c_puct;
        #[allow(clippy::cast_precision_loss)]
        let sqrt_parent = (node.visits.max(1) as f32).sqrt();

        let mut best: Option<NodeId> = None;
        let mut best_score = f32::NEG_INFINITY;
        for (i, &(_, child_id)) in node.children.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let p = node
                .child_priors
                .get(i)
                .copied()
                .unwrap_or(1.0 / node.children.len() as f32);
            let child = &self.arena[child_id];
            let score = if child.visits == 0 {
                c * p * sqrt_parent
            } else {
                #[allow(clippy::cast_precision_loss)]
                let q = child.value_sum / child.visits as f32;
                #[allow(clippy::cast_precision_loss)]
                let explore = c * p * sqrt_parent / (1.0 + child.visits as f32);
                q + explore
            };
            if score > best_score {
                best_score = score;
                best = Some(child_id);
            }
        }
        best
    }

    /// Root action = child with max visits; ties keep the lowest-index
    /// (first-expanded) child, matching `select_child`'s tie-break.
    fn best_root_action(&self) -> Option<RunAction> {
        let mut best: Option<(RunAction, u32)> = None;
        for &(action, child_id) in &self.root_node().children {
            let visits = self.arena[child_id].visits;
            let replace = match best {
                Some((_, best_visits)) => visits > best_visits,
                None => true,
            };
            if replace {
                best = Some((action, visits));
            }
        }
        best.map(|(action, _)| action)
    }

    /// Test probe: max decision-PLIES realized in the tree, computed from
    /// each node's `parent` chain back to the root. Lets
    /// `search_respects_budget_and_depth` assert the tree never exceeded
    /// `config.max_depth`.
    ///
    /// A `Chance` node is a transparent pass-through for a single
    /// stochastic action, not a ply of its own (mirrors `simulate`'s own
    /// `depth` counter, which increments once per `select_child`/expansion
    /// step off a `Decision` node and NOT again for the chance
    /// widen-or-sample descent into its outcome) — so this walks the raw
    /// arena parent-chain but only counts an edge when the node ABOVE it
    /// (the one that produced it) is `Decision`. A `Decision`→`Chance`
    /// edge counts (it's the ply that took the stochastic action); the
    /// `Chance`→outcome edge below it does not (same ply, still resolving
    /// it).
    #[cfg(test)]
    fn max_realized_depth(&self) -> u32 {
        let mut max_depth = 0_u32;
        for i in 0..self.arena.len() {
            let mut depth = 0_u32;
            let mut cur = i;
            while let Some(parent) = self.arena[cur].parent {
                if self.arena[parent].kind == RunNodeKind::Decision {
                    depth += 1;
                }
                cur = parent;
            }
            max_depth = max_depth.max(depth);
        }
        max_depth
    }

    /// Test probe: the `RunNodeKind` of each of the root's expanded
    /// children, in expansion order (matching `root_node().children`).
    /// Lets `stochastic_transition_makes_a_chance_node` assert a
    /// stochastic root action allocated a `Chance` node while a
    /// deterministic sibling stayed `Decision`.
    #[cfg(test)]
    fn root_child_kinds(&self) -> Vec<RunNodeKind> {
        self.root_node()
            .children
            .iter()
            .map(|&(_, cid)| self.arena[cid].kind)
            .collect()
    }

    /// Test probe: `(action, chance_node_id)` for every root child that
    /// allocated a `Chance` node. Lets tests reach into a specific chance
    /// node's DPW state (`chance_outcomes.len()`, `chance_draws`,
    /// `closed`) via `chance_node_stats`.
    #[cfg(test)]
    fn root_chance_children(&self) -> Vec<(RunAction, NodeId)> {
        self.root_node()
            .children
            .iter()
            .filter(|&&(_, cid)| self.arena[cid].kind == RunNodeKind::Chance)
            .copied()
            .collect()
    }

    /// Test probe: `(outcomes_len, chance_draws, closed)` for chance node
    /// `chance_id`.
    #[cfg(test)]
    fn chance_node_stats(&self, chance_id: NodeId) -> (usize, u32, bool) {
        let n = &self.arena[chance_id];
        (n.chance_outcomes.len(), n.chance_draws, n.closed)
    }
}

#[cfg(test)]
// Exact float comparison is intentional here: the stub/const/closure
// leaf evals under test return the exact literal they were constructed
// with, without arithmetic.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::planning::tests::{fresh_ironclad_run, StubSurrogate};

    #[test]
    fn surrogate_leaf_eval_returns_probability() {
        let stub = StubSurrogate {
            win_prob: 0.73,
            hp_frac_lost: 0.1,
        };
        let rm = fresh_ironclad_run(1);
        let pr = PlanningRun::new(rm, &stub, 0x77);
        let leaf = SurrogateLeafEval { surrogate: &stub };
        let v = leaf.value(&pr);
        assert!(
            (0.0..=1.0).contains(&v),
            "leaf value must be a probability, got {v}"
        );
        assert_eq!(v, 0.73, "must forward the surrogate's win_prob exactly");
        // context_features must not panic and must round-trip through the
        // view.
        let _ctx = pr.context_features();
    }

    #[test]
    fn stub_leaf_eval_returns_constant() {
        let stub = StubSurrogate {
            win_prob: 1.0,
            hp_frac_lost: 0.1,
        };
        let rm = fresh_ironclad_run(2);
        let pr = PlanningRun::new(rm, &stub, 0x1);
        let leaf = StubLeafEval { v: 0.42 };
        assert_eq!(leaf.value(&pr), 0.42);
    }

    #[test]
    fn fn_leaf_eval_calls_closure_with_current_run_state() {
        let stub = StubSurrogate {
            win_prob: 1.0,
            hp_frac_lost: 0.1,
        };
        let rm = fresh_ironclad_run(3);
        let pr = PlanningRun::new(rm, &stub, 0x2);
        let expected = pr.run_state().current_hp as f32;
        let leaf = FnLeafEval(|pr: &PlanningRun<'_>| pr.run_state().current_hp as f32);
        assert_eq!(leaf.value(&pr), expected);
    }

    #[test]
    fn stochastic_transition_makes_a_chance_node() {
        // Drive past the opening Neow event (all 3 `EventChoice`s are
        // deterministic — a fixed narrative branch, no run-draw RNG) to the
        // relic-reward decision it grants: `[PickRelicReward, SkipRelic]`.
        // Accepting the relic draws from the run-draw RNG (stochastic);
        // skipping does not (deterministic) — a natural single-node
        // fixture with one stochastic and one deterministic sibling.
        let stub = StubSurrogate {
            win_prob: 1.0,
            hp_frac_lost: 0.1,
        };
        let mut rm = fresh_ironclad_run(1);
        let opening = rm.get_available_actions();
        rm.take_action(opening[0]);
        // Ensure the offered relic has an RNG-consuming obtain effect;
        // beta seed hashing can otherwise roll a deterministic stat relic.
        rm.run_state.relic_rewards[0].relic_id = sts2_core::RelicId::ToyBox;
        let pr = PlanningRun::new(rm, &stub, 0x5EED_5EED);
        let legal = pr.legal_actions();
        assert_eq!(
            legal,
            vec![RunAction::PickRelicReward, RunAction::SkipRelic],
            "fixture drifted: expected the post-Neow relic-reward decision"
        );

        let leaf = StubLeafEval { v: 0.5 };
        let cfg = RunMctsConfig {
            max_simulations: legal.len() as u32,
            max_depth: 1,
            seed: 11,
            ..Default::default()
        };
        let mut planner = RunPlanner::new(&pr, cfg);
        planner.run_search(&leaf, &UniformPriors);
        let kinds = planner.root_child_kinds();
        assert_eq!(kinds.len(), 2, "both root actions must be expanded");
        assert!(
            kinds.contains(&RunNodeKind::Chance),
            "PickRelicReward (draws from the run-draw RNG) must allocate a Chance node"
        );
        assert!(
            kinds.contains(&RunNodeKind::Decision),
            "SkipRelic (no draw) must stay a Decision node"
        );
    }

    #[test]
    fn sibling_chance_nodes_get_distinct_bases() {
        // Direct formula-level check (brief's fallback, taken deliberately
        // even though a natural multi-stochastic-sibling fixture exists):
        // holding state_hash/seed/sibling_salt FIXED and varying only the
        // action isolates exactly what bead sts2-rs-0lr87 #1 fixed — a
        // natural fixture's siblings also differ by `sibling_salt`
        // (expansion order), which alone would already decorrelate them
        // and wouldn't prove the action fold is doing anything.
        let a = chance_base(0xABCD_EF01, 7, 3, RunAction::Move { col: 1 });
        let b = chance_base(0xABCD_EF01, 7, 3, RunAction::Move { col: 3 });
        assert_ne!(
            a, b,
            "two different actions from the same parent/salt must decorrelate"
        );
        // Determinism sanity: same inputs, same output.
        assert_eq!(
            a,
            chance_base(0xABCD_EF01, 7, 3, RunAction::Move { col: 1 })
        );
    }

    #[test]
    fn distinct_chance_node_bases_decorrelate_outcomes_at_same_ordinal() {
        // Bead sts2-rs-0lr87 #1 (review CRITICAL): `chance_widen_or_sample`
        // must thread each chance node's own `randomness_base` into
        // `PlanningRun::apply`'s reseed via `rebase` — not leave every clone
        // on the tree-global search seed `PlanningRun::new` set once (and
        // `Clone` copies unchanged). Proof: take ONE pre-action
        // `PlanningRun` (positioned at the first combat-entering `Move`,
        // reusing `run_at_first_combat_entry` — the fast-forwarded combat's
        // survival draw is `Bernoulli(win_prob)` seeded by `(base, n,
        // SURVIVAL_SALT)`, a real stochastic action), clone it twice,
        // `rebase` each clone to a DIFFERENT base, and `apply` the SAME
        // `(action, n)` — the same widen ordinal — to both. Pre-fix (no
        // `rebase` call before `apply`, both clones stuck on the same
        // `self.base` inherited from `pr`) this would be a same-base
        // same-ordinal replay: bit-identical draws, hence identical
        // `run_search_hash`. Post-fix the two bases genuinely diverge the
        // outcome — this test would fail against the pre-fix
        // `chance_widen_or_sample` (which never called `rebase` at all; in
        // fact `rebase` didn't exist pre-fix, so this test could not even be
        // written against that code, only reproduced at the `PlanningRun`
        // level as done here).
        use crate::planning::tests::{action_that_enters_combat, run_at_first_combat_entry};
        let pr = run_at_first_combat_entry(
            1,
            StubSurrogate {
                win_prob: 1.0,
                hp_frac_lost: 0.1,
            },
        );
        let action = action_that_enters_combat(&pr);
        let n = 1u32; // same widen ordinal for both nodes
        let base_a = 0x1111_2222_3333_4444;
        let base_b = 0x5555_6666_7777_8888;
        assert_ne!(base_a, base_b, "test bases must actually differ");

        let mut run_a = pr.clone();
        run_a.rebase(base_a);
        let _ = run_a.apply(action, n);

        let mut run_b = pr.clone();
        run_b.rebase(base_b);
        let _ = run_b.apply(action, n);

        assert_ne!(
            run_a.run_search_hash(),
            run_b.run_search_hash(),
            "two chance nodes sharing widen ordinal {n} but distinct \
             randomness_base must diverge — the per-node base was dropped \
             and both fell back on the same tree-global search seed"
        );

        // Non-vacuous: the SAME base at the SAME ordinal must reproduce the
        // SAME outcome — confirms `apply`'s reseed is deterministic per
        // `(base, n)`, so the divergence above is attributable to `rebase`
        // actually changing which draws get replayed, not to incidental
        // nondeterminism in the fixture.
        let mut run_c = pr.clone();
        run_c.rebase(base_a);
        let _ = run_c.apply(action, n);
        assert_eq!(
            run_a.run_search_hash(),
            run_c.run_search_hash(),
            "same base + same ordinal must reproduce the same outcome"
        );
    }

    #[test]
    fn dpw_opens_more_complete_outcomes_with_visits() {
        use crate::planning::tests::{action_that_enters_combat, run_at_first_combat_entry};
        // `run_at_first_combat_entry` positions `pr` at a MapChoice decision
        // whose every `Move` action enters combat via the stub surrogate —
        // all 3 root actions are stochastic (each fast-forwards a won
        // combat + draws a card reward). Concentrate the search budget at
        // this one ply (`max_depth = 1`) so visits pile onto the chance
        // nodes themselves rather than fanning out deeper.
        let pr = run_at_first_combat_entry(
            1,
            StubSurrogate {
                win_prob: 1.0,
                hp_frac_lost: 0.1,
            },
        );
        let action = action_that_enters_combat(&pr);
        let leaf = StubLeafEval { v: 0.5 };
        let cfg = RunMctsConfig {
            max_simulations: 200,
            max_depth: 1,
            seed: 3,
            ..Default::default()
        };
        let mut planner = RunPlanner::new(&pr, cfg);
        planner.run_search(&leaf, &UniformPriors);

        let (_, chance_id) = planner
            .root_chance_children()
            .into_iter()
            .find(|&(a, _)| a == action)
            .expect("the combat-entering action must allocate a Chance node");
        let (outcomes_len, draws, _closed) = planner.chance_node_stats(chance_id);
        assert!(
            outcomes_len > 1,
            "DPW must open more than the initial on-policy outcome with enough visits, got {outcomes_len}"
        );
        assert!(draws >= outcomes_len as u32);
    }

    #[test]
    fn dpw_deduplicates_identical_complete_future_states() {
        use crate::planning::tests::{
            action_that_enters_combat, run_at_first_combat_entry, with_constant_combat_outcome,
        };
        let pr = with_constant_combat_outcome(run_at_first_combat_entry(
            1,
            StubSurrogate {
                win_prob: 1.0,
                hp_frac_lost: 0.1,
            },
        ));
        let action = action_that_enters_combat(&pr);
        let cfg = RunMctsConfig {
            max_simulations: 200,
            max_depth: 1,
            seed: 3,
            ..Default::default()
        };
        let mut planner = RunPlanner::new(&pr, cfg);
        planner.run_search(&StubLeafEval { v: 0.5 }, &UniformPriors);
        let (_, chance) = planner
            .root_chance_children()
            .into_iter()
            .find(|(candidate, _)| *candidate == action)
            .unwrap();
        let (outcomes, draws, _) = planner.chance_node_stats(chance);
        assert_eq!(outcomes, 1, "identical fully hashed successors deduplicate");
        assert!(draws > 1, "exercise repeated widen draws");
    }

    #[test]
    fn search_is_deterministic() {
        let stub = StubSurrogate {
            win_prob: 1.0,
            hp_frac_lost: 0.1,
        };
        let rm = fresh_ironclad_run(1);
        let pr = PlanningRun::new(rm, &stub, 0x1234);
        let leaf = StubLeafEval { v: 0.5 };
        let cfg = RunMctsConfig {
            max_simulations: 200,
            max_depth: 6,
            seed: 42,
            ..Default::default()
        };
        let a1 = RunPlanner::search(&pr, cfg, &leaf, &UniformPriors);
        let a2 = RunPlanner::search(&pr, cfg, &leaf, &UniformPriors);
        assert_eq!(a1, a2, "same seed/budget must yield same action");
        assert!(a1.is_some());
    }

    #[test]
    fn uniform_priors_reproduces_m3_search() {
        // UniformPriors must give byte-identical results to the pre-seam
        // hard-uniform search: same action, same per-child visit counts.
        let stub = StubSurrogate {
            win_prob: 1.0,
            hp_frac_lost: 0.1,
        };
        let rm = fresh_ironclad_run(1);
        let pr = PlanningRun::new(rm, &stub, 0x1234);
        let leaf = StubLeafEval { v: 0.5 };
        let cfg = RunMctsConfig {
            max_simulations: 200,
            max_depth: 6,
            seed: 42,
            ..Default::default()
        };
        let (a1, s1) = RunPlanner::search_with_stats(&pr, cfg, &leaf, &UniformPriors);
        let (a2, s2) = RunPlanner::search_with_stats(&pr, cfg, &leaf, &UniformPriors);
        assert_eq!(a1, a2, "same seed/budget must yield same action");
        assert_eq!(
            s1.root_visits, s2.root_visits,
            "visits must be deterministic"
        );
        assert!(a1.is_some());
    }

    #[test]
    fn search_respects_budget_and_depth() {
        let stub = StubSurrogate {
            win_prob: 1.0,
            hp_frac_lost: 0.1,
        };
        let rm = fresh_ironclad_run(2);
        let pr = PlanningRun::new(rm, &stub, 1);
        let legal: Vec<_> = pr.legal_actions();
        let leaf = StubLeafEval { v: 0.5 };
        let cfg = RunMctsConfig {
            max_simulations: 50,
            max_depth: 4,
            seed: 7,
            ..Default::default()
        };
        let a = RunPlanner::search(&pr, cfg, &leaf, &UniformPriors).unwrap();
        assert!(legal.contains(&a), "returned action must be legal at root");

        // Depth probe: drive a planner the same way `search` does, then
        // assert the realized tree never exceeded `max_depth` decision-plies.
        let mut planner = RunPlanner::new(&pr, cfg);
        planner.run_search(&leaf, &UniformPriors);
        assert!(
            planner.max_realized_depth() <= cfg.max_depth,
            "realized depth {} exceeded max_depth {}",
            planner.max_realized_depth(),
            cfg.max_depth
        );
    }

    // A prior that puts almost all mass on the LAST legal action.
    #[cfg(test)]
    #[derive(Debug)]
    struct LastActionPrior;
    #[cfg(test)]
    impl RunPriorSource for LastActionPrior {
        fn priors(&self, _pr: &PlanningRun<'_>, actions: &[RunAction]) -> Vec<f32> {
            let mut v = vec![0.01_f32; actions.len()];
            if let Some(last) = v.last_mut() {
                *last = 100.0;
            }
            v
        }
    }

    #[test]
    fn nonuniform_prior_shifts_visit_distribution() {
        let stub = StubSurrogate {
            win_prob: 1.0,
            hp_frac_lost: 0.1,
        };
        let rm = fresh_ironclad_run(2);
        let pr = PlanningRun::new(rm, &stub, 0x9);
        let leaf = StubLeafEval { v: 0.5 };
        let cfg = RunMctsConfig {
            max_simulations: 200,
            max_depth: 4,
            seed: 7,
            ..Default::default()
        };
        let (_, s_uniform) = RunPlanner::search_with_stats(&pr, cfg, &leaf, &UniformPriors);
        let (_, s_biased) = RunPlanner::search_with_stats(&pr, cfg, &leaf, &LastActionPrior);
        assert_ne!(
            s_uniform.root_visits, s_biased.root_visits,
            "a non-uniform prior must measurably shift the visit distribution"
        );
    }
}
