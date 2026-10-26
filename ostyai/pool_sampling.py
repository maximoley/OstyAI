"""Exact per-update pool quotas, conditional PER, and length-efficient packing."""
import numpy as np

POOL_NAMES = ('easy', 'hard', 'elite', 'boss')
POOL_BUDGETS = dict(easy=32, hard=64, elite=96, boss=128)
POOL_CODES = {name: i for i, name in enumerate(POOL_NAMES)}


def quotas(total, budgets=POOL_BUDGETS):
    mass = np.array([budgets[t] for t in POOL_NAMES], dtype=np.int64)
    base = total * mass // mass.sum()
    remainder = total * mass % mass.sum()
    for i in sorted(range(len(mass)), key=lambda i: (-remainder[i], i))[:int(total-base.sum())]:
        base[i] += 1
    return {t: int(base[i]) for i, t in enumerate(POOL_NAMES)}


class PoolPrioritySampler:
    def __init__(self, priorities, tiers, groups, rng, alpha=1.0, beta=1.0, uniform_mix=0.05):
        self.rng, self.beta = rng, beta
        self.priorities, self.tiers = priorities, tiers
        self.tokens = np.zeros(len(priorities), dtype=np.int32)
        self.valid = np.concatenate(list(groups.values()))
        if not np.isin(tiers[self.valid], range(len(POOL_NAMES))).all():
            raise ValueError('Replay pool tags are missing or invalid')
        for tokens, ids in groups.items(): self.tokens[ids] = tokens
        self.members, self.probabilities = {}, {}
        for name, code in POOL_CODES.items():
            ids = self.valid[tiers[self.valid] == code]
            if not len(ids): raise ValueError(f'Cannot satisfy pool quota: {name} replay bin is empty')
            mass = np.maximum(priorities[ids], 1e-3) ** alpha
            if not np.isfinite(mass).all(): raise ValueError('Nonfinite replay priorities')
            self.members[name] = ids
            self.probabilities[name] = (1-uniform_mix)*mass/mass.sum() + uniform_mix/len(ids)

    def plan(self, batch, microsteps):
        requested = quotas(batch*microsteps)
        chosen, weights = [], []
        for name in POOL_NAMES:
            ids, probabilities = self.members[name], self.probabilities[name]
            at = self.rng.choice(len(ids), requested[name], replace=True, p=probabilities)
            chosen.append(ids[at])
            # q_pool appears in both target and proposal and cancels. The exact
            # number drawn from each pool supplies its weight in the full loss.
            weights.append((1/(len(ids)*probabilities[at]))**self.beta)
        chosen = np.concatenate(chosen)
        weights = np.concatenate(weights).astype(np.float32)
        # Reorder only AFTER drawing the complete mixture. Sorting cannot alter
        # selected indices, weights, or quota; pad mixed boundary microbatches.
        shuffle = self.rng.permutation(len(chosen))
        order = shuffle[np.argsort(self.tokens[chosen[shuffle]], kind='stable')]
        chosen, weights = chosen[order], weights[order]
        plan = []
        for start in range(0, len(chosen), batch):
            ids, w = chosen[start:start+batch], weights[start:start+batch]
            plan.append((int(self.tokens[ids].max()), ids, w))
        assert all(int(np.sum(self.tiers[chosen] == POOL_CODES[t])) == requested[t] for t in POOL_NAMES)
        return plan
