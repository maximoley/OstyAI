# Four-way human combat comparison

[Documentation guide](README.md) · [Project overview](../README.md)

**On this page:**

- [Offline compatibility preparation](#offline-compatibility-preparation)
- [Metrics and timing](#metrics-and-timing)
- [Completed results](#completed-results)
- [Commands](#commands)

This experiment uses all 512 human A10 winning runs: 12,099 independent combat starts × three frozen RNG states = 36,297 trials per configuration. All runs use **80 workers**, execute sequentially with production training paused, and cap actual play at 1000 actions per trial. The actor is the immutable exported step-5500 checkpoint. None of the neural configurations uses persistent search reuse. The policy-only arm evaluates the legal action logits and selects their argmax, with no search, chance sampling, or replay updates.

| Configuration | Simulations per decision | Maximum MCTS rollout length |
|---|---|---:|
| Mentor RL training budget | Common/easy/hard/elite: 250; boss: 1000 | 200 |
| Step 5500, tiered Gumbel | Easy 32; hard 64; elite 96; boss 128 | Not applicable: neural leaf evaluation, no fixed search depth cap |
| Step 5500, uniform Gumbel | 32 for every combat | Not applicable |
| Step 5500, greedy policy | 0; highest-logit legal action | Not applicable |

Mentor uses its existing complete native MCTS, BeliefDraw mode, heuristic rollouts, TexelWide leaf evaluation, RichBoth terminal evaluation, uniform prior and the deployed mode tuning/root guards. Its upstream evaluator is retained: it does **not** substitute the neural model's potion-cost objective. The model runs use the frozen 0/1 per-potion penalties. This is a comparison of complete agents, with this objective difference made explicit.

The previous 1,000/4,000 baseline was stopped at the user's request after 4,978 completed trials; these partial results are retained separately and will not be mixed with the replacement. The 250/1,000 preset passed an 80-worker, 80-trial smoke test on the repaired engine (42 normal, 27 elite, 11 boss; exact simulation accounting verified). This smoke test is not a full benchmark.

A pristine upstream checkout at `342c525aac02605b480c02f467837842f9ccf704` was built and audited, but its importer rejects 2,037 fixtures (`NEOWS_SACRIFICE` and `DOWSING_ROD`) and does not restore sampled opening choices. The user selected the updated OstyAI Rust simulator for compatibility with the newer game data. The replacement uses that existing repaired engine, with no new simulator or MCTS changes. The pristine checkout is retained separately for reference and is not used by this comparison.

## Offline compatibility preparation

<details>
<summary>Expand compatibility preparation and state-equality checks</summary>

No runtime import adapter or alternate MCTS algorithm is introduced. The existing `state_from_dict` / `play_default_mcts` entry points consume copies prepared before the timed runs. The offline example `native/crates/sts2-wire/examples/prepare_mcts_fixtures.rs` normalizes recognized obsolete openings using the repaired simulator's native move definitions. All four configurations consume the same compatible copies, while the repository's original benchmark fixtures remain unchanged.

For this panel, 952 enemy openings require normalization. In 192 Haunted Ship starts, the older loader represents the informational Debuff amount as 0 rather than 3. HAUNT's native effect still applies Weak 3 and adds five Dazed cards. This display-field difference is recorded explicitly; all other reconstructed state fields must compare equal to the original repaired import. Prepared fixtures must produce exactly equal native states through the neural and standard MCTS importers. Frozen RNG state, context, metadata and potion labels are preserved. Consequently these are **new matched compatibility-panel results**, not replacements for the historical step-5500 results.

Preparation takes place once, separately timed; its change log, source manifest hash and prepared-shard hashes accompany results. The runtime library is the unchanged deployed build `6a041f84c93019b6f32ed4977a95806e34e643b532126405925c5d718185e62f`. The discarded adapter build is not used.

</details>

## Metrics and timing

Report trial wins / 36,297, majority combat wins / 12,099 (at least 2/3), and runs with every combat majority-won / 512. Capped/unresolved trials remain in win-rate denominators. Report unresolved counts and HP lost; HP means use completed trials, with both trial-weighted and combat-weighted means retained. Combats start independently; run wins are not linked whole-run rollouts carrying model HP.

The primary runtime is measured outside each benchmark process, from launch to successful completion, including loading, inference compilation, output writing and summary aggregation. Offline compatibility preparation and preflight are excluded. Native per-combat seconds and neural collection/capture statistics are retained separately; summed worker CPU durations are not labeled as wall time. Each complete timed run starts with a fresh result directory. If a run must resume, retained partial data are diagnostic and its interrupted timing must not be presented as a fresh uninterrupted comparison.

MCTS trials are journaled as they finish. Neural trials are saved per shard. The runner verifies complete counts, matching fixture/native identities and checkpoint preservation before resuming the paused trainer. It does not consider a launched or partially completed run a finished benchmark.

## Completed results

The Mentor RL-training-budget arm completed all **36,297 trials** on October 26, 2026, with **33,577 wins (92.51%)**, **11,302 majority-won combats (93.41%)**, and **159/512 runs (31.05%)** meeting the ≥2/3 rule in every fight. **269/512 runs (52.54%)** met the ≥1/3 rule in every fight. No trials were unresolved; mean HP lost was **11.57**. The external full-process timer recorded **4,926.89 seconds (1 h 22 min 7 s)** with 80 workers.

[Source outcomes, provenance, recomputed metrics and timing](../benchmarks/comparison/README.md) are included. This MCTS run finished uninterrupted. The subsequent neural arm initially failed during Torch/Triton startup before completing a shard; its failed 19.29-second attempt was archived separately and a fresh timed run started after restoring the successful preflight's project cache configuration. The completed MCTS outcomes and timing were preserved. Neural results remain pending.

## Commands

<details>
<summary>Expand commands and deployment-specific orchestration notes</summary>

After building the offline preparation example and preparing a manifest with the new shard hashes:

```bash
.venv/bin/python scripts/benchmark_mcts.py --prepared COMPATIBLE_FIXTURES --workers 80 --rollout-depth 200 --budget-preset rl-training --output results/mentor-rl-training
.venv/bin/python scripts/evaluate.py --suite human --human-fixtures COMPATIBLE_FIXTURES --workers 80 --output results/model5500-tiered
.venv/bin/python scripts/evaluate.py --suite human --human-fixtures COMPATIBLE_FIXTURES --workers 80 --uniform-simulations 32 --output results/model5500-uniform32
.venv/bin/python scripts/evaluate.py --suite human --human-fixtures COMPATIBLE_FIXTURES --workers 80 --policy-only --output results/model5500-policy-greedy
```

`run_human_comparison.py` is the training host orchestration script for this explicitly authorized pause/run/resume experiment; its production checkpoint and STOP ownership checks are deployment-specific. It is not invoked by the normal evaluator or by cloning this repository.

The policy-only arm remains queued, and the 8,000-simulation / 400-rollout configuration remains cancelled. The replacement starts in a fresh `mentor_rl_training` output directory with the updated engine identity recorded. Production remains paused at the preserved checkpoint during the four sequential runs. Training resumes only after all four configurations finish and pass verification.

</details>
