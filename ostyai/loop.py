"""Native Rust actions -> pinned NUM1 -> GPU policy/value -> native action queue.

Python orchestrates batches only. Rust owns every Session and constructs every
observation directly in the final pinned slot. No per-action Python objects,
JSON observations, or Python simulator calls occur in the processing loop.
"""

import argparse
import ctypes as C
import json
import hashlib
import os
from ctypes.util import find_library
import time
from collections import Counter, deque
from pathlib import Path

import numpy as np
import torch
from model import CombatTransformer, Config, choose_action

ROOT = Path(__file__).resolve().parent
MODEL = ROOT
LENGTHS = (64, 96, 128, 192, 256, 384, 512, 768, 1024, 1536, 2048, 3072, 4096)


class Descriptor(C.Structure):
    _fields_ = [
        (k, C.c_uint64) for k in ("id", "generation", "observations", "actions", "values", "logits")
    ] + [(k, C.c_uint32) for k in ("count", "max_rows", "row_capacity", "batch_capacity")]


class Native:
    def __init__(self):
        self.path = ROOT / "loop_native.so"
        self.sha256 = hashlib.sha256(self.path.read_bytes()).hexdigest()
        self.lib = C.CDLL(str(self.path))
        signatures = {
            "create": ([C.c_char_p], C.c_void_p),
            "start": ([C.c_void_p], C.c_int),
            "poll": ([C.c_void_p, C.POINTER(Descriptor)], C.c_int),
            "submit": ([C.c_void_p, C.c_uint64, C.c_uint64], C.c_int),
            "report": ([C.c_void_p], C.c_char_p),
            "destroy": ([C.c_void_p], None),
            "error": ([], C.c_char_p),
        }
        for name, (args, res) in signatures.items():
            fn = getattr(self.lib, "sm_loop_" + name)
            fn.argtypes = args
            fn.restype = res
            setattr(self, name, fn)

    def check(self, value):
        if value is None or (isinstance(value, int) and value < 0):
            raise RuntimeError(self.error().decode())
        return value


class CUDA:
    def __init__(self):
        candidate = os.environ.get("OSTYAI_CUDART") or find_library("cudart")
        if not candidate:
            import site
            candidates = [p for base in site.getsitepackages()
                          for p in Path(base).glob("nvidia/cuda_runtime/lib/libcudart.so*")]
            candidate = str(candidates[0]) if candidates else "libcudart.so"
        self.lib = C.CDLL(candidate)
        self.copy2d = self.lib.cudaMemcpy2DAsync
        self.copy2d.argtypes = [
            C.c_void_p,
            C.c_size_t,
            C.c_void_p,
            C.c_size_t,
            C.c_size_t,
            C.c_size_t,
            C.c_int,
            C.c_void_p,
        ]
        self.copy = self.lib.cudaMemcpyAsync
        self.copy.argtypes = [C.c_void_p, C.c_void_p, C.c_size_t, C.c_int, C.c_void_p]
        self.zero = self.lib.cudaMemsetAsync
        self.zero.argtypes = [C.c_void_p, C.c_int, C.c_size_t, C.c_void_p]
        for fn in (self.copy2d, self.copy, self.zero):
            fn.restype = C.c_int

    @staticmethod
    def check(code):
        if code:
            raise RuntimeError(f"CUDA runtime error {code}")


