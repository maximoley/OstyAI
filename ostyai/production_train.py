"""Fresh full-corpus AlphaZero training with effective batches and held-out evaluation."""

import argparse
import hashlib
import json
import os
import signal
import time
from pathlib import Path

import numpy as np
import torch
from accumulated_learner import AccumulatedLearner
from corpus_stream import atomic_json
from prioritized_replay import PrioritizedReplay as Replay
from pool_sampling import POOL_BUDGETS, quotas
from pool_metadata import restore_legacy_pool_tags
from loop import GPU, Native, run_native
from train import prewarm_actor

ROOT = Path(__file__).resolve().parent
STOP = False
SEARCH_SIMULATIONS = 64  # Legacy uniform fallback; production uses per-combat budgets below.
SEARCH_BUDGETS = POOL_BUDGETS
ENVIRONMENT_REVISION = "native-ai-import-v1"


def request_stop(*_):
    global STOP
    STOP = True


def stopping():
    return STOP or (ROOT / "STOP").exists()


def config(fixtures, cases, episode, version, validation=False):
    return dict(
        fixtures=str(fixtures),
        schema=str(ROOT / "schema"),
        cases=cases,
        workers=80,
        batch_size=128,
        buffers=48,
        row_capacity=4096,
        max_actions=1000 if validation else 500,
        simulations=SEARCH_SIMULATIONS,
        simulation_budgets=SEARCH_BUDGETS,
        skip_forced_actions=True,
        considered_actions=8,
        persistent_search=True,
        search_cache_nodes=512,
        record_actions=False,
        record_replay=not validation,
        replay_capacity=cases * 500,
        reward_shaping=True,
        fixed_potion_cost=None,
        actor_version=version,
        episode=episode,
        search_seed=2026102501,
    )


def collect(native, gpu, cfg, replay=None):
    def completed(pool, report):
        if replay is not None:
            assert report["replay_evicted"] == 0
            report["replay_ingest"] = replay.ingest(native, pool, combat_tiers={
                (int(report["episode"]) << 32) | int(row["id"]): row["search_tier"]
                for row in report["outcomes"]
            })

    t = time.perf_counter()
    with torch.autocast("cuda", dtype=torch.bfloat16, cache_enabled=False):
        result = run_native(native, gpu, cfg, on_complete=completed)
    if cfg.get("simulation_budgets"):
        expected = 0
        for row in result["outcomes"]:
            budget = cfg["simulation_budgets"][row["search_tier"]]
            assert row["simulation_budget"] == budget
            assert row["decisions"] == row["searched_decisions"] + row["forced_actions"]
            assert row["simulations"] == budget * row["searched_decisions"]
            expected += row["simulations"]
        assert result["simulations"] == expected
        assert result["simulation_budgets"] == cfg["simulation_budgets"]
    assert result.get("environment_revision") == ENVIRONMENT_REVISION, "Stale native simulator library"
    result["native_sha256"] = native.sha256
    result["total_seconds"] = time.perf_counter() - t
    return result


def actor_fingerprint(gpu):
    h = hashlib.sha256()
    for key, tensor in sorted(gpu.model.state_dict().items()):
        h.update(f"{key}:{tensor.dtype}:{tuple(tensor.shape)}".encode())
        h.update(tensor.detach().contiguous().view(torch.uint8).cpu().numpy().tobytes())
    return h.hexdigest()


