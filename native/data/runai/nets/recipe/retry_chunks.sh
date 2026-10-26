#!/usr/bin/env bash
# Recover any labeling chunk that came back empty (chunk 1/seed_start=100 timed
# out on a single hanging seed), then retrain on the FULL ~500-seed set.
#
# Waits for the main campaign's "CAMPAIGN DONE" marker first so there is no core
# contention. Recovers each empty chunk via 10x10-seed SUB-chunks (a hang then
# costs <=10 seeds, not 100). Backs up the main run's 400-seed nets before the
# full-set retrain overwrites them, so both data points survive.
set -uo pipefail

RS=$HOME/projects/sts2/sts2-mono/sts2-rs
RA="$RS/target/release/run_arena"
PY=$HOME/projects/sts2/sts2-mono/sts2-pycli/.venv/bin/python
TRAIN=$HOME/projects/sts2/sts2-mono/sts2-pycli/python
PANELS="$RS/data/seed-panels"
HV="$RS/data/runai/heuristic_v.toml"
CAMP=$HOME/sts2-campaign-faithful
cd "$CAMP"
export STS2_RUNAI_WEIGHTS="$HV"

echo "==================== RETRY START $(date) ===================="

# 1. Wait for the main campaign to fully finish.
echo "[r] waiting for main campaign CAMPAIGN DONE ...  $(date)"
while ! grep -q "CAMPAIGN DONE" campaign.log 2>/dev/null; do sleep 60; done
echo "[r] main campaign done.  $(date)"

# 2. Back up the main run's 400-seed nets before we overwrite with full-500.
[ -f ranker.json ] && cp ranker.json ranker_s400.json
[ -f value.json ]  && cp value.json  value_s400.json
echo "[r] backed up 400-seed nets -> ranker_s400.json / value_s400.json"

# 3. Recover any empty chunk (healthy chunk ~7000 labels; <100 == failed).
for start in 0 100 200 300 400; do
  n=$(wc -l < "chunks/labels_$start.jsonl" 2>/dev/null || echo 0)
  if [ "$n" -lt 100 ]; then
    echo "[r] recovering chunk $start (had $n labels) via 10x10 sub-chunks  $(date)"
    rm -f chunks/labels_${start}_*.jsonl chunks/decisions_${start}_*.jsonl
    for off in 0 10 20 30 40 50 60 70 80 90; do
      sub=$((start + off))
      timeout 1800 "$RA" label-campaign --seeds 10 --seed-start "$sub" \
        --combat eval-cheap --probe-sims 100 --k 32 --t-turns 8 --jobs 0 \
        --out-labels "chunks/labels_${start}_${off}.jsonl" \
        --out-decisions "chunks/decisions_${start}_${off}.jsonl" \
        > "chunks/log_${start}_${off}.txt" 2>&1
      rc=$?
      echo "[r]   sub seed_start=$sub rc=$rc labels=$(wc -l < chunks/labels_${start}_${off}.jsonl 2>/dev/null || echo 0)  $(date)"
    done
    cat chunks/labels_${start}_*.jsonl    > "chunks/labels_$start.jsonl"    2>/dev/null
    cat chunks/decisions_${start}_*.jsonl > "chunks/decisions_$start.jsonl" 2>/dev/null
    echo "[r] chunk $start recovered: labels=$(wc -l < chunks/labels_$start.jsonl)"
  fi
done

# 4. Re-assemble the full label/decision sets (now including recovered chunks).
cat chunks/labels_*.jsonl    > labels.jsonl    2>/dev/null
cat chunks/decisions_*.jsonl > decisions.jsonl 2>/dev/null
echo "[r] FULL labels=$(wc -l < labels.jsonl) decisions=$(wc -l < decisions.jsonl)  $(date)"

# 5. Re-export + retrain on the full set (overwrites ranker.json/value.json).
echo "[r] export features (full)  $(date)"
"$RA" export-ranker-features --labels labels.jsonl --combat eval-cheap --jobs 0 \
  --out features.jsonl > log_export_full.txt 2>&1
echo "[r] features=$(wc -l < features.jsonl)"
echo "[r] train ranker (full)  $(date)"
"$PY" "$TRAIN/train_ranker.py" train --features features.jsonl --labels labels.jsonl \
  --schema schema.json --out ranker.json > log_train_ranker_full.txt 2>&1
tail -4 log_train_ranker_full.txt
echo "[r] train value (full)  $(date)"
"$PY" "$TRAIN/train_value.py" --features features.jsonl --decisions decisions.jsonl \
  --schema schema.json --out value.json > log_train_value_full.txt 2>&1
tail -5 log_train_value_full.txt

# 6. After-baseline on the FULL-500 nets.
echo "[r] AFTER baseline: full-500 nets on faithful harness (rel-200)  $(date)"
STS2_RUNAI_RANKER="$CAMP/ranker.json" STS2_RUNAI_VALUE="$CAMP/value.json" \
  "$RA" sweep --panel "$PANELS/rel-200.json" --policy ranker --combat eval-cheap \
  --jobs 0 --out after_newnets_full500.jsonl > log_after_full.txt 2>&1
echo "[r] after-full summary:"; grep -iE "won|reach|act|realism|relics|summary" log_after_full.txt | tail -15

echo "==================== RETRY DONE $(date) ===================="
echo "BEFORE (old nets):";        grep -iE "won|realism" log_before.txt      | tail -4
echo "AFTER 400-seed (main):";    grep -iE "won|realism" log_after.txt       | tail -4
echo "AFTER 500-seed (full):";    grep -iE "won|realism" log_after_full.txt  | tail -4
