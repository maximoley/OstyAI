# Hint-sidecar wire-schema version policy

Companion to [`sts2-hint-state-schema.json`](sts2-hint-state-schema.json),
the MCTS hint integration plan at
[`plans/2026-05-14-sts2-cli-mcts-hint-integration.md`](plans/2026-05-14-sts2-cli-mcts-hint-integration.md),
and the divergence note in
[`../tests/parity/known_divergences.md`](../tests/parity/known_divergences.md)
§"2026-05-14 mcts-hint-sidecar-rng-divergence".

## What the wire version is

The hint wire format is a Python `dict` produced by sts2-cli's
`adapter.py` and consumed by `sts2_rs.state_from_dict()`. Every dict
carries a top-level `wire_version: u32` field. The current value is
**1**, defined at:

- `crates/sts2-py/src/state_dict.rs` → `pub const WIRE_VERSION: u32 = 1;`

`REQUIRED_TOP_KEYS` in the same file enumerates the keys that *must*
be present alongside `wire_version`.

## When to bump

| Change | Bumps `WIRE_VERSION`? |
|---|---|
| Add a new **optional** field | No — adapters that omit it stay forward-compatible. |
| Add a new **required** field | Yes — older clients can't satisfy the new contract. |
| Rename a field | Yes. |
| Change the type of an existing field (e.g. `i32 → i64`, list → dict) | Yes. |
| Remove a field | Yes. |
| Change semantics of a field's value space (e.g. enum variants renamed) | Yes. |
| Tighten validation (e.g. start rejecting a value previously accepted) | Yes — old clients that emitted that value would start failing. |

Pure **additions of optional fields** never bump the version: a
newer `sts2_rs` reads them when present, defaults them when absent,
and rejects nothing.

## Detection path

`state_from_dict` validates `wire_version` against the constant in
`state_dict.rs`. The current check (see `parse_state_dict`):

```rust
let wire_version = extract_u32(d, "$", "wire_version")?;
if wire_version != WIRE_VERSION {
    return Err(unsupported_state_error(
        "$.wire_version",
        &format!("expected {WIRE_VERSION}, got {wire_version}"),
    ));
}
```

A mismatched version surfaces as a Python `UnsupportedStateError`
whose message contains **both** the expected and the received
version, so the client can log a useful error.

## Graceful degradation in sts2-cli

When sts2-cli's hint integration catches `UnsupportedStateError`
during a session start (or the first decision point), it must:

1. Log the mismatched versions and the path (`$.wire_version`).
2. Disable the hint sidecar for the rest of the session.
3. Continue the combat normally — C# `Sts2Headless` is authoritative
   and never depended on hints in the first place.

Per-decision `UnsupportedStateError`s (e.g. unknown card id during a
mid-combat schema migration) follow the same disable-for-session
policy; we do not selectively retry.

## Bump procedure (checklist)

When introducing a non-additive change:

1. **Bump the constant.** Edit `crates/sts2-py/src/state_dict.rs`:
   set `WIRE_VERSION` to the next integer.
2. **Update the parser.** Update `parse_state_dict` and any
   `REQUIRED_*_KEYS` to require / validate the new (or
   renamed / retyped) fields.
3. **Re-dump the schema artifact.** Run the existing example that
   serialises the schema to `docs/sts2-hint-state-schema.json`
   (filed under hint-T5, bead `sts2-rs-lmg6`) and commit the
   regenerated JSON.
4. **Update sts2-cli.** Update `python/sts2_hint/adapter.py`
   (filed under hint-T7, bead `sts2-rs-lv9d`) to emit the new
   schema. Bump any local pin / constant it carries.
5. **Add a mismatch test.** Extend
   `crates/sts2-py/tests/state_from_dict.rs` (or whichever test
   module exercises `state_from_dict`) with a fixture that sends
   the *previous* version and asserts the
   `UnsupportedStateError` mentions both versions in its
   message. This guards the negotiation contract going forward.
6. **Cross-link.** Reference the bump in this file, in
   `tests/parity/known_divergences.md` if the bump is observable
   to users, and in the sts2-cli README §"MCTS hint integration
   (optional)".

## Why versioned, not feature-flagged

The hint sidecar is a tightly-coupled producer-consumer pair: the
sts2-cli adapter and the `sts2_rs` parser ship in lock-step from the
same repo pair. A monotonically-incremented integer is sufficient and
keeps the error path trivially diagnosable. We do **not** plan a
capability-negotiation handshake; if that ever becomes necessary, it
would be the trigger to revisit this doc.
