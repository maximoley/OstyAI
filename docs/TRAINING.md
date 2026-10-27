# Training loop

[Documentation guide](README.md) · [Project overview](../README.md)

**On this page:**

- [Training data and generation](#training-data-and-generation)
- [Collection and learner sampling](#collection-and-learner-sampling)
- [Reward](#reward)
- [Run with your own data](#run-with-your-own-data)
- [Research references](#research-references)

## Training data and generation

The current full-corpus stream draws combat **starting states**, then generates new decision examples by playing those states with the actor and search. It does not train by imitating a stored human action sequence. Counts below were checked against the generation manifest, source code hashes, active source manifest, and all 10,000 prepared-bank metadata files on 2026-10-26; see [machine-readable provenance](../provenance/training-data.json).

### Human combat starts

The raw Spire Codex exports cover v0.109.1, v0.110.0, v0.110.1, and v0.111.0. After run-hash deduplication, the source audit contains 44,193 runs. Filtering retains 13,386 standard solo A10 Necrobinder runs, excludes 29,968 cooperative and 839 nonstandard runs, and yields 165,844 candidate combats. Combat reconstruction requires known positive starting HP and maximum HP and uses contextual inventory/history reconstruction. Exact historical RNG states are unavailable; the converter supplies deterministic per-case environment seeds. Training eligibility does **not** require a winning run.

The fixed primary panel contains 4,096 supported combat starts from winning human runs. All 1,815 represented source runs are held out, excluding 43,355 combats and leaving **122,489 human training candidates**. The separate 512-run human benchmark is not a guaranteed held-out set and may overlap training.

### The 100,000 random runs

The original collector generated **100,000 distinct run seeds** with sampling seed `9232026`, A10 Necrobinder, and at most three acts against reference `v0.111.0 / 41cef1ea`. It stored **2,807,379 combat-opening snapshots** in **1,000 SQLite shards**, each covering 100 runs. The collector and run-rules source hashes still match the hashes recorded by that generation job.

1. Randomize run-level choices: legal paths, events, rests and upgrades; take a random offered card; take relics and potions, replacing a random occupied potion slot when necessary; make random affordable shop purchases before leaving.
2. At a combat opening, serialize the full manager state, including RNG and the current deck, relics, HP, enemies, act and floor. Verify restoration and available-action consistency before storing it. Encounter classes come from the actual encounter pool; event combats are recorded separately.
3. **Without playing combat actions**, synthetically end that fight as a victory and execute normal post-combat hooks and rewards. Continue randomized run progression to collect later encounters. Non-combat deaths end the run and are retained rather than replaced.

The manifest records 93,199 synthetic completions and 6,801 event deaths. **This is not a measured full-run or combat win rate.** Because combat actions are bypassed during collection, generated trajectories do not accumulate damage from those fights; events, rests and rewards can still alter the state. The snapshots provide diverse opening states and deck contexts, not proven winning strategies or necessarily winnable combats. The later training rollout does play the combat in the updated Rust engine and learns from its actual outcome.

### Source banks and compatibility filtering

Each of the 10,000 collection banks requests **512 human and 512 generated starts**. The full source sampler uses all 122,489 eligible human candidates and all 2,807,379 generated candidates; it does not use a fixed 8,192-per-source subset or apply an older generated-corpus train/validation seed split. The packaged `ostyai/corpus/manifest.json` retains historical bootstrap metadata; its subset counts do not describe this later full-corpus stream.

Sampling is uniform within each source, without replacement within that source's bank, and independent across banks. Bank RNG is deterministically derived from seed `20261023`, bank index and source identity. Source-conversion failures are logged by identity and resampled until each source contributes 512 converted records. The native preparation pass then checks compatibility with the current interface and logs/removes unsupported starts, including capacity violations; it does not backfill those later removals. Thus a prepared bank can contain slightly fewer than 1,024 starts or depart slightly from 50/50.

Across all prepared banks, **10,185,462 of 10,240,000 starts were accepted**: **5,119,807 human** and **5,065,655 generated**. These are bank entries, not unique combats: starts can recur across banks. This preparation total is not a claim that training has already consumed all banks.

The full raw datasets, generated SQLite shards and prepared training banks are intentionally not distributed with this repository. Frozen benchmark inputs are bundled separately.

## Boss augmentation after step 5613

Production resumed from the preserved step 5,613 checkpoint with boss256 search and doubled boss collection. Each remaining original bank stays immutable. A versioned overlay appends a distinct supported boss start for every original boss, matching the original boss human/generated counts separately. All nonboss starts and original boss starts remain unchanged. Thus the original 50/50 requested source mixture describes the base banks, not necessarily each augmented bank.

The feeder samples the full **12,379 eligible human boss candidates** and uses rejection sampling from all **2,807,379 generated starts** to obtain generated bosses. It preserves primary held-out exclusions, rejects duplicate source identities within a bank, uses independent deterministic bank/source seeds, and validates imports with the current native engine. Per-bank provenance records counts, source identities, rejects and checksums. Existing native-compatible caches are reused only after hash verification. The first resumed banks have 274 and 272 boss starts, double 137 and 136, while every other pool is unchanged.

Code: [boss_augmentation.py](../ostyai/boss_augmentation.py), [prepare_boss_banks.py](../ostyai/prepare_boss_banks.py). [Preservation, smoke and resume evidence](../provenance/changes/changes-20261026-boss256-double/). The published step 5,500 actor and historical evaluation settings are unchanged.

## Collection and learner sampling

Production alternates **collection → one optimizer update → actor synchronization**. An actor is fixed during collection. Rust workers own states, act/search, and write final pinned observations; Python dispatches GPU batches. Completed-combat rows enter bounded FIFO replay. Collection and learning are cyclic on one GPU; this is not simultaneous independent actor/learner training.

One cycle consumes the original approximately 1024-start prepared bank plus its boss-only additions from bank 5613 onward (unsupported original starts can be rejected). Every completed combat contributes its decision records, so fresh examples per cycle vary with combat length. There is no fixed 16,384-fresh-example requirement: **16,384 is the sampled optimizer batch**, and sampling can repeat replay entries.

| Setting | Value |
|---|---:|
| Total target optimizer updates | 10,000 |
| Updates per cycle | 1 |
| Effective batch | 16,384 |
| Microbatch / accumulated microsteps | 256 / 64 |
| Replay capacity | 65,536 examples, FIFO retention |
| Rust workers | 80 |
| Inference batch / buffer slots | 128 / 48 |
| AdamW learning rate / weight decay | 0.0003 / 0.0001 |
| Gradient norm clipping | 1.0 |
| Arithmetic | BF16 autocast with FP32 parameters/loss arithmetic |
| Root considered actions | 8, clipped to legal count and simulation budget |
| Search depth limit | None; budgets count simulations |
| Current persistent graph | Enabled, 512 retained nodes per combat |

| Combat pool | Simulations per genuine decision | Samples per optimizer update |
|---|---:|---:|
| Easy / native Weak | 32 | 1,170 |
| Hard / native Normal, unclassified events | 64 | 2,341 |
| Elite | 96 | 3,511 |
| Boss | 256 | 9,362 |

Within each pool, sampling is proportional to value-error priority (`alpha=1`, epsilon 0.001), mixed with 5% uniform sampling. Importance weight is `1 / (N_pool * P(sample | pool))` (`beta=1`). It corrects within the pool, preserving the requested 7.14/14.29/21.43/57.14% pool objective. Sampled rows are packed into token-length-efficient microbatches; quotas apply to the full update. Forced factual actions execute without search/inference but retain replay observations and terminal value targets; forced belief transitions collapse into parent edges.

The base 50/50 source-bank mixture, boss-only collection augmentation and 7.14/14.29/21.43/57.14 learner-pool mixture act at different stages. Longer fights produce more decision examples. FIFO retention, pool quotas and within-pool priorities further change their representation. There is **no enforced human/generated balance in the optimizer batch**. Search targets and terminal utility are produced by the current actor's rollout; neither source provides the unknown human winning action sequence as supervision.

Loss per example is `importance_weight * (policy_cross_entropy(search_target, network_policy) + (predicted_value - terminal_utility)^2)`. AdamW provides decoupled weight decay; it is not an explicit extra L2 term in the logged total loss. There is no explicit entropy bonus or distance-to-terminal loss discount. Policy and value loss, entropy, target KL, value residuals, gradient norm and per-pool contributions are logged.

## Reward

The exact native utility is:

`win + 0.5 * (max(remaining_hp, 0) / initial_hp) * (1 - spent_potion_cost) + (0 if win else 0.05 * damage_progress)`

It is unclipped. Independent per-potion costs are Uniform[0,1] during training and primary validation. The human benchmark presets each starting potion's cost to 0 for inferred/recorded human use, otherwise 1. New potions in that benchmark cost 1. The ledger, cloning, observation rows, search value and replay target all share this reward context.

## Run with your own data

<details>
<summary>Expand data requirements and training commands</summary>

The full training corpus and replay are intentionally absent. Supply native wire JSONL banks under `ostyai/prepared/bank-000000.jsonl`, etc., preserving the corresponding metadata/manifests and held-out exclusions in `ostyai/corpus/manifest.json`. Unpack benchmark fixtures first. The schema is illustrated by the frozen benchmark JSONL; each row contains `wire`, `metadata`, and the required context/evaluation fields.

```bash
OSTYAI_PYTHON="$PWD/.venv/bin/python" bash ostyai/run.sh production_train.py --cycles 10000 --output production
```

This starts fresh from `ostyai/initial_weights.pt`. It does **not** resume the champion. Exact resume requires a trainer checkpoint plus its referenced replay, optimizer moments and RNG states, deliberately excluded here. To resume your own run, add `--resume` with that run's output directory. `supervise_run.py` manages the default production directory, and `ostyai/STOP` requests a safe stop.

For the published continuation, prepare the immutable boss overlays before consuming bank 5613 onward. Set `OSTYAI_CORPUS_CONVERTER` to the compatible full-source conversion checkout and run `ostyai/prepare_boss_banks.py --start 5613 --target 10000 --workers 8` with its conversion Python. This requires your original full-corpus manifest, source catalog, held-out manifest and prepared banks; these training inputs are not bundled. The feeder and trainer respect `ostyai/STOP`. The trainer waits for each verified overlay.

`prepare_banks.py` is the legacy human/generated corpus conversion entry point. It requires a separate conversion checkout (`OSTYAI_CORPUS_CONVERTER`, containing `full_corpus.py` and its `native-lib`); it is not necessary for inference or for training on already prepared banks. Original human-panel construction scripts under `tools/human-panel-source/` are archival source evidence and require external raw data/conversion dependencies, not included as a one-command dataset download.

Primary search and matched policy-only validation run every 100 optimizer updates; the 512-run human benchmark runs every 500. **The human benchmark is pinned to32/64/96/128 simulations**, independently of the **32/64/96/256 training and primary-search budgets**. This takes effect after step5628, starting with the scheduled step6000 human benchmark. Persistent reuse remains enabled; older pre-reuse results retain that additional configuration difference. Validation preserves the frozen actor and training RNG; results/caches pin provenance. No optimizer updates occur between paired primary evaluations.

</details>

## Research references

- **AlphaZero:** David Silver et al. (2018). [*A general reinforcement learning algorithm that masters chess, shogi, and Go through self-play*](https://doi.org/10.1126/science.aar6404). *Science*, 362(6419), 1140–1144. This motivates the policy/value network, search-generated policy targets and outcome-based value learning.
- **Gumbel search:** Ivo Danihelka, Arthur Guez, Julian Schrittwieser and David Silver (2022). [*Policy improvement by planning with Gumbel*](https://openreview.net/forum?id=bERaNdoegnO). *ICLR 2022*. This motivates sampling root candidates without replacement and allocating simulations with sequential halving for policy improvement.

OstyAI is a custom adaptation: it uses an explicit Rust simulator, stochastic belief draws, historical/generated combat starts and shaped potion/HP utility. It is neither an exact reproduction of these papers nor full-game self-play starting from a new run. The value network predicts combat utility, not a learned transition model.
