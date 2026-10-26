# Measured throughput

[Documentation guide](README.md) · [Project overview](../README.md)

**On this page:**

- [End-to-end search collection](#end-to-end-search-collection)
- [Production cycles](#production-cycles)

Hardware: NVIDIA L40 (46,068 MiB reported), driver 570.172.08; two Intel Xeon Gold 5420+ CPUs, 56 physical / 112 logical cores; production uses 80 Rust workers. PyTorch 2.7.1+cu128 and BF16 inference. Raw hardware and timing reports are in `benchmarks/throughput`.

## End-to-end search collection

The fixed 512-combat diagnostic uses the same actor and starting panel, with full tier budgets 32/64/96/128. Played trajectories and decision counts differ by mode, so this is an end-to-end configuration comparison, not an identical-instruction microbenchmark.

| Mode / pass | Total seconds | Real decisions | Decisions / total second | Loop seconds |
|---|---:|---:|---:|---:|
| No reuse, first | 33.0004 | 21,237 | 643.54 | 31.5755 |
| No reuse, warm | 35.0151 | 21,562 | 615.79 | 32.6556 |
| Persistent reuse, first | 43.1998 | 21,040 | 487.04 | 40.7964 |
| Persistent reuse, warm | 47.6317 | 21,230 | 445.71 | 45.2634 |

Total time includes native orchestration, observations, transfers, neural evaluations and search. Loop time is the narrower report timer. The first no-reuse pass captured four GPU graphs (0.803 seconds); the other three captured none. Reuse was 31–36% slower in these paired passes despite substantial cached-root evaluation reuse. Both reuse passes won 477/512 versus 469/512 without reuse; this small diagnostic does not establish a general strength gain. Raw host dispatch timings must not be presented as pure device transfer or kernel timings.

## Production cycles

The export includes cycles 5401–5508. Pre-reuse cycles 5401–5500 had median collection time 30.17 seconds. The short post-reuse steady sample, cycles 5504–5508, took about **40.61–44.08 seconds per collection-plus-learner cycle**, median **42.90 seconds**. Its collection phase averaged **791.7 actual played decisions/second** across five different ~1024-start banks. This is a different workload from the 512-panel table and should not be directly compared as a mode speedup.

Each learner update processes 16,384 examples in 64 microbatches of 256; the measured learner portion here was approximately 0.38–0.39 seconds. That excludes generating the examples and initial compilation. Validation events add separate pauses, especially the 36,297-trial human suite. The source keeps GPU compilation, collection, learner and validation measurements distinct. These numbers are dated snapshots, not performance guarantees for another GPU or an unmeasured future model.
