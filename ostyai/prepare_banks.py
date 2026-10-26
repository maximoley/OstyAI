"""Extend the existing full-corpus stream without changing its held-out split."""

import argparse
import fcntl
import importlib.util
import json
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SOURCE = Path(os.environ.get("OSTYAI_CORPUS_CONVERTER", str(ROOT / "external-corpus-converter")))
sys.path.insert(0, str(SOURCE))
sys.path.insert(0, str(SOURCE / "native-lib"))


def main():
    if not (SOURCE / "full_corpus.py").exists():
        raise SystemExit("Set OSTYAI_CORPUS_CONVERTER to your full_corpus.py conversion checkout, "
                         "or supply already prepared native wire JSONL banks; see docs/TRAINING.md")
    from corpus_stream import Preparer
    from full_corpus import FullCorpus

    ap = argparse.ArgumentParser()
    ap.add_argument("--start", type=int, default=0)
    ap.add_argument("--target", type=int, default=10000)
    a = ap.parse_args()
    lock = (ROOT / "producer.lock").open("w")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    assert importlib.util.find_spec("sts2_rs") is not None, "Use the v31 corpus-conversion runtime"
    sampler = FullCorpus(ROOT, seed=20261023, workers=8, size=1024)
    preparer = Preparer()
    (ROOT / "producer.pid").write_text(str(os.getpid()))
    try:
        for index in range(a.start, a.target):
            if (ROOT / "STOP").exists():
                break
            sampler.bank(index)
            preparer.bank(index)
            (ROOT / "producer-status.json").write_text(
                json.dumps(dict(last_ready=index, target=a.target, pid=os.getpid()))
            )
    finally:
        sampler.close()


if __name__ == "__main__":
    main()
