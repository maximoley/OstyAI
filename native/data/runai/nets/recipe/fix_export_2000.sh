#!/usr/bin/env bash
# Corrected full-2000 export+retrain+eval. The original scale_2000.sh stage-3
# export ABORTED on the engine-identity guard (labels mix two engine builds:
# chunks 0-400 from the pre-hang-fix binary 1b169c7e, chunks 500-1900 from the
# post-fix rebuild 805f1fda), then trained on the STALE 500-seed features.jsonl.
# Re-export with --allow-engine-mismatch (replays all 2000 seeds under the
# current engine; train_ranker.py drops any genuinely drifted rows — and since
# the hang fix only changes the hang case, which never produced labels, the
# non-hang trajectories are identical so ~nothing drops).
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

echo "==================== FIX-2000 START $(date) ===================="
echo "[f] labels=$(wc -l < labels.jsonl) decisions=$(wc -l < decisions.jsonl)"

"$RA" ranker-schema > schema.json
echo "[f] export features (--allow-engine-mismatch)  $(date)"
"$RA" export-ranker-features --labels labels.jsonl --combat eval-cheap --jobs 0 \
  --allow-engine-mismatch --out features.jsonl > log_export_2000.txt 2>&1
rc=$?
echo "[f] export rc=$rc features=$(wc -l < features.jsonl)  $(date)"
tail -3 log_export_2000.txt

echo "[f] train ranker  $(date)"
"$PY" "$TRAIN/train_ranker.py" train --features features.jsonl --labels labels.jsonl \
  --schema schema.json --out ranker_2000.json > log_train_ranker_2000.txt 2>&1
tail -5 log_train_ranker_2000.txt
echo "[f] train value  $(date)"
"$PY" "$TRAIN/train_value.py" --features features.jsonl --decisions decisions.jsonl \
  --schema schema.json --out value_2000.json > log_train_value_2000.txt 2>&1
tail -6 log_train_value_2000.txt

echo "[f] eval full-2000 nets on release-2000  $(date)"
STS2_RUNAI_RANKER="$CAMP/ranker_2000.json" STS2_RUNAI_VALUE="$CAMP/value_2000.json" \
  "$RA" sweep --panel "$PANEL" --policy ranker --combat eval-cheap --jobs 0 \
  --out conf_2000.jsonl > conf_log_2000.txt 2>&1
echo "[f] full-2000:"; grep -iE "won|completed|realism" conf_log_2000.txt | tail -4

echo "==================== FIX-2000 DONE $(date) ===================="
echo "OLD (2000-eval):";       grep -iE "won|completed" conf_log_old.txt   2>/dev/null | tail -2
echo "full-500 clean (2000):"; grep -iE "won|completed" conf_log_clean.txt 2>/dev/null | tail -2
echo "full-2000 (2000):";      grep -iE "won|completed" conf_log_2000.txt  | tail -2
