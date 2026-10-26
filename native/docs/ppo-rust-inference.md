# PPO run-policy net — Rust inference (ONNX + tract)

Runbook for `crates/sts2-runai/src/ppo/`, the Rust inference path for the
PPO `RunPolicyNet` trained by the sister `sts2-pycli` project (plan:
`sts2-pycli/docs/superpowers/plans/2026-07-07-ppo-rust-inference.md`; spec
Phase 2: `sts2-pycli/docs/superpowers/specs/2026-07-07-ppo-workbench-integration-design.md`).

## Architecture

`sts2-pycli/python/ppo/{vocab,mapfeat,env,encoder,model,net_policy}.py` is
the porting authority: `sts2-runai::ppo` (`vocab`, `mapfeat`, `obs`,
`encoder`, `net`, `policy`) reimplements the same observation → featurize →
forward → argmax pipeline function-for-function over `RunState`/`RunManager`
instead of the PyO3 wrapper. The exported checkpoint is a fixed-shape ONNX
graph (`cat i64[1,320,9]`, `cont f32[1,320,18]`, `sec i64[1,320]`,
`pad_mask bool[1,320]`, `obs_len i64[1]`, `act_mask bool[1,96]` in →
`logits f32[1,96]` + `value f32[1]` out) run through `tract-onnx` (pure
Rust, CPU, batch 1 — no `ort` fallback needed; tract loads/optimizes/runs
the graph with no unsupported ops). `PpoPolicy` (`ppo/policy.rs`) implements
`RunPolicy` and takes the first-max argmax of the masked softmax over every
non-combat decision (greedy deploy only, mirrors `net_policy.sample(greedy=True)`);
it never plays combat itself — combat stays owned by the wrapping
`MctsCombatPolicy` (with a `HeuristicPolicy` absorbing rare MCTS fallbacks),
exactly matching training's decision ownership in `env.py::_advance`.

## Deploying: `run_arena --policy ppo`

```bash
cd sts2-rs
cargo build --release --bin run_arena -p sts2-cli   # once, if not already built
./target/release/run_arena sweep --policy ppo --combat eval-cheap --seeds 5
```

- **A combat budget is mandatory.** `(--policy ppo, --combat none)` is a
  hard CLI error — the net never plays combat, so it needs MCTS to own that
  surface.
