# Matched 80-worker comparison results

[README comparison table](../../README.md#matched-four-agent-comparison) · [Protocol](../../docs/HUMAN_COMPARISON.md)

The Mentor MCTS arm completed on October 26, 2026. The three new step-5500 neural arms remain pending at this documentation update. These are results on the offline compatibility-prepared 512-run panel; they do not replace the historical step-5500 results.

| Metric | Mentor MCTS: 250 non-boss / 1,000 boss simulations, rollout cap 200 |
|---|---:|
| Trial wins | 33,577 / 36,297 (92.51%) |
| Majority combat wins (≥2/3) | 11,302 / 12,099 (93.41%) |
| Every fight ≥2/3 | 159 / 512 runs (31.05%) |
| Every fight ≥1/3 | 269 / 512 runs (52.54%) |
| Unresolved trials | 0 |
| Mean HP lost per trial | 11.5727 |
| Full-process wall time | 4,926.8909 seconds — 1 h 22 min 7 s |
| Workers | 80 |

The wall time is measured by the parent runner and includes process startup, imports, evaluation, result writing and exit. Offline preparation and preflight are excluded. The summary's internal `wall_seconds` is 4,925.2682 seconds and has a narrower scope; the table uses the external timer. The MCTS run was uninterrupted and had no competing trace replays. Neural startup recovery occurred afterward and does not alter its timing.

## Result files

- [summary.json](mentor_rl_training/summary.json): complete aggregate outcomes and breakdowns.
- [readme-metrics.json](mentor_rl_training/readme-metrics.json): table metrics, including independently computed every-fight ≥1/3 count and external wall time.
- [timing.json](mentor_rl_training/timing.json): parent-process timer, successful exit status and timing scope.
- [provenance.json](mentor_rl_training/provenance.json): native and manifest hashes, MCTS settings and objective.
- [trials.json.gz](mentor_rl_training/trials.json.gz): all 36,297 original trial outcomes.
- [combats.json.gz](mentor_rl_training/combats.json.gz): aggregation over three RNG trials per combat.
- [runs.json.gz](mentor_rl_training/runs.json.gz): aggregation of independently initialized combats within each human run.

All published summary counts, HP-loss means, combat rows and run rows were recomputed from the trial outcomes with `ostyai/human_validation_metrics.py` and checked against the saved artifacts. Every-fight ≥1/3 is computed by requiring at least one winning trial for every case in a run. It is not a linked full-run gameplay result.

## Fixture preparation and identity

- [fixture-manifest.json](fixture-manifest.json): complete manifest of the compatibility-prepared panel.
- [prepared-manifest.json](prepared-manifest.json): SHA256 mapping for prepared fixture shards.
- [preparation.json](preparation.json): recorded opening/intent normalizations and native-state equality checks.
- [source-hashes.json](source-hashes.json): original JSON hashes and stored-file hashes for exported source artifacts.

The repository's original frozen fixture shards are unchanged. The prepared copies were generated using the [offline compatibility procedure](../../docs/HUMAN_COMPARISON.md#offline-compatibility-preparation). MCTS uses the updated Rust simulator with its original evaluator; it does not apply the neural model's potion-cost objective. Compare the new neural results when available, retaining this objective distinction.
