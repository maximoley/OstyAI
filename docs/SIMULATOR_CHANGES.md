# Simulator and search modifications

[Documentation guide](README.md) · [Project overview](../README.md)

The foundation is Thomas Friedel's `tfriedel/spire-mentor` Rust simulator, not a simulator written from scratch for OstyAI. Original licenses and notices are retained. `provenance/upstream-differences.json` inventories this snapshot against the recorded upstream checkout commit.

OstyAI adds a native GPU queue and NUM1 observation path, Gumbel sequential-halving root search (up to eight considered actions), a policy/value transformer evaluator, replay export, independent potion-cost terminal utility, tier budgets, forced-action compression, detailed search statistics and the Python learner/validation pipeline.

Belief search samples uncertain transitions from the Rust simulator. Chance outcome widening and cached outcomes allow revisiting earlier outcomes and searching deeper. Forced chains are folded into edges, preserving chance/RNG effects. Search has no fixed depth limit; played-combat action limits are separate. Current persistent search reuses a bounded graph across decisions within a combat and invalidates it on actor change. It still spends the full new per-decision simulation budget. See `native/crates/sts2-py/src/adversarial/` for exact algorithms and `provenance/changes/changes-20261026-tree-reuse/README.md` for reuse invariants and measured tradeoffs.

## Simulator repairs carried forward

- Queen companion death recognizes a dead TorchHeadAmalgam even if a historical counter was absent.
- Fixture import migrates recognized obsolete opening moves for live Skulking Colony, Haunted Ship and Axebots; preserves HP, powers and RNG; rejects unsupported live AI instead of silently using Attack(5). Reconstruction uses ascension-aware AI.
- Already-dead enemies retain harmless historical move identifiers without being treated as invalid live openings.
- Inner autoplay/draw choice continuations finish before outer turn setup resumes (Knowledge Demon / Hellraiser / Sculpting Strike regression).
- Out-of-phase EndTurn is rejected instead of creating a no-op forced-chain loop.
- Tyranny and ToolsOfTheTrade park turn-start continuation only when a real pending choice exists; lethal Rolling Boulder reaches the combat-end tail.

Detailed before/after regressions, isolated Rust test reports, deployment hashes, smoke panels, checkpoint-preservation and resume evidence are in `provenance/changes`. These historical reports are evidence from production deployments, not a claim that every test was rerun during packaging. The package's newly executed checks are listed separately in `provenance/package-verification.json`.

5,353 replay examples from diagnosed fallback encounters were quarantined at step 4242; subsequent production recovery preserved the remaining replay and model/optimizer/RNG state. This repository contains neither the quarantined data nor any production replay checkpoint.

Packaging changes are limited to relocatable runtime/build paths, environment overrides, standalone evaluation/check scripts, and documentation. No production training process or live binary was replaced to build this package.