def validate(native, gpu, output, update, policy_only=False):
    mode = "policy_only" if policy_only else "search"
    prefix = "policy-validation" if policy_only else "validation"
    budgets = None if policy_only else SEARCH_BUDGETS
    model_hash = actor_fingerprint(gpu)
    fixture_hash = hashlib.sha256((ROOT / "validation/fixtures.jsonl").read_bytes()).hexdigest()
    provenance = dict(evaluation_mode=mode, model_sha256=model_hash,
                      fixture_sha256=fixture_hash, native_sha256=native.sha256,
                      update=update, episodes=[1000000, 1000001],
                      simulation_budgets=budgets, simulations=0 if policy_only else SEARCH_BUDGETS,
                      action_selection="legal_argmax" if policy_only else "gumbel_search",
                      max_actions=1000, skip_forced_actions=True,
                      search_reuse=dict(enabled=not policy_only, capacity_per_combat=512, schema=1))
    directory = output / f"{prefix}-{update:06d}"
    directory.mkdir(exist_ok=True)
    provenance_path = directory / "provenance.json"
    if provenance_path.exists():
        assert json.loads(provenance_path.read_text()) == provenance, "Validation cache provenance mismatch"
    else:
        assert not list(directory.glob("replicate-*.json")), "Unversioned validation cache cannot be reused"
        atomic_json(provenance_path, provenance)
    results = []
    source = [json.loads(s) for s in (ROOT / "validation/fixtures.jsonl").read_text().splitlines()]
    started = time.perf_counter()
    for replicate in range(2):
        path = directory / f"replicate-{replicate}.json"
        if path.exists():
            report = json.loads(path.read_text())
            assert report.get("environment_revision") == ENVIRONMENT_REVISION and report.get("native_sha256") == native.sha256, "Cached validation simulator mismatch"
            assert report.get("skip_forced_actions", False), "Cached validation used forced-action search"
            assert report.get("simulation_budgets") == budgets, "Cached validation uses different search budgets"
            assert report.get("model_sha256") == model_hash and report.get("fixture_sha256") == fixture_hash
            assert report.get("evaluation_mode") == mode
        else:
            cfg = config(ROOT / "validation/fixtures.jsonl", 4096, 1000000 + replicate, update, True)
            if policy_only:
                cfg.update(simulations=0, simulation_budgets=None, record_replay=False, persistent_search=False)
            report = collect(native, gpu, cfg)
            report.update(evaluation_mode=mode, model_sha256=model_hash, fixture_sha256=fixture_hash)
            assert report["completed"] == 4096
            assert sorted(r["id"] for r in report["outcomes"]) == list(range(4096))
            for row in report["outcomes"]:
                row.update(replicate=replicate, case_id=source[row["id"]]["metadata"]["case_id"])
                assert all(0 <= p["cost"] <= 1 for p in row["potions"])
            atomic_json(path, report)
        if policy_only:
            assert report["simulations"] == report["search_transitions"] == report["forced_search_transitions"] == 0
            assert report["max_search_depth"] == report["chance_samples"] == 0
            assert report["replay_added"] == 0
            assert all(r["simulations"] == r["searched_decisions"] == 0 for r in report["outcomes"])
        results.extend(report["outcomes"])
        atomic_json(
            output / "status.json",
            dict(
                stage="policy_validation" if policy_only else "validation",
                update=update,
                completed=len(results),
                requested=8192,
                pid=os.getpid(),
            ),
        )
    assert len(results) == 8192 and len({(r["replicate"], r["id"]) for r in results}) == 8192
    completed = [r for r in results if r["terminal"] and not r["truncated"]]
    wins = sum(r["win"] for r in completed)
    # Initial potion costs must remain frozen across every checkpoint.
    signatures = {}
    for r in results:
        n = sum(p is not None for p in source[r["id"]]["context"].get("potions", []))
        signatures[f"{r['replicate']}:{r['id']}"] = [p["cost"] for p in r["potions"][:n]]
    costs = output / "validation-cost-signatures.json"
    if costs.exists():
        assert json.loads(costs.read_text()) == signatures
    else:
        atomic_json(costs, signatures)
    summary = dict(
        environment_revision=ENVIRONMENT_REVISION, native_sha256=native.sha256,
        evaluation_mode=mode, model_sha256=model_hash,
        action_selection=provenance["action_selection"],
        update=update,
        requested=8192,
        completed=len(completed),
        unresolved=8192 - len(completed),
        wins=wins,
        win_rate=wins / len(completed) if completed else None,
        win_rate_lower_bound=wins / 8192,
        mean_return=float(np.mean([r["return"] for r in completed])) if completed else None,
        mean_potions_used=float(np.mean([sum(p["used"] for p in r["potions"]) for r in completed]))
        if completed
        else None,
        seconds=time.perf_counter() - started,
        per_potion_costs="independent uniform [0,1], frozen per case/replicate",
        simulations=0 if policy_only else SEARCH_BUDGETS,
        skip_forced_actions=True,
        fixture_sha256=hashlib.sha256(
            (ROOT / "validation/fixtures.jsonl").read_bytes()
        ).hexdigest(),
    )
    history = output / f"{prefix}.jsonl"
    existing = [json.loads(line) for line in history.read_text().splitlines()] if history.exists() else []
    previous = [r for r in existing if r["update"] == update]
    if previous:
        assert len(previous) == 1
        assert all(previous[0].get(k) == v for k,v in summary.items() if k != "seconds"), "Validation history mismatch"
        summary = previous[0]
        atomic_json(directory / "summary.json", summary)
    else:
        with history.open("a") as f:
            f.write(json.dumps(summary) + "\n")
    atomic_json(directory / "summary.json", summary)
    print(json.dumps({prefix: summary}), flush=True)
    return summary


