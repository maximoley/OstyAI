"""Immutable bank provenance, native compatibility checks, held-out exclusion."""

import ctypes as C
import hashlib
import json
import pickle
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def atomic_json(path, value):
    path = Path(path)
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(value, indent=2) + "\n")
    Path(tmp).replace(path)


class Preparer:
    def __init__(self):
        self.lib = C.CDLL(str(ROOT / "loop_native.so"))
        self.lib.sm_loop_create.argtypes = [C.c_char_p]
        self.lib.sm_loop_create.restype = C.c_void_p
        self.lib.sm_loop_destroy.argtypes = [C.c_void_p]
        self.lib.sm_loop_error.restype = C.c_char_p
        self.excluded = set(
            json.loads((ROOT / "corpus/manifest.json").read_text())["excluded_human_training_runs"]
        )
        self.directory = ROOT / "prepared"
        self.directory.mkdir(exist_ok=True)

    def bank(self, index):
        path = self.directory / f"bank-{index:06d}.jsonl"
        info = path.with_suffix(".json")
        if info.exists():
            return
        src = ROOT / f"full-corpus/bank-{index:06d}.pkl"
        data = src.read_bytes()
        sha = hashlib.sha256(data).hexdigest()
        assert sha == json.loads(src.with_suffix(".json").read_text())["sha256"]
        rows = pickle.loads(data)
        assert all(r["metadata"].get("run_hash") not in self.excluded for r in rows)
        rejected = []
        candidates = []
        for r in rows:
            if len(r["wire"]["player"].get("potions", [])) > 5:
                rejected.append(dict(source=r["source"], reason="native potion capacity is five"))
            else:
                candidates.append(r)
        probe = self.directory / "preflight.jsonl"

        def valid(items):
            if not items:
                return []
            probe.write_text("".join(json.dumps(r, separators=(",", ":")) + "\n" for r in items))
            config = dict(
                fixtures=str(probe),
                schema=str(ROOT / "schema"),
                cases=len(items),
                workers=1,
                batch_size=1,
                buffers=2,
                row_capacity=1024,
                max_actions=500,
                record_replay=False,
                reward_shaping=True,
                simulations=16,
            )
            pool = self.lib.sm_loop_create(json.dumps(config).encode())
            if pool:
                self.lib.sm_loop_destroy(pool)
                return items
            error = self.lib.sm_loop_error().decode()
            if len(items) == 1:
                rejected.append(dict(source=items[0]["source"], reason=error))
                return []
            middle = len(items) // 2
            return valid(items[:middle]) + valid(items[middle:])

        accepted = valid(candidates)
        assert accepted, "No supported states in training bank"
        payload = "".join(json.dumps(r, separators=(",", ":")) + "\n" for r in accepted)
        temp = path.with_suffix(".tmp")
        temp.write_text(payload)
        Path(temp).replace(path)
        atomic_json(
            info,
            dict(
                bank=index,
                attempted=len(rows),
                accepted=len(accepted),
                rejected=rejected,
                source_sha256=sha,
                sha256=hashlib.sha256(payload.encode()).hexdigest(),
                source_counts=dict(Counter(r["source"].split(":")[0] for r in accepted)),
            ),
        )
