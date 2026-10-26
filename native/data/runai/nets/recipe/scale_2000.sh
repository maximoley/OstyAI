#!/usr/bin/env bash
# Scale the faithful-harness retrain from 500 -> 2000 labeling seeds, then
# retrain ranker+value and eval on the 2000-seed RANDOM panel.
#
# Why this is leakage-free: label-campaign labels LITERAL integer seeds
# (seed_start..seed_start+N, i.e. 0..1999); the release-2000 eval panel uses
# large RANDOM seeds (gen_seed-derived) — the two seed spaces don't overlap.
#
# Resumable: a chunk that already has a healthy file (>=100 labels) is skipped.
# Hang-bounded: each 100-seed chunk has a hard timeout; a short/empty chunk is
# recovered via 10x10 sub-chunks (a hang then costs <=10 seeds, not 100). The
# Pillage no-progress soft-lock that timed out chunk-100 last run is FIXED on
# main (023817d5) and baked into the rebuilt run_arena, so timeouts should be
# rare now — the recovery path is belt-and-suspenders.
#
# NOT set -e: one bad chunk must not abort the multi-hour run.
set -uo pipefail

RS=$HOME/projects/sts2/sts2-mono/sts2-rs
RA="$RS/target/release/run_arena"
PY=$HOME/projects/sts2/sts2-mono/sts2-pycli/.venv/bin/python
TRAIN=$HOME/projects/sts2/sts2-mono/sts2-pycli/python
PANEL="$RS/data/seed-panels/release-2000.json"
HV="$RS/data/runai/heuristic_v.toml"
CAMP=$HOME/sts2-campaign-faithful
cd "$CAMP"
mkdir -p chunks
export STS2_RUNAI_WEIGHTS="$HV"

# Seed chunks for the FULL 2000-seed set (0..1900 step 100). 0..400 already
# labeled by the prior campaign; this run fills 500..1900.
ALL_STARTS=$(seq 0 100 1900)

echo "==================== SCALE-2000 START $(date) ===================="
echo "[s] run_arena: $RA  ($(stat -c '%y' "$RA"))"

# --- 0. Preserve the proven full-500 winners before we overwrite. ---
[ -f ranker.json ] && cp -n ranker.json ranker_500.json
[ -f value.json ]  && cp -n value.json  value_500.json
echo "[s] backed up full-500 winners -> ranker_500.json / value_500.json"

label_chunk () { # $1=seed_start  $2=n_seeds
  local start=$1 n=$2
  timeout 9000 "$RA" label-campaign --seeds "$n" --seed-start "$start" \
    --combat eval-cheap --probe-sims 100 --k 32 --t-turns 8 --jobs 0 \
    --out-labels "chunks/labels_$start.jsonl" \
    --out-decisions "chunks/decisions_$start.jsonl" \
    > "chunks/log_$start.txt" 2>&1
}

recover_chunk () { # $1=seed_start  — 10x10 sub-chunks
  local start=$1
  echo "[s] recovering chunk $start via 10x10 sub-chunks  $(date)"
  rm -f chunks/labels_${start}_*.jsonl chunks/decisions_${start}_*.jsonl
  for off in 0 10 20 30 40 50 60 70 80 90; do
    local sub=$((start + off))
    timeout 1800 "$RA" label-campaign --seeds 10 --seed-start "$sub" \
      --combat eval-cheap --probe-sims 100 --k 32 --t-turns 8 --jobs 0 \
      --out-labels "chunks/labels_${start}_${off}.jsonl" \
      --out-decisions "chunks/decisions_${start}_${off}.jsonl" \
      > "chunks/log_${start}_${off}.txt" 2>&1
    echo "[s]   sub seed_start=$sub rc=$? labels=$(wc -l < chunks/labels_${start}_${off}.jsonl 2>/dev/null || echo 0)  $(date)"
  done
  cat chunks/labels_${start}_*.jsonl    > "chunks/labels_$start.jsonl"    2>/dev/null
  cat chunks/decisions_${start}_*.jsonl > "chunks/decisions_$start.jsonl" 2>/dev/null
}

