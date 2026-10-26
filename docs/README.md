# Documentation guide

[Project overview and results](../README.md)

## Understand the project

| Question | Read |
|---|---|
| What are we trying to build, and why combat first? | [Motivation](../README.md#motivation-why-combat-comes-first) |
| What does the model see and predict? | [Architecture](ARCHITECTURE.md) |
| How are search, replay and learning connected? | [Training](TRAINING.md) |
| What happens inside a chance-node search? | [Search walkthrough](SEARCH.md) |
| What differs from the upstream simulator? | [Simulator changes](SIMULATOR_CHANGES.md) and [attribution](../ATTRIBUTION.md) |

## Evaluate and reproduce

| Task | Read |
|---|---|
| Interpret trial, combat and run win rates | [Benchmark definitions](BENCHMARKS.md) |
| Compare Mentor MCTS, neural search and greedy policy | [Four-agent comparison protocol](HUMAN_COMPARISON.md) |
| Build, verify and run the exported model | [Reproduction guide](REPRODUCING.md) |
| Interpret speed measurements fairly | [Throughput](THROUGHPUT.md) |
| Locate fixtures and recorded outcomes | [Benchmark file guide](../benchmarks/README.md) |
| Audit model, simulator and dataset identity | [Provenance file guide](../provenance/README.md) |

## Figures and diagnostic artifacts

[Training figures](assets/) are the supplied snapshots through optimizer step 5500, embedded in the main README. [Soul search artifacts](search-trace/) contain a separate checkpoint-5460 diagnostic, including the full tree and a self-contained interactive viewer. Neither should be treated as a live dashboard.
