"""FIFO retention; simulation-budget pool mixture with conditional prioritized replay."""
import numpy as np
from learner import Replay
from pool_sampling import PoolPrioritySampler, POOL_CODES, POOL_NAMES, quotas


class PrioritizedReplay(Replay):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.priorities = np.ones(self.capacity, np.float64)
        self.tiers = np.full(self.capacity, -1, np.int8)

    def ingest(self, native, pool, limit=2048, combat_tiers=None):
        if combat_tiers is None: raise ValueError('Native per-combat pool tags required')
        old_ids = self.ids.copy()
        old_count = self.count
        initial = {code:max(1.0,float(self.priorities[:self.count][self.tiers[:self.count]==code].max()))
                   if np.any(self.tiers[:self.count]==code) else 1.0 for code in POOL_CODES.values()}
        report = super().ingest(native, pool, limit)
        changed = self.ids != old_ids
        changed[old_count:self.count] = True
        at = np.flatnonzero(changed)
        tags = np.asarray([POOL_CODES[combat_tiers[int(self.ids[i])]] for i in at],dtype=np.int8)
        self.tiers[at] = tags
        self.priorities[at] = np.asarray([initial[int(t)] for t in tags])
        report['retained_by_pool'] = {name:int(np.sum(self.tiers[:self.count]==code)) for name,code in POOL_CODES.items()}
        return report

    def sample_plan(self, batch, microsteps):
        return PoolPrioritySampler(self.priorities, self.tiers, self.groups, self.rng).plan(batch,microsteps)

    def sampling_metrics(self, indices, weights, ce, errors):
        total=len(indices); expected=quotas(total); result={}
        for name,code in POOL_CODES.items():
            mask=self.tiers[indices]==code; n=int(mask.sum());assert n==expected[name]
            w=weights[mask];c=ce[mask];e=errors[mask]
            result[name]=dict(quota=expected[name],sampled=n,fraction=n/total,
                retained=int(np.sum(self.tiers[:self.count]==code)),unique_examples=int(len(np.unique(indices[mask]))),
                importance_weight_mean=float(w.mean()),importance_weight_min=float(w.min()),importance_weight_max=float(w.max()),
                importance_effective_sample_size=float(w.sum()**2/np.square(w).sum()),
                sampled_policy_ce=float(c.mean()),sampled_value_mse=float(np.square(e).mean()),
                weighted_policy_loss_contribution=float(np.sum(w*c,dtype=np.float64)/total),
                weighted_value_loss_contribution=float(np.sum(w*np.square(e),dtype=np.float64)/total))
        return result

    def update_priorities(self, indices, errors):
        assert np.isfinite(errors).all()
        unique, inverse = np.unique(indices, return_inverse=True)
        reduced = np.zeros(len(unique))
        np.maximum.at(reduced, inverse, np.abs(errors))
        self.priorities[unique] = np.maximum(reduced, 1e-3)

    def state_dict(self):
        state = super().state_dict()
        state.update(priorities=self.priorities.copy(), priority_schema=1,
                     tiers=self.tiers[:self.count].copy(), pool_schema=1, pool_names=list(POOL_NAMES))
        return state

    def load_state_dict(self, state):
        super().load_state_dict(state)
        self.priorities[:] = state.get('priorities', 1.0)
        self.tiers.fill(-1)
        if 'tiers' in state:
            assert state['pool_schema']==1 and state['pool_names']==list(POOL_NAMES)
            self.tiers[:self.count]=state['tiers']
        assert np.isfinite(self.priorities).all() and (self.priorities > 0).all()
