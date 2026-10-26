# Combat Engine

The Rust reimplementation of Slay the Spire 2's combat, the search that plays
it, and the run-AI nets served at inference time. Standalone-buildable: it
compiles and tests with no sibling on disk.

Terms shared with the rest of the monorepo live in
[../CONTEXT-MAP.md](../CONTEXT-MAP.md#shared-language).

## Language

### The engine

**Engine**:
This crate workspace's implementation of the game's rules. In any parity discussion it is the *subject*, never the reference.
_Avoid_: sim, simulator, model

**Rules kernel**:
The fixed-point core that resolves a combat action to completion — no floating point, no ambient randomness beyond the seeded streams.
_Avoid_: core loop, resolver

**Hook**:
A subscription point where a card, relic or power injects behaviour into rule resolution. Hook *order* is part of the observable behaviour, not an implementation detail.
_Avoid_: callback, listener, event handler

**State transition**:
One advance of engine state. The unit of the throughput benchmark.
_Avoid_: tick, step, frame

### Search

**Search**:
The Monte-Carlo tree search that chooses a combat action. Distinct from the run policy, which chooses between combats.
_Avoid_: AI, solver, planner

**Sims**:
The search budget for one decision, counted in simulations. Always state it when quoting a result — the same agent at 200 and 50,000 sims is not the same agent.
_Avoid_: iterations, depth, budget

**Rollout**:
One simulated continuation from a search node to a terminal or cut-off state. Search strength comes from rollouts, not from the leaf evaluator.
_Avoid_: playout, sample

**Chance node**:
A search node representing a random outcome the agent cannot choose, such as a card draw. Modelling these explicitly is the default.
_Avoid_: random node, stochastic node

**Leaf eval**:
The evaluator called at a search node that is cut off rather than played out.
_Avoid_: heuristic, static eval

**Position book**:
A frozen set of combat positions with reference actions attached, produced by a much stronger search than the one under test. A reference, not ground truth — it has been measured wrong on a fifth of a class of positions.
_Avoid_: oracle, ground truth, solution book

### Evaluation fixtures

**Seed panel**:
A named, frozen list of seeds used as an evaluation fixture, so two agents are measured on identical work.
_Avoid_: test set, sample, benchmark

**Run corpus**:
Real player runs replayed as fixtures, filtered to the pinned build id. Unfiltered corpora silently contaminate every benchmark.
_Avoid_: dataset, replays, logs

**Combat parity fixture**:
A scripted single fight, frozen with its expected state, that a new mechanic must pass once it stops being a marker stub.
_Avoid_: golden test, snapshot

### Run AI

**Run policy**:
The net choosing between combats — cards to take, paths to walk, shops to buy from. Contrast with search, which plays inside a combat.
_Avoid_: agent, meta policy, outer policy

**Champion**:
The promoted run policy currently serving a character. Defined and pinned in the Integration Hub; see [../sts2-pycli/CONTEXT.md](../sts2-pycli/CONTEXT.md).
_Avoid_: best model, current net, production model

**Value net**:
A net predicting a run's or position's outcome. It is a *signal* consumed by a policy or a search, never a policy in itself.
_Avoid_: critic, evaluator

**Ranker**:
The retired pre-PPO run policy. Deprecated since 2026-07-12 — never the baseline for a new measurement, and its published win rates are not comparable to current ones.
_Avoid_: using it at all; say "the PPO policy" when you mean the current agent
