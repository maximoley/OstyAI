# sts2-bench

Criterion benchmarks for the sts2 Rust port. Phase 0 establishes the harness;
real performance numbers land in Phase 4 (random-action combats/sec) and
Phase 8 (final perf pass).

## Run

```fish
cargo bench -p sts2-bench                 # full suite
cargo bench -p sts2-bench -- --quick      # ≤30s smoke
cargo bench -p sts2-bench --bench combat  # single benchmark
```

## Baselines

```fish
cargo bench -p sts2-bench -- --save-baseline phase-0
# later …
cargo bench -p sts2-bench -- --baseline phase-0
```

HTML reports land under `target/criterion/`.

See `docs/plans/2026-05-06-rust-porting-plan-v2.1-refined.md` for per-phase
perf targets.

## MCTS A/B + oracle harness (bead `sts2-rs-yuyc`)

Four binary examples under `examples/` evaluate MCTS **quality** (not
raw speed) across a corpus of seeded mid-Ironclad states.

**Default corpus: `realistic`** (flipped by `sts2-rs-a6wt`,
2026-05-18). The legacy `starter` corpus from `sts2-rs-yuyc` is kept
opt-in via `--corpus starter` for back-compat. Rationale lives in
`[[feedback_corpus_starter_deck_too_simple]]`: starter-deck states
saturate the signal and have repeatedly produced false +0.00 pp /
FAIL verdicts on Texel + value-net A/B gates.

| Example | Purpose |
|---|---|
| `mcts_corpus` | Build the legacy 50-state starter corpus (20 common + 20 elite + 10 boss from the STS2 act-1 registry). Cheap (<1s). |
| `mcts_corpus_realistic` | Build the default 50-state corpus from real `.run-source` TOML fixtures (sts2-rs-4gei). |
| `mcts_ab` | Run `Mcts::search` at sims ∈ {500, 1000, 2000} for every corpus state. Defaults to realistic. Byte-stable across runs at the same SHA. |
| `mcts_oracle` | Run `Mcts::search` at 50000 sims, cached per (state_id, state_hash). Slow (minutes). Defaults to realistic. |
| `mcts_ab_diff <base> <branch>` | Diff two A/B reports; print a markdown table; exit 1 on regression (tie-rate up >5pp OR calibration MSE vs oracle up >0.01). |

Regeneration order, JSON schemas, and the contrived-regression smoke
test are documented in [`data/README.md`](data/README.md).

Typical workflow:

```fish
git checkout main
cargo run --release --example mcts_corpus            # once
cargo run --release --example mcts_oracle            # once (slow)
cargo run --release --example mcts_ab                # baseline
cp crates/sts2-bench/data/mcts_ab_$(git rev-parse --short HEAD).json /tmp/base.json

git checkout my-branch
cargo run --release --example mcts_ab                # branch under test
cargo run --release --example mcts_ab_diff -- /tmp/base.json \
    crates/sts2-bench/data/mcts_ab_$(git rev-parse --short HEAD).json
```
