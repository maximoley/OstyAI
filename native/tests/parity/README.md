# Parity tests

Fixture store for the combat-parity harnesses. Two eras live here:

1. **Frozen Python-sim fixtures** (`scripted/`, `shrunk/`, `raw-failures/`,
   `map_generator_snapshot.json`) — recorded against the archived
   `sts2-rl-agent` Python sim at pin `01c094d`. The recorder tooling
   (`scripts/record_fixture.py` and friends) has been deleted and the sim is
   no longer checked out anywhere, so these fixtures are **frozen**: they
   remain valid regression pins of the recorded ground truth, but they cannot
   be re-recorded. See `../../PYTHON_SIM_PIN.md` for the retirement note and
   the provenance convention.
2. **C#-oracle-era artifacts** (`blunders/`, `blunder_budgets.toml`, the
   coverage-census and win-rate baseline files) — the live parity target is
   the C# decompilation at `../sts2-wiki/decompiled/v0.107.1/`, driven
   through the oracle harness in `../sts2-pycli` (see `docs/parity-suite.md`
   there).

## Layout

```
tests/parity/
├── scripted/          # frozen fixed-seed fixture traces (run in nextest)
│   └── specs/         # the recorder input specs that produced them (provenance)
├── fuzz/              # empty (.gitkeep) — the seeded Python fuzz loop retired with the recorder
├── shrunk/            # minimized failing cases pinned as regression tests (smoke fixture only)
├── raw-failures/      # raw fuzz failures awaiting shrinking (smoke fixture only)
├── blunders/          # per-floor replay-audit fixtures (sts2-replay-audit)
├── blunder_budgets.toml       # per-class red-flag budgets for `sts2-replay-audit --ci-gate`
├── known_divergences.md       # accepted-divergence ledger (linted, see below)
├── FIXTURE_FORMAT.md          # JSON schema for scripted/*.json
└── map_generator_snapshot.{json,py}  # frozen .NET-Random / map-gen snapshot + retired recorder
```

## Harnesses that consume this directory

All of these run as ordinary integration tests under
`cargo nextest run --workspace` — there is no CI here (this repo deliberately
has no GitHub Actions; local nextest is the gate):

- [`crates/sts2-core/tests/parity_harness.rs`](../../crates/sts2-core/tests/parity_harness.rs)
  — loads every `scripted/*.json`, checks the `BridgeReplayTrace` envelope,
  and byte-compares exporter output against the recorded states (modulo each
  fixture's `compare.exclude_fields`).
- `crates/sts2-content/tests/parity_multi_enemy_dsl.rs`,
  `parity_act23_encounters.rs`, `parity_act1_*.rs` — per-encounter shape
  locks over the same `scripted/` fixtures.
- `crates/sts2-core/tests/map_generator_parity.rs` — asserts against
  `map_generator_snapshot.json`.
- `crates/sts2-core/tests/parity_shrinker.rs` (feature `shrink`, manual) —
  the fixture minimizer; `shrunk/` and `raw-failures/` hold its smoke
  fixtures. Invoke with
  `STS2_SHRINK_INPUT=<path> cargo test -p sts2-core --features shrink --test parity_shrinker`.
- `crates/sts2-cli` (`sts2-replay-audit --ci-gate tests/parity/blunder_budgets.toml`)
  — replay-audit red-flag gate over `blunders/`.
- `crates/sts2-core/tests/known_divergences_lint.rs` — mechanically
  re-triages `known_divergences.md` entries.

## If a scripted fixture goes missing or breaks

Do not look for a recorder — there isn't one anymore. Restore the file from
git history. If the *engine* legitimately changed behavior against a frozen
fixture, the fixture is either updated by hand (it is plain JSON;
`FIXTURE_FORMAT.md` is the schema) with the rationale in the commit, or the
gap is documented in `known_divergences.md`.

New parity coverage for game mechanics should target the C# oracle suite in
`../sts2-pycli` instead of adding Python-sim-style fixtures here.

## Divergence tracking

Accepted divergences (deferred fixes, permanent design differences) are
recorded in [`known_divergences.md`](./known_divergences.md); the entry
schema lives there and is enforced by `known_divergences_lint.rs`. Keep the
list short and link the closing bead to each entry.

## Historical note

The original Python-comparator workflow (nightly fuzz →
`raw-failures/` → shrink → pin into `scripted/`) and its CI wiring are
described in `docs/plans/2026-05-06-rust-porting-plan-v2.1-refined.md`
§"Parity harness runbook". The workflow files it referenced are preserved as
inert templates under `docs/ci-templates/` (formerly `.github/workflows/`,
e.g. `parity-nightly.yml`) — they only activate if deliberately restored in
a standalone export.