class GPU:
    def __init__(self, lanes=2, device=0, profile=False):
        torch.cuda.set_device(device)
        ckpt = torch.load(os.environ.get("OSTYAI_MODEL", str(MODEL / "initial_weights.pt")),
                          map_location="cpu", weights_only=True)
        self.model = CombatTransformer(Config(**ckpt["config"])).cuda().eval()
        self.model.load_state_dict(ckpt["state_dict"])
        torch._dynamo.config.cache_size_limit = 64
        self.encode = torch.compile(self.model.encode, dynamic=True, fullgraph=True)
        self.forward = torch.compile(self.model.forward_tokens, dynamic=True, fullgraph=True)
        self.cuda = CUDA()
        self.copy_stream = torch.cuda.Stream()
        self.compute_stream = torch.cuda.Stream()
        self.lanes = lanes
        self.cache = {}
        self.profile = profile
        self.compilation_s = 0.0

    def shape(self, d):
        size = next((s for s in (8, 32, 64, 128, 256, 512, 1024) if s >= d.count), None)
        tokens = next((s for s in LENGTHS if s >= d.max_rows), None)
        if size is None or tokens is None:
            raise ValueError(f"Unsupported shape {d.count},{d.max_rows}")
        return size, tokens

    @torch.no_grad()
    def get(self, lane, size, tokens):
        key = (lane, size, tokens)
        if key in self.cache:
            return self.cache[key]
        print(f"Preparing GPU graph lane={lane}, states={size}, tokens={tokens}", flush=True)
        begun = time.perf_counter()
        raw = torch.zeros((size, tokens, 16), dtype=torch.int32, device="cuda")
        side = torch.cuda.Stream()
        side.wait_stream(torch.cuda.current_stream())
        with torch.cuda.stream(side):
            for _ in range(3):
                encoded = self.encode(raw)
                logits, values, legal = self.forward(encoded, raw)
                actions = torch.where(raw[:, 0, 14] != 0, -1, choose_action(logits, legal)).to(
                    torch.int32
                )
        torch.cuda.current_stream().wait_stream(side)
        graph = torch.cuda.CUDAGraph()
        with torch.cuda.graph(graph):
            encoded = self.encode(raw)
            logits, values, legal = self.forward(encoded, raw)
            actions = torch.where(raw[:, 0, 14] != 0, -1, choose_action(logits, legal)).to(
                torch.int32
            )
        torch.cuda.synchronize()
        slot = dict(
            raw=raw,
            actions=actions,
            values=values,
            logits=logits,
            graph=graph,
            copied=torch.cuda.Event(enable_timing=self.profile),
            done=torch.cuda.Event(enable_timing=self.profile),
            started=torch.cuda.Event(enable_timing=self.profile),
            inferred=torch.cuda.Event(enable_timing=self.profile),
            compute_started=torch.cuda.Event(enable_timing=self.profile),
        )
        self.cache[key] = slot
        self.compilation_s += time.perf_counter() - begun
        return slot

    def dispatch(self, lane, d):
        size, tokens = self.shape(d)
        slot = self.get(lane, size, tokens)
        raw = slot["raw"]
        cuda = self.cuda
        with torch.cuda.stream(self.copy_stream):
            stream = self.copy_stream.cuda_stream
            if self.profile:
                slot["started"].record(self.copy_stream)
            if d.count < size:
                cuda.check(
                    cuda.zero(
                        raw.data_ptr() + d.count * tokens * 64,
                        0,
                        (size - d.count) * tokens * 64,
                        stream,
                    )
                )
            # True pitched H2D: no CPU contiguous()/gather or repacking.
            cuda.check(
                cuda.copy2d(
                    raw.data_ptr(),
                    tokens * 64,
                    d.observations,
                    d.row_capacity * 64,
                    tokens * 64,
                    d.count,
                    1,
                    stream,
                )
            )
            slot["copied"].record(self.copy_stream)
        with torch.cuda.stream(self.compute_stream):
            self.compute_stream.wait_event(slot["copied"])
            if self.profile:
                slot["compute_started"].record(self.compute_stream)
            slot["graph"].replay()
            if self.profile:
                slot["inferred"].record(self.compute_stream)
            stream = self.compute_stream.cuda_stream
            cuda.check(cuda.copy(d.actions, slot["actions"].data_ptr(), d.count * 4, 2, stream))
            cuda.check(cuda.copy(d.values, slot["values"].data_ptr(), d.count * 4, 2, stream))
            cuda.check(
                cuda.copy2d(
                    d.logits,
                    d.row_capacity * 4,
                    slot["logits"].data_ptr(),
                    tokens * 4,
                    tokens * 4,
                    d.count,
                    2,
                    stream,
                )
            )
            slot["done"].record(self.compute_stream)
        return dict(lane=lane, descriptor=d, slot=slot, size=size, tokens=tokens)


def cpu_fill(d, policy="first"):
    """Diagnostic transport/queue test only; not a production decision path."""
    rows = (C.c_int32 * (d.batch_capacity * d.row_capacity * 16)).from_address(d.observations)
    actions = (C.c_int32 * d.count).from_address(d.actions)
    values = (C.c_float * d.count).from_address(d.values)
    logits = (C.c_float * (d.count * d.row_capacity)).from_address(d.logits)
    for i in range(d.count):
        offset = i * d.row_capacity * 16
        count = rows[offset + 12]
        actions[i] = (
            -1
            if not count or rows[offset + 14]
            else (rows[offset + 13] if policy == "random" else 0)
        )
        values[i] = 0
        for a in range(count):
            logits[i * d.row_capacity + a] = 1.0 if a == actions[i] else 0.0


