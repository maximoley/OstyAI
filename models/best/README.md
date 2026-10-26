# Exported human-benchmark champion

`model.pt` is the inference actor at optimizer update 5500, selected by the highest completed human-panel all-trial win rate at the snapshot time in `provenance/export.json`. It contains configuration, actor tensors, step and actor fingerprint; load with `torch.load(..., weights_only=True)`. It does not contain optimizer moments, replay or RNG state, so it is not an exact training-resume checkpoint.

1,593,346 parameters; four-layer width-128 policy/value transformer, four attention heads and 512-wide feed-forward layers. Inputs are the packaged NUM1 schema, including legal actions and independent potion penalties. Outputs are action logits and an unbounded shaped-return estimate. See `docs/ARCHITECTURE.md` and `docs/TRAINING.md` for full definitions.

Primary search score: 7530/8192. Matched policy-only score: 6561/8192. Human all-trial score: 33265/36297; majority-combat score: 11218/12099; complete majority-winning runs: 122/512. Unresolved trials count as nonwins. Human states may overlap training, and each combat is reset independently; these are not linked model-played full runs.

This model is specialized to Necrobinder combat decisions and the simulator/schema snapshot. It does not provide a complete map, reward, card-pick or merchant agent. Unsupported or changed game content can invalidate predictions. Historical scores use no persistent search reuse; current source includes optional reuse. A freshly rebuilt binary can produce different close decisions due to batching and floating-point behavior.

Weights are included in this private research repository without a new public redistribution license. Upstream game content and human-derived data retain their respective rights. See `ATTRIBUTION.md` and `PROVENANCE.md`.
