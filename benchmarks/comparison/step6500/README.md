# Step 6500 matched human benchmark results

[Main table](../../../README.md#matched-agent-comparison) · [Scheduled step-6500 evaluation](../../human-validation-006500/summary.json) · [Historical timed runs](../README.md)

The three neural runs evaluate the same actor on 12,099 combats × three frozen seeds (36,297 trials), using 80 workers, a 1,000-action cap, and no persistent reuse. Training remained paused at step6500. All runs completed and checkpoint/replay preservation was verified.

| Metric | Mentor net 250/1,000 | Mentor heuristic 250/1,000 | Mentor heuristic 250/500 | OstyAI 6500, boss128 | OstyAI 6500, boss256 | OstyAI 6500, greedy |
|---|---|---|---|---|---|---|
| Simulations per decision | 250 non-boss; 1,000 boss | 250 non-boss; 1,000 boss | 250 non-boss; 500 boss | 32 / 64 / 96 / 128 | 32 / 64 / 96 / 256 | No search; legal argmax |
| Trial win rate | **94.25%** | **92.51%** | **91.91%** | **92.97%** | **93.27%** | **88.46%** |
| Majority combat win rate | **95.16%** | **93.41%** | **92.78%** | **94.01%** | **94.37%** | **89.72%** |
| Every fight ≥2/3 | **41.41%** | **31.05%** | **29.88%** | **31.05%** | **35.16%** | **16.21%** |
| Every fight ≥1/3 | **64.45%** | **52.54%** | **48.44%** | **58.40%** | **60.74%** | **38.28%** |
| Wall time | **101 min 51 s** | **82 min 07 s** | **50 min 51 s** | **35 min 39 s** | **56 min 43 s** | **3 min 37 s** |

| Combat pool | Mentor net 250/1,000 | Mentor heuristic 250/1,000 | Mentor heuristic 250/500 | OstyAI 6500, boss128 | OstyAI 6500, boss256 | OstyAI 6500, greedy |
|---|---|---|---|---|---|---|
| Easy | **99.83%** | **99.75%** | **99.75%** | **99.72%** | **99.72%** | **99.16%** |
| Hard | **98.73%** | **98.32%** | **98.32%** | **98.48%** | **98.49%** | **96.32%** |
| Elite | **93.79%** | **92.48%** | **92.48%** | **93.80%** | **93.56%** | **87.78%** |
| Boss | **78.14%** | **70.72%** | **67.17%** | **71.16%** | **73.31%** | **58.41%** |

## Timing and interpretation

The network-enabled Mentor rerun was recorded later with training paused at 6,817. It used the same repaired simulator/search with the shipped Necrobinder v1 playout network added; the other Mentor columns remain heuristic-only. Its external timer includes up to 10 seconds of completion polling. [Network artifacts and scope](../mentor_net250_1000/README.md). The two-second polling statement below applies to the three neural runs.

Full external process wall time includes startup, imports, compilation/cache loading, all trials, and result writing; fixture preparation and verification are excluded. Existing compilation caches were retained. No unrelated GPU jobs were observed during the new runs; the longstanding Dorado service remained resident. The boss128 pre-wait GPU snapshot contains the earlier AlphaFold job, but the launcher waited for its exit before starting the timer. Completion polling adds at most about two seconds.

These are single sequential measurements, not repeated randomized timing experiments. MCTS uses its original value evaluator and does not apply the neural potion-cost objective. Run metrics aggregate independent historical combat starts, not end-to-end played runs. Losses and unresolved cases both count as nonwins.

The scheduled boss128 evaluation recorded 33,730 wins; the fresh timed boss128 pass recorded 33,744. They are separate runs with different execution/batching contexts. Preserve both records; do not infer a learning change between them.

## Complete result artifacts

- **OstyAI 6500, boss128:** [summary](model6500_boss128/summary.json), [recomputed metrics](model6500_boss128/readme-metrics.json), [timing](model6500_boss128/timing.json), [provenance](model6500_boss128/provenance.json), [all trials](model6500_boss128/trials.json.gz), [combats](model6500_boss128/combats.json.gz), [runs](model6500_boss128/runs.json.gz). All 48 part reports are retained beside these files.
- **OstyAI 6500, boss256:** [summary](model6500_boss256/summary.json), [recomputed metrics](model6500_boss256/readme-metrics.json), [timing](model6500_boss256/timing.json), [provenance](model6500_boss256/provenance.json), [all trials](model6500_boss256/trials.json.gz), [combats](model6500_boss256/combats.json.gz), [runs](model6500_boss256/runs.json.gz). All 48 part reports are retained beside these files.
- **OstyAI 6500, greedy:** [summary](model6500_policy_greedy/summary.json), [recomputed metrics](model6500_policy_greedy/readme-metrics.json), [timing](model6500_policy_greedy/timing.json), [provenance](model6500_policy_greedy/provenance.json), [all trials](model6500_policy_greedy/trials.json.gz), [combats](model6500_policy_greedy/combats.json.gz), [runs](model6500_policy_greedy/runs.json.gz). All 48 part reports are retained beside these files.

[Combined metrics](readme-metrics.json) · [Matched comparison](matched-comparison.json) · [Completion verification](final-verification.json) · [Original/stored artifact hashes](source-hashes.json) · [Recompute](../../../scripts/summarize_step6500.py)

## Combat-level analysis

- [All 12,099 combat instances](combat-analysis/combat-outcomes.csv): each arm's wins, losses and unresolved counts; ranked by nonwins across both search arms, then all three arms.
- [Failed-trial fixture index](combat-analysis/failed-trial-fixture-index.jsonl.gz): 9,186 nonwins across the three runs, retaining original shard row, episode, environment/search seeds, potion costs and hashes.
- [Analysis counts](combat-analysis/summary.json).

Completion-verification hashes identify the original server artifacts; source-hashes.json records the published copies after path sanitization. The failure index resolves combat pools from the frozen reference and retains the native mode label separately.

Fixture paths prefixed with archive/ are portable references to the original comparison archive, not local paths. Recreate the compatible fixtures using the [documented procedure](../../../docs/HUMAN_COMPARISON.md#offline-compatibility-preparation) and verify each shard hash. Preserve original shard row IDs and episodes when reproducing search; singleton reindexing changes search randomness. These files record outcomes, not full action traces.

No targeted training has started. Training on these cases would make later scores on them in-sample diagnostics; reserve separate cases for generalization evaluation. The downloadable model is now step6500.