def validate_pair(native, gpu, output, update):
    with torch.random.fork_rng(devices=list(range(torch.cuda.device_count()))):
        searched = validate(native, gpu, output, update)
        policy = validate(native, gpu, output, update, policy_only=True)
    assert searched["model_sha256"] == policy["model_sha256"]
    comparison = dict(update=update, model_sha256=searched["model_sha256"],
                      native_sha256=native.sha256, fixture_sha256=searched["fixture_sha256"],
                      search_win_rate=searched["win_rate_lower_bound"],
                      policy_win_rate=policy["win_rate_lower_bound"],
                      search_gain_percentage_points=100*(searched["win_rate_lower_bound"]-policy["win_rate_lower_bound"]),
                      search=searched, policy_only=policy)
    atomic_json(output / f"validation-comparison-{update:06d}.json", comparison)
    return searched


def save_checkpoint(output, learner, gpu, replay, cycle, validated, settings):
    torch.cuda.synchronize()
    rp = output / f"replay-{cycle:06d}.pt"
    if not rp.exists():
        torch.save(replay.state_dict(), rp.with_suffix(".tmp"))
        rp.with_suffix(".tmp").replace(rp)
    data = dict(
        schema=3,
        model=learner.model.state_dict(),
        actor=gpu.model.state_dict(),
        optimizer=learner.optimizer.state_dict(),
        updates=learner.updates,
        examples=learner.examples,
        cycle=cycle,
        validated_update=validated,
        settings=settings,
        replay_file=str(rp),
        torch_rng=torch.get_rng_state(),
        cuda_rng=torch.cuda.get_rng_state_all(),
    )
    torch.save(data, output / "latest.tmp")
    (output / "latest.tmp").replace(output / "latest.pt")
    for old in sorted(output.glob("replay-*.pt"))[:-2]:
        old.unlink()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cycles", type=int, default=10000)
    ap.add_argument("--output", type=Path, default=ROOT / "production")
    ap.add_argument("--microbatch", type=int, default=256)
    ap.add_argument("--resume", action="store_true")
    ap.add_argument("--skip-baseline", action="store_true")
    a = ap.parse_args()
    a.output.mkdir(exist_ok=True)
    signal.signal(signal.SIGTERM, request_stop)
    signal.signal(signal.SIGINT, request_stop)
    torch.set_num_threads(1)
    torch.set_float32_matmul_precision("high")
    torch.manual_seed(2026102501)
    settings = dict(
        environment_revision=ENVIRONMENT_REVISION,
        cycles=a.cycles,
        effective_batch=16384,
        microbatch=a.microbatch,
        updates_per_cycle=1,
        validation_every=100,
        policy_validation=dict(every=100, combats=8192, simulations=0,
                               action_selection="legal_argmax", matched_primary=True),
        human_validation_every=500,
        human_validation_runs=512,
        validation_combats=8192,
        workers=80,
        search_reuse=dict(enabled=True, capacity_per_combat=512, schema=1),
        search_simulations=SEARCH_BUDGETS,
        skip_forced_actions=True,
        replay_capacity=65536,
        replay_rows=1024,
        schema_sha256=hashlib.sha256((ROOT / "schema/metadata.json").read_bytes()).hexdigest(),
        seed=2026102501,
        potion_cost="independent per-instance Uniform[0,1]",
        sampler="full-corpus bank stream",
        prioritized_replay=dict(alpha=1.0, beta=1.0, uniform_mix=0.05,
                                priority="absolute network value error", epsilon=1e-3),
        learner_pool_sampling=dict(schema=1, mixture="simulation_budget_proportional",
                                   budgets=SEARCH_BUDGETS, quotas=quotas(16384),
                                   priority_normalization="within_pool", importance_target="uniform_within_pool",
                                   retention="existing_FIFO"),
        logging_schema=3,
    )
    atomic_json(a.output / "settings.json", settings)
    ck = torch.load(ROOT / "initial_weights.pt", map_location="cpu", weights_only=False)
    catalog = json.loads((ROOT / "schema/numeric_catalog.json").read_text())
    assert max(catalog["names"].values()) < ck["config"]["catalog_names"]
    assert (len(catalog["card_names"]) - 3) // 2 < ck["config"]["card_definitions"]
    settings["model_config"] = ck["config"]
    settings["initial_weights_sha256"] = hashlib.sha256(
        (ROOT / "initial_weights.pt").read_bytes()
    ).hexdigest()
    atomic_json(a.output / "settings.json", settings)
    native = Native()
    gpu = GPU()
    replay = Replay(65536, rows=1024)
    learner = AccumulatedLearner(ck, microbatch=a.microbatch)
    cycle = 0
    validated = -1
    budget_changed = False
    if a.resume:
        d = torch.load(a.output / "latest.pt", map_location="cpu", weights_only=False)
        assert all(
            d["settings"][k] == settings[k]
            for k in ["effective_batch", "microbatch", "schema_sha256", "seed"]
        )
        learner.model.load_state_dict(d["model"])
        learner.optimizer.load_state_dict(d["optimizer"])
        gpu.model.load_state_dict(d["actor"])
        learner.updates = d["updates"]
        learner.examples = d["examples"]
        cycle = d["cycle"]
        budget_changed = (d["settings"].get("search_reuse") != settings["search_reuse"]
                          or d["settings"].get("policy_validation") != settings["policy_validation"]
                          or d["settings"].get("search_simulations", 16) != SEARCH_BUDGETS
                          or not d["settings"].get("skip_forced_actions", False)
                          or d["settings"].get("human_validation_every") != 500
                          or d["settings"].get("learner_pool_sampling") != settings["learner_pool_sampling"]
                          or d["settings"].get("environment_revision") != ENVIRONMENT_REVISION)
        progress = a.output / "progress.jsonl"
        if progress.exists():
            entries = [json.loads(line) for line in progress.read_text().splitlines()]
            uncommitted = [e for e in entries if e["cycle"] > cycle]
            if uncommitted:
                atomic_json(a.output / f"uncommitted-progress-{time.time_ns()}.json", uncommitted)
            committed = {e["cycle"]: e for e in entries if e["cycle"] <= cycle}
            temporary = progress.with_suffix(".tmp")
            temporary.write_text(
                "".join(json.dumps(committed[k]) + "\n" for k in sorted(committed))
            )
            temporary.replace(progress)
        validated = d["validated_update"]
        replay.load_state_dict(torch.load(d["replay_file"], map_location="cpu", weights_only=False))
        migration = restore_legacy_pool_tags(replay, ROOT)
        print(json.dumps(dict(replay_pool_migration=migration)), flush=True)
        torch.set_rng_state(d["torch_rng"])
        torch.cuda.set_rng_state_all(d["cuda_rng"])
        print(json.dumps(dict(resumed_from_cycle=cycle, optimizer_updates=learner.updates, replay_count=replay.count, previous_search_simulations=d["settings"].get("search_simulations", 16), search_simulations=SEARCH_BUDGETS)), flush=True)
    else:
        assert not (a.output / "latest.pt").exists(), "Refusing to overwrite an existing run"
    atomic_json(
        a.output / "process.json", dict(pid=os.getpid(), started=time.time(), resume=a.resume)
    )
    prewarm_actor(gpu)
    if validated < 0 and not a.skip_baseline:
        baseline = validate_pair(native, gpu, a.output, 0)
        assert baseline["completed"] == 8192, (
            "Initial validation must complete every held-out combat before training"
        )
        validated = 0
    # Catch a checkpoint committed immediately before a scheduled validation.
    if cycle and cycle % 100 == 0 and validated < cycle and not stopping():
        validate_pair(native, gpu, a.output, cycle)
        validated = cycle
        save_checkpoint(a.output, learner, gpu, replay, cycle, validated, settings)
    if cycle and cycle % 100 == 0 and validated == cycle and not stopping():
        policy_summary = a.output / f"policy-validation-{cycle:06d}" / "summary.json"
        if not policy_summary.exists():
            with torch.random.fork_rng(devices=list(range(torch.cuda.device_count()))):
                validate(native, gpu, a.output, cycle, policy_only=True)
            save_checkpoint(a.output, learner, gpu, replay, cycle, validated, settings)
    def human_validation():
        from secondary_validation import evaluate
        return evaluate(native, gpu, a.output, cycle, collect, config, stopping)
    if cycle and cycle % 500 == 0 and not stopping():
        human_validation()
        save_checkpoint(a.output, learner, gpu, replay, cycle, validated, settings)
    while cycle < a.cycles and not stopping():
        index = cycle
        path = ROOT / f"prepared/bank-{index:06d}.jsonl"
        info = path.with_suffix(".json")
        while not info.exists() and not stopping():
            atomic_json(
                a.output / "status.json",
                dict(stage="waiting_for_bank", cycle=cycle, bank=index, pid=os.getpid()),
            )
            time.sleep(1)
        if stopping():
            break
        meta = json.loads(info.read_text())
        assert hashlib.sha256(path.read_bytes()).hexdigest() == meta["sha256"]
        started = time.perf_counter()
        atomic_json(
            a.output / "status.json",
            dict(stage="collecting", cycle=cycle + 1, updates=learner.updates, pid=os.getpid()),
        )
        report = collect(native, gpu, config(path, meta["accepted"], cycle + 1, cycle), replay)
        outcomes = report.pop("outcomes")
        costs = [p["cost"] for row in outcomes for p in row["potions"]]
        assert costs and all(0 <= v <= 1 for v in costs) and len(set(costs)) > 1
        atomic_json(
            a.output / "status.json",
            dict(stage="learning", cycle=cycle + 1, updates=learner.updates, pid=os.getpid()),
        )
        metrics = learner.update(replay)
        learner.publish(gpu)
        cycle += 1
        assert learner.updates == cycle
        entry = dict(
            environment_revision=ENVIRONMENT_REVISION, native_sha256=native.sha256,
            cycle=cycle,
            optimizer_updates=learner.updates,
            training_examples=learner.examples,
            fresh_examples=report["replay_ingest"]["added"],
            replay_count=replay.count,
            fresh_return_mean=report["replay_ingest"]["fresh_return_mean"],
            fresh_return_std=report["replay_ingest"]["fresh_return_std"],
            replay_censored=report["replay_censored"],
            bank=index,
            unsupported_starts=len(meta["rejected"]),
            collected_combats=meta["accepted"],
            collection_seconds=report["total_seconds"],
            search_simulation_budget=report["simulation_budgets"],
            search_by_tier=report["search_by_tier"],
            search_simulations=report["simulations"],
            played_decisions=report["steps"],
            forced_actions=report["forced_actions"],
            searched_decisions=report["steps"] - report["forced_actions"],
            search_reuse=report["search_reuse"],
            cached_search_edges=report["cached_edges"],
            search_max_depth=report["max_search_depth"],
            search_transitions=report["search_transitions"],
            forced_search_transitions=report["forced_search_transitions"],
            learner=metrics,
            seconds=time.perf_counter() - started,
            potion_cost_count=len(costs),
            potion_cost_mean=float(np.mean(costs)),
        )
        with (a.output / "progress.jsonl").open("a") as f:
            f.write(json.dumps(entry) + "\n")
        print(json.dumps(entry), flush=True)
        if budget_changed or cycle % 10 == 0 or cycle == a.cycles or stopping():
            save_checkpoint(a.output, learner, gpu, replay, cycle, validated, settings)
            budget_changed = False
        if cycle % 100 == 0 and not stopping():
            validate_pair(native, gpu, a.output, cycle)
            validated = cycle
            save_checkpoint(a.output, learner, gpu, replay, cycle, validated, settings)
        if cycle % 500 == 0 and not stopping():
            human_validation()
            save_checkpoint(a.output, learner, gpu, replay, cycle, validated, settings)
        atomic_json(
            a.output / "status.json",
            dict(
                stage="running",
                cycle=cycle,
                updates=learner.updates,
                validated_update=validated,
                pid=os.getpid(),
            ),
        )
    if replay.count:
        save_checkpoint(a.output, learner, gpu, replay, cycle, validated, settings)
    atomic_json(
        a.output / "status.json",
        dict(
            stage="complete" if cycle == a.cycles else "stopped",
            cycle=cycle,
            updates=learner.updates,
            validated_update=validated,
            pid=os.getpid(),
        ),
    )


if __name__ == "__main__":
    main()
