#!/usr/bin/env python3
"""Smoke test: RunManager.full_map() matches the web UI `FullMap` schema.

Asserts the live PyO3 surface (after `maturin develop` in crates/sts2-py)
produces row-nested nodes with PascalCase `type`, object-shaped children,
visited/current flags, a separate boss meta-node, and current_coord.

Usage (from sts2-rs repo root, after `maturin develop`):
    python tests/parity/full_map_schema_smoke.py
Exit 0 on success; raises AssertionError otherwise.
"""

from __future__ import annotations

import sts2_rs

PASCAL = {
    "Monster", "Elite", "Boss", "Shop", "RestSite",
    "Treasure", "Ancient", "Unknown", "Unassigned",
}


def main() -> None:
    rm = sts2_rs.RunManager(seed=42, character="Ironclad")
    fm = rm.full_map()
    assert fm is not None, "fresh RunManager has an Act-1 map"
    assert set(fm) >= {"rows", "boss", "current_coord"}, fm.keys()

    rows = fm["rows"]
    assert isinstance(rows, list) and rows, "rows is a non-empty list of rows"
    for i, row in enumerate(rows):
        for n in row:
            assert n["row"] == i, f"row {i} holds only row-{i} nodes"
            assert n["type"] in PASCAL, f"unexpected type {n['type']!r}"
            assert isinstance(n["visited"], bool)
            assert isinstance(n["current"], bool)
            for ch in n["children"]:
                assert set(ch) == {"col", "row"}, f"child must be {{col,row}}: {ch}"

    boss = fm["boss"]
    assert boss is not None and boss["type"] == "Boss", boss
    boss_key = (boss["col"], boss["row"])
    flat = {(n["col"], n["row"]) for row in rows for n in row}
    assert boss_key not in flat, "boss meta-node must not appear in rows"

    print("full_map schema smoke test: OK")


if __name__ == "__main__":
    main()
