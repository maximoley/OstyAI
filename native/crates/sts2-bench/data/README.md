# `sts2-bench/data/`

Generated data artifacts for the bead `sts2-rs-yuyc` MCTS A/B + oracle
harness. The directory is tracked (`.gitkeep` keeps it alive), but the
JSON outputs themselves are gitignored — regenerate them locally.

## Files

| File | Producer | Notes |
|---|---|---|
| `mcts_corpus.json` | `cargo run --release --example mcts_corpus` | 50 seeded mid-Ironclad states (20 common + 20 elite + 10 boss). Cheap (<1s). |
| `mcts_ab_<git-short-sha>.json` | `cargo run --release --example mcts_ab` | Per-state per-budget search results @ sims ∈ {500, 1000, 2000}. Byte-stable across runs at the same SHA. |
| `mcts_oracle.json` | `cargo run --release --example mcts_oracle` | 50k-sim ground truth, cached per `(state_id, state_hash)`. Slow (minutes). |

## Regeneration order

1. `cargo run --release --example mcts_corpus` (only after editing the corpus generator or the encounter registry).
2. `cargo run --release --example mcts_ab` (run on every branch you want to compare).
3. `cargo run --release --example mcts_oracle` (run once, then commit the file or keep it locally cached).
4. `cargo run --release --example mcts_ab_diff -- data/mcts_ab_<base>.json data/mcts_ab_<branch>.json`

## Smoke test (regression detection)

```sh
# 1. Snapshot the current branch's A/B run.
cargo run --release --example mcts_ab
cp data/mcts_ab_$(git rev-parse --short HEAD).json data/mcts_ab_baseline.json

# 2. Synthetically convert ~80 wins → ties at sims=1000 across every
#    state. Pushes tie-rate ~6pp above baseline (threshold is 5pp).
python3 -c '
import json
p = "crates/sts2-bench/data/mcts_ab_baseline.json"
d = json.load(open(p))
for r in d["per_state"]:
    v = r["results"]["1000"]
    bump = min(80, v["wins"])
    v["wins"] -= bump
    v["ties"] += bump
json.dump(d, open(p.replace("baseline", "perturbed"), "w"), indent=2)
'

# 3. Diff — should flag the perturbed file.
cargo run --release --example mcts_ab_diff -- \
    data/mcts_ab_baseline.json data/mcts_ab_perturbed.json
```
