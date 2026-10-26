# Changelog

All notable changes to **sts2-rs** are documented here. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **STS2 act1_weak monster wiring + 354 card registry expansion** (`sts2-rs-yioi` H3, `sts2-rs-pjln` H4). Two parallel content-surface fixes:
  - **Monsters (H3):** 6 real STS2 monsters from `sts2_env/monsters/act1_weak.py` are now selectable via `--monsters` / `--encounter`: `shrinker-beetle` (HP 38-40), `fuzzy-wurm-crawler` (HP 55-57), `leaf-slime-s` (HP 11-15), `twig-slime-s` (HP 7-11), `leaf-slime-m` (HP 32-35), `twig-slime-m` (HP 26-28). The DSL factories in `sts2-content::monsters::act1_weak` already existed but were unreachable; H3 adds `MonsterKind` variants + `monster_ai()` dispatch + `SELECTABLE_MONSTERS` entries. `resolve_encounter` also gained a single-monster fallback so any tag listed in `SELECTABLE_MONSTERS` works as `--encounter <tag>` without a hand-coded alias.
  - **Cards (H4):** every auto-generated `CardSpec` factory across `sts2-content/src/cards/*_specs_generated.rs` is now registered in `deck_card_spec_pool` — Ironclad 86 + Silent 88 + Defect 87 + Colorless 18 + Status 75 = **354 cards** total selectable via `--deck` and surfaced in `list cards` (up from 30). New `spec_index.rs` aggregates the per-class factory slices; the registry merge prefers curated entries on `CardId` collision so hand-rolled Phase-4 listener bindings (Strike/Defend/Bash) aren't overridden by their generated POD specs. `PlayerClassHint::Defect` added for class-locked random decks. Caveat: some `play_*` handlers are conservative-marker stubs that no-op when played (per the 2026-05-12 reality-check §G3); `list content` calls this out.
  - **Follow-up tracker (`sts2-rs-78ks`):** the remaining ~117 STS2 monster DSL factories across `act1/act2/act3/act4/shared` are still unreachable from the engine — the closed P5 umbrella `sts2-rs-ux2` shipped the DSL bodies but not the per-act `MonsterKind` wiring.
- **`sts2-cli combat --monsters` + `--encounter random-N`** (`sts2-rs-45xt`, cliext wave H2). Compose ad-hoc encounters from the 6 implemented monster kinds without going through the alias table or writing a scenario file. `--monsters cultist,sentry,green-louse` builds a 3-monster fight inline; the tag parser is dual-form (same as the card/relic name lookups — `Cultist`, `acid_slime_s`, `green louse` all work). `--encounter random-N` picks N monsters (1..=5) uniformly from the implemented pool using the existing `SALT_ENCOUNTER` fork (seed-reproducible). `--encounter random-pair` / `random-triple` are convenience aliases. `--monsters` and an explicit `--encounter` are mutually exclusive; unknown tags surface a did-you-mean suggestion.
- **`sts2-cli list` — implemented-only by default, `--all` for full enum, new `list content` summary** (`sts2-rs-ef4x`, cliext wave H1). `list cards` previously dumped all 579 `CardId` enum tokens, of which only 31 actually have factories; same problem for relics (72 of 289 seedable) and potions (57 of 63 usable in combat). The default output now filters to the implemented surface and prepends a one-line `# <kind>: N of M implemented` header so the porting gap is visible. `--all` restores the full dump. A new `list content` subcommand prints the cross-cutting inventory (monsters / cards / relics / potions / encounters) with the same counts plus a pointer to the phase that's still in flight for each.
- **`sts2-cli combat` — MCTS move-suggestion hint** (`sts2-rs-s3tb`, cliext wave G1). The interactive REPL gained a `hint [SIMS]` verb that runs `sts2-mcts` on the current state (default 1000 sims) and prints the top-3 root actions ranked by visit count with `Q`-value estimates — the game state is cloned, not advanced. The new `--mcts-hint-default <SIMS>` flag auto-runs the hint at the top of every player decision (off by default). Two knobs surface alongside it: `--mcts-c-puct <F>` (UCT exploration constant, default `sqrt(2)`) and `--mcts-rollout-depth <N>` (rollout cap, default 200). `help` lists the new verb. Lets a human player use the CLI as an MCTS-guided sandbox.
- **`sts2-cli combat --random-deck-character` + `--random-deck-size`** (`sts2-rs-9yoh`, cliext wave F4). The `--deck-preset random` and `--random-all` paths now produce class-locked decks of configurable size instead of a fixed-size mix of every character's cards. `--random-deck-character <ironclad|silent>` pins the class; when omitted, the class is itself drawn from `--seed` via a new `SALT_CLASS` fork (`random_class()`) so a single seed still reproduces the full scenario. `--random-deck-size N` (default 10, bounded 1..=200) replaces the previous hardcoded `RANDOM_DECK_SIZE = 10`. When `--random-deck-character` is auto-drawn (no explicit user pin AND no explicit `--player`), `--player` is realigned to match the deck class so the printed banner and the per-class HP/starter-relic seeding stay consistent.
- **`sts2-cli combat --print-scenario`** (`sts2-rs-92sq`, cliext wave E1). Resolve the scenario (CLI flags + optional `--scenario` file merge) and dump it to stdout as a `schema = 1` TOML block, then exit 0 without running combat. The output is a strict subset of the loader format so it round-trips through `--scenario <file>` — a `cargo test` pins byte-identical idempotence for both relic-only and deck-bearing dumps.
- **`sts2-cli list <kind>` — full implementations** (`sts2-rs-92sq`, cliext wave E1). The four arms now print one row per registered id as `<PascalCase>  "<display string>"`:
  - `list encounters` → the alias table from cliext B2 (9 entries today: 6 single-monster + 3 multi-monster groups).
  - `list cards` → 579 rows (every `CardId` variant in declaration order).
  - `list relics` → 289 rows (every `RelicId` variant).
  - `list potions` → 63 rows (every `PotionKind` variant).
