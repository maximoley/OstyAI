"""Create a local linker alias for CUDA wheels that ship only libcudart.so.12."""
from pathlib import Path
import site
root=Path(__file__).resolve().parents[1]
paths=[p for s in site.getsitepackages() for p in Path(s).glob('nvidia/cuda_runtime/lib/libcudart.so*')]
if not paths: raise SystemExit('CUDA runtime wheel not found; use a CUDA toolkit and CUDA_LIB_DIR instead.')
d=root/'.venv/cuda-link';d.mkdir(exist_ok=True)
for p in paths:
    dest=d/p.name
    if not dest.exists():dest.symlink_to(p)
alias=d/'libcudart.so'
if not alias.exists():alias.symlink_to(paths[0])
print(d)
