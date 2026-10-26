# Browser workbench: wasm size and in-browser search speed (M1 exit criteria)

Context: `../../../docs/browser-workbench.md`. Two numbers gate Milestone 1 —
the wasm payload has to be phone-downloadable (criterion: **< 4 MB brotli**)
and the single-threaded combat search has to be usable on a phone (this is
what sizes the M2 advisor budgets). Both measured on the M1 commit.

## Payload

`sts2_workbench_wasm_bg.wasm` from `just wasm-build` (`wasm-release` profile,
`panic = "abort"`, no `wasm-opt` — not installed on the build host):

| | bytes |
|---|---|
| raw | 5,109,022 |
| gzip -9 | 1,530,328 |
| brotli q11 (Node zlib) | 1,061,665 |

Before the content registries were linked in (the `ensure_content_installed`
fix) the raw file was 4.75 MB — dead-code elimination had dropped every relic
and potion hook, which is how the missing registry install went unnoticed
until the golden replay.

## Search speed (`POST /api/bench`)

`bench::run`: the hint endpoint's single-tree search (`mcts_recommend`'s
config, `threads = 1`, heuristic rollouts, clairvoyant hidden info) over the
LIVE combat, then seeded heuristic playouts. The host times the call.

Host: 4-core Intel Xeon @ 2.10 GHz (remote dev container), idle. Both rows are
the first Act-1 Monster fight of seed `BENCH1` / Ironclad / A0 — the native
probe walked to a Fuzzy Wurm Crawler fight, the browser probe (which took the
map through the SPA's normalized options) to a Seapunk fight, so the two are
comparable in order of magnitude, not per sim.

| sims | native release (ms) | ms / sim | Chrome, wasm in Worker (ms) | ms / sim | Xiaomi Pad Mini, Chrome 152 (ms) | ms / sim |
|---|---|---|---|---|---|---|
| 200 | 6.7 | 0.033 | 49 | 0.242 | 14 | 0.072 |
| 800 | 23.2 | 0.029 | 61 | 0.076 | 25 | 0.031 |
| 1000 | 26.0 | 0.026 | 94 | 0.094 | 24 | 0.024 |
| 4000 | 110.3 | 0.028 | 209 | 0.052 | 93 | 0.023 |
| 200 heuristic playouts (3,115 actions native / 2,635 browser) | 3.9 | — | 10 | — | — | — |

The tablet column (2026-09-03, the Firebase preview build `0f7237a2b21d`,
the app's ⏱ bench button in an Act-1 fight) is *faster* per sim than the
desktop column: the desktop rows were taken on a 2.1 GHz Xeon dev container,
the tablet on a current mobile SoC. Per sim it sits within 1.1× of native
release on that Xeon.

`GET /api/live/state` (serialize + enrich a combat step): 0.6 ms native, 0.6 ms
through the worker round trip.

Reading: the wasm build runs the search at roughly 2–3× native cost once the
tree is warm (the 200-sim row is dominated by fixed setup on the browser
side). Today's advisor presets (Quick 200 / 800 sims, Deep 1000 / 4000) are
all well under a second on a desktop core with one thread — and under 100 ms
at 4000 sims on the tablet — so M2 can ship the same presets single-threaded
and leave wasm threads (COOP/COEP) to M5. To add a device: open the static
build there, get into a fight, press **⏱ bench** in the top bar (it alerts the
table and copies it to the clipboard).

Caveat: the first-turn fight of an Act-1 starter deck is a small tree; a
late-act fight with a 30-card deck will be several times slower per sim.
