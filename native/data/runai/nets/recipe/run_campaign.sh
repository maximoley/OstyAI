#!/usr/bin/env bash
# Faithful-harness retrain campaign (branch runai-retrain-faithful).
# Unattended: old-net baseline -> chunked labeling -> export -> train
# ranker+value -> new-net baseline -> summary. NOT set -e: one bad chunk
# must not abort the whole multi-hour run.
set -uo pipefail

RS=$HOME/projects/sts2/sts2-mono/sts2-rs
RA="$RS/target/release/run_arena"
PY=$HOME/projects/sts2/sts2-mono/sts2-pycli/.venv/bin/python
TRAIN=$HOME/projects/sts2/sts2-mono/sts2-pycli/python
PANELS="$RS/data/seed-panels"
HV="$RS/data/runai/heuristic_v.toml"
PILOT=$HOME/sts2-campaign-pilot
CAMP=$HOME/sts2-campaign-faithful
cd "$CAMP"
mkdir -p chunks
export STS2_RUNAI_WEIGHTS="$HV"

echo "==================== CAMPAIGN START $(date) ===================="

# --- 0. BEFORE: old (relic-less-trained) nets on the FAITHFUL harness ---
echo "[0] BEFORE baseline: old pilot nets on faithful harness (rel-200)  $(date)"
STS2_RUNAI_RANKER="$PILOT/ranker-1500.json" STS2_RUNAI_VALUE="$PILOT/value-1500.json" \
  "$RA" sweep --panel "$PANELS/rel-200.json" --policy ranker --combat eval-cheap \
  --jobs 0 --out before_oldnets.jsonl > log_before.txt 2>&1
echo "[0] before summary:"; grep -iE "won|reach|act|realism|relics|summary" log_before.txt | tail -15

# --- 1. LABELING: 500 seeds in 5x100 chunks (resilient + hang-bounded) ---
for start in 0 100 200 300 400; do
  echo "[1] label chunk seed_start=$start  $(date)"
  timeout 5400 "$RA" label-campaign --seeds 100 --seed-start "$start" \
    --combat eval-cheap --probe-sims 100 --k 32 --t-turns 8 --jobs 0 \
    --out-labels "chunks/labels_$start.jsonl" --out-decisions "chunks/decisions_$start.jsonl" \
    > "chunks/log_$start.txt" 2>&1
  rc=$?
  echo "[1] chunk $start rc=$rc labels=$(wc -l < chunks/labels_$start.jsonl 2>/dev/null || echo 0)  $(date)"
done
cat chunks/labels_*.jsonl    > labels.jsonl    2>/dev/null
cat chunks/decisions_*.jsonl > decisions.jsonl 2>/dev/null
echo "[1] TOTAL labels=$(wc -l < labels.jsonl) decisions=$(wc -l < decisions.jsonl)"

# --- 2. SCHEMA + FEATURE EXPORT ---
echo "[2] schema + export  $(date)"
"$RA" ranker-schema > schema.json
"$RA" export-ranker-features --labels labels.jsonl --combat eval-cheap --jobs 0 \
  --out features.jsonl > log_export.txt 2>&1
echo "[2] features=$(wc -l < features.jsonl)"

# --- 3. TRAIN ranker + value ---
echo "[3] train ranker  $(date)"
"$PY" "$TRAIN/train_ranker.py" train --features features.jsonl --labels labels.jsonl \
  --schema schema.json --out ranker.json > log_train_ranker.txt 2>&1
echo "[3] ranker tail:"; tail -4 log_train_ranker.txt
echo "[3] train value  $(date)"
"$PY" "$TRAIN/train_value.py" --features features.jsonl --decisions decisions.jsonl \
  --schema schema.json --out value.json > log_train_value.txt 2>&1
echo "[3] value tail:"; tail -5 log_train_value.txt

# --- 4. AFTER: new nets on the FAITHFUL harness ---
echo "[4] AFTER baseline: new nets on faithful harness (rel-200)  $(date)"
STS2_RUNAI_RANKER="$CAMP/ranker.json" STS2_RUNAI_VALUE="$CAMP/value.json" \
  "$RA" sweep --panel "$PANELS/rel-200.json" --policy ranker --combat eval-cheap \
  --jobs 0 --out after_newnets.jsonl > log_after.txt 2>&1
echo "[4] after summary:"; grep -iE "won|reach|act|realism|relics|summary" log_after.txt | tail -15

echo "==================== CAMPAIGN DONE $(date) ===================="
echo "BEFORE (old nets):"; grep -iE "won|realism" log_before.txt | tail -4
echo "AFTER (new nets):";  grep -iE "won|realism" log_after.txt  | tail -4
