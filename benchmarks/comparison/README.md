# Matched 80-worker comparison results

[README comparison table](../../README.md#matched-agent-comparison) · [Protocol](../../docs/HUMAN_COMPARISON.md)

All three required configurations completed on October 26, 2026. Uniform-32 was cancelled before starting. These results use the offline compatibility-prepared 512-run panel and do not replace the historical step-5500 results. [Complete comparison](comparison.json) · [Suite verification](results-verification.json).

| Metric | Mentor MCTS: 250 non-boss / 1,000 boss, rollout cap 200 | Step 5500: tiered 32/64/96/128, no reuse | Step 5500: greedy policy, no search |
|---|---:|---:|---:|
| Trial wins | 33,577 / 36,297 (92.51%) | 33,263 / 36,297 (91.64%) | 29,068 / 36,297 (80.08%) |
| Majority combat wins (≥2/3) | 11,302 / 12,099 (93.41%) | 11,217 / 12,099 (92.71%) | 9,846 / 12,099 (81.38%) |
| Every fight ≥2/3 | 159 / 512 runs (31.05%) | 118 / 512 runs (23.05%) | 14 / 512 runs (2.73%) |
| Every fight ≥1/3 | 269 / 512 runs (52.54%) | 256 / 512 runs (50.00%) | 65 / 512 runs (12.70%) |
| Unresolved trials | 0 | 11 | 1,126 |
| Mean HP lost per completed trial | 11.5727 | 14.3910 | 18.9591 |
| Full-process wall time | 4,926.8909 seconds — 1 h 22 min 7 s | 2,684.7494 seconds — 44 min 45 s | 315.9987 seconds — 5 min 16 s |
| Workers | 80 | 80 | 80 |

Unresolved/capped trials count as nonwins in the trial and majority metrics. HP means exclude unresolved trials. Tiered search improves trial win rate by **11.56 percentage points** over greedy policy; greedy policy completes the panel **8.50× faster**. Run-level outcomes aggregate independently initialized combats, not end-to-end played runs.

The wall time is measured by the parent runner and includes process startup, imports, evaluation, result writing and exit. Offline preparation and preflight are excluded. The MCTS summary's internal `wall_seconds` is 4,925.2682 seconds and has a narrower scope; the table uses the external timer. The MCTS run was uninterrupted and had no competing trace replays. Neural startup recovery occurred afterward and does not alter its timing.

The successful tiered process also ran uninterrupted. When uniform-32 was cancelled, a replacement orchestrator adopted the already-running tiered process and retained its original launch time; completion was detected by polling at 0.1-second intervals. Its timing record therefore has `adopted_process=true` and `exit_code=null` (the replacement could not `waitpid` another parent's child). Completion is established by all 48 saved parts and complete verified outcome artifacts, not an invented exit code. The earlier failed 19.29-second startup is excluded from this successful run's timing.

The policy-only process exited successfully with code 0. Its **315.9987-second external timer** includes startup and result writing; the summary's **307.9345-second evaluation timer** and **263.6714-second collection timer** have narrower scopes. All three timed arms completed before Queen diagnostic replays began. Production training resumed after verification.

## Result files

### Mentor MCTS

- [summary.json](mentor_rl_training/summary.json): complete aggregate outcomes and breakdowns.
- [readme-metrics.json](mentor_rl_training/readme-metrics.json): table metrics, including independently computed every-fight ≥1/3 count and external wall time.
- [timing.json](mentor_rl_training/timing.json): parent-process timer, successful exit status and timing scope.
- [provenance.json](mentor_rl_training/provenance.json): native and manifest hashes, MCTS settings and objective.
- [trials.json.gz](mentor_rl_training/trials.json.gz): all 36,297 original trial outcomes.
- [combats.json.gz](mentor_rl_training/combats.json.gz): aggregation over three RNG trials per combat.
- [runs.json.gz](mentor_rl_training/runs.json.gz): aggregation of independently initialized combats within each human run.

### Tiered step 5500

- [summary.json](model5500_tiered_no_reuse/summary.json) and [readme-metrics.json](model5500_tiered_no_reuse/readme-metrics.json): saved and independently recomputed metrics.
- [timing.json](model5500_tiered_no_reuse/timing.json) and [provenance.json](model5500_tiered_no_reuse/provenance.json): full-process timer, model, native, fixture and search identities.
- [trials.json.gz](model5500_tiered_no_reuse/trials.json.gz), [combats.json.gz](model5500_tiered_no_reuse/combats.json.gz), and [runs.json.gz](model5500_tiered_no_reuse/runs.json.gz): complete outcomes at each aggregation level.

### Greedy policy-only step 5500

- [summary.json](model5500_policy_greedy/summary.json) and [readme-metrics.json](model5500_policy_greedy/readme-metrics.json): saved and independently recomputed metrics, including every-fight ≥1/3.
- [timing.json](model5500_policy_greedy/timing.json) and [provenance.json](model5500_policy_greedy/provenance.json): full-process timer, successful exit, model/native/fixture hashes, and zero-search legal-argmax configuration.
- [trials.json.gz](model5500_policy_greedy/trials.json.gz), [combats.json.gz](model5500_policy_greedy/combats.json.gz), and [runs.json.gz](model5500_policy_greedy/runs.json.gz): all trial, combat and run outcomes.

All published summary counts, HP-loss means, combat rows and run rows were recomputed from the trial outcomes with `ostyai/human_validation_metrics.py` and checked against the saved artifacts. Every-fight ≥1/3 is computed by requiring at least one winning trial for every case in a run. It is not a linked full-run gameplay result.

## Fixture preparation and identity

- [fixture-manifest.json](fixture-manifest.json): complete manifest of the compatibility-prepared panel.
- [prepared-manifest.json](prepared-manifest.json): SHA256 mapping for prepared fixture shards.
- [preparation.json](preparation.json): recorded opening/intent normalizations and native-state equality checks.
- [source-hashes.json](source-hashes.json): original JSON hashes and stored-file hashes for exported source artifacts.

The repository's original frozen fixture shards are unchanged. The prepared copies were generated using the [offline compatibility procedure](../../docs/HUMAN_COMPARISON.md#offline-compatibility-preparation). MCTS uses the updated Rust simulator with its original evaluator; it does not apply the neural model's potion-cost objective. Retain this objective distinction when comparing the agents.
