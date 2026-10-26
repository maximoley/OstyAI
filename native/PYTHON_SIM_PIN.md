# Python sim pin — HISTORICAL

> **Status (2026-08): the pin regime is retired.** The `sts2-rl-agent` Python
> sim is archived and **no longer checked out** anywhere on disk; the
> pin-verification and fixture-recording scripts that depended on it have
> been deleted (this repo has no GitHub Actions CI — local
> nextest/clippy/fmt are the only gates).
> The parity target today is the C# decompilation at
> `../sts2-wiki/decompiled/v0.107.1/` via the C# oracle harness in
> `../sts2-pycli` (see `docs/parity-suite.md` there). This file is kept
> because the port-pin header convention below is still load-bearing as
> *provenance*: many crate files carry `// Ported from sts2-rl-agent@01c094d:…`
> headers, and `crates/sts2-core/src/lib.rs` cites this document.

The Rust port originally parity-tested against the Python simulator at
`sts2-rl-agent/sts2_env/`. The commit was pinned so port targets didn't
drift under the port.

## Final pin (historical provenance)

- Repo: `sts2-rl-agent` (archived, not checked out)
- Commit: `01c094d87056dfca351d002fe2af231c54f5d163`
- Short sha: `01c094d`
- Pinned date: 2026-05-06
- Commit subject: "Expand parity coverage and align gameplay semantics"

The pin was never bumped and never will be — do not "fix" the headers to a
newer sha, and do not resurrect the old bump rule (which predated the
nextest-only rule and instructed `cargo test`).

## Per-port-file convention (still enforced)

Every Rust port file references its Python source path AND the pinned commit
short sha in a header comment:

```rust
// Ported from sts2-rl-agent@01c094d:sts2_env/cards/effects/strike.py
```

This makes provenance greppable: `rg -n 'sts2-rl-agent@01c094d'` finds every
file whose combat-loop scaffolding came from the Python port.

Glue files (registration shims, dispatch tables, DSL re-exports) without a
direct Python counterpart opt out by placing `// port-pin-skip` in the first
20 lines.

`scripts/check-port-pins.sh` (and the `.fish` wrapper) enforces the header
convention — it scans every `.rs` file under
`crates/sts2-content/src/{cards,relics,monsters}` and fails on a missing
header. To extend the scan set, set `PORT_PIN_DIRS` to a space-separated list
of repo-relative paths.

## Retired verification (tooling deleted)

`scripts/check-python-sim-pin.sh` (and its `.fish` wrapper) verified the
sibling `../sts2-rl-agent` checkout was at the pinned sha. With the checkout
gone that check became permanently unsatisfiable, so the script and its
callers — the fixture recorder `scripts/record_fixture.py`, the Cultist fuzz
loop `scripts/run_fuzz_cultist.sh` / `scripts/fuzz_cultist.py`, and
`scripts/verify_mcts_2sigma.py` — were deleted in 2026-08. The parity
fixtures they recorded (`tests/parity/scripted/`, `tests/parity/fuzz/`) are
**frozen**: they remain valid regression pins of the recorded ground truth,
but there is no way to re-record them and none is planned.