def run_native(native, gpu, config, policy="network", on_tick=None, on_complete=None):
    pool = native.check(native.create(json.dumps(config).encode()))
    if not pool:
        raise RuntimeError(native.error().decode())
    available = deque(range(gpu.lanes)) if gpu else deque()
    inflight = []
    stats = Counter()
    shapes = Counter()
    cache_before = len(gpu.cache) if gpu else 0
    compile_before = gpu.compilation_s if gpu else 0
    deadline = time.monotonic() + 1800
    begun = time.perf_counter()
    try:
        native.check(native.start(pool))
        finished = False
        while not finished:
            progressed = False
            if on_tick is not None:
                on_tick()
            for item in list(inflight):
                slot = item["slot"]
                if slot["done"].query():
                    d = item["descriptor"]
                    if config.get("verify_exports", False):
                        # Before releasing the lease, verify actual DMA bytes
                        # and graph outputs against direct compiled inference.
                        host = np.ctypeslib.as_array(
                            (C.c_int32 * (d.count * d.row_capacity * 16)).from_address(
                                d.observations
                            )
                        ).reshape(d.count, d.row_capacity, 16)
                        actual = slot["raw"][: d.count].cpu().numpy()
                        np.testing.assert_array_equal(actual, host[:, : item["tokens"]])
                        logits, values, legal = gpu.forward(gpu.encode(slot["raw"]), slot["raw"])
                        expected_actions = torch.where(
                            slot["raw"][:, 0, 14] != 0, -1, choose_action(logits, legal)
                        ).to(torch.int32)
                        returned_actions = np.ctypeslib.as_array(
                            (C.c_int32 * d.count).from_address(d.actions)
                        )
                        returned_values = np.ctypeslib.as_array(
                            (C.c_float * d.count).from_address(d.values)
                        )
                        np.testing.assert_array_equal(
                            expected_actions[: d.count].cpu().numpy(), returned_actions
                        )
                        np.testing.assert_allclose(
                            values[: d.count].cpu().numpy(), returned_values, rtol=1e-5, atol=1e-6
                        )
                        returned_logits = np.ctypeslib.as_array(
                            (C.c_float * (d.count * d.row_capacity)).from_address(d.logits)
                        ).reshape(d.count, d.row_capacity)
                        np.testing.assert_allclose(
                            logits[: d.count].cpu().numpy(),
                            returned_logits[:, : item["tokens"]],
                            rtol=1e-5,
                            atol=1e-6,
                        )
                        stats["gpu_observations_verified"] += d.count
                    t = time.perf_counter()
                    native.check(native.submit(pool, d.id, d.generation))
                    stats["native_submit_s"] += time.perf_counter() - t
                    if gpu.profile:
                        stats["h2d_ms"] += slot["started"].elapsed_time(slot["copied"])
                        stats["gpu_compute_ms"] += slot["compute_started"].elapsed_time(
                            slot["inferred"]
                        )
                        stats["gpu_queue_wait_ms"] += slot["copied"].elapsed_time(
                            slot["compute_started"]
                        )
                        stats["d2h_ms"] += slot["inferred"].elapsed_time(slot["done"])
                    inflight.remove(item)
                    available.append(item["lane"])
                    progressed = True
            while gpu is None or available:
                d = Descriptor()
                t = time.perf_counter()
                code = native.check(native.poll(pool, C.byref(d)))
                stats["native_poll_s"] += time.perf_counter() - t
                if code == 2:
                    assert not inflight
                    finished = True
                    break
                if code == 0:
                    break
                progressed = True
                stats["observations"] += d.count
                stats["batches"] += 1
                if gpu:
                    lane = available.popleft()
                    t = time.perf_counter()
                    item = gpu.dispatch(lane, d)
                    inflight.append(item)
                    stats["gpu_dispatch_host_s"] += time.perf_counter() - t
                    stats["h2d_bytes"] += d.count * item["tokens"] * 64
                    stats["d2h_bytes"] += d.count * (8 + item["tokens"] * 4)
                    stats["padded_states"] += item["size"] - d.count
                    shapes[f"{item['size']}x{item['tokens']}"] += 1
                else:
                    cpu_fill(d, policy)
                    native.check(native.submit(pool, d.id, d.generation))
            if time.monotonic() > deadline:
                raise TimeoutError("Native closed loop exceeded 30 minutes")
            if not progressed:
                # Yield to native workers; avoid burning a core on an empty queue.
                stats["idle_polls"] += 1
                time.sleep(0.00002)
        if gpu:
            torch.cuda.synchronize()
        wall = time.perf_counter() - begun
        report = json.loads(native.check(native.report(pool)))
        assert report["completed"] == config["cases"] and report["fatal"] is None
        forced = report.get("forced_actions", 0)
        # Forced replay records are CPU exports, but are never GPU requests.
        report["forced_replay_exports"] = forced if config.get("record_replay", True) else 0
        assert report["exports"] == (stats["observations"] + report["forced_replay_exports"]
            + report.get("search_reuse", {}).get("root_evaluation_hits", 0))
        if config.get("simulation_budgets"):
            tiers = report["search_by_tier"]
            assert sum(t["searched_decisions"] for t in tiers.values()) == report["steps"] - forced
            assert report["simulations"] == sum(
                config["simulation_budgets"][tier] * counts["searched_decisions"]
                for tier, counts in tiers.items()
            )
        else:
            assert report["simulations"] == (report["steps"] - forced) * config.get("simulations", 16)
        assert (
            report["exports"]
            == report["steps"] - forced + report["forced_replay_exports"]
            + config["cases"]
            + report["simulations"]
            - report["terminal_backups"]
            - report.get("search_reuse", {}).get("cycle_backups", 0)
        )
        report.update(
            seconds=wall,
            decisions_per_second=report["steps"] / wall,
            observations_per_second=report["exports"] / wall,
            simulations_per_second=report["simulations"] / wall,
            transitions_per_second=(report["search_transitions"] + report["steps"]) / wall,
            combats_per_second=config["cases"] / wall,
            gpu_stats=dict(stats),
            gpu_shapes=dict(shapes),
            policy=policy,
            new_graph_shapes=(len(gpu.cache) - cache_before if gpu else 0),
            compile_capture_seconds=(gpu.compilation_s - compile_before if gpu else 0),
        )
        if on_complete is not None:
            on_complete(pool, report)
        return report
    finally:
        # Host arena must outlive BOTH DMA directions, even on an exception.
        if gpu:
            torch.cuda.synchronize()
        native.destroy(pool)


