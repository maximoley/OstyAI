# Four-way human combat comparison

This experiment uses all 512 human A10 winning runs: 12,099 independent combat starts × three frozen RNG states = 36,297 trials per configuration. All runs use **80 workers**, execute sequentially with production training paused, and cap actual play at 1000 actions per trial. The actor is the immutable exported step-5500 checkpoint. None of the neural configurations uses persistent search reuse. The policy-only arm evaluates the legal action logits and selects their argmax, with no search, chance sampling, or replay updates.

| Configuration | Simulations per decision | Maximum MCTS rollout length |
|---|---|---:|
| Mentor deployed validation (`eval-cheap`) | Common/easy/hard/elite: 1000; boss: 4000 | 200 |
| Step 5500, tiered Gumbel | Easy 32; hard 64; elite 96; boss 128 | Not applicable: neural leaf evaluation, no fixed search depth cap |
| Step 5500, uniform Gumbel | 32 for every combat | Not applicable |
| Step 5500, greedy policy | 0; highest-logit legal action | Not applicable |

Mentor uses its existing complete native MCTS, BeliefDraw mode, heuristic rollouts, TexelWide leaf evaluation, RichBoth terminal evaluation, uniform prior and the deployed mode tuning/root guards. Its upstream evaluator is retained: it does **not** substitute the neural model's potion-cost objective. The model runs use the frozen 0/1 per-potion penalties. This is a comparison of complete agents, with this objective difference made explicit.

## Offline compatibility preparation

No runtime import adapter or alternate MCTS algorithm is introduced. The existing `state_from_dict` / `play_default_mcts` entry points consume copies prepared before the timed runs. The offline example `native/crates/sts2-wire/examples/prepare_mcts_fixtures.rs` normalizes recognized obsolete openings using the repaired simulator's native move definitions. All four configurations consume the same compatible copies, while the repository's original benchmark fixtures remain unchanged.

For this panel, 952 enemy openings require normalization. In 192 Haunted Ship starts, the older loader represents the informational Debuff amount as 0 rather than 3. HAUNT's native effect still applies Weak 3 and adds five Dazed cards. This display-field difference is recorded explicitly; all other reconstructed state fields must compare equal to the original repaired import. Prepared fixtures must produce exactly equal native states through the neural and standard MCTS importers. Frozen RNG state, context, metadata and potion labels are preserved. Consequently these are **new matched compatibility-panel results**, not replacements for the historical step-5500 results.

Preparation takes place once, separately timed; its change log, source manifest hash and prepared-shard hashes accompany results. The runtime library is the unchanged deployed build `6a041f84c93019b6f32ed4977a95806e34e643b532126405925c5d718185e62f`. The discarded adapter build is not used.

## Metrics and timing

Report trial wins / 36,297, majority combat wins / 12,099 (at least 2/3), and runs with every combat majority-won / 512. Capped/unresolved trials remain in win-rate denominators. Report unresolved counts and HP lost; HP means use completed trials, with both trial-weighted and combat-weighted means retained. Combats start independently; run wins are not linked whole-run rollouts carrying model HP.

The primary runtime is measured outside each benchmark process, from launch to successful completion, including loading, inference compilation, output writing and summary aggregation. Offline compatibility preparation and preflight are excluded. Native per-combat seconds and neural collection/capture statistics are retained separately; summed worker CPU durations are not labeled as wall time. Each complete timed run starts with a fresh result directory. If a run must resume, retained partial data are diagnostic and its interrupted timing must not be presented as a fresh uninterrupted comparison.

MCTS trials are journaled as they finish. Neural trials are saved per shard. The runner verifies complete counts, matching fixture/native identities and checkpoint preservation before resuming the paused trainer. It does not consider a launched or partially completed run a finished benchmark.

## Commands

After building the offline preparation example and preparing a manifest with the new shard hashes:

```bash
.venv/bin/python scripts/benchmark_mcts.py --prepared COMPATIBLE_FIXTURES --workers 80 --rollout-depth 200 --output results/mentor-default
.venv/bin/python scripts/evaluate.py --suite human --human-fixtures COMPATIBLE_FIXTURES --workers 80 --output results/model5500-tiered
.venv/bin/python scripts/evaluate.py --suite human --human-fixtures COMPATIBLE_FIXTURES --workers 80 --uniform-simulations 32 --output results/model5500-uniform32
.venv/bin/python scripts/evaluate.py --suite human --human-fixtures COMPATIBLE_FIXTURES --workers 80 --policy-only --output results/model5500-policy-greedy
```

`run_human_comparison.py` is the Zeus orchestration script for this explicitly authorized pause/run/resume experiment; its production checkpoint and STOP ownership checks are deployment-specific. It is not invoked by the normal evaluator or by cloning this repository.

The policy-only arm was added while the first MCTS process was running. The controller was replaced without interrupting that benchmark or its workers; its original launch timestamp is retained. This first arm uses process-exit polling at 0.1-second intervals for completion timing, which can add polling/scheduling latency. Later arms are timed directly around their child process.

The user subsequently removed the queued 8000-simulation, rollout-cap-400 MCTS configuration because of runtime. It was never launched as a full benchmark; its earlier 80-trial preflight is retained only as a diagnostic, not a benchmark result. The active default MCTS process and its original timing were preserved. Training resumes after the four configurations listed above finish.
