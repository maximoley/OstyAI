"""Inference-only fused semantic assembly. Training retains the autograd path."""

import torch
import triton
import triton.language as tl


@triton.jit
def _assemble(
    cats,
    projection,
    semantic,
    gate,
    out,
    N,
    D: tl.constexpr,
    KIND_OFFSET: tl.constexpr,
    R: tl.constexpr,
):
    row = tl.program_id(0) * R + tl.arange(0, R)
    col = tl.arange(0, D)
    valid = row < N
    atom = tl.full((R, D), 0, tl.float32)
    for i in tl.static_range(9):
        ids = tl.load(cats + row * 9 + i, valid, 0)
        vec = tl.load(semantic + ids[:, None] * D + col[None, :], valid[:, None], 0)
        atom += vec
    kind_id = tl.load(cats + row * 9, valid, 0)
    kind = tl.where(kind_id > 0, kind_id - KIND_OFFSET, 0)
    g = tl.load(gate + kind[:, None] * D + col[None, :], valid[:, None], 0)
    p = tl.load(projection + row[:, None] * D + col[None, :], valid[:, None], 0).to(tl.float32)
    value = atom / 3.0 + p * (1.0 + tl.extra.cuda.libdevice.tanh(g))
    tl.store(out + row[:, None] * D + col[None, :], value, valid[:, None])


def assemble(cats, projection, semantic, gate, kind_offset, block_rows=16):
    output = torch.empty_like(projection, dtype=torch.float32)
    n = cats.numel() // 9
    _assemble[triton.cdiv(n, block_rows),](
        cats,
        projection,
        semantic,
        gate,
        output,
        n,
        projection.shape[-1],
        kind_offset,
        block_rows,
        num_warps=4,
    )
    return output
