"""Hash a deterministic native walk for comparison between two library builds."""
import argparse
import hashlib
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import numpy as np
import run_workers
from fast_workers import FastWorkers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--native-library', type=Path, required=True)
    parser.add_argument('--character', choices=['Necrobinder', 'Ironclad'], required=True)
    args = parser.parse_args()
    # Keep the prepared schema; select only the library under comparison.
    run_workers.R = args.native_library.resolve().parent
    rng = np.random.default_rng(3841)
    digest = hashlib.sha256()
    workers = FastWorkers(16, 4, seed=20261105, pinned=False, character=args.character)
    try:
        workers.refill(2048)
        for _ in range(256):
            workers.observe_all()
            for name in ['lengths', 'counts', 'private_lengths', 'metadata']:
                digest.update(np.ascontiguousarray(getattr(workers, name)).tobytes())
            for i in range(workers.n):
                digest.update(workers.raw[i, :workers.lengths[i]].tobytes())
                digest.update(workers.private[i, :workers.private_lengths[i]].tobytes())
            actions = np.array([rng.integers(n) for n in workers.counts], np.int32)
            digest.update(workers.advance(actions).tobytes())
            digest.update(workers.outcomes.tobytes())
            workers.reset()
        digest.update(json.dumps(workers.save_journal(), sort_keys=True, separators=(',', ':')).encode())
        print(json.dumps(dict(character=args.character,frames=4096,trace_sha256=digest.hexdigest())))
    finally:
        workers.close()


if __name__ == '__main__':
    main()