- **Model resolution is per-character** (`net::onnx_model_path_for`):
  `STS2_RUNAI_PPO=<path/to/model.onnx>` env var if set (an explicit override
  that wins for **every** character), else the sibling `sts2-pycli`
  checkout's committed champion for the run's character (resolved relative to
  `CARGO_MANIFEST_DIR`, so this only works when `sts2-rs/` and `sts2-pycli/`
  are siblings, per the monorepo layout):
  - Ironclad → `data/ppo/ppo_fullrun_it350.onnx` (P5 champion, PR #207)
  - Silent → `data/ppo/ppo_fullrun_silent_asc_it300.onnx` (GH #333 event
    pilot leg 2; greedy A0–A10 38.44% vs the retired it390's 20.97% on 2175
    paired cells, positive at 10 of 11 ascensions)
  - other characters → no committed champion yet ⇒ `--policy ppo` panics
    unless `STS2_RUNAI_PPO` is set.

  `run_arena sweep` threads the panel row's character in automatically; the
  back-compat `onnx_model_path()`/`PpoPolicy::from_env()` shims resolve
  Ironclad (used by the parity tests and the PyO3 `PpoRunAgent`). To add a
  new character, extend `champion_onnx_filename` in `net.rs` and drop the
  committed `.onnx` beside it. Per-character `.onnx` files are named
  `ppo_fullrun_<character>_it<N>.onnx` (Ironclad keeps the historical
  un-prefixed `ppo_fullrun_it350.onnx`).
- **Export budget** (GH #315, 2026-07-29). The graph has fixed shapes:
  `T_MAX=320` total tokens, `A_MAX=160` action slots. `PpoNet::load` reads
  both off the loaded graph (`cont` input fact → `T_MAX`, `act_mask` input
  fact → `A_MAX`) rather than duplicating them as Rust consts, so a re-export
  at different values cannot silently mismatch the runtime. `export_onnx.py`
  is the single source of truth for the defaults.

  Overflow is **fatal, never truncated** — the policy panics rather than score
  a decision with options hidden from it. What binds, given
  `obs ≈ deck + 68` (1 state token + deck + relics + potions + map nodes) and
  `act ≈ deck` on the per-deck-card surfaces (shop card removal, campfire
  upgrade):

  | constraint | binds at |
  |---|---|
  | `act ≤ A_MAX` (160) | deck ≈ 160 |
  | `obs + act ≤ T_MAX` (320) | **deck ≈ 126** ← the live binder |

  `A_MAX` was 96 until #315, which bound at deck ≈ 96 and killed an Ironclad
  A9 run mid-flight on a 103-option shop removal. Raising it is nearly free
  (action slots are gathered through a per-token linear head; attention runs
  over `T_MAX` regardless), whereas raising `T_MAX` costs O(T²) attention on
  every non-combat decision — hence 160/320.

  Every sweep row now carries `ppo_peak_act_slots` / `ppo_peak_tokens` (the
  run's high-water marks), so headroom against those two numbers is
  observable without a special build. Decks above ~126 still panic; the
  `panic_message` column on the row names the surface, deck size and floor.
- **Release smoke** (2026-07-07, it350): `--seeds 5` → 5/5 runs completed,
  4W/1L, 1/2527 combat decisions fell back to `HeuristicPolicy`
  (not-surfaced) and was absorbed cleanly with no panic, ~62s wall.

## Checkpoint-swap procedure

Any time the PPO snapshot authority changes (new campaign checkpoint), the
full chain below must be re-run — this is the entire cost of a swap; the
Rust encoder/net code does not change unless the Python encoder changed too
(see the tripwire rule below). Replace `<N>` with the new iteration number
throughout.

```bash
# 1. Snapshot the newest finalized checkpoint into the repo (from sts2-pycli/):
cd sts2-pycli
uv run python python/ppo/snapshot_ckpt.py \
  --src /tmp/ppo-m5 --stem main --out data/ppo \
  --note "<campaign description>"

# 2. Re-export ONNX (module-style invocation — a flat-path invocation fails
#    on the ppo package's relative imports). external_data=False is baked
#    into export_onnx.py; do not drop it (see provenance note below).
PYTHONPATH=python uv run python -m ppo.export_onnx \
  --ckpt data/ppo/ppo_fullrun_it<N>.pt --out data/ppo/ppo_fullrun_it<N>.onnx

# 3. Regenerate the parity fixtures (needs a ranker net on STS2_RUNAI_RANKER
#    for the PpoRunEnv driver's underlying combat policy):
STS2_RUNAI_RANKER=../sts2-rs/data/runai/nets/ranker_v107.json \
  PYTHONPATH=python uv run python -m ppo.dump_parity_fixtures \
  --ckpt data/ppo/ppo_fullrun_it<N>.pt

# 4. Re-run every Rust PPO gate:
cd ../sts2-rs
cargo nextest run -p sts2-runai \
  --test ppo_encoder_parity --test ppo_net_parity --test ppo_net_smoke --test ppo_policy_run
#   (note: --test flags, not bare name filters — bare filters don't match
#   integration test binaries)

# 5. Re-run the Python-vs-Rust parity gates:
cd ../sts2-pycli
uv run pytest tests/test_ppo_rust_parity.py tests/test_ppo_onnx_export.py -q
```

All gates must be green before the new checkpoint is deployed. Update
`STS2_RUNAI_PPO` (or the sibling default path if the new `.onnx` replaces
`ppo_fullrun_it350.onnx` under `data/ppo/`) and re-run the release smoke
above before calling the swap complete.

### First swap performed: it390 → it350 (done)

The initial Phase-2 port shipped against `ppo_fullrun_it390.pt` (a
mid-campaign convenience snapshot). PR #207 then blessed the it350 P5
champion (`ppo_fullrun_it350.pt`, 84.0% G8) and made it the committed
snapshot, removing it390. The swap procedure above was run end-to-end
against it350 — re-export `ppo_fullrun_it350.onnx` → regenerate the parity
fixtures → all Rust `nextest` ppo gates + the `pytest` parity gates green —
and it350 is now the deployed authority (the committed `.onnx` and the
`net.rs` sibling default). This was the procedure's first real exercise; a
future checkpoint just repeats the same five steps.

## Encoder-tweak rule

Any change to `sts2-pycli/python/ppo/{env,encoder,mapfeat,vocab}.py` (or
`model.py`'s schema constants) **must** be mirrored in the Rust port
(`sts2-runai/src/ppo/{obs,encoder,mapfeat,vocab}.rs`) **and** the parity
fixtures regenerated (step 3 above). Gate A1 (`ppo_encoder_parity`,
featurization — exact int + f32-bit equality against the fixture rows) is
the tripwire that catches forgetting: a stale Rust port against a changed
Python encoder fails that gate immediately, before it can silently
misfeaturize a deployed run. Do not hand-wave this gate away or widen its
tolerance to make a change land — it exists specifically because the
Python files are the porting authority and the Rust port has no other
enforcement mechanism.

## Gate reference

| Gate | What it checks | Command | it350 result |
|---|---|---|---|
| A1 (encoder parity) | Rust featurizer == Python `featurize_row()`, exact | `cargo nextest run -p sts2-runai --test ppo_encoder_parity` | 150/150 exact |
| B (net parity) | Rust tract forward == torch forward, π/V < 1e-4 | `cargo nextest run -p sts2-runai --test ppo_net_parity` | max Δπ=1.13e-6, ΔV=5.36e-7 |
| smoke | tract loads/runs the committed `.onnx` | `cargo nextest run -p sts2-runai --test ppo_net_smoke` | pass |
| policy_run | `PpoPolicy` drives a full run end-to-end | `cargo nextest run -p sts2-runai --test ppo_policy_run` | pass |
| A2 (obs JSON) | Rust `PpoObs` JSON == Python `observe()` dict | `uv run pytest tests/test_ppo_rust_parity.py -q` | 12/12 seeds |
| C (argmax identity) | Rust policy argmax == Python policy argmax, live run | `uv run pytest tests/test_ppo_rust_parity.py -q` | 20/20 seeds (1e-5 near-tie carve-out for 2 hand-verified sub-ULP ties) |
| ONNX export shape | fixed-shape wrapper ≡ variable-shape torch forward | `uv run pytest tests/test_ppo_onnx_export.py -q` | pass |

All gates gracefully skip (not fail) when the `.onnx` model or fixture file
isn't present in a given checkout — see `ppo::onnx_model_path()` and
`ppo::test_fixture_path()`.
