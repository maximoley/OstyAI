# PPO-only full-run training

Source for the **Ironclad A10 and Necrobinder A10 November 2026 runs**. One transformer plays combat, routing, rewards, shops, events and rest sites, with **no search, teacher actions or imitation learning**. Each character starts from fresh weights.

This is a separate training path from the combat AlphaZero model in [`ostyai/`](../ostyai/). The existing downloadable checkpoint and combat benchmark tables refer to that earlier model, not these PPO runs.

## Contents

- [Model](#model)
- [Training recipe](#training-recipe)
- [Combat practice](#combat-practice)
- [Build and run](#build-and-run)
- [Tests](#tests)
- [Logs and recovery](#logs-and-recovery)
- [Source guide](#source-guide)
- [Provenance and credits](#provenance-and-credits)

## Model

| Component | Configuration |
|---|---|
| Parameters | 10,865,924 |
| Public encoder | 8 pre-norm transformer layers, width 256, 8 heads, FFN 1,024 |
| Inputs | Compositional 256-wide embeddings from variable-length 16-integer entity records |
| Public observations | Combat objects, deck/inventory, map and route information, legal rewards, shops, events and selections |
| Policy | Separate combat and noncombat heads score each engine-supplied legal action using its referenced records and global state |
| Critics | Separate combat and noncombat values; a private encoder adds privileged information after the public trunk |
| Precision | BF16 trunk; FP32 value projections, log-softmax, parameters and optimizer moments |
| Collection inference | Length buckets, pinned arrays, compiled forwards, CUDA graphs and fused Triton token assembly |

The actor cannot read the privileged draw order or future-information records. Tests check unchanged actor logits and absent actor-to-private gradients when private inputs change. Legal-action order and record references come from Rust. Forced single choices are advanced without a model row; exact duplicate card actions are conservatively merged.

## Training recipe

| Setting | Value |
|---|---|
| Workers / active environments | 80 Rust threads / 512 continuously refilled environments |
| Collected decisions per policy update | 32,768 meaningful choices before filtering |
| Advantage filter | 60th-percentile cutoff on absolute raw GAE; retain about 40%, with minimum magnitude 0.001; separate full-run/practice thresholds |
| Fitting | One epoch; optimizer minibatches up to 8,192; physical microbatches up to 1,024 and 98,304 tokens |
| Discount / GAE lambda | Gamma 1; combat 0.95; noncombat 0.97 |
| PPO | Clip 0.2; negative-advantage dual clip 3; detached proximal/behavior importance cap 2 |
| Value loss / gradient clipping | MSE coefficient 0.5, no value clipping / global norm 1 |
| AdamW | Trunk/policy LR 1e-4; private encoder/value LR 3e-4; weight decay 1e-4 |
| LR control | 10-update warmup; multiplier 0.3–2; ×1.5 below KL 0.005, ÷1.5 above KL 0.02 |
| KL guard | Exact categorical KL on up to 8,192 fixed probe rows after each optimizer minibatch; KL ≥0.06 stops the remaining fit, retaining the crossing step |
| Validation | Greedy EMA 0.996, no search, 1,024 frozen seeds; two hours after launch, then two hours after each completed evaluation |

Rewards are **+0.002 per cleared floor, +0.05 per boss defeated and +1 per full-run win**. There is no elite bonus, HP shaping or potion penalty. Death or a cap adds zero and ends continuation; rewards already earned remain. Limits are **1,000 meaningful choices or 100 turns per combat, and 6,000 choices per run**.

Combat GAE bootstraps from the next run-state value and ends its lambda trace at that boundary. Noncombat rows can be held until segment completion or four collections; their proximal action probabilities and values are refreshed before fitting. Thus this is not a strictly single-version rollout fit. The separate behavior importance correction and age logs are retained.

The exact character configurations are [`configs/ironclad.json`](configs/ironclad.json) and [`configs/necrobinder.json`](configs/necrobinder.json). They preserve the original **November 11–12, 2026** LR anneal and stop timestamps and the **10-billion-decision** ceiling. The clocks for collected decisions, policy updates and AdamW steps are logged separately.

## Combat practice

Practice starts come **only from the model's own full runs**, captured before entering elite or boss fights. The previous human/synthetic combat corpus is not loaded by this recipe.

| Sampling control | Schedule over the first 2 billion collected decisions |
|---|---|
| Target share of environment slots in practice | 40% → 25% |
| Boss probability when requesting a practice start | 40% → 75% |
| Boss act weights | Act 1: 15%; Act 2: 20%; Act 3 double boss: 65% |
| Elite act weights | Act 1: 20%; Act 2: 40%; Act 3: 40% |

Six shared FIFO queues hold up to 2,560 snapshots each. A category needs eight entries to activate; entries expire after 50 million decisions or four reuses. The restored deck, relics and run state are retained while future `RunRngSet` streams are reseeded. Ordinary practice ends at the first postcombat noncombat state and bootstraps its run value. Act 3 double-boss practice continues to the actual terminal state.

These are **sampling targets, not guaranteed realized percentages**. Unavailable categories fall back to full runs, and longer fights produce more decision rows. The curriculum is not fixed at 37% bosses. Late-act snapshots require the policy to reach those encounters.

The native sampler, queue limits and caps are implemented in [`native/fullrun/runvalue.rs`](native/fullrun/runvalue.rs). Their JSON descriptions document the tested recipe; editing those descriptions alone does not change Rust constants.

## Build and run

Requires Linux, an NVIDIA GPU, a CUDA-compatible driver, Python 3.12 and Rust 1.96.0. From the repository root:

```bash
python3.12 -m venv .venv
source .venv/bin/activate
python -m pip install -r ppo/requirements.txt
python ppo/build_native.py --character Necrobinder --jobs 2
python ppo/prepare_run.py --character Necrobinder \
  --native-library ppo/build/necrobinder/loop_native.so \
  --output results/ppo-necrobinder
bash results/ppo-necrobinder/run.sh supervise_run.py
```

Use `Ironclad`, `ppo/build/ironclad/loop_native.so` and a different output directory for Ironclad. The native factory is character-specific; a mismatched library is rejected before environments are created. The preparation tool refuses to overwrite an existing run.

The builder copies the pinned [`../native/`](../native/) base to an isolated build directory, verifies its input hashes, then adds the four PPO Rust files. It leaves the existing simulator and combat model library untouched. `--assemble-only` verifies and assembles sources without compiling. CUDA runtime discovery uses the active Python installation or `CUDA_RUNTIME_LIBRARY`; build artifacts record the resulting library hash.

**For a new campaign after the historical deadline**, explicitly supply both `--anneal-start` and `--stop-at` to `prepare_run.py`, using ISO timestamps with UTC offsets. This moves the freeze/anneal/stop window without changing rewards or PPO settings. For example:

```bash
python ppo/prepare_run.py --character Necrobinder \
  --native-library ppo/build/necrobinder/loop_native.so \
  --output results/ppo-new-campaign \
  --anneal-start 2027-01-01T06:00:00-05:00 \
  --stop-at 2027-01-02T06:00:00-05:00
```

The supervisor waits for an idle GPU and never stops another GPU process. Keep the virtual environment active, or set `OSTYAI_PYTHON` to its Python executable. On a fresh campaign, the 100,352-start queue is generated before collecting rollouts and replenished between collections.

## Tests

Packaging tests need only the Python standard library:

```bash
python -m unittest discover -s ppo/tests -v
```

After preparing a run, set `RUN` to that directory. The following checks do not train a model or use a GPU:

```bash
RUN=results/ppo-necrobinder
python -m pytest -q "$RUN/tests" --ignore="$RUN/tests/test_character.py"
python "$RUN/tests/test_native.py"
python "$RUN/tests/test_model.py"
python -m pytest -q "$RUN/tests/test_character.py"
```

The first command tests returns, filtering, loss contracts, LR/KL/deadline handling and supervisor stops. The remaining checks require the built native library: ordered-array parity across worker counts, exact journal restore, all six practice boundaries, cap handling, character isolation, and actor/critic information separation. `deadline_integration.py` and `check_checkpoint.py` are separate GPU diagnostics for an isolated run, not automatic tests against a live campaign.

Both character builds passed these checks for this publication. A separate deterministic 4,096-position walk per character matched the original library's public/private arrays, transitions, outcomes and final journal exactly. See [`verification.json`](verification.json). These CPU/native checks do not constitute a new GPU throughput or playing-strength benchmark.

## Logs and recovery

| File under a prepared run | Contents |
|---|---|
| `production/progress.jsonl` | Per-update stage times, rows, filtering, loss, KL, LR, entropy, practice admissions and floors |
| `production/full-run-results.jsonl` | Individual completed full-run outcomes |
| `production/practice-results.jsonl` | Individual practice outcomes and category |
| `production/validation-*/` | Frozen actor/native/config provenance, 16 parts and all 1,024 run results |
| `production/latest.pt`, `previous.pt` | Model, optimizer, EMAs, controller, held rows, native journals, active runs, start queues and RNG states |
| `production/status.json`, `service.json`, `gpu.jsonl` | Trainer/supervisor state and GPU-overlap monitoring |

Create `STOP` in the prepared run or its `production/` directory to stop trainer and supervisor after saving. Restarting the supervisor loads only a provenance-matching checkpoint. It respects the deadline, never clears STOP, and reports repeated failures without checkpoint progress. Cross-deadline partial rollouts are saved unfitted and require an explicit migration before extending that campaign. Exact saved state is restored; future CUDA training trajectories are not promised bitwise identical.

No PPO checkpoints, optimizer states, live logs or full training datasets are bundled. The frozen validation seed list and shared numeric schema are included. Practice banks are generated during training.

## Source guide

| Source | Purpose |
|---|---|
| [`trainer/model.py`](trainer/model.py), [`fast_tokenizer.py`](trainer/fast_tokenizer.py) | Public/private actor-critic and fused token assembly |
| [`trainer/actor.py`](trainer/actor.py), [`rollout.py`](trainer/rollout.py) | Compiled inference, CUDA graphs, packing and minibatch grouping |
| [`trainer/fast_workers.py`](trainer/fast_workers.py), [`run_workers.py`](trainer/run_workers.py) | Cached Rust observations and compact bulk C ABI |
| [`trainer/collect_fast.py`](trainer/collect_fast.py) | Optimized collection, boundaries and outcomes; `collect.py` retains the reference collector |
| [`trainer/returns.py`](trainer/returns.py), [`filtering.py`](trainer/filtering.py) | GAE, held run decisions, normalization and advantage selection |
| [`trainer/learner.py`](trainer/learner.py) | PPO/value/entropy losses, AdamW, EMA and KL control |
| [`trainer/training_schedule.py`](trainer/training_schedule.py) | Decision ceiling, multiplier freeze, anneal and deadline |
| [`trainer/train_production.py`](trainer/train_production.py) | Collection/fit loop, checkpointing, validation and metrics |
| [`trainer/validation.py`](trainer/validation.py) | Frozen greedy full-run evaluation and cached-part checks |
| [`trainer/supervise_run.py`](trainer/supervise_run.py), [`operations.py`](trainer/operations.py) | Single-trainer ownership, STOP, recovery and durable writes |
| [`native/fullrun.rs`](native/fullrun.rs) | Full-run NUM1 encoder, legal actions and character-bound factory |
| [`native/fullrun/`](native/fullrun/) | Native PPO transitions, historical mixed-curriculum support and current own-run practice/cache implementation |
| [`trainer/tests/`](trainer/tests/) | Original correctness, restoration and deadline checks |

## Provenance and credits

[`SOURCE.json`](SOURCE.json) records the original trainer/overlay/seed/schema hashes and portable packaging changes. [`native-base.json`](native-base.json) pins the already-published simulator inputs used by the builder. Model, learner, returns, filtering, optimized collection, schedule and validation math are retained from the live runs. Host-specific interpreter/CUDA paths and the resident-process exemption were removed only in this distribution.

The simulator is derived from **Thomas Friedel's [spire-mentor](https://github.com/tfriedel/spire-mentor)**, with the original [MIT license and notices](../native/LICENSE) retained. The run-continuation recipe is an independent adaptation of the [published PPO plan in PR #45](https://github.com/tfriedel/spire-mentor/pull/45), pinned in the configs; it does not contain that project's unpublished PPO trainer or its expert warm start. [AlphaDefect's public description](https://github.com/hmty-CHEN/alphadefect-evaluation-runs/blob/main/docs/model.md) informed the full-run/curriculum and privileged-critic design. Advantage filtering was inspired by [Ataraxos](https://github.com/AtaraxosAI/stratego). This is not a reproduction of either system.

The existing [repository licensing scope](../LICENSE) applies. The CC BY-SA grant for the separately bundled combat-model weights does not license this source or imply that PPO weights are included. Slay the Spire 2 belongs to Mega Crit.
