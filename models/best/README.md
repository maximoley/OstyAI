# Exported step-6500 actor

`model.pt` is the frozen inference actor at optimizer update **6,500**, matching the published primary, scheduled human and three timed human evaluations. It contains configuration, actor tensors and step; load with `torch.load(..., weights_only=True)`. Optimizer, replay and RNG state are excluded.

The model has **1,593,346 parameters**: a four-layer, width-128 policy/value transformer with four attention heads and 512-wide feed-forward layers. See [architecture](../../docs/ARCHITECTURE.md), [training](../../docs/TRAINING.md) and [metadata](metadata.json).

| Evaluation | Wins / requested | Win rate |
|---|---:|---:|
| Scheduled primary search, boss256 | 7,660 / 8,192 | 93.51% |
| Matched primary greedy policy | 7,284 / 8,192 | 88.92% |
| Timed human, boss128 | 33,744 / 36,297 | 92.97% |
| Timed human, boss256 | 33,854 / 36,297 | 93.27% |
| Timed human, greedy policy | 32,107 / 36,297 | 88.46% |

Search uses easy32/hard64/elite96 and the stated boss budget, with reuse off. Unresolved trials count as nonwins. The separate scheduled human evaluation scored 33,730/36,297; do not substitute timed outcomes into scheduled training history. [Full matched results](../../benchmarks/comparison/step6500/README.md).

Human states may overlap training, and each combat resets independently; run aggregates are not end-to-end model-played runs. This actor handles Necrobinder combat decisions, not map, reward or merchant decisions. Changed simulator content or asynchronous GPU batching can affect trajectories.

Weights: [CC BY-SA 4.0](../WEIGHTS-LICENSE.md), Maxim Oleynikov, OstyAI. Third-party game content and human-derived data retain their respective rights.
