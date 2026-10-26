"""Native completed-combat queue -> bounded GPU replay -> captured AlphaZero updates."""

import copy
import ctypes as C
import gc
import time

import numpy as np
import torch
from loop import LENGTHS
from model import CombatTransformer, Config


class ReplayDescriptor(C.Structure):
    _fields_ = [(k, C.c_uint64) for k in ("raw", "policy", "values", "ids", "versions")] + [
        (k, C.c_uint32) for k in ("count", "rows")
    ]


class Replay:
    def __init__(self, capacity=65536, rows=256, seed=20261024):
        self.capacity = capacity
        self.rows = rows
        self.cursor = 0
        self.count = 0
        self.total = 0
        self.raw = torch.zeros((capacity, rows, 16), dtype=torch.int32, device="cuda")
        self.policy = torch.zeros((capacity, rows), device="cuda")
        self.value = torch.zeros(capacity, device="cuda")
        self.lengths = np.zeros(capacity, np.int32)
        self.ids = np.zeros(capacity, np.uint64)
        self.versions = np.zeros(capacity, np.uint64)
        self.rng = np.random.default_rng(seed)
        self.groups = {}
        self.added_versions = {}

    def ingest(self, native, pool, limit=2048):
        f = native.lib.sm_loop_replay
        f.argtypes = [C.c_void_p, C.POINTER(ReplayDescriptor), C.c_uint32]
        f.restype = C.c_int
        limit = min(limit, self.capacity)
        added = 0
        return_sum = 0.0
        return_square_sum = 0.0
        t = time.perf_counter()
        while True:
            d = ReplayDescriptor()
            n = native.check(f(pool, C.byref(d), limit))
            if not n:
                break
            if d.rows > self.rows:
                self.ensure_rows(d.rows)
            raw = np.ctypeslib.as_array(
                (C.c_int32 * (n * d.rows * 16)).from_address(d.raw)
            ).reshape(n, d.rows, 16)
            pi = np.ctypeslib.as_array((C.c_float * (n * d.rows)).from_address(d.policy)).reshape(
                n, d.rows
            )
            z = np.ctypeslib.as_array((C.c_float * n).from_address(d.values))
            ids = np.ctypeslib.as_array((C.c_uint64 * n).from_address(d.ids))
            versions = np.ctypeslib.as_array((C.c_uint64 * n).from_address(d.versions))
            legal = raw[:, 0, 12]
            assert (
                np.all(raw[:, 0, 1] == 2)
                and np.all(legal > 0)
                and np.all(np.isfinite(z))
                and np.all(np.isfinite(pi))
            )
            np.testing.assert_allclose(pi.sum(1), 1, atol=2e-6)
            assert not np.any(pi[np.arange(d.rows)[None, :] >= legal[:, None]])
            # All transfers complete before the next native drain recycles pointers.
            at = (np.arange(n) + self.cursor) % self.capacity
            ti = torch.as_tensor(at, device="cuda")
            self.raw[ti, : d.rows] = torch.as_tensor(raw.copy(), device="cuda")
            self.policy[ti, : d.rows] = torch.as_tensor(pi.copy(), device="cuda")
            if d.rows < self.rows:
                self.policy[ti, d.rows :] = 0
            self.value[ti] = torch.as_tensor(z.copy(), device="cuda")
            self.lengths[at] = raw[:, 0, 10] + 1
            self.ids[at] = ids
            self.versions[at] = versions
            for v, c in zip(*np.unique(versions, return_counts=True)):
                self.added_versions[int(v)] = self.added_versions.get(int(v), 0) + int(c)
            self.cursor = (self.cursor + n) % self.capacity
            self.count = min(self.capacity, self.count + n)
            self.total += n
            added += n
            return_sum += float(np.sum(z, dtype=np.float64))
            return_square_sum += float(np.square(z.astype(np.float64)).sum())
        self.reindex()
        torch.cuda.synchronize()
        mean = return_sum / added if added else None
        return dict(added=added, retained=self.count, seconds=time.perf_counter() - t,
                    fresh_return_mean=mean, fresh_return_std=(max(0.0, return_square_sum / added - mean**2)**0.5 if added else None))

    def ensure_rows(self, required):
        if required <= self.rows:
            return
        rows = next((n for n in LENGTHS if n >= required), None)
        if rows is None:
            raise ValueError(f"Replay row count unsupported: {required}")
        raw = torch.zeros((self.capacity, rows, 16), dtype=torch.int32, device="cuda")
        policy = torch.zeros((self.capacity, rows), device="cuda")
        raw[:, :self.rows].copy_(self.raw)
        policy[:, :self.rows].copy_(self.policy)
        self.raw, self.policy, self.rows = raw, policy, rows

    def reindex(self):
        self.groups = {}
        last = 0
        for s in LENGTHS:
            ids = np.flatnonzero((self.lengths > last) & (self.lengths <= s))
            if len(ids):
                self.groups[s] = ids
            last = s

    def indices(self, batch, tokens=None):
        if tokens is None:
            sizes = list(self.groups)
            weights = np.array([len(self.groups[s]) for s in sizes], float)
            weights /= weights.sum()
            tokens = int(self.rng.choice(sizes, p=weights))
        idx = self.rng.choice(self.groups[tokens], size=batch, replace=True)
        return tokens, idx

    def state_dict(self):
        # Store only populated rows and their maximum needed token capacity.
        n = self.count
        max_rows = int(self.lengths[:n].max())
        return dict(
            capacity=self.capacity,
            rows=self.rows,
            cursor=self.cursor,
            count=n,
            total=self.total,
            raw=self.raw[:n, :max_rows].cpu(),
            policy=self.policy[:n, :max_rows].cpu(),
            value=self.value[:n].cpu(),
            lengths=self.lengths[:n].copy(),
            ids=self.ids[:n].copy(),
            versions=self.versions[:n].copy(),
            rng=self.rng.bit_generator.state,
        )

    def load_state_dict(self, d):
        assert d["capacity"] == self.capacity
        self.ensure_rows(d["rows"])
        n = d["count"]
        s = d["raw"].shape[1]
        self.raw[:n, :s].copy_(d["raw"])
        self.policy[:n, :s].copy_(d["policy"])
        self.value[:n].copy_(d["value"])
        for k in ("lengths", "ids", "versions"):
            getattr(self, k)[:n] = d[k]
        for k in ("cursor", "count", "total"):
            setattr(self, k, d[k])
        self.rng.bit_generator.state = d["rng"]
        self.reindex()


