"""Proportional PER with exact marginal probabilities across length buckets."""

import numpy as np


class PrioritySampler:
    def __init__(self, priorities, groups, rng, alpha=1.0, beta=1.0, uniform_mix=0.05):
        self.rng = rng
        self.beta = beta
        self.groups = groups
        self.sizes = list(groups)
        self.ids = np.concatenate([groups[s] for s in self.sizes])
        mass = np.maximum(priorities[self.ids], 1e-3) ** alpha
        self.probabilities = np.zeros(len(priorities), np.float64)
        self.probabilities[self.ids] = (1 - uniform_mix) * mass / mass.sum() + uniform_mix / len(mass)
        self.n = len(mass)
        self.bucket_mass = np.array([self.probabilities[groups[s]].sum() for s in self.sizes])

    def sample(self, batch):
        slot = self.rng.choice(len(self.sizes), p=self.bucket_mass)
        tokens = self.sizes[slot]
        ids = self.groups[tokens]
        chosen = self.rng.choice(ids, batch, replace=True, p=self.probabilities[ids] / self.bucket_mass[slot])
        weights = (1 / (self.n * self.probabilities[chosen])) ** self.beta
        return tokens, chosen, weights.astype(np.float32)
