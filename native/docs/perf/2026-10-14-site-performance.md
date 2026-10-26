# Engine performance on the docs site: re-measurement (2026-10-14)

> **Superseded** by [`2026-10-14-benchmarks.md`](2026-10-14-benchmarks.md),
> which the site and both READMEs now cite. This document's random-fight
> (W1) numbers were measured with the pre-#762 monster builder; the harness
> reproduces them with `site_random_bench --legacy-spawn` (35,607 wins,
> digest `b00b9c215952899c`). Its "~23,000×" against the shipped game was the
> wall time of one cold game process per fight against one Rust thread, which
> is mostly cold-start CPU and the harness's polling sleeps on the game's
> side; the newer document replaces it with saturated-throughput and
> CPU-time ratios. The text below is kept as it was.

This is the source for the engine and search timings on the documentation
site (`docs/site/index.html`, `ai.html`), in the root README and in
`sts2-rs/README.md`, and for the PPO collection speed-up on
`docs/site/training.html`. It replaces the earlier figures (see "Earlier
figures" below), which came from different benchmarks.

Two engines were measured on the same machine, with the same harness code, in
interleaved runs:

- **previous engine**: the engine as of 12 October 2026 (its last engine
  change was on 13 September 2026);
- **new engine**: the October 2026 engine-speed work, as of 14 October 2026.

## Published values

One definition per metric. Every page uses these values and this rounding.

| Metric | Definition | Published |
|---|---|---|
| Random-play fights per second | `site_random_bench`, new engine, one pinned core | ~59 k (59,000) |
| Engine actions per second | the same runs; an action is a card play or an end of turn | ~1.3 M (~0.77 µs each) |
| Actions per fight | the same runs | 22.0 |
| The shipped game, run headless | `cs_random_fights.py`: actions per second of play | ~56 actions/s (~18 ms each, 95% interval 17–19 ms) |
| Rust vs the shipped game | engine actions/s ÷ the game's actions/s | ~23,000× |
| The browser hint's search, native | `hint_bench`, 1,000 simulations, one thread | ~14 ms |
| The same search as WASM in Chrome | headless Chromium, 1,000 simulations | ~19 ms |
| WASM vs native | WASM time ÷ native time, same position, 1,000 / 2,000 / 4,000 simulations | 1.3–1.4× |
| The browser hint at its default budget | WASM in Chrome, 2,000 simulations | ~41 ms |
| The advisor's search | `advisor_bench.py`, 8,000 simulations, one thread, Act-1 boss | ~280 ms |
| Speed-up on the random fights | new ÷ previous, fights/s | 2.2× |
| Speed-up of the browser hint's search | new ÷ previous, 1,000 and 4,000 simulations | 1.9× |
| Speed-up of the advisor's search | new ÷ previous, 8,000 simulations, one thread | 1.8× |
| PPO collection, one worker | previous ÷ new, CPU time of six fixed runs | 2.4× |

## Results

Medians over runs. For the search timings each run reports the median of 7
timed repetitions after warm-ups, so the value is a median of medians.
Speed-ups are given as the ratio of medians and as the median of the per-pair
ratios [min, max].

### Random-play fights (`site_random_bench`, one core)

110,000 fights per run, 7 runs per engine.

| | previous | new | speed-up |
|---|---|---|---|
| fights/s | 26,734 (25,569–26,918) | 58,999 (58,165–59,402) | 2.207; per pair 2.222 [2.172, 2.322] |
| actions/s | 587,779 | 1,297,150 | |
| µs per action | 1.70 | 0.77 | |
| user-space instructions per run | 3.272 × 10¹⁰ | 1.576 × 10¹⁰ | 2.076× fewer |

Actions per fight 21.986; the random player wins 32% of the fights.

Continuity only, not published: the older `xsim_random_bench --mode combat`
workload (no relics, card registry only, the August 2026 harness) gives
38,663 → 57,202 fights/s, 1.480× (per pair 1.480 [1.476, 1.486]), with
1.374× fewer instructions. It gains less because it has no relics: much of
the new engine's saving is in relic dispatch, which a relic-free fight never
pays. Its previous-engine value is within 1.1% of the August 2026 shootout
on the same workload (38,246 fights/s, older engine).

### The shipped game, run headless (`cs_random_fights.py`)

66 fights (3 per encounter), engine-independent, measured once. All 66
completed with no anomaly.

| | value |
|---|---|
| actions per second of play | 55.79 |
| ms per action | 17.93 (bootstrap 95% over fights: 17.3–18.6) |
| fights per second of play | 2.54 |
| actions per fight | 21.97 |
| random player's win rate | 32% |
| time inside `legal_actions` round trips | 2.3% of play (0.41 ms per call) |
| CPU time of the game's process ÷ play time | 56% |
| Rust actions/s ÷ the game's actions/s | 23,252 (with the game side's 95% interval: 22,455–24,099) |
| the same ratio in fights/s (cross-check) | 23,234 |

