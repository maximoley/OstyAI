#!/usr/bin/env python3
"""Generate parity snapshots for the wn4wc (P9.2.b) map-generator port.

Captures:
1. `Rng(seed).next_int(0, 1_000_000_000)` × 12 for seeds [0, 1, 42, 2026, -1, INT_MIN]
   — validates the .NET-Random port byte-for-byte.

2. `generate_act_map(15, Rng(seed), ...)` shape + per-point room types
   for seeds [0, 42, 2026] (asc=0, replace=False, act_index=0).

Output: JSON to stdout. The Rust integration test at
`tests/parity/map_generator_parity.rs` parses this and asserts on it.

HISTORICAL — recorder retired. This script imported the archived
`sts2-rl-agent` Python sim, which is no longer checked out anywhere on disk
(see `PYTHON_SIM_PIN.md`). The committed `map_generator_snapshot.json` is
frozen ground truth; the script is kept only as the provenance record of how
that snapshot was produced.

Original usage (run from sts2-rs repo root, requires a ../sts2-rl-agent
checkout at the pin):
    python tests/parity/map_generator_snapshot.py > tests/parity/map_generator_snapshot.json
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PY_REPO = ROOT.parent / "sts2-rl-agent"
if not PY_REPO.is_dir():
    sys.exit(
        "sts2-rl-agent not found at ../sts2-rl-agent — the Python sim is "
        "archived and this recorder is retired. The committed "
        "map_generator_snapshot.json is frozen; see PYTHON_SIM_PIN.md."
    )
sys.path.insert(0, str(PY_REPO))

from sts2_env.core.rng import Rng, deterministic_hash_code  # noqa: E402
from sts2_env.map.generator import generate_act_map, generate_golden_path_map  # noqa: E402


RNG_SEEDS = [0, 1, 42, 2026, -1, -2_147_483_648]
MAP_SEEDS = [0, 42, 2026]


def rng_sequence(seed: int) -> list[int]:
    rng = Rng(seed)
    return [rng.next_int(0, 1_000_000_000) for _ in range(12)]


def map_snapshot(seed: int) -> dict:
    rng = Rng(seed)
    act = generate_act_map(
        num_rooms=15,
        rng=rng,
        ascension_level=0,
        replace_treasure_with_elite=False,
        act_index=0,
    )
    points = sorted(act.all_points(), key=lambda p: (p.row, p.col))
    return {
        "seed": seed,
        "map_length": act.map_length,
        "point_count": len(points),
        "points": [
            {"col": p.col, "row": p.row, "type": p.point_type.name}
            for p in points
        ],
        "edges": sorted(
            [
                (p.col, p.row, c.col, c.row)
                for p in points
                for c in p.children
            ]
        ),
    }


def golden_path_snapshot() -> dict:
    m = generate_golden_path_map(1)
    points = sorted(m.all_points(), key=lambda p: (p.row, p.col))
    return {
        "map_length": m.map_length,
        "points": [
            {"col": p.col, "row": p.row, "type": p.point_type.name}
            for p in points
        ],
    }


def hash_codes() -> dict[str, int]:
    return {
        "monster": deterministic_hash_code("monster"),
        "event": deterministic_hash_code("event"),
        "treasure": deterministic_hash_code("treasure"),
        "rewards": deterministic_hash_code("rewards"),
        "": deterministic_hash_code(""),
    }


def main() -> None:
    out = {
        "rng_sequences": {str(s): rng_sequence(s) for s in RNG_SEEDS},
        "hash_codes": hash_codes(),
        "maps": [map_snapshot(s) for s in MAP_SEEDS],
        "golden_path": golden_path_snapshot(),
    }
    json.dump(out, sys.stdout, indent=2)
    print()


if __name__ == "__main__":
    main()
