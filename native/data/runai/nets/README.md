# Run-AI trained nets (value nets)

Only the **value nets** live here now, stored via **git-LFS**
(`.gitattributes` tracks `*value*.json` here as LFS objects; the text
sidecars — this README, `manifest.json`, `recipe/` — are plain git).

**The ranker nets were removed (2026-07-12)** along with the whole
pick-ranker pipeline (`RankerNet` / `--policy ranker` / `train_ranker.py` /
the self-play retrain loop) after the project switched to **PPO** as the
deployed run policy (`--policy ppo`, `sts2-pycli/python/ppo/`). The dated
entries in `manifest.json` that describe ranker nets are retained as
provenance records for past experiments; the net files themselves and the
commands they mention no longer exist. Recover them from git history if
ever needed (`git log -- data/runai/nets`).

## What's still here

- **`value_human_a0.json`** — M6(c) human-outcome value net, all-ascension
  retrain (see `manifest.json` `human_value`). Loadable via
  `STS2_RUNAI_VALUE` for `--policy value-net` sweeps and V-pathing
  experiments. Note the `value_net_dropped` manifest entry: V-pathing was
  measured net-negative and is OFF in deployment.
**`value_full500.json` (the older self-play value net) was removed 2026-08-07**
— superseded by `value_human_a0` per its own `blessed_pair` label. Recover it
from git history if a historical comparison arm is ever needed:
`git checkout <pre-removal-rev> -- sts2-rs/data/runai/nets/value_full500.json`.

```bash
RS=sts2-rs
STS2_RUNAI_VALUE=$RS/data/runai/nets/value_human_a0.json \
  $RS/target/release/run_arena sweep \
    --panel $RS/data/seed-panels/release-2000.json \
    --policy value-net --combat eval-cheap \
    --out $RS/target/run-arena/value-net-release-2000.jsonl
```

## Staleness policy

Nets are tied to the feature schema they were trained against
(`eval::ranker_features::feature_schema_id()` — the schema module keeps its
historical name and must stay byte-stable, since the shipped value-net JSONs
hard-refuse to load on a `schema_id` mismatch). The `engine` hash in
`manifest.json` is a freshness *stamp*, not a gate. To rebake a value net:
`sts2-cli`'s `human_features` binary regenerates the per-floor rows from the
run corpus, and `sts2-pycli/python/train_value.py` trains from them (the
`recipe/` scripts are the historical ranker-era campaign drivers and no
longer run end-to-end).

## What's intentionally NOT here

The raw label campaign artifacts (`labels.jsonl`, `decisions.jsonl`,
`features.jsonl`) were regenerable training inputs and too large for git
history — the ranker-era ones are gone with the pipeline.