### The browser hint's search (`/api/bench`, seed BENCH1, first fight)

Native (`hint_bench`, one core), ms per search, 5 runs per engine:

| simulations | previous | new | speed-up |
|---|---|---|---|
| 200 | 5.34 | 2.85 | |
| 800 | 21.11 | 11.15 | |
| 1,000 | 26.65 | 13.99 | 1.905; per pair 1.908 [1.828, 1.913] |
| 2,000 | 54.47 | 28.60 | 1.905; per pair 1.906 [1.851, 1.925] |
| 4,000 | 113.09 | 59.80 | 1.891; per pair 1.892 [1.855, 1.899] |

WASM, ms per search, 5 runs per engine, and the WASM ÷ native ratio of the
same engine:

| simulations | Chrome, previous | Chrome, new | ratio (new) | Node, previous | Node, new | ratio (new) |
|---|---|---|---|---|---|---|
| 1,000 | 36.1 | 18.7 | 1.337 | 34.86 | 18.6 | 1.330 |
| 2,000 | 78.9 | 41.1 | 1.437 | 73.43 | 40.0 | 1.399 |
| 4,000 | 153.9 | 80.4 | 1.344 | 150.31 | 81.0 | 1.355 |

The previous engine's ratios are similar (Chrome 1.355 / 1.449 / 1.361).

### The advisor's search (`advisor_bench.py`, 8,000 simulations)

Ironclad A5, 16-card deck, 55/80 HP, Burning Blood, against the Act-1 boss
Vantom. 4 runs per engine.

| threads | previous | new | speed-up |
|---|---|---|---|
| 1 | 498.1 ms (495.6–501.4) | 281.6 ms (280.8–283.5) | 1.769; per pair 1.770 [1.755, 1.776] |
| 8 | 74.5 ms (73.2–74.9) | not published (see below) | not published |

### PPO collection, one worker (`ppo_collect.py`)

Six Ironclad runs (seeds 700–705, A0/3/6/10, Act 1 rolled, 250 / 1,000
simulations) through the trainer's `play_batch` with the Ironclad champion,
plain release builds of the Python extension, 2 runs per engine. CPU time
(`perf stat` task-clock of the whole process): 72.69 s → 29.94 s, 2.428×
(per pair 2.427–2.429). This is not engine speed alone: the new code also
skips the combat search when a decision has exactly one legal move, which the
decisions do not show. A separate measurement on 13 October 2026 (the same
six runs, three builds run at once on separate cores) put the new engine's
profile-guided build, the one training uses, at 3.07× the previous engine's
plain build: CPU time 74.74 s → 24.32 s, with 31.16 s for the new engine's
plain build. It was not repeated here.

## Identity checks

The engines must play and search identically; only then is a timing ratio a
speed-up.

- **Random fights**: actions, wins and a digest folding every fight's length
  and outcome are identical in all 14 runs (2,418,456 actions, 35,607 wins,
  digest `b00b9c215952899c`). The continuity workload's actions and wins are
  identical in all 6 runs.
