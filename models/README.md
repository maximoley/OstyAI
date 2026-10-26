# Exported model

[Project overview](../README.md) · [Architecture](../docs/ARCHITECTURE.md)

| File | Purpose |
|---|---|
| [best/model.pt](best/model.pt) | Trained step-5500 actor, loaded by `scripts/evaluate.py` |
| [best/metadata.json](best/metadata.json) | Architecture, selection rationale, model hashes and associated evidence |

The model has **1,593,346 parameters**. “Best” means the highest completed **human-benchmark all-trial win rate** among the included checkpoints 4000, 4500, 5000 and 5500. It does not mean best on every panel: step 5300 scored higher on primary search validation.

The exported actor supports both legal-action argmax and neural search. Historical step-5500 human results used tier budgets 32/64/96/128 without persistent reuse. The packaged simulator is newer; [reproduction notes](../docs/REPRODUCING.md) distinguish matching settings from exact historical trajectories.

`ostyai/initial_weights.pt` is the original random initialization, not the trained model. Optimizer state, replay and the full production resume checkpoint are intentionally excluded. Verify the actor with:

```bash
.venv/bin/python scripts/verify_model.py
```
