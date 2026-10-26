"""Bounded restart and explicit STOP handling for this one owned training run."""

import fcntl
import json
import os
import subprocess
import time
from pathlib import Path

from corpus_stream import atomic_json

ROOT = Path(__file__).resolve().parent
OUT = ROOT / "production"


def main():
    OUT.mkdir(exist_ok=True)
    lock = (OUT / "supervisor.lock").open("w")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    atomic_json(OUT / "service.json", dict(supervisor_pid=os.getpid(), stage="starting"))
    for attempt in range(3):
        if (ROOT / "STOP").exists():
            break
        command = [
            "bash",
            str(ROOT / "run.sh"),
            str(ROOT / "production_train.py"),
            "--cycles",
            "10000",
            "--output",
            str(OUT),
        ]
        if (OUT / "latest.pt").exists():
            command += ["--resume"]
        with (OUT / "train.log").open("ab") as log:
            child = subprocess.Popen(
                command, cwd=ROOT, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT
            )
        atomic_json(
            OUT / "service.json",
            dict(
                supervisor_pid=os.getpid(),
                trainer_pid=child.pid,
                stage="running",
                attempt=attempt,
                command=command,
            ),
        )
        code = child.wait()
        status = (
            json.loads((OUT / "status.json").read_text()) if (OUT / "status.json").exists() else {}
        )
        if code == 0 or (ROOT / "STOP").exists():
            atomic_json(
                OUT / "service.json",
                dict(
                    supervisor_pid=os.getpid(),
                    trainer_pid=child.pid,
                    stage=status.get("stage", "stopped"),
                    exit_code=code,
                ),
            )
            return
        atomic_json(
            OUT / "service.json",
            dict(
                supervisor_pid=os.getpid(),
                trainer_pid=child.pid,
                stage="restart_pending",
                exit_code=code,
                attempt=attempt,
            ),
        )
        time.sleep(20)
    atomic_json(
        OUT / "service.json",
        dict(supervisor_pid=os.getpid(), stage="stopped" if (ROOT / "STOP").exists() else "failed"),
    )


if __name__ == "__main__":
    main()