- **Advisor's search**: a digest of every root action's visits, wins, losses,
  Q and value sum plus the principal variation is identical for both engines
  in every run (one thread `a2833dcde062`, eight threads `7c30afa17608`).
- **Browser hint's search**: native, Node and Chrome, on both engines, sit on
  the same position (Fuzzy Wurm Crawler at 57 HP, the starter hand) and pick
  the same first move at all five budgets. This is a weaker check than a
  digest; the advisor's digest is the strong evidence that the searches are
  identical.
- **PPO collection**: each run's record hash is identical between the
  engines, and each run loaded its own build of the extension.
- **The shipped game**: all 22 start states hold Burning Blood only and the
  10-card starter deck; the 22 encounters are the same ids, in the same
  order, as the Rust harness.

## Method

### Machine and builds

- AMD EPYC 7443P (24 cores, 48 threads, four 6-core L3 domains), Linux 6.8,
  `schedutil` governor, transparent huge pages `madvise`.
- Rust 1.96.0 from `rust-toolchain.toml`. Native harnesses: the workspace
  `release` profile (fat LTO, one codegen unit, `opt-level` 3, no
  `target-cpu`), no profile-guided optimisation, system allocator. The Python
  extension: the same profile, a plain build like `maturin build --release`
  (the new one links mimalloc). WASM: the `wasm-release` profile,
  wasm-bindgen 0.2.120 `--target web`, `wasm-opt -O3`, the same steps as
  `sts2-pycli/scripts/workbench_static.py wasm`.
- Node 25.9.0, headless Chromium 149.0.7827.55 (Playwright), .NET 9.0.313,
  Python 3.13.12.
- Both engines were exported from version control and built in separate
  target directories with identical flags and no environment overrides.

### Procedure

- Measured on 14 October 2026, 02:26–03:11 UTC.
- Runs alternate between the engines (previous, new, new, previous, …).
  Pairs: 7 for the random fights, 3 for the continuity workload, 5 for each
  search runtime, 4 for the advisor, 2 for PPO collection.
- Before every metric the host had to be quiet: 1-minute load below 1.5, the
  whole machine below 3% busy over 5 s, and the metric's own CPUs and their
  SMT siblings below 5% busy over 2 s (`cpu_busy.py`). No metric was forced.
  The check runs before a metric, not during it. The host had short
  background bursts that night; they show in Chrome's run-to-run spread
  (8–13%), not in the single-thread metrics (0.4–5%). The 8-thread advisor's
  63% spread had another cause, its CPU mask (see "Not published").
- Pinning: single-thread metrics on core 20 (its SMT sibling idle); the WASM
  runtimes on CPUs 18–23; the advisor on CPUs 6–13; the shipped game on CPUs
  12–17 and 36–41 (its game thread plus the .NET pool). The machine's L3
  domains are CPUs 0–5, 6–11, 12–17 and 18–23 (with their SMT siblings), so
  the advisor's 6–13 straddles two of them.
- An earlier attempt the same night measured everything except the advisor
  and agrees to within 2% (PPO collection gave 2.47× there).

### Workloads

**Random-play fights** (`sts2-rs/crates/sts2-bench/examples/site_random_bench.rs`).
Ironclad A0, the starter deck at 80/80 HP plus Burning Blood, with every
content registry installed, as the run AI does. Seed *s* fights the Act-1
Overgrowth encounter *s* mod 22, so 2,200 seeds cover its 22 encounters (4
weak, 12 normal, 3 elite, 3 boss) equally, with the real monster AI. Each seed's start
state is built once, untimed; each fight clones it (timed) and plays
uniformly random legal moves to the end. 50 fights per seed, one thread.

```sh
# from sts2-rs/
cargo build --release -p sts2-bench --example site_random_bench
perf stat -e instructions:u,task-clock -- \
    taskset -c 20 target/release/examples/site_random_bench \
    --seeds 2200 --seed-start 0 --reps 50 --relics burning-blood --mix round-robin
```

The remaining commands run from the repository root unless marked.

