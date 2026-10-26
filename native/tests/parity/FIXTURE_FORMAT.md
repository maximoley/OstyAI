# Scripted Parity Fixture Format

This document defines the JSON schema conventions for files under
`tests/parity/scripted/*.json`.

Use this as the source of truth for fixture authors and reviewers.

## 1) Top-Level Structure

Each scripted parity fixture is a single JSON object with this shape:

```json
{
  "version": 1,
  "mode": "combat",
  "metadata": {
    "scenario": "cultist-empty-turn-cycle",
    "seed": 0,
    "encounter": "Cultist",
    "python_pin": "01c094d",
    "rust_pin": "abc1234",
    "recorded_ts": "2026-05-06T00:00:00Z"
  },
  "compare": {
    "exclude_fields": [
      "/enemies/0/intent",
      "/enemies/0/intent_damage",
      "/enemies/0/intent_hits"
    ],
    "exclude_fields_reason": "Intent rolls diverge across Python MT and Rust Xoshiro RNGs."
  },
  "initial_state": {},
  "steps": [
    {
      "action": {},
      "resulting_state": {}
    }
  ]
}
```

Required and optional fields:

- `metadata.scenario` (recommended): human-readable scenario key.
- `metadata.seed` (required): RNG seed used for the trace.
- `metadata.encounter` (required): encounter name.
- `metadata.python_pin` (required): source Python simulator commit SHA.
- `metadata.rust_pin` (required): Rust commit SHA used during recording.
- `metadata.recorded_ts` (required): RFC3339 timestamp for recorder run.
- `compare.exclude_fields` (optional): list of JSON-pointer-like path strings.
- `compare.exclude_fields_reason` (optional): one-line explanation for exclusions.
- `initial_state` (required): normalized bridge state after setup, before first action.
- `steps` (required): ordered list of `{action, resulting_state}` transitions.

If `compare` is absent, full byte-equality is expected for all fields.

## 2) `compare.exclude_fields` Path Syntax

`exclude_fields` entries are root-relative paths that must start with `/`.
Paths are matched literally against paths produced by the harness diff walker.

Examples:

- Leaf scalar exclusion: `/round_number`
- Nested object leaf exclusion: `/enemies/0/intent`
- Whole subtree exclusion: `/enemies/0`

Matching behavior:

- Exact path match is excluded.
- If a parent path is excluded, the entire subtree under that path is excluded.
- Path entries are plain strings; keep keys/indexes exactly as they appear in JSON.

## 3) When To Use Exclusions

Do use exclusions when divergence is expected by design across languages.

- Primary case today: cross-language RNG implementation differences
  (Python uses Mersenne Twister, Rust uses Xoshiro256++) for stochastic
  intent rolls and shuffle orders.

Do not use exclusions as preventive armor for fields that currently agree.

- If a deterministic scenario agrees now, keep strict equality so parity tests
  still catch regressions.

Every non-empty `exclude_fields` list should include
`compare.exclude_fields_reason` with a short rationale reviewers can audit.

## 4) Examples

Strict-equality (no `compare` block) — preferred default:

- `tests/parity/scripted/cultist-empty-turn-cycle-seed0.json`
  - The qct-era intent exclusions were retired once the deterministic
    `cultist_dark_strike_loop` AI was confirmed not to consume RNG bits
    (the loop yields `Attack(damage=6, hits=1)` every turn). The fixture
    now passes strict byte-equality and serves as the canonical "no
    `compare` block" template.

Legitimate divergence (RNG-driven intent roll) — exclusion required:

- `tests/parity/scripted/jawworm-rng-divergence-seed0.json`
  - JawWorm's intent selection consumes RNG bits, so Python's Mersenne
    Twister and Rust's Xoshiro256++ produce different intent triples after
    the first `end_turn`. The fixture excludes
    `/enemies/0/intent`, `/enemies/0/intent_damage`, `/enemies/0/intent_hits`.
  - Per §3, the exclusion list SHOULD be paired with
    `compare.exclude_fields_reason`; this fixture predates that convention
    and may be backfilled.

## 5) Optional `relics` spec slot (sts2-rs-zizk)

Specs under `tests/parity/scripted/specs/` accept an optional top-level
`relics` list naming the relics that should be installed on the player
before combat starts:

```json
{
  "name": "burning-blood-setup-only-seed0",
  "seed": 0,
  "character": "Ironclad",
  "encounter": "Cultist",
  "player": { "max_hp": 80 },
  "enemies": [],
  "relics": ["BurningBlood"]
}
```

Each entry is a relic-id string. Both the CamelCase class name
(`"BurningBlood"`) and the SCREAMING_SNAKE enum name
(`"BURNING_BLOOD"`) are accepted — the Python recorder resolves the
string via `sts2_env.relics.registry.create_relic_by_name` and writes
the relic instances into `combat.relics` (i.e.
`combat.current_player_state.relics`).

The on-disk bridge-state shape produced by
`combat_state_to_bridge_state` does NOT surface relics: Python's
exporter at pin `01c094d` omits them (`bridge_replay.py:344-374`), and
the Rust mirror in `crates/sts2-core/src/parity.rs` matches. Relics
influence the trace only through their listener side-effects (HP,
block, draw, …). The `relics` slot exists so event-reward / starter
relic parity fixtures (downstream of `sts2-rs-46ms`) can install the
relic on both sides before the action log runs.

When extending the harness with a fixture that uses this slot, the
Rust-side reconstruction is responsible for pushing the matching
`Relic { kind, .. }` records onto `state.relics` before invoking
`combat_state_to_bridge_state`; see the `zizk_relic_install` test
module in `crates/sts2-core/tests/parity_harness.rs`.

## 6) Validator + Consumer

Validation/enforcement locations:

- Recorder validation (historical): the since-deleted `scripts/record_fixture.py`
  validated `compare.exclude_fields` shape at emit time; the recorder is retired
  and fixtures are frozen (see `../../PYTHON_SIM_PIN.md`).
- Harness consumption: `crates/sts2-core/tests/parity_harness.rs`
  (`pretty_diff_excluding`) applies exclusions during state comparison.

Both sides should remain in sync with this document.

## 7) Cross-References

- Plan v2.1 refined, Phase-3 done-when target:
  `docs/plans/2026-05-06-rust-porting-plan-v2.1-refined.md` line 102.
- Cross-language RNG note:
  `crates/sts2-core/src/monsters.rs` lines 22-29.
- CASS decomposition lineage and finding trail:
  entry `b-mouew62r-oztlwe` (finding 7 context for fixture-format docs).
