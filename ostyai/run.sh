#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
export OMP_NUM_THREADS="${OMP_NUM_THREADS:-1}"
export TORCHINDUCTOR_COMPILE_THREADS="${TORCHINDUCTOR_COMPILE_THREADS:-4}"
export TORCHINDUCTOR_CACHE_DIR="$PWD/inductor-cache"
export TRITON_CACHE_DIR="$PWD/triton-cache"
export PYTHONPATH="$PWD:${PYTHONPATH:-}"
exec "${OSTYAI_PYTHON:-../.venv/bin/python}" "$@"