# --- 1. LABELING: fill every chunk that isn't already healthy. ---
for start in $ALL_STARTS; do
  n=$(wc -l < "chunks/labels_$start.jsonl" 2>/dev/null || echo 0)
  if [ "$n" -ge 100 ]; then
    echo "[1] chunk $start already healthy ($n labels) — skip  $(date)"
    continue
  fi
  echo "[1] label chunk seed_start=$start  $(date)"
  label_chunk "$start" 100
  rc=$?
  n=$(wc -l < "chunks/labels_$start.jsonl" 2>/dev/null || echo 0)
  echo "[1] chunk $start rc=$rc labels=$n  $(date)"
  if [ "$n" -lt 100 ]; then
    recover_chunk "$start"
    echo "[1] chunk $start recovered: labels=$(wc -l < chunks/labels_$start.jsonl 2>/dev/null || echo 0)  $(date)"
  fi
done

# --- 2. ASSEMBLE the full 0..1900 set (explicit list — no glob, no sub dup). ---
: > labels.jsonl; : > decisions.jsonl
nchunks=0
for start in $ALL_STARTS; do
  if [ -s "chunks/labels_$start.jsonl" ]; then
    cat "chunks/labels_$start.jsonl"    >> labels.jsonl
    cat "chunks/decisions_$start.jsonl" >> decisions.jsonl
    nchunks=$((nchunks + 1))
  fi
done
echo "[2] assembled $nchunks chunks: labels=$(wc -l < labels.jsonl) decisions=$(wc -l < decisions.jsonl)  $(date)"

# --- 3. SCHEMA + FEATURE EXPORT ---
"$RA" ranker-schema > schema.json
echo "[3] export features  $(date)"
"$RA" export-ranker-features --labels labels.jsonl --combat eval-cheap --jobs 0 \
  --out features.jsonl > log_export_2000.txt 2>&1
echo "[3] features=$(wc -l < features.jsonl)  $(date)"

# --- 4. TRAIN ranker + value on the full set (overwrites ranker.json/value.json). ---
echo "[4] train ranker  $(date)"
"$PY" "$TRAIN/train_ranker.py" train --features features.jsonl --labels labels.jsonl \
  --schema schema.json --out ranker.json > log_train_ranker_2000.txt 2>&1
tail -4 log_train_ranker_2000.txt
echo "[4] train value  $(date)"
"$PY" "$TRAIN/train_value.py" --features features.jsonl --decisions decisions.jsonl \
  --schema schema.json --out value.json > log_train_value_2000.txt 2>&1
tail -5 log_train_value_2000.txt
cp ranker.json ranker_2000.json
cp value.json  value_2000.json

# --- 5. EVAL the full-2000 nets on the 2000-seed RANDOM panel. ---
echo "[5] eval full-2000 nets on release-2000  $(date)"
STS2_RUNAI_RANKER="$CAMP/ranker_2000.json" STS2_RUNAI_VALUE="$CAMP/value_2000.json" \
  "$RA" sweep --panel "$PANEL" --policy ranker --combat eval-cheap --jobs 0 \
  --out conf_2000.jsonl > conf_log_2000.txt 2>&1
echo "[5] full-2000:"; grep -iE "won|completed|realism" conf_log_2000.txt | tail -4

echo "==================== SCALE-2000 DONE $(date) ===================="
echo "OLD (2000-eval):";          grep -iE "won|completed" conf_log_old.txt   2>/dev/null | tail -2
echo "full-500 clean (2000):";    grep -iE "won|completed" conf_log_clean.txt 2>/dev/null | tail -2
echo "full-2000 (2000):";         grep -iE "won|completed" conf_log_2000.txt  | tail -2
