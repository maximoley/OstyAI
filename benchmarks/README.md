# Benchmarks and recorded results

[Project results](../README.md#current-results) · [Metric definitions](../docs/BENCHMARKS.md)

**On this page:**

- [Inputs](#inputs)
- [Published outcomes](#published-outcomes)
- [New agent comparison](#new-agent-comparison)
- [Verification](#verification)

## Inputs

| Location | Contents |
|---|---|
| [fixtures/human/](fixtures/human/) | Frozen 512-run panel: 12,099 combat starts × three RNG variants; compressed JSONL and manifest |
| [fixtures/primary/](fixtures/primary/) | 4,096 frozen starts evaluated with two episodes for 8,192 primary trials |

Run `scripts/unpack_benchmarks.py` from the repository root to prepare inputs for evaluation. These are benchmark inputs, not the full training corpus. Human inputs can overlap training. The [original panel notes](../provenance/human-panel-original.md) explain selection, compatibility exclusions, RNG construction and potion-cost inference.

## Published outcomes

The [Mentor network rerun](comparison/mentor_net250_1000/README.md) includes complete outcomes and verified timing for the shipped Necrobinder v1 playout weights.

| Location | Contents |
|---|---|
| [human-validation-005500/](human-validation-005500/) | Summary, provenance, compressed per-trial / per-combat / per-run results and per-part outputs |
| [validation-005500/](validation-005500/) | Primary search validation |
| [policy-validation-005500/](policy-validation-005500/) | Matched primary greedy-policy validation; not the full human policy benchmark |
| [validation-comparison-005500.json](validation-comparison-005500.json) | Matched primary search/policy comparison |
| [readme-step5500-metrics.json](readme-step5500-metrics.json) | README table values, including every-fight ≥1/3 run count, with source hashes |
| [history/](history/) | Historical primary, policy-only and human validation JSONL |
| [human-hp-baseline.json](human-hp-baseline.json) | Human HP-loss baseline analysis |
| [comparison/](comparison/) | Completed 80-worker Mentor MCTS and tiered step-5500 comparison results, full-process timing and compatibility preparation provenance |
| [potion-sensitivity/](potion-sensitivity/) | Starting-potion use versus assigned cost at steps 5300–5500, with search and greedy policy; data and plot reproduction |
| [throughput/](throughput/) | Hardware and timing measurements; see [timing scope](../docs/THROUGHPUT.md) |

Use wins divided by **all requested trials**, including capped/unresolved nonwins. Run-level metrics aggregate independent combat starts, not linked model-played runs. Historical settings and simulator hashes vary; keep their provenance intact.

## New agent comparison

The [matched comparison](../docs/HUMAN_COMPARISON.md) evaluates Mentor MCTS with RL-training budgets and step-5500 tiered search and zero-search greedy policy. Uniform-32 was cancelled before starting. All active configurations use 80 workers and the same offline compatibility-prepared panel. All three required configurations are complete; their [outcomes, verification and full-process timing](comparison/) are separate from the historical reports above.

## Verification

From the repository root:

```bash
.venv/bin/python scripts/verify_benchmarks.py
```

This checks frozen-input hashes and compressed files, and recomputes the saved human aggregates from trial results. It does not rerun combats or claim a rebuilt simulator will reproduce every historical trajectory bitwise.
