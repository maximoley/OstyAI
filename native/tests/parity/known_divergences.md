# Known parity divergences

This file is the single source of truth for accepted parity divergences between
the Rust simulator (`sts2-rs`) and the Python reference simulator
(`sts2-rl-agent` at the pin recorded in `PYTHON_SIM_PIN.md`). It is populated
only when fuzz finds a real divergence that is verified, shrunk, and either
deferred to a later phase or accepted as a permanent design difference.

The file is intentionally **empty until the first divergence is recorded**.
That is the desired baseline.

## Triage rule (per the v2.1 plan)

When fuzz uncovers divergences during a phase:

- **≤5 divergences** — phase ships and each entry is recorded here.
- **6–20 divergences** — phase ships, but a `fuzz-debt` issue is filed against
  the next phase's bead set to drive them back to zero.
- **20+ divergences** — phase is blocked; stop and resolve before advancing.

See `docs/plans/2026-05-06-rust-porting-plan-v2.1-refined.md` §"Failure-shrinking
loop" and `tests/parity/README.md` for the workflow that produces entries here.

## Schema

Bead `sts2-rs-pmf` (P6-1b) tightened the schema to a typed manifest so the
`crates/sts2-core/tests/known_divergences_lint.rs` lint can mechanically
re-triage stale entries. Each divergence is one entry in this exact shape:

```markdown
## <YYYY-MM-DD> <slug>

- **Fixture:** tests/parity/shrunk/<file>.json
- **Diff:** <one-line summary>
- **Python source:** sts2-rl-agent@<sha>:sts2_env/cards/effects/reaper.py:42
- **Status:** investigated | accepted-permanently | deferred-to-bead-<id>
- **Bead:** sts2-rs-<id>    # required iff Status starts with "deferred-"
- **Expires:** YYYY-MM-DD    # required for "investigated" or "deferred-"
- **Rationale:** <prose>
- **Resolution:** <prose>
```

## Entries

(none — `player-powers-exporter-stub` was resolved on 2026-05-06 alongside the Wave-B integration commit; the exporter now emits real `player.powers` entries via the same `_normalize_powers`-shaped filter used for enemies. All 6 previously-ignored byte-equality tests in `parity_harness.rs::wave_a_fixtures` now pass.)

## 2026-05-12 event-reward-relic-stub-handlers

