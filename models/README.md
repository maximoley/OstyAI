# Exported model

[Project overview](../README.md) · [Architecture](../docs/ARCHITECTURE.md)

| File | Purpose |
|---|---|
| [best/model.pt](best/model.pt) | Trained step-6500 actor, loaded by `scripts/evaluate.py` |
| [best/metadata.json](best/metadata.json) | Architecture, selection rationale, model hashes and associated evidence |

The model has **1,593,346 parameters**. The bundled actor is checkpoint **6,500**, selected using completed scheduled human benchmarks through that step. Newer primary-only evaluations do not establish a better human-benchmark model. [Scores and scope](best/README.md).

The exported actor supports both legal-action argmax and neural search. Step-6500 human results used tier budgets 32/64/96/128 without persistent reuse. The packaged simulator is newer; [reproduction notes](../docs/REPRODUCING.md) distinguish matching settings from exact historical trajectories.

`ostyai/initial_weights.pt` is the original random initialization, not the trained model. Optimizer state, replay and the full production resume checkpoint are intentionally excluded. Verify the actor with:

```bash
.venv/bin/python scripts/verify_model.py
```

The trained weights use **[CC BY-SA 4.0](WEIGHTS-LICENSE.md)**: attribution required, commercial use allowed, and distributed adaptations share alike. This is a weights-only grant.
