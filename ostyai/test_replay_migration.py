"""Exercise old checkpoint migration and priority/RNG persistence on GPU replay."""

import copy
import numpy as np
from prioritized_replay import PrioritizedReplay


def main():
    replay = PrioritizedReplay(8, rows=64)
    replay.count = 8
    replay.cursor = 3
    replay.lengths[:] = [50, 60, 80, 90, 80, 50, 50, 90]
    replay.ensure_rows(96)
    replay.reindex()
    replay.update_priorities(np.array([0, 0, 2]), np.array([0.1, 0.7, 2.0]))
    assert replay.priorities[0] == 0.7 and replay.priorities[2] == 2
    saved = copy.deepcopy(replay.state_dict())
    expected = replay.sample_plan(8, 4)
    restored = PrioritizedReplay(8, rows=64)
    restored.load_state_dict(saved)
    assert restored.rows == 96 and restored.cursor == 3
    actual = restored.sample_plan(8, 4)
    for a, b in zip(expected, actual):
        assert a[0] == b[0]
        np.testing.assert_array_equal(a[1], b[1])
        np.testing.assert_array_equal(a[2], b[2])
    saved.pop("priorities")
    restored.load_state_dict(saved)
    np.testing.assert_array_equal(restored.priorities, np.ones(8))
    print("PASS: old replay migration, growth, duplicate errors, priority and RNG restore")


if __name__ == "__main__":
    main()