**The shipped game** (`scripts/site-bench/cs_random_fights.py`). The same
fights through the game's own assembly in the project's headless harness
(`sts2-pycli/src/Sts2Headless`, Release build), which talks JSON over a pipe.
Each fight is a fresh game process: a played save opens on the map with the
starter deck and Burning Blood, the harness enters the encounter, and the
driver plays uniformly random choices from `legal_actions` until the fight
leaves combat. Only play is timed, from the first `legal_actions` call to the
response that leaves combat; boot and setup are not. Both sides enumerate the
legal actions at every decision. It needs the game's assemblies
(`docs/bring-your-own-game.md`).

```sh
STS2_HEADLESS_BIN=<release build>/Sts2Headless STS2_GAME_DIR=<game assemblies> \
taskset -c 12-17,36-41 python -I scripts/site-bench/cs_random_fights.py \
    --pycli sts2-pycli --per-encounter 3 --out cs.jsonl
```

**The browser hint's search** (`sts2-rs/crates/sts2-workbench/examples/hint_bench.rs`,
`scripts/site-bench/wasm_hint_bench.mjs`, `chrome_bench.html` +
`chrome_bench_page.js` + `chrome_driver.mjs`). All three walk seed BENCH1
(Ironclad A0, Overgrowth) to its first fight with the same rule, print a
position fingerprint, and time `POST /api/bench {sims, seed: 7}`, the
workbench's own bench button. One untimed 4,000-simulation call warms up
first, then per budget one untimed and 7 timed calls. Chrome runs it on the
page's main thread, so the app's Web Worker round trip (about 0.6 ms) is not
included.

```sh
# from sts2-rs/
cargo build --release -p sts2-workbench --example hint_bench
taskset -c 20 target/release/examples/hint_bench BENCH1 7
# from the repository root
taskset -c 18-23 node scripts/site-bench/wasm_hint_bench.mjs <wasm pkg dir> BENCH1 7
# serve chrome_bench.html, chrome_bench_page.js and pkg/ from one directory, then:
PW_ROOT=sts2-pycli/web taskset -c 18-23 node scripts/site-bench/chrome_driver.mjs \
    http://127.0.0.1:<port>/chrome_bench.html BENCH1 7
```

**The advisor's search** (`scripts/site-bench/advisor_bench.py`). The
advisor's `sts2_rs.mcts_recommend` call with its settings: seed 0,
common random numbers off, c_puct from the advisor's per-tree budget rule
(2.5 here). One warm-up call per cell, then 7 timed calls.

```sh
taskset -c 6-13 python -I scripts/site-bench/advisor_bench.py \
    --mod-dir <dir with sts2_rs.abi3.so> --threads 1,8 --sims 8000 --reps 7
```

That is the command as run. For the 8-thread cell, give the search a mask of
whole L3 domains (`taskset -c 6-17` here) or pin each worker to its own CPU:
6–13 is what made the 8-thread figure unpublishable (see "Not published").

Both searches, the hint's and the advisor's, run as those callers configure
them: hidden draw order (belief-draw), draw widening 2.0, heuristic rollouts
to depth 200, no transposition table, and the search library's default
end-of-turn widening (0.5 / 0.4). The run agent's search widens end-of-turn
nodes more, so these timings are not the run agent's cost per simulation.

**Superseded by GH #758 (2026-10-15):** the hint and the advisor now search
with the run agent's end-of-turn widening (5.787 / 0.436). `hint_bench`
BENCH1, CPU 20, three interleaved runs per arm, median of 9: 2,000 sims
28.6 ms → 30.9 ms (1.08×), 1,000 sims 13.9 ms → 15.0 ms (1.08×). The
advisor's 8,000-sim boss search was not re-timed.

**PPO collection** (`scripts/site-bench/ppo_collect.py`). Both engines run
the same committed Python sources; only the extension differs. The
checkpoint is the Ironclad champion's training checkpoint, which the public
snapshot does not include.

