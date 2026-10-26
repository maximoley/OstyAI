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

| Location | Contents |
|---|---|
| [human-validation-005500/](human-validation-005500/) | Summary, provenance, compressed per-trial / per-combat / per-run results and per-part outputs |
| [validation-005500/](validation-005500/) | Primary search validation |
| [policy-validation-005500/](policy-validation-005500/) | Matched primary greedy-policy validation; not the full human policy benchmark |
| [validation-comparison-005500.json](validation-comparison-005500.json) | Matched primary search/policy comparison |
| [readme-step5500-metrics.json](readme-step5500-metrics.json) | README table values, including every-fight ≥1/3 run count, with source hashes |
| [history/](history/) | Historical primary, policy-only and human validation JSONL |
| [human-hp-baseline.json](human-hp-baseline.json) | Human HP-loss baseline analysis |
| [potion-sensitivity/](potion-sensitivity/) | Starting-potion use versus assigned cost at steps 5300–5500, with search and greedy policy; data and plot reproduction |
| [throughput/](throughput/) | Hardware and timing measurements; see [timing scope](../docs/THROUGHPUT.md) |

Use wins divided by **all requested trials**, including capped/unresolved nonwins. Run-level metrics aggregate independent combat starts, not linked model-played runs. Historical settings and simulator hashes vary; keep their provenance intact.

## New agent comparison

The [four-agent comparison](../docs/HUMAN_COMPARISON.md) evaluates default Mentor MCTS and three step-5500 modes: tiered search, uniform-32 search, and zero-search greedy policy. All use 80 workers and the same offline compatibility-prepared panel. Its full-process timing and new results are separate from the historical reports above. The README marks incomplete results as pending.

## Verification

From the repository root:

```bash
.venv/bin/python scripts/verify_benchmarks.py
```

This checks frozen-input hashes and compressed files, and recomputes the saved human aggregates from trial results. It does not rerun combats or claim a rebuilt simulator will reproduce every historical trajectory bitwise.
