# Known parity divergences — pointer

The single source of truth for accepted parity divergences between the Rust
simulator (`sts2-rs`) and the Python reference simulator
(`sts2-rl-agent` at the pin recorded in `PYTHON_SIM_PIN.md`) lives at:

- **`tests/parity/known_divergences.md`**

That file is the one the `crates/sts2-core/tests/known_divergences_lint.rs`
schema lint validates. Bead `sts2-rs-pmf` (P6-1b) pinned the typed-manifest
schema (`Status:` / `Bead:` / `Expires:` fields). New entries go in
`tests/parity/known_divergences.md` and must satisfy the lint.

This pointer file exists because bead `sts2-rs-7ez` (P7-5b) was authored
against an earlier draft that referenced `docs/known_divergences.md` —
keeping the pointer here so future readers don't end up in two places.
