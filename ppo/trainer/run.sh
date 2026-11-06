#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
export OMP_NUM_THREADS=1
export TORCHINDUCTOR_CACHE_DIR="${TORCHINDUCTOR_CACHE_DIR:-$PWD/inductor-cache}"
export TRITON_CACHE_DIR="${TRITON_CACHE_DIR:-$PWD/triton-cache}"
exec "${OSTYAI_PYTHON:-python3}" "$@"
