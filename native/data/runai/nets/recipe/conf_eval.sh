#!/usr/bin/env bash
# High-power confirmation eval: old-pilot vs 400-seed vs full-500 nets on the
# 2000-seed panel, to resolve whether the rel-200 4.0% (full-500) is a real
# improvement over the old 2.0% baseline or just small-sample noise.
set -uo pipefail

RS=$HOME/projects/sts2/sts2-mono/sts2-rs
RA="$RS/target/release/run_arena"
PANEL="$RS/data/seed-panels/release-2000.json"
HV="$RS/data/runai/heuristic_v.toml"
CAMP=$HOME/sts2-campaign-faithful
PILOT=$HOME/sts2-campaign-pilot
cd "$CAMP"
export STS2_RUNAI_WEIGHTS="$HV"

echo "==================== CONF EVAL START $(date) ===================="

echo "[c] OLD pilot nets (relic-less-trained)  $(date)"
STS2_RUNAI_RANKER="$PILOT/ranker-1500.json" STS2_RUNAI_VALUE="$PILOT/value-1500.json" \
  "$RA" sweep --panel "$PANEL" --policy ranker --combat eval-cheap --jobs 0 \
  --out conf_old.jsonl > conf_log_old.txt 2>&1
echo "[c] OLD:"; grep -iE "won|completed|realism" conf_log_old.txt | tail -4

echo "[c] 400-seed faithful nets  $(date)"
STS2_RUNAI_RANKER="$CAMP/ranker_s400.json" STS2_RUNAI_VALUE="$CAMP/value_s400.json" \
  "$RA" sweep --panel "$PANEL" --policy ranker --combat eval-cheap --jobs 0 \
  --out conf_s400.jsonl > conf_log_s400.txt 2>&1
echo "[c] 400-seed:"; grep -iE "won|completed|realism" conf_log_s400.txt | tail -4

echo "[c] full-500 faithful nets  $(date)"
STS2_RUNAI_RANKER="$CAMP/ranker.json" STS2_RUNAI_VALUE="$CAMP/value.json" \
  "$RA" sweep --panel "$PANEL" --policy ranker --combat eval-cheap --jobs 0 \
  --out conf_full500.jsonl > conf_log_full.txt 2>&1
echo "[c] full-500:"; grep -iE "won|completed|realism" conf_log_full.txt | tail -4

echo "==================== CONF EVAL DONE $(date) ===================="
echo "OLD:";      grep -iE "won|completed" conf_log_old.txt  | tail -3
echo "400-seed:"; grep -iE "won|completed" conf_log_s400.txt | tail -3
echo "full-500:"; grep -iE "won|completed" conf_log_full.txt | tail -3