class Learner:
    def __init__(self, checkpoint, learning_rate=3e-4):
        self.model = CombatTransformer(Config(**checkpoint["config"])).cuda().train()
        self.model.load_state_dict(checkpoint["state_dict"])
        self.optimizer = torch.optim.AdamW(
            self.model.parameters(),
            lr=learning_rate,
            weight_decay=1e-4,
            fused=True,
            capturable=True,
        )
        self.forward = torch.compile(self.model, dynamic=True, fullgraph=True)
        self.stream = torch.cuda.Stream()
        self.cache = {}
        self.updates = 0
        self.examples = 0
        self.compile_seconds = 0.0
        self.pending = None
        self.last_metrics = None

    def loss(self, raw, pi, z):
        logits, value, legal = self.forward(raw)
        # Finite masking avoids the invalid 0 * -inf term in cross entropy.
        logp = logits.masked_fill(~legal, -1e9).log_softmax(-1)
        policy = -(pi * logp).sum(-1).mean()
        value_loss = (value - z).square().mean()
        return policy + value_loss, policy, value_loss

    def get(self, replay, batch, tokens):
        key = (batch, tokens)
        if key in self.cache:
            return self.cache[key]
        torch.cuda.synchronize()
        begun = time.perf_counter()
        print(f"Preparing learner graph batch={batch}, tokens={tokens}", flush=True)
        before = {k: v.detach().clone() for k, v in self.model.state_dict().items()}
        moments = {
            p: {k: v.clone() if torch.is_tensor(v) else copy.deepcopy(v) for k, v in st.items()}
            for p, st in self.optimizer.state.items()
        }
        _, idx = replay.indices(batch, tokens)
        raw = replay.raw[:, :tokens].index_select(0, torch.as_tensor(idx, device="cuda"))
        pi = replay.policy[:, :tokens].index_select(0, torch.as_tensor(idx, device="cuda"))
        z = replay.value.index_select(0, torch.as_tensor(idx, device="cuda"))

        def step():
            self.optimizer.zero_grad(set_to_none=True)
            with torch.autocast("cuda", dtype=torch.bfloat16, cache_enabled=False):
                total, ce, mse = self.loss(raw, pi, z)
            total.backward()
            norm = torch.nn.utils.clip_grad_norm_(self.model.parameters(), 1.0, foreach=True)
            self.optimizer.step()
            return torch.stack((total.detach(), ce.detach(), mse.detach(), norm.detach()))

        with torch.cuda.stream(self.stream):
            for _ in range(3):
                metrics = step()
        torch.cuda.synchronize()
        graph = torch.cuda.CUDAGraph()
        with torch.cuda.graph(graph, stream=self.stream):
            metrics = step()
        torch.cuda.synchronize()
        # Capturing/warming must not count as optimizer updates or change weights.
        self.model.load_state_dict(before)
        for p, st in self.optimizer.state.items():
            for k, v in st.items():
                if torch.is_tensor(v):
                    v.copy_(moments[p][k] if p in moments else torch.zeros_like(v))
        slot = dict(raw=raw, pi=pi, z=z, graph=graph, metrics=metrics, event=torch.cuda.Event())
        self.cache[key] = slot
        self.compile_seconds += time.perf_counter() - begun
        return slot

    def clear_graphs(self):
        self.poll(True)
        torch.cuda.synchronize()
        self.optimizer.zero_grad(set_to_none=True)
        self.cache.clear()
        gc.collect()
        torch.cuda.empty_cache()

    def prepare(self, replay, batch):
        for s in replay.groups:
            self.get(replay, batch, s)

    def enqueue(self, replay, batch, tokens=None):
        if self.pending is not None:
            raise RuntimeError("Only one optimizer update may be in flight")
        tokens, idx = replay.indices(batch, tokens)
        slot = self.get(replay, batch, tokens)
        with torch.cuda.stream(self.stream):
            ti = torch.as_tensor(idx, device="cuda")
            slot["raw"].copy_(replay.raw[:, :tokens].index_select(0, ti))
            slot["pi"].copy_(replay.policy[:, :tokens].index_select(0, ti))
            slot["z"].copy_(replay.value.index_select(0, ti))
            slot["graph"].replay()
            slot["event"].record(self.stream)
        self.pending = (slot, batch)
        self.updates += 1
        self.examples += batch

    def poll(self, wait=False):
        if self.pending is None:
            return True
        slot, _ = self.pending
        if wait:
            slot["event"].synchronize()
        if not slot["event"].query():
            return False
        self.last_metrics = slot["metrics"].cpu().tolist()
        if not np.isfinite(self.last_metrics).all():
            raise FloatingPointError("Nonfinite learner loss/gradient")
        self.pending = None
        return True

    def train(self, replay, batch, updates):
        begun = time.perf_counter()
        before = self.updates
        for _ in range(updates):
            self.enqueue(replay, batch)
            self.poll(True)
        elapsed = time.perf_counter() - begun
        return dict(
            updates=self.updates - before,
            seconds=elapsed,
            updates_per_second=updates / elapsed,
            examples_per_second=updates * batch / elapsed,
            batch=batch,
            loss=self.last_metrics,
        )

    def publish(self, gpu):
        self.poll(True)
        torch.cuda.synchronize()
        gpu.model.load_state_dict(
            self.model.state_dict()
        )  # in-place copies preserve graph pointers

    def snapshot(self):
        self.poll(True)
        torch.cuda.synchronize()
        return dict(
            model=copy.deepcopy(self.model.state_dict()),
            optimizer=copy.deepcopy(self.optimizer.state_dict()),
            updates=self.updates,
            examples=self.examples,
        )

    def restore(self, state):
        self.poll(True)
        torch.cuda.synchronize()
        self.model.load_state_dict(state["model"])
        # Keep storage captured in existing CUDA graphs alive.
        saved = state["optimizer"]
        params = [p for group in self.optimizer.param_groups for p in group["params"]]
        ids = [i for group in saved["param_groups"] for i in group["params"]]
        for p, i in zip(params, ids):
            for k, v in self.optimizer.state[p].items():
                if torch.is_tensor(v):
                    v.copy_(saved["state"][i][k])
        self.updates = state["updates"]
        self.examples = state["examples"]
