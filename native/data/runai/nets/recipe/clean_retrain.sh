#!/usr/bin/env bash
# Rebuild CLEAN labels/decisions (the retry script's `cat labels_*.jsonl` glob
# double-counted the recovered chunk-100 sub-chunk files). Retrain on the clean
# ~500-seed set and re-eval on 2000 seeds for a trustworthy full-500 number.
set -uo pipefail

RS=$HOME/projects/sts2/sts2-mono/sts2-rs
RA="$RS/target/release/run_arena"
PY=$HOME/projects/sts2/sts2-mono/sts2-pycli/.venv/bin/python
TRAIN=$HOME/projects/sts2/sts2-mono/sts2-pycli/python
PANEL="$RS/data/seed-panels/release-2000.json"
HV="$RS/data/runai/heuristic_v.toml"
CAMP=$HOME/sts2-campaign-faithful
cd "$CAMP"
export STS2_RUNAI_WEIGHTS="$HV"

echo "==================== CLEAN RETRAIN START $(date) ===================="

# Preserve the contaminated (duplicate-weighted) full-500 nets + data.
cp labels.jsonl    labels_dup.jsonl    2>/dev/null || true
cp decisions.jsonl decisions_dup.jsonl 2>/dev/null || true
[ -f ranker.json ] && cp ranker.json ranker_dup500.json
[ -f value.json ]  && cp value.json  value_dup500.json

# Rebuild from TOP-LEVEL chunks only (explicit list — no glob, no sub-chunk dup).
: > labels.jsonl; : > decisions.jsonl
for start in 0 100 200 300 400; do
  [ -s "chunks/labels_$start.jsonl" ]    && cat "chunks/labels_$start.jsonl"    >> labels.jsonl
  [ -s "chunks/decisions_$start.jsonl" ] && cat "chunks/decisions_$start.jsonl" >> decisions.jsonl
done
echo "[k] CLEAN labels=$(wc -l < labels.jsonl)  decisions=$(wc -l < decisions.jsonl)"

# Re-export + retrain on clean data.
echo "[k] export features (clean)  $(date)"
"$RA" export-ranker-features --labels labels.jsonl --combat eval-cheap --jobs 0 \
  --out features.jsonl > log_export_clean.txt 2>&1
echo "[k] features=$(wc -l < features.jsonl)"
echo "[k] train ranker (clean)  $(date)"
"$PY" "$TRAIN/train_ranker.py" train --features features.jsonl --labels labels.jsonl \
  --schema schema.json --out ranker.json > log_train_ranker_clean.txt 2>&1
tail -3 log_train_ranker_clean.txt
echo "[k] train value (clean)  $(date)"
"$PY" "$TRAIN/train_value.py" --features features.jsonl --decisions decisions.jsonl \
  --schema schema.json --out value.json > log_train_value_clean.txt 2>&1
tail -4 log_train_value_clean.txt

# Re-eval on 2000 seeds.
echo "[k] eval clean full-500 on 2000 seeds  $(date)"
STS2_RUNAI_RANKER="$CAMP/ranker.json" STS2_RUNAI_VALUE="$CAMP/value.json" \
  "$RA" sweep --panel "$PANEL" --policy ranker --combat eval-cheap --jobs 0 \
  --out conf_clean500.jsonl > conf_log_clean.txt 2>&1
echo "[k] CLEAN full-500:"; grep -iE "won|completed|realism" conf_log_clean.txt | tail -4

echo "==================== CLEAN RETRAIN DONE $(date) ===================="
echo "OLD (2000):";        grep -iE "won|completed" conf_log_old.txt   | tail -2
echo "dup full-500 (2000):"; grep -iE "won|completed" conf_log_full.txt | tail -2
echo "CLEAN full-500 (2000):"; grep -iE "won|completed" conf_log_clean.txt | tail -2
