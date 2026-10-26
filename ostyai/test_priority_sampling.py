"""Check sampling marginals, bucket correction, and the uniform floor."""

import numpy as np
from priority_sampling import PrioritySampler


def main():
    priorities = np.array([0.001, 1., 2., 9., 30.])
    groups = {64: np.array([0, 1]), 128: np.array([2, 3, 4])}
    sampler = PrioritySampler(priorities, groups, np.random.default_rng(42))
    p = sampler.probabilities
    expected = 0.95 * priorities / priorities.sum() + 0.05 / 5
    np.testing.assert_allclose(p, expected)
    losses = np.array([9., 4., 1., 0.2, 0.01])
    np.testing.assert_allclose((p * (1 / (5*p)) * losses).sum(), losses.mean())
    counts = np.zeros(5)
    for _ in range(20000):
        tokens, ids, weights = sampler.sample(16)
        assert np.isin(ids, groups[tokens]).all()
        np.testing.assert_allclose(weights, 1 / (5*p[ids]), rtol=1e-6)
        np.add.at(counts, ids, 1)
    np.testing.assert_allclose(counts / counts.sum(), p, atol=0.006)
    assert (1 / (5*p)).max() <= 20
    uniform = PrioritySampler(np.ones(5), groups, np.random.default_rng(1))
    np.testing.assert_allclose(uniform.probabilities, np.full(5, 0.2))
    print("PASS: marginals, importance correction, length buckets, uniform floor")


if __name__ == "__main__":
    main()
