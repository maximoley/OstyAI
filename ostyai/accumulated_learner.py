"""One AdamW update after a complete effective batch of captured microbatches."""

import copy
import time

import numpy as np
import torch
from learner import Learner


METRIC_NAMES = (
    "total_loss", "policy_ce", "value_mse", "sampled_policy_ce", "sampled_value_mse",
    "policy_entropy", "target_policy_entropy", "policy_target_kl",
    "predicted_value_mean", "sampled_return_mean", "value_absolute_error",
    "importance_corrected_return_mean",
)


class AccumulatedLearner(Learner):
    def __init__(self, checkpoint, effective_batch=16384, microbatch=256, amp=True):
        super().__init__(checkpoint)
        assert effective_batch % microbatch == 0
        self.effective_batch = effective_batch
        self.microbatch = microbatch
        self.microsteps = effective_batch // microbatch
        self.amp = amp
        self.metrics_sum = torch.zeros(len(METRIC_NAMES), device="cuda")
        self.batch_metrics = torch.zeros((self.microsteps, len(METRIC_NAMES)), device="cuda")
        self.batch_errors = torch.zeros((self.microsteps, self.microbatch), device="cuda")
        self.batch_policy_errors = torch.zeros_like(self.batch_errors)
        # Every captured backward and optimizer graph shares these stable buffers.
        for p in self.model.parameters():
            p.grad = torch.zeros_like(p)
        self.finish_graph = None

    def clear_graphs(self):
        torch.cuda.synchronize()
        self.cache.clear()
        self.finish_graph = None
        self.optimizer.zero_grad(set_to_none=False)
        import gc

        gc.collect()
        torch.cuda.empty_cache()

    def get(self, replay, batch, tokens):
        assert batch == self.microbatch
        key = (batch, tokens)
        if key in self.cache:
            return self.cache[key]
        torch.cuda.synchronize()
        start = time.perf_counter()
        print(f"Capture accumulated backward: microbatch={batch}, tokens={tokens}", flush=True)
        _, idx = replay.indices(batch, tokens)
        ids = torch.as_tensor(idx, device="cuda")
        raw = replay.raw[:, :tokens].index_select(0, ids)
        pi = replay.policy[:, :tokens].index_select(0, ids)
        z = replay.value.index_select(0, ids)

        weights = torch.ones(batch, device="cuda")
        residuals = torch.zeros(batch, device="cuda")
        policy_errors = torch.zeros(batch, device="cuda")
        batch_metrics = torch.zeros(len(METRIC_NAMES), device="cuda")

        def backward():
            with torch.autocast(
                "cuda", dtype=torch.bfloat16, enabled=self.amp, cache_enabled=False
            ):
                logits, value, legal = self.forward(raw)
                logp = logits.float().masked_fill(~legal, -1e9).log_softmax(-1)
                ce_each = -(pi * logp).sum(-1)
                error = value.float() - z
                mse_each = error.square()
                ce = (weights * ce_each).mean()
                mse = (weights * mse_each).mean()
                total = ce + mse
            (total / self.microsteps).backward()
            residuals.copy_(error.detach().abs())
            policy_errors.copy_(ce_each.detach())
            target_entropy = -(pi * pi.clamp_min(1e-12).log()).sum(-1).mean()
            entropy = -(logp.exp() * logp).sum(-1).mean()
            batch_metrics.copy_(torch.stack((
                total.detach(), ce.detach(), mse.detach(), ce_each.detach().mean(),
                mse_each.detach().mean(), entropy.detach(), target_entropy,
                ce_each.detach().mean() - target_entropy, value.detach().float().mean(),
                z.mean(), residuals.mean(), (weights * z).mean(),
            )))
            self.metrics_sum.add_(batch_metrics / self.microsteps)

        with torch.cuda.stream(self.stream):
            for _ in range(3):
                backward()
        torch.cuda.synchronize()
        graph = torch.cuda.CUDAGraph()
        with torch.cuda.graph(graph, stream=self.stream):
            backward()
        torch.cuda.synchronize()
        self.optimizer.zero_grad(set_to_none=False)
        self.metrics_sum.zero_()
        self.cache[key] = dict(raw=raw, pi=pi, z=z, weights=weights, residuals=residuals, policy_errors=policy_errors, metrics=batch_metrics, graph=graph)
        self.compile_seconds += time.perf_counter() - start
        return self.cache[key]

    def prepare_finish(self):
        if self.finish_graph is not None:
            return
        torch.cuda.synchronize()
        before = {k: v.detach().clone() for k, v in self.model.state_dict().items()}
        moments = {
            p: {k: v.clone() if torch.is_tensor(v) else copy.deepcopy(v) for k, v in s.items()}
            for p, s in self.optimizer.state.items()
        }
        self.optimizer.zero_grad(set_to_none=False)

        def finish():
            norm = torch.nn.utils.clip_grad_norm_(self.model.parameters(), 1.0, foreach=True)
            self.optimizer.step()
            return torch.cat((self.metrics_sum, norm.detach().reshape(1)))

        with torch.cuda.stream(self.stream):
            for _ in range(3):
                metrics = finish()
        torch.cuda.synchronize()
        graph = torch.cuda.CUDAGraph()
        with torch.cuda.graph(graph, stream=self.stream):
            metrics = finish()
        torch.cuda.synchronize()
        self.model.load_state_dict(before)
        for p, state in self.optimizer.state.items():
            for key, value in state.items():
                if torch.is_tensor(value):
                    value.copy_(moments[p][key] if p in moments else torch.zeros_like(value))
        self.finish_graph = dict(graph=graph, metrics=metrics, event=torch.cuda.Event())

    def update(self, replay, plan=None):
        planning_started = time.perf_counter()
        if plan is None:
            plan = (replay.sample_plan(self.microbatch, self.microsteps)
                    if hasattr(replay, "sample_plan") else
                    [replay.indices(self.microbatch) for _ in range(self.microsteps)])
        plan = [(p[0], p[1], p[2] if len(p) == 3 else np.ones(len(p[1]), np.float32)) for p in plan]
        assert len(plan) == self.microsteps and all(len(ids) == self.microbatch for _, ids, _ in plan)
        planning_seconds = time.perf_counter() - planning_started
        rng = copy.deepcopy(replay.rng.bit_generator.state)
        for tokens in sorted({tokens for tokens, _, _ in plan}):
            self.get(replay, self.microbatch, tokens)
        replay.rng.bit_generator.state = rng
        self.prepare_finish()
        torch.cuda.synchronize()
        start = time.perf_counter()
        with torch.cuda.stream(self.stream):
            self.optimizer.zero_grad(set_to_none=False)
            self.metrics_sum.zero_()
            for microstep, (tokens, indices, weights) in enumerate(plan):
                slot = self.cache[self.microbatch, tokens]
                ids = torch.as_tensor(indices, device="cuda")
                slot["raw"].copy_(replay.raw[:, :tokens].index_select(0, ids))
                slot["pi"].copy_(replay.policy[:, :tokens].index_select(0, ids))
                slot["z"].copy_(replay.value.index_select(0, ids))
                slot["weights"].copy_(torch.as_tensor(weights, device="cuda"))
                slot["graph"].replay()
                self.batch_errors[microstep].copy_(slot["residuals"])
                self.batch_policy_errors[microstep].copy_(slot["policy_errors"])
                self.batch_metrics[microstep].copy_(slot["metrics"])
            self.finish_graph["graph"].replay()
            self.finish_graph["event"].record(self.stream)
        self.finish_graph["event"].synchronize()
        all_metrics = self.finish_graph["metrics"].cpu().tolist()
        self.last_metrics = all_metrics[:3] + [all_metrics[-1]]
        named = dict(zip(METRIC_NAMES, all_metrics[:-1]))
        named["gradient_norm_before_clip"] = all_metrics[-1]
        named["gradient_clip_scale"] = min(1.0, 1.0 / (all_metrics[-1] + 1e-6))
        indices = np.concatenate([p[1] for p in plan])
        weights = np.concatenate([p[2] for p in plan])
        errors = self.batch_errors.cpu().numpy().reshape(-1)
        pool_metrics = replay.sampling_metrics(indices, weights, self.batch_policy_errors.cpu().numpy().reshape(-1), errors) if hasattr(replay, "sampling_metrics") else None
        if pool_metrics:
            np.testing.assert_allclose(sum(x["weighted_policy_loss_contribution"] for x in pool_metrics.values()), named["policy_ce"], rtol=2e-5, atol=2e-6)
            np.testing.assert_allclose(sum(x["weighted_value_loss_contribution"] for x in pool_metrics.values()), named["value_mse"], rtol=2e-5, atol=2e-6)
        if hasattr(replay, "update_priorities"):
            replay.update_priorities(indices, errors)
        microbatch_metrics = [dict(zip(METRIC_NAMES, row)) for row in self.batch_metrics.cpu().tolist()]
        for i, row in enumerate(microbatch_metrics):
            row.update(microbatch=i, tokens=plan[i][0])
        if not np.isfinite(self.last_metrics).all():
            raise FloatingPointError("Nonfinite accumulated loss/gradient")
        self.updates += 1
        self.examples += self.effective_batch
        assert all(int(s["step"].item()) == self.updates for s in self.optimizer.state.values())
        seconds = time.perf_counter() - start
        return dict(
            seconds=seconds,
            examples=self.effective_batch,
            microbatch=self.microbatch,
            microsteps=self.microsteps,
            updates=self.updates,
            loss=self.last_metrics,
            metrics=named,
            microbatch_metrics=microbatch_metrics,
            sampling_seconds=planning_seconds,
            replay_sampling=dict(
                prioritized=hasattr(replay, "sample_plan"),
                by_pool=pool_metrics,
                unique_examples=int(len(np.unique(indices))),
                importance_weight_mean=float(weights.mean()),
                importance_weight_min=float(weights.min()),
                importance_weight_max=float(weights.max()),
                importance_effective_sample_size=float(weights.sum()**2 / np.square(weights).sum()),
                mean_actor_age=float(np.mean(self.updates - 1 - replay.versions[indices].astype(np.int64))),
            ),
            learning_rate=self.optimizer.param_groups[0]["lr"],
            weight_decay=self.optimizer.param_groups[0]["weight_decay"],
            examples_per_second=self.effective_batch / seconds,
        )