- **README — "Flexible combat exploration" section** documenting the cliext combat / list subcommand layout, all combat flags (deck / relic / potion preset + inline + file forms, `--scenario`, `--print-scenario`, `--encounter`), the scenario-file format, and the dual-form name-acceptance contract.

### Changed

- **`sts2-cli` — breaking invocation change** (`sts2-rs-9nwp`, cliext wave A2). The CLI is now subcommand-shaped: every flag that used to live on the bare binary now sits under `sts2-cli combat ...`. A new `sts2-cli list <encounters|cards|relics|potions>` subcommand was added — stubbed at A2, filled in at E1 (`sts2-rs-92sq`). Bare `sts2-cli` with no subcommand exits 2 with clap help. Scripted-replay / determinism tests and the fixtures README were updated to prepend `combat` to every invocation.

## [0.1.0] — 2026-05-12

First tagged release. Phase-0 through Phase-8 of the v2.1 porting plan complete; single-threaded engine + MCTS ship-ready, with the Ironclad-vs-Cultist vertical slice as the canonical perf workload.

### Added

- **Phase 0–1 — Foundations.** Workspace scaffold (`sts2-core`, `sts2-content`, `sts2-py`, `sts2-bench`), toolchain pin + workspace lints, JSON parity exporter matching the Python sim's bridge-replay shape, CardStore + pile primitives, 47 hook signatures + `ListenerKind` + `HookId`, PyO3 skeleton, GitHub Actions CI, and the `tests/parity/` scaffolding with `known_divergences.md`.
- **Phase 2 — Damage + powers.** `apply_damage` with the `modify_hp_lost` chain, damage / block pipeline with rational `DamageMul`, 10 core player-side powers, enemy-side power equivalents, listener-mutation refactor via the deferred-effect queue, 1k-seed Cultist fuzz validation gate.
- **Phase 3 — Turn loop.** Player turn-start, player end-turn pipeline, enemy turn-start / turn-exec / turn-end with Vuln/Weak/Frail tick-down, `check_combat_end` + `Action::EndTurn` dispatcher, multi-state intent rolls via persistent `IntentHistory`.
- **Phase 4 — Vertical slice.** Ironclad starter deck content, stub monster system + 5 Act-1 enemies, Burning Blood relic + `RelicKind`/`RelicSpec` scaffolding, random-playout fuzzer (10k seeds × 5 encounters), first MCTS demo via PyO3 rollouts.
- **Phase 5 — Monster DSL.** `MonsterAi` DSL skeleton + 24 monster `PowerKind` variants; 29 Act-1, 27 Act-2, 28 Act-3, 22 Act-4, 18 shared/event/boss monsters ported (106 total); `MonsterId` + `MonsterAi` fields on `Creature`; multi-enemy fuzz harness + 25 parity fixtures + `monster_intent_roll` bench.
- **Phase 6 — Card + power coverage.** Auto-generated `CardId` (579 variants) and `PowerKind` (267 variants) enums via `scripts/scrape_*_ids.py`; `PlayCard` dispatch registry; Strike/Defend/Bash card ports with Vulnerable application; card POD extended with `combat_vars` + `enchantments` fields; 21-variant card-enchantment system + persistent card state. `sts2-cli` scaffolded (lightspeed-parity REPL) + Nibbit encounter.
- **Phase 7 — Relics, ascension.** Auto-generated `RelicId` enum (289 variants); `ascension` field on `CombatState`; relic dispatch registry + 3 starter relics with exhaustive tests.
- **Phase 8 — MCTS + perf + Python surface.**
  - New `sts2-mcts` crate with single-threaded UCT search (`Mcts` + `MctsConfig`), 1000-sim default, configurable rollout depth.
  - Two criterion benches: `mcts_smoke` (searches/sec) and `transitions_per_sec`. `bench.yml` (then at `.github/workflows/`, now archived at `docs/ci-templates/bench.yml`) ran them on every push.
  - PyO3 surface complete: `legal_actions`, `state_hash`, `pending_choice`, `terminal_value`, plus a `PyMcts` wrapper around the Rust MCTS.
  - `state_hash` module (`crates/sts2-core/src/hashing.rs`) — stable serialized hash for transposition / parity / RL replay.
  - Profile-driven optimization pass: `scripts/flamegraph.fish` runbook, `mimalloc` feature flag on `sts2-core` (default off; behind a `--features mimalloc` gate for allocator experiments), full perf rationale in the maintainers' perf notes.

