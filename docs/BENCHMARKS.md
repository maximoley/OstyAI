# Frozen validation and model selection

[Documentation guide](README.md) · [Benchmark file guide](../benchmarks/README.md) · [Current results](../README.md#current-results)

**On this page:**

- [Human panel](#human-panel)
- [Primary matched search/policy panel](#primary-matched-searchpolicy-panel)
- [Provenance](#provenance)
- [Timing and matched agent comparisons](#timing-and-matched-agent-comparisons)

The selected model is the step-5500 human-panel champion among completed benchmark checkpoints 4000, 4500, 5000 and 5500 at export time. It leads all three human win metrics. Step 5300 has a higher primary search score; this selection is not a claim of universally best performance.

| Human checkpoint | All-trial win rate | Majority combat win rate | Every-combat run win rate |
|---|---:|---:|---:|
| 4000 | 91.0323% | 91.7679% | 14.8438% |
| 4500 | 90.4290% | 91.6109% | 19.9219% |
| 5000 | 91.0599% | 92.0655% | 20.3125% |
| 5500 | 91.6467% | 92.7184% | 23.8281% |

Historical rows span changes in search budgets and simulator repairs. They are training-history measurements, not a controlled architecture ablation. Native hashes and original reports remain unchanged in `benchmarks/history` and `provenance/changes`.

## Human panel

512 complete native-compatible solo A10 winning human runs, restricted to unmodded content; 12,099 combat starts and three frozen independent RNG starts each: 36,297 trials. Potion penalties are 0 for recorded/inferred human use in that combat and 1 otherwise. See the original selection and inference notes in `provenance/human-panel-original.md` and the frozen manifest.

Report overall trial wins divided by all 36,297 requested trials. A combat wins with at least two of three trial wins. Also report the less strict run criterion: every combat has at least one winning trial out of three. A run wins only if every combat wins by that majority. Starts are independent historical snapshots: this does not carry model HP, rewards or deck changes between fights. Fixtures may overlap training, so the human benchmark is not a held-out generalization estimate. It is conditioned on successful human runs and native compatibility.

Step 5500: 33,265 trial wins, 11,218 majority combat wins, 122 complete majority-winning runs, and 258/512 runs (50.3906%) with at least one winning trial in every combat. The last count is recomputed from the saved combat results in [README metrics](../benchmarks/readme-step5500-metrics.json). Nine unresolved trials count as nonwins. Mean HP lost is 14.4089 over completed trials; averaging each combat's completed-trial mean gives 14.4111. HP loss is initial minus final HP and can be negative after healing. Unresolved trials are excluded from HP means. Per-trial, per-combat, per-run and per-shard outputs are compressed JSON; the full summary includes act, encounter, room type, game version and seed breakdowns.

## Primary matched search/policy panel

4096 frozen fixtures × two episodes (1000000 and 1000001) = 8192 trials every 100 optimizer updates. Independent per-potion penalties are frozen, actions capped at 1000, and no learning occurs between search and legal-argmax policy evaluation. Human evaluation occurs every 500 updates.

Step 5500 search: **7530/8192 = 91.9189%**, four unresolved. Policy only: **6561/8192 = 80.0903%**, 242 unresolved. The original summary's `win_rate` uses completed trials; use `wins/requested` or `win_rate_lower_bound` for all-requested comparisons. Do not accidentally drop capped combats from the denominator.

## Provenance

<details>
<summary>Expand model, fixture, and simulator identity details</summary>

Model tensor fingerprints use two historical conventions: human hashes names plus tensor bytes; primary hashes names, dtype, shape and tensor bytes. `scripts/verify_model.py` proves that both identify the same exported actor. Step-5500 native hash starts `ce7318db`; the packaged current source's deployed native starts `6a041f84` and adds persistent graph reuse. Published benchmark results predate reuse. Neither hash is expected to match a binary rebuilt at a new path with a new compiler.

`scripts/verify_benchmarks.py` verifies frozen shard hashes, the primary fixture hash, compressed-file integrity, and recomputes human aggregate tables from all trial results. Full training data and raw human exports are omitted.

</details>

## Timing and matched agent comparisons

The historical step-5500 summary contains `seconds=1258.2581` and `collection_seconds=2383.7098`. The first times an evaluation invocation; the second sums per-part collection timers, including any cached parts; do not present either as a verified uninterrupted process wall time. The [new comparison](HUMAN_COMPARISON.md) records full process wall time for default Mentor MCTS, step-5500 tiered search, uniform-32 search, and greedy policy, all using 80 workers.

Success of the original human run establishes a historical win for each fight, not guaranteed solvability of every reconstructed randomized start. Read results as a diagnostic of combat execution on a selected population of successful human decks.
