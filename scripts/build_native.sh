#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PYO3_PYTHON="${PYO3_PYTHON:-$ROOT/.venv/bin/python}"
if [[ -z "${CUDA_LIB_DIR:-}" ]]; then
  CUDA_LIB_DIR="$($PYO3_PYTHON -c 'import site,pathlib; p=[p.parent for s in site.getsitepackages() for p in pathlib.Path(s).glob("nvidia/cuda_runtime/lib/libcudart.so*")]; print(p[0] if p else "/usr/local/cuda/lib64")')"
  export CUDA_LIB_DIR
fi
if [[ ! -e "$CUDA_LIB_DIR/libcudart.so" ]]; then
  echo "CUDA_LIB_DIR must contain libcudart.so (CUDA development linker name)." >&2
  echo "For a wheel runtime, use scripts/cuda_link_dir.py first." >&2
  exit 1
fi
cd "$ROOT/native"
cargo build --release --locked -j "${OSTYAI_BUILD_JOBS:-8}" -p sts2-py --lib --features native-gpu-feeder,pyo3/extension-module
cp "${CARGO_TARGET_DIR:-$ROOT/native/target}/release/libsts2_rs.so" "$ROOT/ostyai/loop_native.so"
echo "Built $ROOT/ostyai/loop_native.so"