### Performance

Measured on a typical developer workstation (Linux 6.8, glibc, single-threaded). Reproduce with `cargo bench -p sts2-bench`:

- **2.67 M state transitions/sec** — `transitions_per_sec/endturn_full_combat`. Comfortably above the v2.1 plan's ≥1 M transitions/sec gate.
- **~190 k combats/sec** (derived; ~14 transitions per random-play combat).
- **161 searches/sec** for MCTS at 1000 simulations/move on Ironclad-vs-Cultist — `mcts_smoke/ironclad_vs_cultist_1000sims`. This is the algorithmic ceiling for single-threaded UCT on this workload.

### Known limitations (deferred to post-v0.1.0)

- **MCTS throughput** is single-threaded UCT at 161 searches/sec. Parallel rollouts (root + tree parallelization), virtual-loss, leaf-eval policy, and transposition tables are deferred — tracked in bead `sts2-rs-apl` ("P8+: Parallel rollouts + leaf-eval policy for >1000 searches/sec MCTS").
- **`legal_actions` potion surfacing** is untargeted; the P7 targeted-potion predicate (which potions can be played and against whom, given the current state) is deferred.
- **Card / power / monster content** ships starter-Ironclad-deck + the Act-1 vertical slice as the perf-validated path. Remaining content beads still open after v0.1.0:
  - `sts2-rs-2un` — P6 umbrella: remaining 245 powers + 577 cards + four-character coverage.
  - `sts2-rs-fxw` — P6-10: character-coverage matrix + 10k Act-1 fuzz + perf bench (≥80k combats/sec).
  - `sts2-rs-h9u` — P7 umbrella: relics, potions, orbs, enchantments, afflictions, ascension (only 3 starter relics + the dispatch registry shipped in v0.1.0).
  - `sts2-rs-spy` — P7-7: orbs (Defect mechanic) + Channel/Evoke action + Focus synergy.
  - `sts2-rs-d19` — P7-8: enchantment dispatch audit + Affliction support + 10k Act-1 fuzz × ascension × char.
  - `sts2-rs-3li` — P8 umbrella: MCTS integration + final perf pass (closed by this release).
- **`sts2-cli` v1.1 follow-ups** — `sts2-rs-ag8` (replace local starter-card-specs with `sts2-content`) and `sts2-rs-e9s` (seed-0 long-determinism JSONL digest fixture).
- **Wheel publication** — intentionally out of scope (private project, no public PyPI publication). Use `maturin develop` from the rl-agent venv for editable installs, or `maturin build --release` from `crates/sts2-py` for an optional local wheel in `target/wheels/`.

[0.1.0]: https://github.com/thomasf/sts2-rs/releases/tag/v0.1.0
