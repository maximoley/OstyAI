"""Versioned, resumable AlphaZero-style actor/replay/learner loop on one GPU."""

import argparse
import json
import time
from pathlib import Path

import torch
from learner import Learner, Replay
from loop import GPU, LENGTHS, Native, run_native

ROOT = Path(__file__).parent


def collect(
    native, gpu, config, replay, learner=None, updates=0, batch=1024, mode="cycles", ingest=True
):
    done = 0

    def tick():
        nonlocal done
        if learner is not None and mode == "concurrent" and learner.poll() and done < updates:
            learner.enqueue(replay, batch)
            done += 1

    def complete(pool, report):
        nonlocal done
        # No replay writes while an optimizer graph may be reading the bank.
        ingested = False
        if learner is not None:
            learner.poll(True)
            if mode == "cycles":
                if ingest and not ingested:
                    report["replay_ingest"] = replay.ingest(native, pool)
                    ingested = True
                learner.train(replay, batch, updates)
                done = updates
            elif done < updates:
                learner.train(replay, batch, updates - done)
                done = updates
        if ingest and not ingested:
            report["replay_ingest"] = replay.ingest(native, pool)

    started = time.perf_counter()
    with torch.autocast("cuda", dtype=torch.bfloat16, cache_enabled=False):
        report = run_native(native, gpu, config, on_tick=tick, on_complete=complete)
    report["joint_seconds"] = time.perf_counter() - started
    report["learner_updates"] = done
    report["joint_updates_per_second"] = done / report["joint_seconds"] if done else 0
    return report


def prewarm_actor(gpu, max_tokens=256, batch=128):
    with torch.autocast("cuda", dtype=torch.bfloat16, cache_enabled=False):
        for b in (8, 32, 64, 128, 256):
            if b > batch:
                break
            for s in LENGTHS:
                if s > max_tokens:
                    break
                for lane in range(gpu.lanes):
                    gpu.get(lane, b, s)


def checkpoint(path, learner, gpu, replay, version, cycle, settings):
    learner.poll(True)
    torch.cuda.synchronize()
    replay_path = path.with_name(f"{path.stem}-replay-{cycle:06d}-{learner.updates:09d}.pt")
    torch.save(replay.state_dict(), replay_path.with_suffix(".tmp"))
    replay_path.with_suffix(".tmp").replace(replay_path)
    data = dict(
        schema=2,
        model=learner.model.state_dict(),
        optimizer=learner.optimizer.state_dict(),
        actor=gpu.model.state_dict(),
        updates=learner.updates,
        examples=learner.examples,
        actor_version=version,
        cycle=cycle,
        settings=settings,
        torch_rng=torch.get_rng_state(),
        cuda_rng=torch.cuda.get_rng_state_all(),
        replay_file=str(replay_path),
        config=learner.model.cfg.__dict__,
    )
    torch.save(data, path.with_suffix(".tmp"))
    path.with_suffix(".tmp").replace(path)
    # Publish the checkpoint pointer before pruning older owned replay generations.
    generations = sorted(path.parent.glob(f"{path.stem}-replay-*.pt"))
    for old in generations[:-2]:
        old.unlink()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--fixtures",
        required=True,
        help="Explicit training corpus; benchmark fixtures are diagnostic only",
    )
    ap.add_argument("--cases", type=int, default=1024)
    ap.add_argument("--workers", type=int, default=80)
    ap.add_argument("--cycles", type=int, default=2)
    ap.add_argument("--updates", type=int, default=128)
    ap.add_argument("--train-batch", type=int, default=256)
    ap.add_argument("--actor-batch", type=int, default=128)
    ap.add_argument("--mode", choices=["cycles", "concurrent"], default="cycles")
    ap.add_argument("--capacity", type=int, default=65536)
    ap.add_argument("--output", type=Path, default=ROOT / "training")
    ap.add_argument("--resume", type=Path)
    ap.add_argument("--diagnostic", action="store_true")
    a = ap.parse_args()
    a.output.mkdir(parents=True, exist_ok=True)
    if "late_act_8196_v1" in a.fixtures and not a.diagnostic:
        raise ValueError(
            "Benchmark fixtures require --diagnostic; do not train production weights on held-out benchmarks"
        )
    torch.set_num_threads(1)
    torch.set_float32_matmul_precision("high")
    torch.manual_seed(20261024)
    ck = torch.load(ROOT / "initial_weights.pt", map_location="cpu", weights_only=False)
    native = Native()
    gpu = GPU()
    replay = Replay(a.capacity)
    learner = Learner(ck)
    version = 0
    cycle = 0
    config = dict(
        fixtures=a.fixtures,
        cases=a.cases,
        workers=a.workers,
        batch_size=a.actor_batch,
        buffers=48,
        row_capacity=1024,
        max_actions=500,
        simulations=16,
        considered_actions=8,
        record_actions=True,
        record_replay=True,
        reward_shaping=True,
        actor_version=0,
        episode=0,
        search_seed=20261024,
    )
    settings = {k: str(v) if isinstance(v, Path) else v for k, v in vars(a).items()}
    if a.resume:
        d = torch.load(a.resume, map_location="cpu", weights_only=False)
        for key in ("fixtures", "capacity", "train_batch", "mode"):
            if d["settings"][key] != settings[key]:
                raise ValueError(f"Resume must preserve {key}")
        learner.model.load_state_dict(d["model"])
        learner.optimizer.load_state_dict(d["optimizer"])
        gpu.model.load_state_dict(d["actor"])
        learner.updates = d["updates"]
        learner.examples = d["examples"]
        version = d["actor_version"]
        cycle = d["cycle"]
        replay.load_state_dict(torch.load(d["replay_file"], map_location="cpu", weights_only=False))
        torch.set_rng_state(d["torch_rng"])
        torch.cuda.set_rng_state_all(d["cuda_rng"])
    else:
        seed = collect(native, gpu, config, replay)
        seed.pop("outcomes")
        (a.output / "seed_collection.json").write_text(json.dumps(seed, indent=2))
    rng_state = replay.rng.bit_generator.state
    learner.prepare(replay, a.train_batch)
    prewarm_actor(gpu)
    replay.rng.bit_generator.state = rng_state
    for _ in range(a.cycles):
        cycle += 1
        config.update(actor_version=version, episode=cycle, fixture_offset=cycle * a.cases)
        r = collect(native, gpu, config, replay, learner, a.updates, a.train_batch, a.mode)
        learner.publish(gpu)
        version += 1
        r.update(
            cycle=cycle,
            actor_version_published=version,
            optimizer_updates=learner.updates,
            loss=learner.last_metrics,
            diagnostic=a.diagnostic,
        )
        r.pop("outcomes")
        with (a.output / "progress.jsonl").open("a") as f:
            f.write(json.dumps(r) + "\n")
        checkpoint(a.output / "latest.pt", learner, gpu, replay, version, cycle, settings)
        print(json.dumps(r), flush=True)
    print("DONE", flush=True)


if __name__ == "__main__":
    main()