@torch.inference_mode()
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--simulations", type=int, default=16)
    ap.add_argument("--considered", type=int, default=8)
    ap.add_argument("--cases", type=int, default=8196)
    ap.add_argument("--workers", type=int, default=80)
    ap.add_argument("--batch", type=int, default=128)
    ap.add_argument("--buffers", type=int, default=48)
    ap.add_argument("--rows", type=int, default=1024)
    ap.add_argument("--lanes", type=int, default=2)
    ap.add_argument("--trials", type=int, default=3)
    ap.add_argument("--policy", choices=["network", "first", "random"], default="network")
    ap.add_argument("--profile", action="store_true")
    ap.add_argument("--verify-exports", action="store_true")
    ap.add_argument("--output", default="results.json")
    args = ap.parse_args()
    torch.set_num_threads(1)
    native = Native()
    gpu = GPU(args.lanes, profile=args.profile) if args.policy == "network" else None
    config = dict(
        simulations=args.simulations,
        considered_actions=args.considered,
        cases=args.cases,
        workers=args.workers,
        batch_size=args.batch,
        buffers=args.buffers,
        row_capacity=args.rows,
        max_actions=500,
        profile=args.profile,
        record_actions=True,
        verify_exports=args.verify_exports,
        record_replay=args.simulations > 0,
    )
    results = []
    with torch.autocast("cuda", dtype=torch.bfloat16, cache_enabled=False):
        # Warm-up includes fixture loading, first-time compilation and graph discovery.
        for trial in range(args.trials + 1):
            report = run_native(native, gpu, config, args.policy)
            report["trial"] = trial
            report["warmup"] = trial == 0
            report["steady_state"] = trial > 0 and report["new_graph_shapes"] == 0
            results.append(report)
            if gpu and trial == 0:
                # Capture every batch size on both lanes up to the longest
                # observed sequence before measuring steady-state trials.
                max_tokens = max(k[2] for k in gpu.cache)
                for size in (8, 32, 64, 128, 256, 512, 1024):
                    if size > args.batch:
                        continue
                    for tokens in LENGTHS:
                        if tokens > max_tokens:
                            break
                        for lane in range(gpu.lanes):
                            gpu.get(lane, size, tokens)
            Path(args.output).write_text(json.dumps(dict(config=config, runs=results), indent=2))
            print(json.dumps({k: v for k, v in report.items() if k != "outcomes"}), flush=True)
    print("DONE", flush=True)


if __name__ == "__main__":
    main()
