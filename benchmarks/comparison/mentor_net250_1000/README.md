# Mentor MCTS with Necrobinder playout network

[Main comparison](../../../README.md#matched-agent-comparison) · [Earlier heuristic baseline](../mentor_rl_training/provenance.json)

The shipped Necrobinder v1 playout weights scored **34,209/36,297 trial wins (94.25%)**, with **zero unresolved trials**, in **101 min 51 s**. This is **632 additional wins (+1.74 percentage points)** over the earlier heuristic-only 250/1,000 benchmark. All trials use the same frozen combat starts, three RNG seeds and potion context.

| Metric | Result |
|---|---:|
| Combat majority wins | **95.16%** |
| Every fight ≥2/3 seeds | **41.41%** |
| Every fight ≥1/3 seeds | **64.45%** |
| Easy trial wins | **99.83%** |
| Hard trial wins | **98.73%** |
| Elite trial wins | **93.79%** |
| Boss trial wins | **78.14%** |

## Configuration and scope

- **80 workers**, **250 non-boss / 1,000 boss simulations**, rollout cap **200**, factual action cap **1,000**.
- BeliefDraw, TexelWide, RichBoth, uniform tree prior and the previous root guards/evaluator. The network chooses rollout actions; it is not the OstyAI actor.
- Shipped weights: [upstream Necrobinder network](https://github.com/tfriedel/spire-mentor/blob/52bfee16387685a415abc22255c7b58e62f0f91e/sts2-rs/data/rollout_nets/necrobinder.json), unchanged from the October 25 snapshot. Feature version 1, 32 state / 24 action features, temperature 1.0, early End Turn allowed.
- The v1 inference path was backported into an isolated copy of the same repaired simulator/search as the earlier benchmark. This isolates the network addition; it is **not a full upgrade to the latest upstream engine/search**. See [deployment hashes](deployment.json) and [provenance](provenance.json).
- Six loader/scoring/sampling tests and two fixed-state/loading tests passed. Fixed-hand logits match upstream golden bits. Independent NumPy scoring matched 711 logits across 80 frozen starts (maximum absolute error 0.00000763); the 80-combat smoke resolved every fight. The upstream randomized trajectory golden has different hands/state counts across simulator versions and was not reblessed.

Run metrics aggregate independently reset historical combats, not end-to-end played runs. The MCTS evaluator retains its own potion objective; frozen OstyAI potion costs remain recorded. Training stayed paused at step 6,817, with checkpoint preservation verified.

## Timing

The external **6,110.9553-second** timer includes startup, imports, all trials and output writing, plus **up to approximately 10 seconds of completion-polling overhead**. Setup, compilation and smoke testing are excluded. Existing compilation caches were retained, as for the earlier comparisons. This is a single-run measurement. Start/end GPU snapshots showed only the background Dorado service; CPU contention was not sampled continuously. [Timing record](timing.json).

## Results and verification

[Summary](summary.json) · [Exact counts](readme-metrics.json) · [Pool counts](pool-win-rates.json) · [Completion verification](verification.json) · [Preflight](preflight.json) · [Smoke](smoke-verification.json)

[All trials](trials.json.gz) · [Combats](combats.json.gz) · [Runs](runs.json.gz) · [Combat CSV](combat-outcomes.csv) · [Exact fixture index for all 2,088 losses](failed-trial-fixture-index.jsonl.gz)

The completion verification checked every trial ID, fixture hash, environment seed and potion cost against the original heuristic benchmark. Network execution counters recorded 13,495,061,197 actual playout selections. [Source/copy hashes](source-hashes.json) retain original server hashes and hashes of the published copies. `archive/` paths are portable references to the original benchmark archive, not personal server paths. Recompute with `python scripts/verify_mentor_net.py`.
