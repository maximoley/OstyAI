"""Check a full-batch AdamW update against accumulated graph microbatches."""

import copy
import os
import gc
import json
from dataclasses import asdict
from pathlib import Path

import numpy as np
import torch
from accumulated_learner import AccumulatedLearner
from learner import Replay
from model import CombatTransformer, Config

ROOT = Path(__file__).resolve().parent


def main():
    torch.set_num_threads(1)
    torch.manual_seed(2026102407)
    panel = torch.load(
        os.environ["OSTYAI_TEST_REPLAY"],
        map_location="cpu",
        weights_only=False,
    )
    candidates = np.flatnonzero(panel["lengths"] <= 64)[:512]
    replay = Replay(512, rows=64)
    replay.raw.copy_(panel["raw"][candidates, :64])
    replay.policy.copy_(panel["policy"][candidates, :64])
    replay.value.copy_(panel["value"][candidates])
    replay.lengths[:] = panel["lengths"][candidates]
    replay.count = 512
    replay.reindex()
    model = CombatTransformer(Config(width=32, heads=4, layers=1, ff=64)).cuda()
    ck = dict(config=asdict(model.cfg), state_dict=copy.deepcopy(model.state_dict()))
    optimizer = torch.optim.AdamW(
        model.parameters(), lr=3e-4, weight_decay=1e-4, fused=True, capturable=True
    )
    accumulated = AccumulatedLearner(ck, effective_batch=512, microbatch=128, amp=False)
    plan = [(64, np.arange(i * 128, (i + 1) * 128)) for i in range(4)]
    errors = []
    for _ in range(2):
        optimizer.zero_grad(set_to_none=True)
        logits, value, mask = model(replay.raw)
        loss = (
            -(replay.policy * logits.masked_fill(~mask, -1e9).log_softmax(-1)).sum(-1).mean()
            + (value - replay.value).square().mean()
        )
        loss.backward()
        torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0, foreach=True)
        optimizer.step()
        metrics = accumulated.update(replay, plan=plan)
        error = max(
            (a - b).abs().max().item()
            for a, b in zip(model.parameters(), accumulated.model.parameters())
        )
        errors.append(error)
        assert error < 2e-5, (error, metrics)
        assert abs(metrics["loss"][0] - loss.item()) < 1e-5
    result = dict(
        passed=True,
        fp32_full_batch_vs_accumulated_max_parameter_errors=errors,
        one_optimizer_step_per_effective_batch=True,
        clip_once_after_accumulation=True,
    )
    (ROOT / "accumulation-test.json").write_text(json.dumps(result, indent=2))
    print(result, flush=True)
    del model, optimizer, accumulated, panel, replay
    gc.collect()
    torch.cuda.empty_cache()


if __name__ == "__main__":
    main()
