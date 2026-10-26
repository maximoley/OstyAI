# Persistent per-combat Gumbel search graph

User-authorized migration after a clean pause at optimizer step 5500. The simulator's transition rules, model, reward, potion costs, learner, replay, search budgets, widening constants and validation schedules are unchanged. Search now retains and reuses computation, so policies and subsequent trajectories can change.

## Ownership and identity

Each combat Record owns a bounded search graph. It survives actual moves and turns, stays exclusively owned while moving between Rust workers, and is dropped when the combat/collection ends. No global shared lock is introduced. The actor is immutable during a collection; actor-version changes invalidate the entire graph.

A table hash is only an index into a collision bucket. A hit requires full CombatState equality after the existing belief-root canonicalization (unknown draw-pile order and hidden combat RNG are replaced with fixed canonical values), plus the Session reward/continuation/history/command signature. Known draw pins, enemy AI, card IDs, upgrades, enchantments, powers, suspended effects, pending selections, potion instance costs/spending and reward progress remain significant. Frozen context and initial-cost maps use their shared Arc owner identities, avoiding repeated serialization of the large context JSON. The table is per combat, never shared between independently created contexts. Derived export caches are excluded. This is deliberately conservative: visually identical states with different histories, counters, IDs or continuation state do not merge.

## Search semantics

- Matching states share policy/value evaluations and outgoing search statistics. Incoming parent-action Q statistics remain on their own edges.
- Chance edges retain a list of sampled outcome references, including repeated references to the same node. This preserves empirical sampling multiplicity when duplicate states merge. Cached-outcome selection remains uniform over samples.
- New stochastic outcomes still require simulator execution. A transposition hit avoids duplicate neural evaluation and allows descent through the existing graph.
- Rerooting retains outgoing Q/visits and descendants. A separate fresh-root visit counter drives the sequential-halving schedule and final eligibility. Every real searched decision still receives its full 32/64/96/128 new simulations.
- Root evaluations are reused when exact identity and actor match; replay observations are still freshly exported from the factual state.
- Forced factual moves still execute immediately and retain replay rows. Forced belief chains stay compressed within edges.
- A repeated node on the active path bootstraps its stored value once to avoid infinite graph traversal. No depth limit is added.

## Bounds and eviction

Default retained capacity is 512 nodes per combat. Before each new search, compact the graph, prioritizing the new root and its descendants, then recent alternatives. A search can temporarily add at most its budget plus a new root (maximum 641 nodes with the current 128 budget). If an outcome is evicted from a chance edge, invalidate the entire cached outcome list rather than retaining a biased subset. Q evidence remains; new samples use the current decision's search seed. There is no checkpoint serialization of these transient actor graphs; model/optimizer/replay/RNG persistence remains unchanged.

## Configuration and logging

Native flags: persistent_search (default false for old callers), search_cache_nodes (512). Production explicitly enables reuse for training and both search validation panels. Zero-search policy validation disables it. Reports and progress log search_reuse with transposition_hits, persistent_hits, root_reuses, root_evaluation_hits, reused_nodes, evicted_nodes, cycle_backups and peak_nodes_per_combat. Existing cached-edge, transition, simulation and max-depth counters remain available.

## Validation and migration

The partly completed step5500 human benchmark finishes with the original native hash and actor before deployment. Completed historical benchmarks retain their original provenance; they can be recognized on resume only if model, manifest and identity match their completed summary. Incomplete caches still require exact current provenance. Future validations use the new native hash and reuse configuration.

Evidence includes before-* checkpoint/replay/native/source backups, pause-verification.json, rust-tests-final.log, matched disabled-path parity, fixed-actor throughput comparison, small-capacity eviction smoke, a full next training bank with replay ingestion, policy-only smoke, checkpoint-preservation.json, deployment.json and resume-verification.json. No diagnostic optimizer state is promoted to production.

## Verified results

24 Rust tests passed. Disabled-path parity matched all outcomes/actions for 24 fixed combats. Two 512-combat fixed-actor comparisons (forward and reverse order) produced 477 wins with reuse versus 469 without; all combats resolved. This small diagnostic panel does not establish a general strength gain. End-to-end times were 43.20/47.63 seconds with reuse versus 33.00/35.02 seconds without: reuse is currently 31–36% slower overall, despite avoiding roughly 70% of root evaluations. Memoized identity hashes reduced overhead substantially from the first implementation.

The full next training bank (1,019 combats) resolved completely and ingested 36,005 replay examples. Tiny-capacity eviction and zero-search policy checks passed, as did historical validation cache identity/rejection tests. The deployed native hash is `6a041f84c93019b6f32ed4977a95806e34e643b532126405925c5d718185e62f`. See smoke-results.json and deployment.json.

## Resumed production

Verified advancing updates 5501 and 5502 under native hash6a041f84c93019b6f32ed4977a95806e34e643b532126405925c5d718185e62f, with a valid step5501 checkpoint. All model/optimizer/replay tensors are finite, priorities positive, pool tags valid, search accounting and exact learner quotas correct. Trainer PID3830640 and supervisor PID3830639 are live and load the deployed library. First new cycle reused24,223 root evaluations across34,134 searched decisions (71.0%); collection44.7s and complete first cycle56.8s including first learner capture. The old5500 human summary remains byte-identical. Hourly recovery instructions preserve this deployment. See resume-verification.json.
