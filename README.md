# OstyAI

Necrobinder combat policy/value learning for Slay the Spire 2: a modified Rust simulator, GPU compositional state encoder, four-layer transformer, Gumbel search with belief draws, and an AlphaZero-style learner.

**Simulator credit:** built on **[Thomas Friedel's spire-mentor](https://github.com/tfriedel/spire-mentor)**, especially its `sts2-rs` combat engine, content, wire codec and original MCTS/belief-draw implementation. This repository does not claim authorship of that engine. See [ATTRIBUTION.md](ATTRIBUTION.md), the unchanged [upstream simulator license](native/LICENSE), and [provenance](PROVENANCE.md). Not affiliated with Mega Crit.

## Included snapshot

| Component | Included |
|---|---|
| Simulator | Current modified Rust workspace, including import/continuation repairs and persistent search graph reuse |
| Model | **1,593,346 parameters**, step **5500**, selected by best completed human-benchmark all-trial win rate |
| GPU encoder | NUM1 numeric records → compositional embeddings, numeric projection and relation binding |
| Training | Native worker/search queue, GPU evaluation, prioritized replay, accumulated learner and supervisor |
| Benchmark | Frozen 512-human-run panel, three RNG starts per combat, full per-trial/per-combat/per-run results |
| Evidence | Validation histories, measured throughput, repair/deployment evidence and checksums |

**Full training datasets, replay buffers, optimizer checkpoint and compiled machine-specific binaries are excluded.** The frozen validation inputs are included separately; they are not the full training corpus.

## Changes relative to the upstream Rust simulator

OstyAI builds on the combat rules, content implementations, state reconstruction, original MCTS and belief-draw machinery from **[Thomas Friedel's spire-mentor / sts2-rs](https://github.com/tfriedel/spire-mentor)**. The packaged source adds a neural search and learning pipeline around that engine, together with targeted simulator repairs. The [upstream difference inventory](provenance/upstream-differences.json) compares the source snapshot with upstream checkout commit `e7a923e90dc731e83683791b6c4d4a4059656149`; original copyright and license notices are retained.

**Search and neural integration:**

- **GPU evaluation pipeline:** Rust workers export compact NUM1 state and legal-action records into pinned buffers. GPU token construction, batched transformer inference and asynchronous feedback supply policy logits and values to search; completed combats supply replay examples to the learner.
- **Neural Gumbel search:** a custom sequential-halving root search considers up to eight legal actions, with belief draws and chance-outcome widening/reuse for uncertain transitions. Per-combat simulation budgets replace a single uniform budget; search has no fixed depth cap.
- **Forced actions:** sole legal actions execute immediately in real play. Inside search, forced chains collapse into the parent edge without intermediate tree nodes or neural evaluations, while retaining RNG/chance effects. Real forced decisions still contribute replay observations and terminal value targets.
- **Persistent search graph:** the current source can retain up to 512 nodes per combat across decisions, reuse matching belief states, and invalidate cached evaluations when the actor changes. Each new decision still receives its full simulation budget. This change postdates the published step-5500 benchmark; its measured runtime tradeoffs are documented in [THROUGHPUT.md](docs/THROUGHPUT.md).
- **Reward and evaluation context:** independent potion-instance penalties follow cloned states, observations, search and replay. The pipeline adds tier-level search accounting, prioritized replay, matched search/policy-only evaluation and the frozen human-run benchmark.

**Simulator correctness repairs:**

- Restore ascension-aware enemy AI and migrate recognized obsolete openings for Skulking Colony, Haunted Ship and Axebots without resetting HP, powers or RNG. Unsupported live AI fails explicitly instead of falling back to a generic attack; already-dead enemies are handled separately.
- Recognize Queen companion death when the historical TorchHeadAmalgam counter was absent.
- Finish inner autoplay/draw choices before resuming outer turn setup, and reject out-of-phase EndTurn actions that could otherwise create a no-op loop.
- Resume Tyranny / ToolsOfTheTrade turn-start choices only when a pending choice actually exists, allowing lethal effects to reach the combat-end path.

See [simulator change details](docs/SIMULATOR_CHANGES.md) and [repair/deployment evidence](provenance/changes/) for regressions, tests and historical native-library identities.

## Model architecture and training scheme

### Architecture

The exported model has **1,593,346 parameters** and jointly predicts legal-action scores and shaped combat return. Its [configuration and hashes](models/best/metadata.json) accompany the [weights](models/best/model.pt).

| Component | Implementation |
|---|---|
| Input | Variable-length NUM1 state/action records, `int32[B, S, 16]`; token buckets up to 4096 rows |
| Token construction | Shared semantic embeddings, 40 signed-log1p numeric features, and owner/child/reference relations; potion penalties are explicit inputs |
| Transformer | 4 pre-layer-normalized blocks, width 128, 4 attention heads, feed-forward width 512, GELU, no dropout |
| Policy head | Action + owner + target + summary vectors → 128 → one logit per legal action; padding is masked |
| Value head | Summary vector → 128 → one unconstrained shaped-return estimate, not a win probability |
| GPU execution | Triton token assembly for inference, an autograd-compatible training path, BF16 autocast, Torch compile and CUDA graphs |

The full encoding and tensor layout are described in [ARCHITECTURE.md](docs/ARCHITECTURE.md); the implementation is in [ostyai/model.py](ostyai/model.py).

### Training scheme

Training follows an **AlphaZero-style collection → learning → actor synchronization** cycle. With the actor fixed, 80 Rust workers collect combats from a prepared bank of approximately 1024 starts. Their decision examples enter a **65,536-example FIFO replay buffer**. Each cycle samples **16,384 replay examples**, accumulates gradients across **64 microbatches of 256**, performs one optimizer update, then synchronizes the actor. The target is 10,000 optimizer updates; the included champion is from step 5500. Fresh examples collected per cycle vary with combat length.

Search budgets and learner quotas are coupled in the packaged configuration:

| Combat pool | Simulations per non-forced decision | Samples per optimizer update |
|---|---:|---:|
| Easy / native Weak | 32 | 1,638 |
| Hard / native Normal, unclassified events | 64 | 3,277 |
| Elite | 96 | 4,915 |
| Boss | 128 | 6,554 |

Within each pool, examples are sampled by absolute value-error priority, mixed with 5% uniform sampling. Importance weights `1 / (N_pool * P(sample | pool))` correct sampling **within that pool**, preserving the intended 10/20/30/40% pool mixture. Quotas apply to the complete optimizer update; individual microbatches are arranged for token-length efficiency.

The loss combines importance-weighted **policy cross-entropy against the search target** and **value MSE against terminal utility**. AdamW uses learning rate `3e-4`, weight decay `1e-4`, and gradient-norm clipping at `1.0`. There is no explicit entropy bonus or distance-to-terminal loss discount. Terminal utility is:

```text
win + 0.5 * (max(final_hp, 0) / initial_hp) * (1 - spent_potion_cost)
    + (0 if win else 0.05 * damage_progress)
```

Training and primary validation assign independent Uniform[0,1] potion costs. The human benchmark uses cost 0 for recorded/inferred human potion use and 1 otherwise. The return is unclipped and can exceed 1.

Every **100 updates**, the same frozen actor runs primary search and zero-search legal-argmax policy evaluation on the matched **8192-trial** panel. Every **500 updates**, it also runs the [512-human-run benchmark](benchmarks/fixtures/human/): **12,099 combats × three frozen RNG starts**, with majority combat wins and every-combat run wins recorded. Human fixtures may overlap training. See [TRAINING.md](docs/TRAINING.md) for the full loop and [BENCHMARKS.md](docs/BENCHMARKS.md) for evaluation definitions and limitations.

## Best human benchmark

Step 5500: **33,265 / 36,297 trials won (91.65%)**, **11,218 / 12,099 combats won by a 2-of-3 majority (92.72%)**, and **122 / 512 runs with every combat majority-won (23.83%)**. Exact values and HP-loss metrics are in [the original summary](benchmarks/human-validation-005500/summary.json).

Each combat starts independently from the reconstructed human state. The run metric is **not** a linked whole-run rollout carrying model HP between combats. This human panel can overlap training and is not a held-out generalization claim. See [BENCHMARKS.md](docs/BENCHMARKS.md).

The step-5500 results used the preceding simulator build **without persistent search reuse**. The current simulator includes reuse. Both native hashes and the actor hash are preserved; the historical results are not relabeled as current-simulator results. Step 5300 had a higher primary search-panel win rate, so “best” here specifically means **human-benchmark champion**, not best on every metric.

## Quick start — Linux, NVIDIA CUDA

Tested stack: Python 3.12, PyTorch 2.7.1+cu128, Triton 3.3.1, Rust 1.96.0. The production machine is an NVIDIA L40 with 80 CPU workers. The Rust engine also contains game-free CPU tests; the complete neural loop requires CUDA.

```bash
uv venv --python 3.12
uv pip install --python .venv/bin/python -r requirements.txt
export CUDA_LIB_DIR="$(.venv/bin/python scripts/cuda_link_dir.py)"
bash scripts/build_native.sh
.venv/bin/python scripts/unpack_benchmarks.py
.venv/bin/python scripts/verify_model.py
.venv/bin/python scripts/evaluate.py --suite smoke --output results/smoke
```

Use `--suite human` for the complete 512-run benchmark or `--suite primary` for 8192 trials. Add `--policy-only` to the primary suite for legal-argmax zero-search evaluation. Add `--reuse` for the current persistent-search mode; without it, the search mode matches the historical step-5500 setting. Builds can have different hashes, and GPU batching can affect close decisions; exact bitwise historical outcome reproduction is not promised. See [reproduction instructions](docs/REPRODUCING.md).

## Documentation

- [Architecture and tokenization](docs/ARCHITECTURE.md)
- [Training loop, reward, replay and hyperparameters](docs/TRAINING.md)
- [Benchmarks and limitations](docs/BENCHMARKS.md)
- [Measured throughput and timing definitions](docs/THROUGHPUT.md)
- [Simulator modifications](docs/SIMULATOR_CHANGES.md)
- [Release verification](provenance/package-verification.json)

`ostyai/initial_weights.pt` is the original random initialization for fresh training. **The trained champion is `models/best/model.pt`.** The evaluation command loads the champion explicitly. Training does not start simply by cloning or importing this repository.