```sh
perf stat -e instructions:u,task-clock -- \
    taskset -c 20 env OMP_NUM_THREADS=1 MKL_NUM_THREADS=1 PYTHONPATH=<mod dir>:sts2-pycli/python \
    python scripts/site-bench/ppo_collect.py Ironclad <checkpoint.pt> roll out.txt 6 1
```

## Caveats

1. **Random play.** The fights are uniformly random legal moves, which lose
   68% of the time, so they are short (22 actions on average). They measure
   the engine, not the AI.
2. **The speed-ups depend on the workload.** 2.2× is for these fights, the
   starter deck plus one relic. With no relics the same engines differ by
   1.48×. The searches gain 1.9×, the advisor on one thread 1.8×, PPO
   collection 2.4×.
3. **The 23,000× is not a like-for-like language comparison.** The game's
   process is busy for only 56% of play time; the gap mixes the game's
   computation, its asynchronous action pipeline and the harness's JSON round
   trips. Each fight also runs in a fresh process and is timed until it
   leaves combat, so it carries a fixed cost per fight (about 60 ms, 95%
   interval 11–117 ms, from a linear fit of play time on actions, plus about
   15.2 ms per action). The Rust side pays no such cost. At the margin the
   ratio is therefore about 20,000×; the published 23,000× is engine actions
   per second divided by the game's, as defined.
4. **The search timings repeat one search.** Every repetition runs the same
   seed-7 search on the same position, which warms caches and branch
   predictors in a way a real hint does not. The bias is the same for both
   engines and is the bench button's own method.
5. **One machine.** A server CPU on Linux. The advisor row describes this
   machine, not a player's PC with the game running; "WASM in Chrome" is
   headless Chromium on the same machine, not a phone or tablet.
6. **Plain builds.** No figure uses profile-guided optimisation except the
   cited 3.07× for PPO collection.
7. **The extension's allocator setting changed after this measurement.** As
   measured, the new extension's mimalloc returned freed memory to the
   system one second after a free, so a call made more than a second after
   the previous one faulted that memory back in: at a 1.5 s gap the 8-thread
   advisor call took 46.1 ms against 39.8 ms on the system allocator. The
   extension now never returns it (39.7 ms at the same gap). Every figure
   here times calls back to back, where the effect was 1–2% at most, so
   they stand. It does not explain the 8-thread spread below.

## Not published, and why

- **The advisor on 8 threads.** On the new engine the per-run medians were
  68.5, 40.3, 50.3 and 39.9 ms (a 63% spread), against a stable 73–75 ms on
  the previous engine, and the search results were identical. A later
  diagnosis found the cause in the harness, not the engine. The mask 6–13
  covers six CPUs of one L3 domain and two of the next; the scheduler looks
  for an idle CPU only inside a thread's L3 domain, so it kept two of the
  search's own workers on one CPU, in 21.7% of repetitions (0.8% with the
  whole-domain mask 6–17, 0–0.3% with one worker pinned per CPU). Every slow
  repetition with a waiting worker had two of our own workers waiting. The
  previous engine stacks too, less visibly at its longer call time.
  Background bursts were real but secondary. With 6–17 or one-per-CPU
  pinning the new engine's 8-thread call takes about 40 ms, but that was not
  measured under this protocol, so the site still gives the one-thread
  figure.
- **A tablet figure.** It cannot be measured on this machine.
- **`transitions_per_sec`** (criterion). It is an end-of-turn-only fixture
  with no cards, relics or monster AI, kept as a regression canary.

## Earlier figures

Earlier versions of the site and READMEs quoted ~1.4–1.5 M state transitions
per second (the end-of-turn fixture), ~100,000 random fights per second
(derived from that fixture, never measured, and relic-free), ~7,000× the
headless game (a different fixture, on a loaded host), a tablet search time
(a different position, device and search), and ~49 k simulations per second
(10 threads on a laptop, a configuration the advisor does not run). Those
came from different benchmarks and are not comparable with the numbers
above: neither the drop from 100,000 to 59,000 nor the rise from 7,000× to
23,000× is a change in the engine.
