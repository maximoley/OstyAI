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
