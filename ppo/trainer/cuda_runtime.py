"""Locate the CUDA runtime supplied by the selected PyTorch environment/toolkit."""
import ctypes.util
import os
from pathlib import Path
import site
import sys


def runtime_library_path():
    explicit = os.environ.get('CUDA_RUNTIME_LIBRARY')
    if explicit:
        path = Path(explicit).expanduser()
        if not path.is_file():
            raise FileNotFoundError(f'CUDA_RUNTIME_LIBRARY is not a file: {path}')
        return str(path.resolve())
    roots = []
    if os.environ.get('CUDA_LIB_DIR'):
        roots.append(Path(os.environ['CUDA_LIB_DIR']).expanduser())
    roots.extend(Path(p) / 'nvidia/cuda_runtime/lib' for p in site.getsitepackages())
    roots.append(Path(site.getusersitepackages()) / 'nvidia/cuda_runtime/lib')
    roots.extend([Path(sys.prefix) / 'lib', Path('/usr/local/cuda/lib64')])
    for root in roots:
        for pattern in ['libcudart.so', 'libcudart.so.*']:
            matches = sorted(root.glob(pattern))
            if matches:
                return str(matches[0].resolve())
    name = ctypes.util.find_library('cudart')
    if name:
        return name
    raise RuntimeError('CUDA runtime not found; install CUDA-enabled PyTorch or set CUDA_RUNTIME_LIBRARY.')
