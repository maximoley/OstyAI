# Script guide

**On this page:**

- [Setup and verification](#setup-and-verification)
- [Evaluation](#evaluation)
- [Deployment-specific orchestration](#deployment-specific-orchestration)

Run commands from the repository root. [Quick start](../README.md#quick-start) · [Reproduction guide](../docs/REPRODUCING.md)

## Setup and verification

| Script | Purpose |
|---|---|
| [cuda_link_dir.py](cuda_link_dir.py) | Find/create the CUDA runtime link directory for the native build |
| [build_native.sh](build_native.sh) | Build the packaged Rust native library |
| [unpack_benchmarks.py](unpack_benchmarks.py) | Unpack included benchmark inputs; does not fetch training data |
| [verify_model.py](verify_model.py) | Check exported model identity and architecture |
| [verify_benchmarks.py](verify_benchmarks.py) | Verify fixture hashes and recompute historical human metrics |

## Evaluation

| Script | Purpose |
|---|---|
| [evaluate.py](evaluate.py) | Exported actor on smoke, primary or human fixtures; supports policy-only, tiered, uniform-budget and reuse options |
| [benchmark_mcts.py](benchmark_mcts.py) | Existing Mentor MCTS on offline compatibility-prepared human fixtures |

Example modes, all with 80 workers:

```bash
.venv/bin/python scripts/evaluate.py --suite human --workers 80 --output results/human-tiered
.venv/bin/python scripts/evaluate.py --suite human --workers 80 --uniform-simulations 32 --output results/human-uniform32
.venv/bin/python scripts/evaluate.py --suite human --workers 80 --policy-only --output results/human-policy
```

Search defaults to no persistent reuse. `--policy-only` uses zero search and greedy legal-action selection. Use a new output directory for each evaluation. For controlled cross-agent timings, use the identical prepared panel and the [comparison protocol](../docs/HUMAN_COMPARISON.md).

## Deployment-specific orchestration

[run_human_comparison.py](run_human_comparison.py) and [finalize_human_comparison.py](finalize_human_comparison.py) coordinate the authorized training host experiment, verify results and resume its paused production run. They retain experiment-specific checkpoint/native identities and STOP ownership checks, while the production and comparison roots are supplied explicitly. They are not general setup scripts and should not be invoked automatically on another machine.

The archival accumulation checks accept an external replay file via `OSTYAI_TEST_REPLAY`; it is not bundled. Native queue diagnostic defaults resolve `ostyai/schema` and `ostyai/validation/fixtures.jsonl` from the repository root after fixture unpacking. Their oracle inputs remain external.
