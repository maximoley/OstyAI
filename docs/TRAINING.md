# Training loop

Production alternates **collection → one optimizer update → actor synchronization**. An actor is fixed during collection. Rust workers own states, act/search, and write final pinned observations; Python dispatches GPU batches. Completed-combat rows enter bounded FIFO replay. Collection and learning are cyclic on one GPU; this is not simultaneous independent actor/learner training.

One cycle consumes a prepared bank of approximately 1024 combat starts (unsupported starts can be rejected). Every completed combat contributes its decision records, so fresh examples per cycle vary with combat length. There is no fixed 16,384-fresh-example requirement: **16,384 is the sampled optimizer batch**, and sampling can repeat replay entries.

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
| Easy / native Weak | 32 | 1,638 |
| Hard / native Normal, unclassified events | 64 | 3,277 |
| Elite | 96 | 4,915 |
| Boss | 128 | 6,554 |

Within each pool, sampling is proportional to value-error priority (`alpha=1`, epsilon 0.001), mixed with 5% uniform sampling. Importance weight is `1 / (N_pool * P(sample | pool))` (`beta=1`). It corrects within the pool, preserving the requested 10/20/30/40% pool objective. Sampled rows are packed into token-length-efficient microbatches; quotas apply to the full update. Forced factual actions execute without search/inference but retain replay observations and terminal value targets; forced belief transitions collapse into parent edges.

Loss per example is `importance_weight * (policy_cross_entropy(search_target, network_policy) + (predicted_value - terminal_utility)^2)`. AdamW provides decoupled weight decay; it is not an explicit extra L2 term in the logged total loss. There is no explicit entropy bonus or distance-to-terminal loss discount. Policy and value loss, entropy, target KL, value residuals, gradient norm and per-pool contributions are logged.

## Reward

The exact native utility is:

`win + 0.5 * (max(remaining_hp, 0) / initial_hp) * (1 - spent_potion_cost) + (0 if win else 0.05 * damage_progress)`

It is unclipped. Independent per-potion costs are Uniform[0,1] during training and primary validation. The human benchmark presets each starting potion's cost to 0 for inferred/recorded human use, otherwise 1. New potions in that benchmark cost 1. The ledger, cloning, observation rows, search value and replay target all share this reward context.

## Run with your own data

The full training corpus and replay are intentionally absent. Supply native wire JSONL banks under `ostyai/prepared/bank-000000.jsonl`, etc., preserving the corresponding metadata/manifests and held-out exclusions in `ostyai/corpus/manifest.json`. Unpack benchmark fixtures first. The schema is illustrated by the frozen benchmark JSONL; each row contains `wire`, `metadata`, and the required context/evaluation fields.

```bash
OSTYAI_PYTHON="$PWD/.venv/bin/python" bash ostyai/run.sh production_train.py --cycles 10000 --output production
```

This starts fresh from `ostyai/initial_weights.pt`. It does **not** resume the champion. Exact resume requires a trainer checkpoint plus its referenced replay, optimizer moments and RNG states, deliberately excluded here. To resume your own run, add `--resume` with that run's output directory. `supervise_run.py` manages the default production directory, and `ostyai/STOP` requests a safe stop.

`prepare_banks.py` is the legacy human/generated corpus conversion entry point. It requires a separate conversion checkout (`OSTYAI_CORPUS_CONVERTER`, containing `full_corpus.py` and its `native-lib`); it is not necessary for inference or for training on already prepared banks. Original human-panel construction scripts under `tools/human-panel-source/` are archival source evidence and require external raw data/conversion dependencies, not included as a one-command dataset download.

Primary search and matched policy-only validation run every 100 optimizer updates; the 512-run human benchmark runs every 500. Validation preserves the frozen actor and training RNG; results/caches pin provenance. No optimizer updates occur between paired primary evaluations.