- **Fixture:** _(none — purely a stub-coverage divergence; no parity fixture exists yet because event-spawn hooks aren't wired)_
- **Diff:** All 35 `RelicRarity.EVENT` relics from `sts2-rl-agent@01c094d:sts2_env/relics/shop_event.py` are registered against the dispatch table with marker `EffectResult::Resolved` handlers. Real per-relic behaviour (`after_obtained` heals/max-HP gains, `on_card_added_to_deck` duplication, `modify_shop_price` discounts, pet summons, etc.) is not yet wired — Rust runs that hold these relics behave as if the relic had no effect.
- **Python source:** `sts2-rl-agent@01c094d:sts2_env/relics/shop_event.py` (per-class listener methods on `BigMushroom`, `BingBong`, `BoneTea`, `Byrdpip`, `ChosenCheese`, `DarkstonePeriapt`, `DaughterOfTheWind`, `DreamCatcher`, `EmberTea`, `FakeAnchor`, `FakeBloodVial`, `FakeHappyFlower`, `FakeLeesWaffle`, `FakeMango`, `FakeMerchantsRug`, `FakeOrichalcum`, `FakeSneckoEye`, `FakeStrikeDummy`, `FakeVenerableTeaSet`, `ForgottenSoul`, `FragrantMushroom`, `FresnelLens`, `HandDrill`, `HistoryCourse`, `LostWisp`, `MawBank`, `MrStruggles`, `PollinousCore`, `RoyalPoison`, `SwordOfStone`, `SwordOfJade`, `TeaOfDiscourtesy`, `TheBoot`, `WongoCustomerAppreciationBadge`, `WongosMysteryTicket`).
- **Status:** deferred-to-bead-sts2-rs-01o
- **Bead:** sts2-rs-01o
- **Expires:** 2026-07-08
- **Rationale:** Per bead `sts2-rs-7ez` P7-5b scope, this bead is the pre-split-B of `shop_event.py` and only ports the **dispatch surface** (no `NotImplemented` for any of the 35 `RelicRarity.EVENT` `RelicId`s). Concrete plumbing requires Phase-7 surfaces (`Creature.gain_max_hp`, `Creature.duplicate_last_added_card`, `combat.shop_price`, `RunState.relic_rewards`, pet-system bus, deck-shuffle hook fan-out). The complementary `RelicRarity.ANCIENT` + `RelicRarity.SHOP` entries from the same Python file are owned by sibling bead `sts2-rs-yyw` (P7-5).
  - **2026-05-13 audit update:** `sts2-rs-d19` closed without delivering this work (its actual scope was the enchantment dispatch audit + Affliction support + 10k Act-1 fuzz). The `sts2-rs-n3w` umbrella was filed to redo the analysis; sub-beads `my8` (audit doc), `jlc` (combat surfaces), `bms` (runstate surfaces), and the four other P7-surf beads (`2ax`/`tcf`/`anx`/`62t`/`txd`/`3f8`) have all landed. The audit at `docs/reality-checks/2026-05-12-event-reward-audit.md` reclassified the 35 relics (Easy 12 / Medium 12 / Hard 11), corrected ~20 stale Rust doc-comments (separate session, doc-fix-only pass landed 2026-05-13), and surfaced a follow-up gap: the new `relic_effects::dispatch_*` hook channels are bus-only — `combat_loop` still calls the older `hooks::dispatch_*` family for `before_combat_start`, `after_combat_end`, `after_obtained`, `after_card_played`, etc. A runtime-wiring bead must connect the two before listener-style retrofits become observable.
- **Resolution:** `sts2-rs-01o` walks the audit's Easy/Medium/Hard tiers in waves, replacing each `on_trigger_<relic>` marker body with the real per-relic listener registrations. Per-relic dispatch tests stay green throughout (they assert `EffectResult::Resolved`, not behaviour); the umbrella retrofit adds parity fixtures asserting the wired-up behaviour against `CombatState` / `RunState`. **Pre-requisite:** the runtime-wiring follow-up bead (filed 2026-05-13) must land first — it teaches `combat_loop` / `RunState` to fire the `relic_effects::dispatch_*` channels alongside `hooks::dispatch_*`.

## 2026-05-14 mcts-hint-sidecar-rng-divergence

- **Fixture:** _(none — sidecar-only divergence; rolled-out hint scores are advisory, not state replays. No parity fixture is meaningful because the C# engine is authoritative for actual game state.)_
- **Diff:** When `sts2-rs` is used as the MCTS **hint sidecar** for `sts2-cli` (driven by the C# `Sts2Headless` engine), rollouts are seeded from `state.seed + turn` and consume their own RNG cursor. The real C# engine has an independent RNG stream (card-draw / shuffle / enemy-intent / crit rolls). Therefore: rollouts launched from a wire-injected state do **not** reproduce the live game's outcome distribution. Win-probability percentages emitted by `mcts_recommend(...)` are useful for **ranking** the legal actions at the current decision point but must **not** be interpreted as absolute win-rate predictions for the in-progress combat.
- **Python source:** _n/a — this is a Rust ↔ C# divergence, not Rust ↔ Python. The wire schema lives at `crates/sts2-py/src/state_dict.rs` (constant `WIRE_VERSION`, current value 1) and is documented at `docs/sts2-hint-state-schema.json`._
- **Status:** accepted-permanently
- **Rationale:** Synchronising RNG streams between Sts2Headless (C#) and sts2-rs (Rust) is out of scope for the hint-sidecar integration — see decisions table in `docs/plans/2026-05-14-sts2-cli-mcts-hint-integration.md` §"Non-goals" and §"Architectural decisions". The integration explicitly accepts rollout divergence in exchange for keeping the C# engine authoritative and avoiding a full RNG cross-port. Hint UX surfaces win-prob as a *ranking signal* (best/second-best action), never as a survival forecast; sts2-cli copy ("MCTS suggests …") and the MCP `recommend_combat_mcts` tool description reflect this. See also `docs/schema-version-policy.md` for the wire-version negotiation policy that gates the integration, and the sts2-cli README "MCTS hint integration (optional)" section for the user-facing description.
- **Resolution:** Documented here as an accepted permanent divergence. No further action is planned unless MegaCrit exposes an RNG-state extraction API on the C# side, at which point a separate bead would port the RNG state across the wire and replay rollouts deterministically. Cross-links: `crates/sts2-py/src/state_dict.rs`, `docs/sts2-hint-state-schema.json`, `docs/schema-version-policy.md`, `docs/plans/2026-05-14-sts2-cli-mcts-hint-integration.md`, sts2-cli `README.md` §"MCTS hint integration (optional)".

## 2026-05-15 starter-relic-behavior-no-parity-fixture-yet

- **Fixture:** _(none — listener-level divergence; per-channel Rust unit tests in `crates/sts2-core/src/relic_effects.rs` cover the four starter relic behaviors but no Python-vs-Rust scripted JSON fixture exists yet.)_
- **Diff:** Bead `sts2-rs-6ynt.1` rewired the four canonical Ironclad-reachable starter relics (`BurningBlood`, `CrackedCore`, `RingOfTheSnake`, `DivineRight`) plus `BoundPhylactery` off `RelicEffect::Noop` onto real `RelicEffect` AST variants and registered per-channel listeners that mutate `CombatState` (heal, channel Lightning orb, modify hand-draw, gain stars). The Python pin's matching `RelicInstance` subclasses fire on the same dispatch channels, but the parity-fixture runner cannot currently drive a scripted post-victory round-trip (no `RunState`-level relic-pickup surface in Rust at `01c094d`, no Necrobinder `summon_osty` runtime, no orb-emit count in the parity wire schema). Rust-side unit tests verify the listener behavior end-to-end; Python parity is asserted only at the dispatcher level (NotImplemented for un-owned, Resolved for owned).
- **Python source:** `sts2-rl-agent@01c094d:sts2_env/relics/starter.py` lines 25-33 (BurningBlood), 53-62 (RingOfTheSnake), 83-93 (CrackedCore), 113-125 (BoundPhylactery), 148-157 (DivineRight).
- **Status:** deferred-to-bead-sts2-rs-6ynt.1
- **Bead:** sts2-rs-6ynt.1 (parent), sts2-rs-s16y (setup-only follow-up landed 2026-05-17)
- **Expires:** 2026-08-15
- **Rationale:** Scripted parity requires the run-builder to materialize a `RelicInstance` on the Python side and a Rust-side `Relic` POD in `state.relics` with matching uids, then run a tiny combat to victory and snapshot HP/orbs/stars/etc. The umbrella `sts2-rs-01o` (P7 relic surfaces) and `sts2-rs-j87` (dispatch wiring) already cover the dispatch invariants; behavioral parity is best deferred to the run-builder integration bead so the fixture exercises the real pickup-time materialization path rather than synthesizing a `Relic` row in test setup.
- **Resolution:** `sts2-rs-s16y` (2026-05-17) landed **setup-only** parity fixtures for all four remaining starter relics (`cracked-core-setup-only-seed0`, `ring-of-the-snake-setup-only-seed0`, `divine-right-setup-only-seed0`, `bound-phylactery-setup-only-seed0`) following the `zizk` BurningBlood pattern: Python recorder validates registry lookup + `RelicInstance` instantiation, Rust replay validates the analogous `state.relics` push, byte-equality holds on the bridge envelope (since bridge schema omits relics by design at pin `01c094d`). Full **behavioral** fixtures (`burning-blood-heal-after-victory-seed0.json`, `cracked-core-lightning-round1-seed0.json`, etc.) remain deferred until the run-builder + orb-emit wire-schema extensions land; file as a separate sub-bead at that point. The Rust-side unit tests in `relic_effects.rs::tests` (10 tests landed by `sts2-rs-6ynt.1`) continue to gate the listener behavior today.

## 2026-05-16 turn-end-in-hand-debt-and-skip-next-tick

- **Fixture:** _(none — listener-level divergence; per-effect Rust unit tests in `crates/sts2-core/src/combat_loop.rs::tests` (12 new tests under the `turn_end_*` prefix) cover all 8 turn-end-in-hand triggers but no Python-vs-Rust scripted JSON fixture for the curse pile exists yet — recording requires a hand-injection extension in `scripts/record_fixture.py` that the curse-spawn surface doesn't yet expose.)_
- **Diff:** Bead `sts2-rs-6ynt.3.1` ports `_execute_turn_end_in_hand_effect` (Python `combat.py:2338-2443`) for all 8 trigger CardIds (BECKON, BURN, BAD_LUCK, DEBT, DECAY, DOUBT, REGRET, SHAME). Two known divergences from the Python pin remain:
  1. ~~**DEBT** — Python mutates `combat.gold` directly (`combat.py:2404`); the Rust `CombatState` has no gold field at this pin (gold lives on `RunState`, accessible only from the run layer). The Rust handler is a documented combat-side no-op; the curse still moves Hand → Play → Discard and fires `after_card_discarded`. The actual gold-loss semantics will land alongside a run-layer integration that observes the after-card-discarded fire (or a parallel deferred-effect queue from combat → run).~~ **Resolved by `sts2-rs-3bie` (2026-05-17), corrected 2026-06-21 (parity `gold_plus10_combat`):** The 2026-05-17 fix routed the loss through a `CombatState::pending_run_gold_loss` accumulator that `RunState::apply_pending_combat_gold_loss` was meant to drain — but that drain had no production caller, so the gold loss was silently dropped (live combat gold never changed; C# decremented by 10). DEBT now mutates the live combat gold directly in the `CardId::Debt` arm of `execute_turn_end_in_hand_effect`: `lost = min(10, gold); gold -= lost`, mirroring C# `Debt.OnTurnEndInHand` (`Mathf.Min(GoldVar(10), Owner.Gold)`) then `PlayerCmd.LoseGold` (`Gold = Max(0, Gold - amount)`). The loss is visible live mid-combat and propagates to `rs.gold` via the `exit_combat` writeback. The old accumulator field and drain are now dead (TODO: remove).
  2. ~~**DOUBT / SHAME `skip_next_tick`** — Python sets `power.skip_next_tick = True` on the freshly-applied WEAK / FRAIL when the target did not previously have the power (`combat.py:2421-2423` / `2440-2442`). The Rust `Power` POD has no `skip_next_tick` field at this pin (see `hooks.rs:2302` doc-comment that introduced this divergence for the original Vulnerable / Weak / Frail port). Net: the freshly-applied WEAK / FRAIL ticks at the very next enemy turn-end (one extra tick than Python).~~ **Resolved by `sts2-rs-j0ek` (2026-05-17):** `Power.skip_next_tick: bool` field added; `apply_power_to_creature` / `apply_power_to_creature_with_applier` set it on fresh-apply for Vulnerable / Weak / Frail; new `Effect::TickPowerOrSkip` consumes the flag on first tick at `on_turn_end_enemy_side`. 5 new unit tests gate the behaviour (3 in `card_effects::tests`, 2 in `hooks::tests`).
- **Python source:** `sts2-rl-agent@01c094d:sts2_env/core/combat.py:2403-2405` (DEBT); `sts2-rl-agent@01c094d:sts2_env/core/combat.py:2417-2423,2437-2442` (DOUBT / SHAME `skip_next_tick`); `sts2-rl-agent@01c094d:sts2_env/powers/common.py:75-129` (the original Vulnerable / Weak / Frail tick where `skip_next_tick` would gate the first decrement).
- **Status:** deferred-to-bead-sts2-rs-6ynt.3.1
- **Bead:** sts2-rs-6ynt.3.1
- **Expires:** 2026-08-16
- **Rationale:** Both gaps are explicitly out of scope for `sts2-rs-6ynt.3.1`: the bead spec for the Debt arm calls out "combat may still need to fire even if run-layer completes the gold change" (i.e. accepts the combat-side no-op), and `skip_next_tick` is a `Power`-POD shape change that touches the entire power-tick listener surface (not a single-arm fix). Wiring either gap from this bead would force scope-creep into the run-layer integration / power-shape refactor respectively.
- **Resolution:** File two follow-up beads at the next session-end sync: (a) a run-layer Debt-gold bead that observes `after_card_discarded` for `CardId::Debt` and decrements `RunState.gold`, and (b) a `Power.skip_next_tick: bool` field bead that retrofits the Vulnerable / Weak / Frail tick listener to short-circuit on the flag (consumed-on-tick semantics). The Rust unit tests in `combat_loop.rs::tests` (12 turn-end tests landed by `sts2-rs-6ynt.3.1`) gate the in-scope behaviour today; both follow-ups extend or replace the relevant tests when they land.

## 2026-05-18 turret-operator-pattern-a-rebaseline

- **Fixture:** _(none — Rust deliberately diverges from the Python pin to track C# decomp v0.103.2; no fixture is meaningful while the divergence is intentional. A parity fixture will be filed once `../sts2-rl-agent` upstreams the same rebaseline.)_
- **Diff:** Rust's `TurretOperator` HP is now fixed at 41 (was `gen_range(28..=30)`) per C# decomp `MegaCrit.Sts2.Core.Models.Monsters/TurretOperator.cs` v0.103.2 (`MinInitialHp = MaxInitialHp = 41`; Tough Ascension: 51). The Python pin `01c094d` still ships 28-30; running the parity harness against that pin on any combat involving `TurretOperator` will diverge until upstream catches up. The C# 3-move chain + Fire-3 attack variant (vs Rust's current 2-move `DEPLOY_TURRET → SHOOT(9)` self-loop) is deferred per the bead description.
- **Python source:** `sts2-rl-agent@01c094d:sts2_env/monsters/act3.py` (`make_turret_operator` factory).
- **Status:** deferred-to-bead-sts2-rs-kgwj
- **Bead:** sts2-rs-tnok (this fix); umbrella `sts2-rs-kgwj` (Pattern-A monster rebaseline)
- **Expires:** 2026-08-18
- **Rationale:** Pattern-A of the 2026-05-18 monster-stat audit (`docs/perf/2026-05-18-monster-stat-audit.md`): the Python pin is older than C# v0.103.2 on this monster; the values are internally consistent on the Python side but the game moved on. Pattern-A precedent: PG (fwbs), ThievingHopper (l8yi), SlimedBerserker (6v3s), Ovicopter (7qd1).
- **Resolution:** Once `../sts2-rl-agent` upstreams the C# rebaseline (separate sts2-rl-agent bead outside this project, tracked under umbrella `sts2-rs-kgwj`), bump the `PYTHON_SIM_PIN.md` SHA and remove this entry. Until then the divergence is the deliberate, audited choice.

## 2026-05-18 thieving-hopper-pattern-a-rebaseline

- **Fixture:** _(none — Rust deliberately diverges from the Python pin to track C# decomp v0.103.2; no fixture is meaningful while the divergence is intentional. A parity fixture will be filed once `../sts2-rl-agent` upstreams the same rebaseline.)_
- **Diff:** Rust's `ThievingHopper` now matches C# decomp `MegaCrit.Sts2.Core.Models.Monsters/ThievingHopper.cs` v0.103.2 — HP 79..=84 (was 15..=18) and a 5-move chain `THIEVERY(17) → FLUTTER(Buff Flutter 5) → HAT_TRICK(21) → NAB(14) → ESCAPE → ESCAPE (self-loop)` (was a 1-move `MUG(8)` self-loop). The Python pin `01c094d` still ships the stale 15..=18 HP / 1-move MUG variant; running the parity harness against that pin on any combat involving `ThievingHopper` will diverge until upstream catches up.
- **Python source:** `sts2-rl-agent@01c094d:sts2_env/monsters/act2.py` (`make_thieving_hopper` factory + matching `effect_fn` for `MUG`).
- **Status:** deferred-to-bead-sts2-rs-kgwj
- **Bead:** sts2-rs-l8yi (this fix); umbrella `sts2-rs-kgwj` (Pattern-A monster rebaseline)
- **Expires:** 2026-08-18
- **Rationale:** Pattern-A of the 2026-05-18 monster-stat audit (`docs/perf/2026-05-18-monster-stat-audit.md`): the Python pin is older than C# v0.103.2 on this monster; the values are internally consistent on the Python side but the game moved on. The fwbs precedent (PhantasmalGardener; 2026-05-18) established that mirroring stale Python content blocks the replay-harness from running real fixtures — flipping to C# fidelity is the higher-leverage choice for engine evaluation, accepting the parity divergence as a known cost. THIEVERY's `CardDebuffIntent` (card-steal) and ESCAPE's `EscapeIntent` are dropped from the intent payload because neither variant exists in `crate::monsters::Intent` yet; `Intent::Unknown` is used as the escape placeholder, and the steal-from-deck mechanics need a separate dispatcher bead. `EscapeArtistPower(5)` from `AfterAddedToRoom` is also deferred (same posture as PG's Skittish gap on `sts2-rs-fwbs`).
- **Resolution:** Once `../sts2-rl-agent` upstreams the C# rebaseline (separate sts2-rl-agent bead outside this project, tracked under umbrella `sts2-rs-kgwj`), bump the `PYTHON_SIM_PIN.md` SHA and remove this entry. Until then the divergence is the deliberate, audited choice and the parity harness skips ThievingHopper-involving fixtures.

## 2026-05-18 ovicopter-pattern-a-rebaseline

- **Fixture:** _(none — Rust deliberately diverges from the Python pin to track C# decomp v0.103.2; no fixture is meaningful while the divergence is intentional. A parity fixture will be filed once `../sts2-rl-agent` upstreams the same rebaseline.)_
- **Diff:** Rust's `Ovicopter` now matches C# decomp `MegaCrit.Sts2.Core.Models.Monsters/Ovicopter.cs` v0.103.2 — HP 124..=132 (was 67..=72), a 4-move chain `LAY_EGGS(Summon 3 ToughEggs) → SMASH(16) → TENDERIZER(7 + Vulnerable 2) → SUMMON_BRANCH → {LAY_EGGS if alive teammates ≤ 3 else NUTRITIONAL_PASTE(Buff Strength 3) → SMASH}` (was a 2-move `LAY_EGGS(Summon 1 ToughEgg) ↔ DIVE(14)` cycle). The LAY_EGGS spawn count flipped from 1 → 3 to match the C# `LayEggsMove` loop. The Python pin `01c094d` still ships the stale 67..=72 HP / 2-move / 1-spawn variant; running the parity harness against that pin on any combat involving `Ovicopter` will diverge until upstream catches up.
- **Python source:** `sts2-rl-agent@01c094d:sts2_env/monsters/act2.py:342-344` (`make_ovicopter` factory + LAY_EGGS `combat.add_enemy(create_tough_egg(rng))` single-spawn, DIVE effect_fn).
- **Status:** deferred-to-bead-sts2-rs-kgwj
- **Bead:** sts2-rs-7qd1 (this fix); umbrella `sts2-rs-kgwj` (Pattern-A monster rebaseline)
- **Expires:** 2026-08-18
- **Rationale:** Same Pattern-A reasoning as the 2026-05-18 thieving-hopper / slimed-berserker entries above — the Python pin is older than C# v0.103.2 on this monster. The fwbs (PG) / l8yi (TH) / 6v3s (SB) precedents established that mirroring stale Python blocks the replay-harness on real fixtures, and flipping to C# fidelity is the higher-leverage choice. The `SUMMON_BRANCH_STATE` conditional is encoded via `ConditionFn::OwnerAware` (counts alive enemies in `state.enemy_uids` minus owner), matching the C# `teammates.Count(c => c.IsAlive) <= 3` predicate.
- **Resolution:** Once `../sts2-rl-agent` upstreams the C# rebaseline (separate sts2-rl-agent bead outside this project, tracked under umbrella `sts2-rs-kgwj`), bump the `PYTHON_SIM_PIN.md` SHA and remove this entry. Until then the divergence is the deliberate, audited choice and the parity harness skips Ovicopter-involving fixtures.

## 2026-05-23 act1-map-residual-2pct-undocumented-game-behavior

- **Fixture:** _(none — corpus is the scraped v0.103.2 `.run` files at `../sts2_run_scraper/data/files{,-spire-codex}/`; harness is `sts2-cli/python/r3_map_parity.py --codex --no-require-win`.)_
- **Diff:** Across 1300 v0.103.2 Ironclad single-player runs (primary + spire-codex corpus), the Rust Act-1 map generator matches the scraped visited-path room-type sequence on 1272/1300 = **97.85%**. The residual 28/1300 = 2.15% are split:
  - 11/28 scraped paths contain *consecutive same-restricted-type rooms* (e.g. `rest_site → rest_site`, `elite → elite`) that the documented `StandardActMap.IsValidPointType` check (`MegaCrit.Sts2.Core.Map/StandardActMap.cs:382-456`, `_parentMapPointRestrictions = {Elite, RestSite, Treasure, Shop}` applied to both parents and Children) explicitly forbids.
  - 17/28 scraped paths are individually-legal sequences whose required topology Rust cannot reach given the documented `MapPathPruning.PruneAndRepair` 3-iteration loop.
- **Python source:** _n/a — this is a Rust ↔ scraped-real-game divergence, not Rust ↔ Python. The Python pin's map generator has its own (worse) parity gap so it isn't the comparison baseline. The decompiled C# spec lives at `../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Map/{StandardActMap,MapPathPruning,MapPostProcessing}.cs`._
- **Status:** accepted-permanently
- **Rationale:** A fresh side-by-side C# console harness (`/tmp/csharp_parity/Program.cs`, ported afresh from the same decompiled source files as Rust) produces **bit-identical Act-1 maps to Rust** for every residual seed tested after fixing one `PruneAllButLast` count-vs-index guard bug (same class as `sts2-rs-o4mnu`). So both independent ports of the decompiled source agree with each other; the real game binary diverges from both on these 28 seeds. The remaining gap is therefore in a code path neither static-source-reading port has captured — candidates include (a) a modifier setting `MapPointTypeCounts.PointTypesThatIgnoreRules` beyond the single source-visible `BigGameHunter` (Elite-only) entry, (b) an undocumented RNG-order issue in the outer `PruneAndRepair` 3-iteration loop that both ports got "right" the same wrong way, (c) scraped-corpus quirks (multiplayer-flagged-as-SP seeds, build-id mislabeling). Resolving any of these requires loading `MegaCrit.Sts2.Core.dll` against .NET 9 and running `StandardActMap` directly against each residual seed — beyond what decompiled-source analysis alone can resolve.
- **Resolution:** Documented here as an accepted permanent divergence at the 97.85% parity floor. The fix that lifted parity from 86%→98% (the `prune_all_but_last` count-based guard) shipped in commit `797262c` (2026-05-23, bead `sts2-rs-o4mnu`). Memory file `project_v103_act1_pathlen_off_by_one.md` carries the full residual analysis. Re-open if (i) a future game patch ships a decompilation that explains the gap, (ii) someone wires a `MegaCrit.Sts2.Core.dll` parity rig that can A/B against the 28 residual seeds, or (iii) the residual rate rises materially above 2.15% on a larger corpus (would indicate the residual is structurally more than the current ~uniform 1.5% per-ascension Bernoulli noise floor).

## 2026-05-18 slimed-berserker-pattern-a-rebaseline

- **Fixture:** _(none — Rust deliberately diverges from the Python pin to track C# decomp v0.103.2; no fixture is meaningful while the divergence is intentional. A parity fixture will be filed once `../sts2-rl-agent` upstreams the same rebaseline.)_
- **Diff:** Rust's `SlimedBerserker` now matches C# decomp `MegaCrit.Sts2.Core.Models.Monsters/SlimedBerserker.cs` v0.103.2 — HP 266..=276 (was 60..=65) and a 4-move chain `VOMIT_ICHOR(StatusCard 10 Slimed) → FURIOUS_PUMMELING(MultiAttack 4×4) → LEECHING_HUG(Debuff Weak 3 + Buff Strength 3) → SMOTHER(Attack 30) → VOMIT (loop)` (was a 3-move `SLASH(8) → SLAM(14) → ENRAGE(Buff Strength 3) → SLASH` cycle). The Python pin `01c094d` still ships the stale 60..=65 HP / 3-move variant; running the parity harness against that pin on any combat involving `SlimedBerserker` will diverge until upstream catches up.
- **Python source:** `sts2-rl-agent@01c094d:sts2_env/monsters/act3.py` (`make_slimed_berserker` factory + matching `effect_fn`s for SLASH/SLAM/ENRAGE).
- **Status:** deferred-to-bead-sts2-rs-kgwj
- **Bead:** sts2-rs-6v3s (this fix); umbrella `sts2-rs-kgwj` (Pattern-A monster rebaseline)
- **Expires:** 2026-08-18
- **Rationale:** Same Pattern-A reasoning as the 2026-05-18 thieving-hopper entry above — the Python pin is older than C# v0.103.2 on this monster; the values are internally consistent on the Python side but the game moved on. The fwbs (PhantasmalGardener) and l8yi (ThievingHopper) precedents established that mirroring stale Python content blocks the replay-harness from running real fixtures — flipping to C# fidelity is the higher-leverage choice for engine evaluation. The `VOMIT_ICHOR` `StatusIntent(10)` payload is encoded as `Intent::StatusCard` (telegraph only); the actual Slimed-card injection lives behind the per-move status-card dispatcher rather than the intent payload, mirroring the existing pattern for `NOISE` etc. The `LEECHING_HUG` multi-intent encodes both Debuff(Weak,3) and Buff(Strength,3) as ordered `Intent` entries on a single move per the C# `MoveState(..., new DebuffIntent(), new BuffIntent())` shape — already a supported pattern in `act2`/`act3`.
- **Resolution:** Once `../sts2-rl-agent` upstreams the C# rebaseline (separate sts2-rl-agent bead outside this project, tracked under umbrella `sts2-rs-kgwj`), bump the `PYTHON_SIM_PIN.md` SHA and remove this entry. Until then the divergence is the deliberate, audited choice and the parity harness skips SlimedBerserker-involving fixtures.

## 2026-05-26 neows-bones-w9e-grant-pool-coupling

- **Fixture:** _(none — replay-side gap; surfaced via the R-Act1-Replay corpus, not a scripted parity fixture. A corpus anchor for a scraped run that picks `NEOWS_BONES` would pin byte-faithfulness once W9c/W9d land.)_
- **Diff:** `RelicId::NeowsBones` `after_obtained` is ported (bead `sts2-rs-wjuoi`, W9e): it shuffles the 23-relic single-player Neow valid pool with `PlayerRng.Rewards`, grants the first 2, and adds 1 generatable curse via `Rng.Niche` (`MegaCrit.Sts2.Core.Models.Relics/NeowsBones.cs`). One residual faithfulness gap remains:
  - **Niche curse-pool order:** C# samples a `HashSet<CardModel>` via `Rng.Niche.NextItem`; the Rust port models its enumeration as `CurseCardPool.GenerateAllCards` order filtered to the 10 `CanBeGeneratedByModifiers == true` curses. HashSet enumeration order is an implementation detail — needs a scraped-corpus anchor to confirm the chosen-curse identity matches.

  _(The former gaps "HeftyTablet grant skipped" and "PhialHolster grant id-only" are both **closed** as of W9d (`sts2-rs-w6hdj`) and W9c (`sts2-rs-zgy5m`, 2026-05-26): both relics now have a registered `after_obtained`, so when NeowsBones' shuffle surfaces either in the top 2 the grant fires its own effect — HeftyTablet's 3-Rare-select + Injury, PhialHolster's +1 slot + 2-potion roll — consuming RNG faithfully.)_
- **C# source:** `../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Relics/NeowsBones.cs`, `.../Models.Events/Neow.cs:49-105` (`AllPossibleOptions`), `.../Models.CardPools/CurseCardPool.cs`.
- **Status:** deferred-to-bead-sts2-rs-wjuoi
- **Bead:** sts2-rs-wjuoi (this port)
- **Expires:** 2026-07-31
- **Rationale:** The shuffle/take-2/curse-draw skeleton is RNG-faithful and every one of the 23 pool relics now grants cleanly with its own registered `after_obtained`. The only open question is the Niche curse-draw identity, pending a scraped-corpus anchor.
- **Resolution:** Confirm the chosen-curse identity against a scraped run that picks NeowsBones; reorder the curse pool if it diverges, then remove this entry.

## 2026-05-26 hefty-tablet-w9d-rare-pool-order

- **Fixture:** _(none — replay-side; a scraped run that picks `HEFTY_TABLET` at Neow would pin the rolled-card identity.)_
- **Diff:** `RelicId::HeftyTablet` `after_obtained` is ported (bead `sts2-rs-w6hdj`, W9d): roll 3 distinct Rare cards from the character pool via `PlayerRng.Rewards` (`CardFactory.CreateForReward`, Uniform odds, `Rarity == Rare`, `NoUpgradeRoll`), offer them as a skippable 1-of-3 card-select through the new `RunState.pending_card_selects` surface, and always add 1 Injury curse to the deck (`MegaCrit.Sts2.Core.Models.Relics/HeftyTablet.cs`). Residual gap: the rolled-card **identity** depends on the per-character spec-slice order matching C# `CardPool.GenerateAllCards`; the RNG **draw count** is faithful regardless. The unpicked-option card-choice run-history breadcrumbs (`HeftyTablet.cs:39-45`, cosmetic) are not modeled.
- **C# source:** `../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Relics/HeftyTablet.cs`, `.../Factories/CardFactory.cs:72-199` (`CreateForReward`).
- **Status:** deferred-to-bead-sts2-rs-w6hdj
- **Bead:** sts2-rs-w6hdj (this port)
- **Expires:** 2026-07-31
- **Rationale:** The choice surface + Injury + RNG draw-count are faithful; the only open question is whether the Rust character spec-slice enumeration order matches the C# pool for the `NextItem` index→card mapping. Same pool-order class as the NeowsBones Niche-curse caveat; best resolved with a scraped-corpus `HEFTY_TABLET` anchor.
- **Resolution:** Confirm the rare-pool order against a scraped run that picks HeftyTablet; reorder the per-character spec slice if it diverges, then remove this entry.

## 2026-05-26 phial-holster-w9c-potion-pool-order

- **Fixture:** _(none — replay-side; a scraped run that picks `PHIAL_HOLSTER` at Neow would pin the rolled-potion identity.)_
- **Diff:** `RelicId::PhialHolster` `after_obtained` is ported (bead `sts2-rs-zgy5m`, W9c): grant +1 max potion slot, then roll 2 random out-of-combat potions via the `CombatPotionGeneration` RNG (`PotionFactory.CreateRandomPotionsOutOfCombat`) and procure each through the `should_procure_potion` veto channel (`MegaCrit.Sts2.Core.Models.Relics/PhialHolster.cs`). The option pool is the character pool (`<Character>4Epoch.Potions`) concatenated before the 45-potion `SharedPotionPool`, and the per-potion roll is `NextFloat()` rarity (`<=0.1` Rare / `<=0.35` Uncommon / else Common) then `NextItem` over the rarity-filtered subsequence. Two residual caveats:
  1. **Unlock epochs assumed revealed:** the pool omits `UnlockState`/epoch gating (`SharedPotionPool.GetUnlockedPotions` removes `Potion1Epoch`/`Potion2Epoch` potions when those epochs are not revealed; character pools gate on `<Character>4Epoch`). The Rust port assumes all revealed — the steady state for any account in the scraped corpus. A not-yet-unlocked account would draw from a smaller pool.
  2. **Potion identity vs scraped corpus:** the pool *order* is ported from the C# declaration order (so the `NextItem` index→potion mapping should match), but this is unconfirmed against a real run. The RNG **draw count** (`2 * Potions` = 4 draws) is faithful regardless.
- **C# source:** `../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Relics/PhialHolster.cs`, `.../Factories/PotionFactory.cs:27-61`, `.../Models.PotionPools/SharedPotionPool.cs`, `.../Timeline.Epochs/<Character>4Epoch.cs`.
- **Status:** deferred-to-bead-sts2-rs-zgy5m
- **Bead:** sts2-rs-zgy5m (this port)
- **Expires:** 2026-07-31
- **Rationale:** The +1 slot, the Sozu-vetoable procure, and the RNG draw-count are faithful; the open questions are the unlock-epoch assumption and the pool-order index→potion identity. Same pool-order class as the NeowsBones Niche-curse and HeftyTablet rare-pool caveats; best resolved with a scraped-corpus `PHIAL_HOLSTER` anchor.
- **Resolution:** Confirm the rolled-potion identity against a scraped run that picks PhialHolster; if it diverges, revisit the pool order and/or model `UnlockState`, then remove this entry.

## 2026-06-13 corpse-state-untargeted-enemy-card-suppressed

- **Fixture:** _(none — engine-internal `legal_actions`/`play_card` consistency fix, not a Python-vs-Rust scripted fixture. Repro is the full-run driver: `run_arena one-cell --seed 130853117 --policy heuristic --combat eval-cheap` livelocked with `final_phase=Combat` before this fix; deterministic seeds 130853117 / 1635992940 under both heuristic and tiered policies.)_
- **Diff:** When combat is held open with NO living enemy — a pending-revive `Adaptable` corpse (TestSubject, bead `sts2-rs-jnf4a`) — Rust `legal_actions` no longer surfaces `Enemy`-target cards (Strike/Bash/…). C# `RunSimulator.LegalActions` offers them once untargeted (`target_index = null`) because its `DoPlayCard` auto-targets `Enemies.FirstOrDefault(IsAlive) = none` and the card still resolves its self/block side against a null target. Every Rust `Enemy`-card handler instead resolves `target.expect("…: Enemy-targeted")`, so `play_card` rejects a `target: None` `Enemy` play up front (`StepResult::InvalidTarget`, no state mutation). Offering it therefore produced a legal action that resolution always no-ops, which livelocked any deterministic policy (it re-picks the no-op forever; the run-hash watchdog then trips). Non-`Enemy` cards (block/self/AoE — handlers that tolerate an empty board) and `EndTurn` are still surfaced, so the corpse state still has a real progress action (EndTurn → enemy turn → revive). Bead `sts2-rs-e5bsp` (GH #43).
- **Python source:** sts2-rl-agent@01c094d (combat-loop scaffolding reference only); C# source: `../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core/RunSimulator.cs` (`LegalActions`, untargeted `AnyEnemy` emission) + per-card `OnPlay` null-target tolerance.
- **Status:** accepted-permanently
- **Rationale:** A faithful C# port would require every Rust `Enemy`-card handler to accept a `None` target and no-op its damage side — a broad refactor across all `*: Enemy-targeted` handlers, with no behavioural payoff: the only reachable state is the rare `Adaptable` corpse, and the player's executable options (block/self cards + EndTurn) are unchanged. Suppressing the un-executable option restores the `legal_actions`/`play_card` invariant that every surfaced action does something when applied.
- **Resolution:** If the `Enemy`-handler family is ever migrated to tolerate a null target (matching C# self/block-on-no-target), re-add the untargeted emission and delete this entry.
