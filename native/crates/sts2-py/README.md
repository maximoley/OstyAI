# sts2-py

PyO3 bindings for the `sts2-core` combat engine. Built locally with
[maturin](https://github.com/PyO3/maturin) and consumed by the sister
Python projects on the dev box. No PyPI publication — this is a
private workspace.

## Why no PyPI

There is intentionally no public release pipeline:

- `sts2-rs` is a private project; the workspace policy is "no public
  PyPI publication" (see `docs/reality-checks/2026-05-12-gap-report.md`
  §G11 and the bead `sts2-rs-3lo` re-scope).
- The only consumers — `../sts2-rl-agent/` and `../sts2-pycli/` — live
  next to this repo on the same workstation, so a path-based editable
  install is faster, simpler, and always in sync with the current
  Rust commit.
- No `release.yml`, no `maturin publish`, no PyPI tokens. The
  archived CI templates under `docs/ci-templates/` (formerly
  `.github/workflows/`, inert in the monorepo — see that
  directory's README) deliberately do **not** reference
  `upload.pypi.org`.

If that ever changes, swap the path-based install in the consumers
for a normal `dependencies = ["sts2-rs>=X"]` entry and add the
publish step then — not before.

## Build & install (the supported path)

The supported install is `maturin develop --release` from inside the
target Python project's venv. From the rl-agent venv:

```fish
# one-time, in the consuming venv:
uv pip install maturin

# rebuild + install into the active venv as an editable extension:
cd ../sts2-rs/crates/sts2-py
maturin develop --release

# verify:
python -c "import sts2_rs; print(sts2_rs.version())"
```

`maturin develop` compiles the `cdylib` and drops the resulting
`sts2_rs.<abi>.so` directly into the active venv's `site-packages/`,
so `import sts2_rs` works alongside the consumer's own packages
(e.g. `import sts2_env` in rl-agent). No wheel file is written to
disk in this mode.

Re-run `maturin develop --release` whenever you change Rust code that
the Python side imports — there is no auto-rebuild hook. See
`../sts2-pycli/README.md` "Stale wheel after a Rust-side bump" for the
force-clean recipe if `import sts2_rs` keeps loading an old build.

## Optional: build a local wheel

If you want a portable artifact (e.g. to copy to another machine, or
to keep an installable snapshot of a specific commit), build a wheel
without installing it:

```fish
cd crates/sts2-py
maturin build --release
# wheel lands in ../../target/wheels/sts2_rs-*.whl
```

Install the produced wheel into any venv with
`pip install target/wheels/sts2_rs-*.whl` (or
`uv pip install …`). This path is **optional** — `maturin develop`
remains the contract for day-to-day work.

## How the sister projects pick it up

- **`../sts2-pycli/`** (the C# `Sts2Headless` host with a Python MCTS
  hint sidecar) declares the dependency as a path-based editable
  install in its `pyproject.toml`:
  ```toml
  [tool.uv.sources]
  sts2-rs = { path = "../sts2-rs/crates/sts2-py", editable = true }
  ```
  Its `setup.sh` runs `maturin develop --release` against the active
  venv (skip the Rust step with `STS2_SKIP_RUST=1`).
- **`../sts2-rl-agent/`** runs `maturin develop --release` from this
  crate inside its training/eval venv — there is no path entry in its
  `pyproject.toml`; the `sts2_rs` extension just appears in
  `site-packages/` after the develop call and is imported directly.

In both cases the install is editable in the maturin sense (the venv
points at this source tree) but **not** auto-rebuilding — Rust
changes require a fresh `maturin develop --release`.

## Crate layout

- `src/` — `#[pymodule]` surface (`legal_actions`, `state_hash`,
  `pending_choice`, `terminal_value`, `PyMcts`, …).
- `pyproject.toml` — maturin build config; `module-name = "sts2_rs"`,
  `features = ["pyo3/extension-module"]`.
- `Cargo.toml` — `crate-type = ["cdylib", "rlib"]`. The `rlib` is so
  the in-tree integration tests under `tests/` can link against the
  crate without `extension-module`.
- `examples/`, `tests/` — Rust-side smoke tests for the bindings.
