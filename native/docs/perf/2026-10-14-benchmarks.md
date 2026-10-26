# Benchmarks: one core, the whole machine, the shipped game, training and other simulators (2026-10-14)

This is the source for every performance number on the documentation site
(`docs/site/benchmarks.html`, and the summaries on `index.html`, `ai.html`
and `training.html`), in the public README (`README.md` at the root of the
public repository) and in `sts2-rs/README.md`. It supersedes
[`2026-10-14-site-performance.md`](2026-10-14-site-performance.md), whose
random-fight numbers used the pre-#762 monster builder and whose comparison
with the shipped game was the cold one-process wall time (see "Changes
against the earlier figures").

Engines, both built from version control with the same harness code:

- **previous**: `d3ea66a51` (12 October 2026), the engine before the
  engine-speed work;
- **new**: `6d1918237` (PR #761, not merged at the time), plus the benchmark
  harnesses described here.

Other simulators: [sansb/sts2-sim](https://github.com/sansb/sts2-sim) at
`c61336cf05d289ae01ab9254beaa62c1b4ee407c` (12 October 2026) and
[lain-wood/sts2core](https://github.com/lain-wood/sts2core) at
`3b2d969d025b9e64bc96b9a15928bd2a0d824bf2` (25 September 2026).

Every value below comes from the analysis of the raw run outputs:
`scripts/site-bench/bench/results/analyze.py`, followed by
`scripts/site-bench/bench/results/apply_xcheck.py`. The second script
re-derives the headline numbers independently (117 metrics, largest
difference 0.06%, all from the rounding of printed rates) and adds the
training typical-iteration figures, from the watchdog-free replay
(`scripts/site-bench/bench/training/diag/`), and the cross-engine
whole-machine ratios. The analysis keys each metric; the key is given in
brackets where it helps to trace a published value.

**Rounding.** The site rounds most measured values to three significant
figures (actions per fight to two decimals, percentages to whole points);
counts, dates, build ids and digests are exact. This document gives
the measured values (medians over runs, with [min, max] over runs where
useful) and the published rounding.

## Definitions

- **One core**: one single-threaded process pinned to CPU 20, with its SMT
  sibling CPU 44 idle. For the shipped game, which runs several threads per
  process: one game process and its Python driver confined to CPUs 20 and 44,
  both hardware threads of that core.
- **Both threads of one core**: two of our processes on CPUs 20 and 44 (the
  `1core-smt` preset). For the game: 2, 4, 8 or 16 game processes on CPUs 20
  and 44 (cells B2–B16).
- **Whole machine**: all 24 cores / 48 threads. For a Rust engine, the
  better of `phys24` (24 processes, one per core) and `smt48` (48 processes,
  one per hardware thread). For the game, the best of cells C1–C6 (24 to 384
  processes).
- **Action**: one state transition, a card play or an end of turn (in our
  engine, one `apply` call).
- **Aggregate rate**: the sum of the processes' own rates, all processes
  forked, pinned and released at the same instant by `scale_runner.py`. The
  timed windows overlap by at least 0.915 in every W1 and W2 preset run
  (0.978 or more in our W1 runs). The wall-clock alternative (total work ÷ first start to
  last end, which also charges process start and the slowest straggler) is
  recorded too: for our `smt48` W1 run it is 29.6 M actions/s against
  31.0 M.
- **W1, random play**: Ironclad A0, the 10-card starter deck at 80/80 HP
  and Burning Blood, against the 22 Act-1 Overgrowth encounters round-robin
  (seed *s* fights encounter *s* mod 22: 4 weak, 12 normal, 3 elites, 3
  bosses), uniformly random legal moves to the end of the fight; 2,200
  seeds × 50 fights = 110,000 fights per run (× 200 = 440,000 in the scaling
  runs). Each seed's start state is built once, untimed, the way a real run
  builds it (monster AI, spawn position and HP roll as in `run_bridge`);
  each fight copies it (timed).
- **The instance player**: one choice per card in hand per target, the
  random player of our engine and of the game. sansb and sts2core merge
  identical actions in their legal-action lists, so their own uniformly
  random player is a different, weaker player; our harnesses for them also
  offer `--policy instance`.
- **W2**: sansb's own benchmark fixture, `ironclad_toadpoles_uniform_v1`:
  Ironclad at 68/80 HP, no relic, hand S S D S S, draw pile S D D Bash D,
  two Toadpoles (26 and 25 HP). The fixture omits the ascension field, which
  sansb reads as **Ascension 10**, so every engine plays it at A10. 240,000
  random playouts with a 64-action cap; clone-and-apply per step unless
  stated.

## Published values

| Metric | Definition | Measured | Published | Key |
|---|---|---|---|---|
| Fights/s, one core | W1, new engine, CPU 20, 7 runs | 59,359.9 [58,620, 59,886] | 59,400 (~59 k) | `ours.w1.1core.new.fights_per_s` |
| Actions/s, one core | same runs | 1,304,796 [1,288,534, 1,316,356] | 1.30 M (~1.3 M) | `ours.w1.1core.new.actions_per_s` |
| Time per action | same runs | 766.4 ns | 766 ns (~0.77 µs) | `ours.w1.1core.new.ns_per_action` |
| New vs previous, one core | median of 7 ABBA pair ratios | 2.247 [2.173, 2.366] | 2.25× | `ours.w1.1core.speedup_new_vs_base.median_pair_ratio` |
| Whole machine, actions/s | `smt48`, 3 repeats | 30,968,200 | 31.0 M | `ours.scale.w1.new.whole_machine_best.actions_per_s` |
| Whole machine, fights/s | same | 1,409,470 | 1.41 M | `ours.scale.w1.new.smt48.agg_fights_per_s` |
| Whole machine ÷ one core | ÷ the same series' 1-process run (1,319,993 actions/s) | 23.46 | 23.5× | `ours.scale.w1.new.whole_machine_best.vs_1core` |
| Shipped game vs ours, one core, best measured | ours one thread ÷ the game's best one-core cell (B16, not levelled off; see below) | 1,497.8 | 1,500× | `ratio.saturated_one_core.ours_1thread` |
| Same, both threads on each side | ours `1core-smt` ÷ B16 | 1,627.6 | 1,630× | `ratio.saturated_one_core.ours_both_threads` |
| Shipped game vs ours, whole machine | ours `smt48` ÷ the game's best (C5) | 2,433.2 | 2,430× | `ratio.whole_machine` |
| CPU time per action, process | game cell A process CPU ÷ ours | 2,968.4 | 2,970× | `ratio.cpu_time.process` |
| CPU time per action, game thread | game cell A game-thread CPU ÷ ours | 2,250.8 | 2,250× | `ratio.cpu_time.gamethread` |
| One cold game process (retired figure) | game cell A0 wall ÷ ours | 23,408.6 | 23,400× | `ratio.one_process_wall.cold_A0` |
| One warm game process | game cell A wall ÷ ours | 13,525.1 | 13,500× | `ratio.one_process_wall.warm_A` |
| Browser hint, 1,000 sims, native | ms per search | 13.66 | 13.7 ms (~14 ms) | `search.hint.native.new.sims1000.ms` |
| Browser hint, 1,000 sims, Chromium | ms per search, WASM | 18.4 | 18.4 ms (~18 ms) | `search.hint.chrome.new.sims1000.ms` |
| Browser hint, 2,000 sims, Chromium | ms per search, WASM | 39.9 | 39.9 ms | `search.hint.chrome.new.sims2000.ms` |
| Hint new vs previous, Chromium, 1,000 sims | median of 5 pair ratios | 1.929 | 1.93× | `search.hint.chrome.speedup_new_vs_base.sims1000.median_pair_ratio` |
| Advisor, 8,000 sims, 1 thread | ms | 274.9 | 275 ms | `search.advisor.1t.new.ms` |
| Advisor, 8,000 sims, 8 threads (CPUs 6–17) | ms | 39.58 | 39.6 ms (~40 ms) | `search.advisor.8t-l3.new.ms` |
| Training, typical iteration, new PGO | 959 games + update | 208.0 s | 208 s | `training.typical.new-pgo.iter_s` |
| Training, typical iteration, previous plain | same | 608.7 s | 609 s | `training.typical.base-plain.iter_s` |
| Training speed-up, typical iteration | previous ÷ new PGO | 2.927 | 2.93× | `training.typical.speedup.pgo_vs_base.iter_s` |
| 150 iterations at the typical rate | new PGO / previous | 8.66 h / 25.36 h | 8.66 h / 25.4 h (~9 h / ~25 h) | `training.typical.*.campaign_150_iters_h` |
| Training, one core | previous ÷ new PGO, CPU time of 6 runs | 3.159 | 3.16× | `training.percore.speedup.pgo_vs_base.ratio_of_medians` |
| vs sansb, one core | ours (no relic) ÷ sansb (instance), median of 7 round ratios | 3.134 | 3.13× | `xsim.w1.ratio.ours-norelic_over_sansb-inst.median_round_ratio` |
| vs sansb, whole machine | best preset each (ours `smt48`, sansb `phys24`), ours with Burning Blood live | 3.521 | 3.52× (about 3.25× without our relic, scaled by the one-core ratio 3.134 / 3.391; no relic-free whole-machine run) | `xsim.scale.w1.ratio.whole_machine_best.ours_over_sansb` |
| vs sts2core, one core | ours ÷ sts2core (instance), median of 7 round ratios | 0.6403 | sts2core 1.56× | `xsim.w1.ratio.ours_over_core-inst.median_round_ratio` |
| vs sts2core, whole machine | best preset each (ours `smt48`, sts2core `phys24`) | 0.6922 | sts2core 1.44× | `xsim.scale.w1.ratio.whole_machine_best.ours_over_sts2core` |

## Results

### Hardware and software

| Item | Value |
|---|---|
| System | ASUS ESC4000A-E10 (board KRPG-U8-M Series), BIOS 0402 |
| CPU | AMD EPYC 7443P 24-Core Processor (Zen 3 "Milan"); 1 socket, 24 cores, 48 threads (SMT on), 1 NUMA node |
| Clocks | boost up to 4.036 GHz; top non-boost P-state 2.85 GHz; `schedutil`, boost on |
| Caches | 32 KiB L1d, 32 KiB L1i, 512 KiB L2 per core (shared by its two threads); L3 128 MiB = 4 × 32 MiB, one per 6-core CCD |
| L3 domains | CPUs 0–5,24–29 / 6–11,30–35 / 12–17,36–41 / 18–23,42–47 |
| RAM | 256 GiB: 4 × 64 GB Samsung M393A8G40AB2-CWE, DDR4-3200 RDIMM, dual-rank, ECC, in slots C1, D1, G1, H1 (A1, B1, E1, F1 empty), so 4 of 8 channels: 102.4 GB/s theoretical peak |
| GPUs | 4 × NVIDIA GeForce RTX 3090, 24 GiB, driver 610.43.02 |
| OS | Ubuntu 24.04.4 LTS, Linux 6.8.0-124-generic (`PREEMPT_DYNAMIC`), no `isolcpus` / `nohz_full`; transparent huge pages `madvise` |
| Rust | rustc 1.96.0 (LLVM 22.1.2), pinned by `sts2-rs/rust-toolchain.toml` |
| Our release profile | fat LTO, `codegen-units` 1, `opt-level` 3, `panic = "unwind"`, no `target-cpu`, no PGO, glibc malloc (the Python extension links mimalloc; the training PGO build is labelled) |
| .NET | SDK 9.0.313, runtime 9.0.15, default runtime config (tiered compilation, dynamic PGO, workstation GC); game assembly `sts2.dll` md5 `282bfb4ccec5b17c51dea6b37ccb4791` (v0.107.1) |
| Python / PyTorch | CPython 3.13.12; torch 2.12.1+cu130 (CUDA 13.0, cuDNN 9.20.0) |
| Node / browser | Node 25.9.0; Playwright 1.61.0 with headless Chromium 149.0.7827.55; wasm-bindgen 0.2.120, wasm-opt (binaryen) 132 |
| perf | 6.8.12 |

### Our engine, one core (W1, CPU 20, CPU 44 idle, 7 ABBA pairs)

| Engine | fights/s | actions/s | ns/action | instructions/action | cycles/action | IPC | GHz |
|---|---|---|---|---|---|---|---|
| previous `d3ea66a51` | 26,431 [25,309, 26,971] | 580,976 [556,323, 592,858] | 1,721 | 13,549 | 6,860 | 1.97 | 3.90 |
| new `6d1918237` | 59,360 [58,620, 59,886] | 1,304,796 [1,288,534, 1,316,356] | 766 | 6,525 | 3,147 | 2.07 | 3.88 |

Speed-up: ratio of medians 2.246, median of 7 pair ratios 2.247 [2.173,
2.366]; the previous engine executes 2.08× the instructions per action. Both
did the same work in all 14 runs: 110,000 fights, 35,646 wins (32.41%),
2,417,920 actions (21.98 per fight), digest `c78f3428460d718b`.

`--relics none` (same work, same digest): 1,203,214 actions/s against
1,297,978 with Burning Blood in the same rotation; none ÷ Burning Blood per
round 0.924, instructions per action ÷ 1.031. The relic does not change
random play (it only heals after a won fight), so the relic-free run is
slower for identical work. The cause was not investigated.

The perf counters (instructions, cycles, IPC, GHz) cover the whole process,
including the untimed start-state builds.

### Scaling (`scale_runner.py`, 3 repeats, medians)

New engine, W1 (440,000 fights per process):

| Preset | CPUs | procs | actions/s (sum) | fights/s | per physical core | per process | efficiency | GHz | IPC | min overlap | max process spread |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1core | 20 (44 idle) | 1 | 1,319,993 | 60,077 | 1,319,993 | 1,319,993 | 1.000 | 3.91 | 2.11 | 1.000 | 0.0% |
| 1core-smt | 20, 44 | 2 | 1,417,841 | 64,531 | 1,417,841 | 708,921 | 0.537 | 3.90 | 1.14 | 0.998 | 0.5% |
| ccx | 18–23 | 6 | 7,838,039 | 356,735 | 1,306,340 | 1,306,340 | 0.989 | 3.87 | 2.11 | 0.980 | 5.9% |
| ccx-smt | 18–23, 42–47 | 12 | 8,368,034 | 380,857 | 1,394,672 | 697,336 | 0.528 | 3.86 | 1.13 | 0.986 | 1.3% |
| phys12 | 0–2, 6–8, 12–14, 18–20 | 12 | 15,600,150 | 710,015 | 1,300,012 | 1,300,012 | 0.985 | 3.87 | 2.10 | 0.984 | 8.4% |
| phys24 | 0–23 | 24 | 30,190,031 | 1,374,049 | 1,257,918 | 1,257,918 | 0.953 | 3.79 | 2.08 | 0.979 | 13.3% |
| smt48 | 0–47 | 48 | 30,968,187 | 1,409,466 | 1,290,341 | 645,171 | 0.488 | 3.65 | 1.14 | 0.978 | 5.3% |

Efficiency = aggregate ÷ (processes × the same repeat's one-process rate on
CPU 20). SMT gain: `1core-smt` ÷ `1core` 1.074, `ccx-smt` ÷ `ccx` 1.067,
`smt48` ÷ `phys24` 1.028.

Previous engine, W1: `1core` 592,756; `1core-smt` 631,570; `phys24`
13,617,458; `smt48` 13,813,200 actions/s (23.3× its `1core`). New ÷
previous by preset (ratio of medians; the engines ran about 45 minutes
apart): `1core` 2.227, `1core-smt` 2.245, `phys24` 2.217, `smt48` 2.242.

New engine, W2 (clone-and-apply, A10, 240,000 playouts per process):
`1core` 654,519; `ccx` 3,856,511 (0.982); `phys24` 14,384,135 (0.916);
`smt48` 13,970,663 actions/s (0.444 per process; SMT costs 3%).

Independent hint searches, new engine (searches/s, median ms per search,
efficiency):

| Preset | 200 sims | 1,000 sims | 4,000 sims |
|---|---|---|---|
| 1core | 357.1 (2.80 ms, 1.00) | 72.78 (13.74 ms, 1.00) | 16.95 (59.01 ms, 1.00) |
| ccx | 2,103 (2.85 ms, 0.98) | 396.4 (15.05 ms, 0.91) | 90.62 (66.00 ms, 0.89) |
| phys24 | 8,177 (2.93 ms, 0.95) | 1,483 (16.14 ms, 0.85) | 334.4 (71.42 ms, 0.82) |
| smt48 | 7,802 (6.13 ms, 0.45) | 1,491 (32.15 ms, 0.43) | 348.3 (137.56 ms, 0.43) |

800 and 2,000 simulations fall in between (`smt48`: 1,881 and 713.4
searches/s). The "whole machine" hint figure is picked at 1,000 simulations
(`smt48`); at 200 simulations `phys24` is higher.

### The shipped game, run headless

The game's own `sts2.dll` (v0.107.1), IL-patched and run by
`sts2-pycli/src/Sts2Headless` (Release build, .NET 9.0.15) with Godot
stubbed out, driven over JSON on a pipe by a Python driver
(`scripts/site-bench/cs_random_fights.py` for cell A0,
`scripts/site-bench/cs_fights_pool.py` for the rest), playing W1 with
uniformly random choices from the game's `legal_actions`. Every process has
its own driver on the same CPUs. Cell A0 starts a fresh process per fight;
the other cells reuse warm processes: an untimed reset per fight (`load_save`,
`select_map_node`, `load_save`, `enter_room`, about 30 ms warm) brings the
game back to a start state verified equal to the fresh process's, including
RNG counters, hand, draw order and monsters. Warm cells play 220 untimed
fights per process first.

| Cell | Layout | ms/action (wall, per process) | actions/s | process CPU ms/action | game-thread CPU ms/action |
|---|---|---|---|---|---|
| A0 | a fresh process per fight (cold), 66 fights, CPUs 20, 44 | 17.94 | 55.74 | 10.11 | – |
| A | one warm process, CPUs 20, 44, 220 fights | 10.366 | 96.47 play only; 85.84 with the reset | 2.275 | 1.725 |

Cell A: process CPU utilisation during play 21.9%; the game thread blocks
voluntarily 8.58 times per action; driver CPU 0.47 ms/action. Cold against
warm on the same 220 fights: wall 1.031×, CPU 1.376×; the first fight runs
at 18.51 ms/action wall and 11.43 ms CPU.

| Cell | procs | placement | actions/s | fights/s | actions/s while all busy | per physical core | busy % (cell CPUs) | MHz | process CPU ms/action | wall ms/action per process |
|---|---|---|---|---|---|---|---|---|---|---|
| B2 | 2 | CPUs 20, 44 | 173.7 | 8.0 | 174.6 | 173.7 | 23.4 | 2,652 | 1.889 | 10.18 |
| B4 | 4 | CPUs 20, 44 | 351.7 | 16.2 | 364.5 | 351.7 | 30.7 | 3,838 | 1.142 | 9.66 |
| B8 | 8 | CPUs 20, 44 | 627.3 | 28.9 | 655.5 | 627.3 | 59.5 | 3,814 | 1.237 | 10.66 |
| B16 | 16 | CPUs 20, 44 | 871.1 | 40.1 | 897.9 | 871.1 | 89.2 | 3,810 | 1.333 | 15.29 |
| C1 | 24 | 1 per core | 2,137.7 | 97.4 | 2,175.5 | 89.1 | 12.5 | 2,814 | 1.588 | 9.77 |
| C2 | 48 | 1 per thread | 4,244.3 | 193.4 | 4,419.1 | 176.8 | 15.2 | 3,723 | 0.920 | 9.54 |
| C3 | 48 | unpinned | 4,253.4 | 193.8 | 4,454.6 | 177.2 | 18.8 (all) | 3,390 | 1.217 | 9.46 |
| C4 | 96 | 4 per core | 7,901.8 | 359.5 | 8,259.0 | 329.2 | 37.8 | 3,854 | 1.398 | 10.17 |
| C5 | 192 | 8 per core | 12,727.1 | 579.0 | 13,227.8 | 530.3 | 67.2 | 3,668 | 1.675 | 12.53 |
| C6 | 384 | 16 per core | 12,515.0 | 569.3 | 13,549.8 | 521.5 | 85.6 | 3,690 | 2.059 | 23.93 |

Throughput in B and C includes the per-fight reset. Best one core: B16,
871.1 actions/s. B16 is the largest one-core layout tried, and the game had
not levelled off there: throughput still rose from 8 to 16 processes (627.3
→ 871.1) with the CPUs 89.2% busy, so the per-core ratio may be somewhat
lower with more processes (at full CPU, 871.1 ÷ 0.892 ≈ 977 actions/s would
give about 1,340×). Best whole machine: C5, 12,727.1 actions/s (530.3 per
core, 60.9% of 24 × B16). No cell saturates its CPUs, and C6 is slower than
C5 although busier.

Ratios, ours ÷ the game:

| Ratio | value | published | numerator | denominator |
|---|---|---|---|---|
| one process, cold (the retired figure) | 23,409 | 23,400× | game A0 17.94 ms/action | ours 0.7664 µs/action |
| one process, warm | 13,525 | 13,500× | game A 10.366 ms/action | ours 0.7664 µs |
| CPU per action, process | 2,968 | 2,970× | game A 2.275 ms | ours 0.7664 µs |
| CPU per action, game thread | 2,251 | 2,250× | game A 1.725 ms | ours 0.7664 µs |
| one core, best measured, ours one thread | 1,498 | 1,500× | ours 1,304,796 actions/s | game B16 871.1 |
| one core, both threads each | 1,628 | 1,630× | ours `1core-smt` 1,417,841 | game B16 871.1 |
| whole machine | 2,433 | 2,430× | ours `smt48` 30,968,187 | game C5 12,727.1 |
| whole machine, against the game's all-busy window | 2,286 | – | ours 30,968,187 | game C6 all-busy 13,549.8 |

Sensitivities: the game-thread CPU of the most favourable cell (C2, 0.731
ms/action) gives 954×, so the game-thread ratio spans 954–2,251× and the
process ratio 1,200–2,968× across cells; using the scaling series' own
one-core run (1,319,993) gives 1,515× for the one-core ratio. Our
process is CPU-bound, so 766 ns is also its CPU time per action.

**Where the cold 17.94 ms/action goes**: about 2.28 ms warm game CPU (cell
A) + about 7.84 ms extra cold-start CPU (A0's 10.11 ms CPU minus A's 2.28)
+ about 7.83 ms off the CPU (the harness's `Thread.Sleep(1)` polling in
`WaitForActionExecutor`, `sts2-pycli/src/Sts2Headless/RunSimulator.cs`).
So the retired 23,000× was about 44% polling sleep, 44% extra cold-start CPU
and 13% the game's warm CPU work on the game's side. Warm, the process is
78% asleep.

**Same work.** Random play on the game (cells C4–C6, 8,448 fights, seeds
0–8,447): 32.24% wins, 21.98 actions per fight; ours (110,000 fights):
32.41%, 21.98. Per encounter, 21 of 22 within |z| ≤ 2 (largest |z| 2.05),
largest actions-per-fight gap 0.57.

### Search

Browser hint (`hint_bench`, seed BENCH1, first fight: Fuzzy Wurm Crawler at
57 HP, the starter hand; `POST /api/bench {sims, seed: 7}`; per run the
median of 7 timed repetitions; 5 runs per engine, ABBA). Native on CPU 20;
WASM on CPUs 18–23.

| sims | native prev ms | native new ms | Node prev | Node new | Chromium prev | Chromium new | native ×  | Node × | Chromium × | Node ÷ native (new) | Chromium ÷ native (new) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 200 | 5.39 | 2.79 | 7.38 | 3.90 | 7.40 | 3.90 | 1.95 | 1.91 | 1.90 | 1.40 | 1.40 |
| 800 | 21.21 | 11.02 | 28.65 | 15.01 | 30.50 | 15.00 | 1.93 | 1.91 | 1.93 | 1.36 | 1.36 |
| 1,000 | 26.52 | 13.66 | 35.54 | 18.63 | 35.60 | 18.40 | 1.96 | 1.90 | 1.93 | 1.36 | 1.35 |
| 2,000 | 55.17 | 27.85 | 74.02 | 39.64 | 74.70 | 39.90 | 1.97 | 1.88 | 1.90 | 1.42 | 1.43 |
| 4,000 | 114.58 | 58.76 | 150.90 | 79.36 | 150.50 | 79.40 | 1.95 | 1.89 | 1.92 | 1.35 | 1.35 |

Speed-ups (×) are medians of the 5 pair ratios, previous ms ÷ new ms.

Advisor (`advisor_bench.py`: `sts2_rs.mcts_recommend` with the advisor's
settings, 8,000 simulations; Ironclad A5, 16-card deck at 55/80 HP with
Burning Blood against the Act-1 boss Vantom; one warm-up call, then 7 timed
calls; 4 runs per engine, ABBA):

| Threads | previous ms | new ms | new sims/s | speed-up (median pair) |
|---|---|---|---|---|
| 1 (CPU 20) | 490.9 | 274.9 | 29,097 | 1.785 |
| 8, mask 6–17 (two whole L3 domains, 12-worker pool) | 76.4 | 39.6 | 202,240 | 1.896 |
| 8, one worker pinned per CPU 6–13 (`--pin-workers`) | 73.2 | 39.4 | 202,848 | 1.859 |

Eight pinned threads are 6.97× one thread (6.95× on the 6–17 mask). Both searches run with the
draw order hidden (belief-draw), as their deployed callers configure them,
and the search library's default end-of-turn widening (0.5 / 0.4). Since
GH #758 both use the run agent's 5.787 / 0.436; the hint's 2,000-sim search
measured 1.08× slower for it (`2026-10-14-site-performance.md`), the
advisor's was not re-timed.

### Training

**One worker on one core** (`ppo_collect.py`: 6 whole Ironclad runs, seeds
700–705, ascensions cycling 0/3/6/10, act 1 rolled, the Ironclad champion,
MCTS 250 / 1,000 at bosses; CPU 20; `perf stat` task-clock of the whole
process; 3 rounds, order reversed every round):

| Build | task-clock s (6 runs) | CPU s per run | runs/hour/core |
|---|---|---|---|
| previous, plain (what training ran before PR #761) | 74.04 [73.70, 74.08] | 12.34 | 292 |
| new, plain | 30.09 [30.08, 30.69] | 5.02 | 718 |
| new, PGO (what training runs now) | 23.44 [23.11, 23.66] | 3.91 | 921 |

Ratio of medians: new ÷ previous 2.460, PGO ÷ previous 3.159, PGO ÷ new
1.284. All nine runs produced byte-identical game records.

**Iteration 0 as the trainer ran it** (`train_iter_bench.py`: `ppo.trainer`
with the deployed recipe, Ironclad, 960 games, 250 / 1,000 simulations, A0–A10
uniform, act 1 rolled, 46 workers = cpu_count − 2, the PPO update on one RTX
3090, initialised from the Ironclad champion; 2 runs per build, order
reversed per round):

| Build | collect s | train s | iter s | games/h | CPU s/game | util_collect | GPU busy in update | errors |
|---|---|---|---|---|---|---|---|---|
| previous, plain (lower bound: 959 of 960 games) | ≥ 1,178.7 | 34.05 | ≥ 1,213.1 | ≤ 2,932 | 27.43 | 0.500 | 86.9% | 1, 1 |
| new, plain | 536.5 | 34.05 | 570.9 | 6,442 | 11.71 | 0.469 | 87.4% | 0, 0 |
| new, PGO | 381.1 | 34.15 | 415.6 | 9,068 | 8.43 | 0.472 | 87.0% | 0, 0 |

The GPU is idle during collection (0%). The new builds play identical games
(one fingerprint in all four runs); the previous engine's fingerprint
differs only through its dropped game.

**Why the previous engine dropped a game.** The trainer's stall watchdog
(`ppo/workers.py`, `DEFAULT_RUN_TIMEOUT_S = 600.0` in both the previous and
the new trainer) abandons pending runs once no run has completed for 600 s.
Submission 703 (seed 10,000,703, A4, Act 1 Underdocks, a loss at floor 31
after 46 run-level decisions on every build) is legitimately long: one step,
the floor-16 Act-1 boss fight at 1,000 simulations per decision, takes
1,176.8 s of the game's 1,184.7 s on the previous engine (222.5 of 225.1 s on
new PGO). In a replay of the same pool without the watchdog, the other 959
games finish at 574.6 s and game 703 then runs alone until 1,682 s, so a
600 s watchdog fires at about 1,175 s, matching the trainer's 1,177.0 /
1,180.4 s. All 960 games match across the three builds.

| Iteration 0 decomposed (replay, no watchdog, 46 workers) | previous plain | new plain | new PGO |
|---|---|---|---|
| replay collect, all 960 games (s) | 1,682.0 | 533.7 | 379.7 |
| trainer collect for comparison (s) | 1,177.0 / 1,180.4 (cut at 959) | 535.4 / 537.6 | 378.8 / 383.5 |
| the other 959 games done by (s) | 574.6 | 241.7 | 173.8 |
| game 703 starts at / wall in the pool (s) | 408.2 / 1,273.4 | 172.9 / 360.3 | 123.8 / 255.4 |
| game 703 running alone at the end (s, share of collect) | 1,107.4 (66%) | 292.0 (55%) | 205.9 (54%) |
| game 703 played on its own (s) | 1,197.6 | 324.4 | 226.3 |
| total CPU, all 960 games (s) / median per game (s) | 26,674 / 25.91 | 11,041 / 10.96 | 7,904 / 7.94 |
| **typical iteration** = the other 959 games + the trainer's train_s (s) | **608.7** | **275.8** | **208.0** |
| **150 iterations at that rate (h)** | **25.4** | **11.5** | **8.66** |
| typical games/h (959 games) | 6,008 | 14,283 | 19,864 |

| Ratio | previous ÷ new plain | previous ÷ new PGO | new plain ÷ new PGO |
|---|---|---|---|
| typical iteration | 2.21 | 2.93 | 1.33 |
| typical collection (the other 959 games) | 2.38 | 3.31 | 1.39 |
| total CPU, all 960 games | 2.42 | 3.37 | 1.40 |
| median CPU per game | 2.36 | 3.26 | 1.38 |
| one-core benchmark (above) | 2.46 | 3.16 | 1.28 |
| iteration-0 collect as the trainer measured it (previous cut off) | 2.20 | 3.09 | 1.41 |
| iteration-0 collect, same work uncut (inflated by the outlier) | 3.15 | 4.43 | 1.41 |

The throughput measures agree at about 2.4× (plain) and 3.3× (PGO). Whole
machine ÷ one worker on one core (typical games/h ÷ one-core runs/h;
different games, so indicative): 20.6× previous, 19.9× new plain, 21.6× new
PGO. Saving per 150-iteration leg with PGO on the typical basis: 16.7 h.

Historical anchors (Ironclad, 960 × 250/1,000, this host, older engine
builds): the current champion's own September leg (iterations 400–549) had
a collect median of 500.5 s and took 23.9 h for 150 iterations; in the four
August–September Ironclad campaigns (762 iterations), 21 iterations (2.8%)
hit the same watchdog and 19 more (2.5%) ran below util_collect 0.7. Real
campaigns run at a median util_collect of 0.94–0.95.

### Cross-engine

W1, one core (CPU 20, 7 rounds, the six runs rotated every round):

| Engine, player | fights/s | actions/s | ns/action | win rate | actions/fight | IPC | GHz | digest |
|---|---|---|---|---|---|---|---|---|
| ours (new, Burning Blood live) | 59,050 | 1,297,978 | 770 | 32.41% | 21.98 | 2.06 | 3.89 | `c78f3428460d718b` |
| ours (new, `--relics none`) | 54,739 | 1,203,214 | 831 | 32.41% | 21.98 | 1.98 | 3.89 | `c78f3428460d718b` |
| sansb, instance player | 17,457 | 383,245 | 2,609 | 32.53% | 21.95 | 1.84 | 3.90 | `dda07e263c74afe9` |
| sansb, its merged-action uniform player | 20,300 | 385,811 | 2,592 | 20.30% | 19.01 | 1.85 | 3.90 | `0e9e0d42139c4a46` |
| sts2core, instance player | 90,234 | 2,027,806 | 493 | 35.00% | 22.47 | 3.07 | 3.89 | `2575e7b84eb83af7` |
| sts2core, its merged-action player | 106,283 | 2,062,580 | 485 | 21.00% | 19.41 | 3.08 | 3.88 | `3e3d440a028383b2` |

Paired (same round) actions/s ratios, median of 7: ours ÷ sansb-instance
3.391 [3.347, 3.415]; ours (no relic) ÷ sansb-instance 3.134 [3.081, 3.180];
ours ÷ sts2core-instance 0.640 [0.630, 0.663]; ours ÷ sts2core-own 0.630.
With their own merged-action players: ours ÷ sansb-own 3.406, ours (no
relic) ÷ sansb-own 3.119, ours ÷ sts2core-own 0.630 (sts2core 1.59×). The
instance weighting is our harness code inside their timed loop; their own
players run at 0.7% (sansb) and 1.7% (sts2core) more actions per second
(ratio of medians; sansb's paired per-round ratio is 0.998), while playing
different, shorter games, so the two effects are not separable.
Instructions per action: ours 6,525, sansb 21,898, sts2core 5,927.

W1, whole machine (`scale_runner.py`, instance player, sansb `--reps 100`
and sts2core `--reps 400` per process, 3 repeats), actions/s:

| Preset | ours | sansb | sts2core |
|---|---|---|---|
| 1core | 1,319,993 | 388,022 | 2,014,540 |
| 1core-smt | 1,417,841 | 340,719 | 2,196,282 |
| ccx | 7,838,039 | 2,290,801 | 11,568,220 |
| phys24 | 30,190,031 | **8,794,662** | **44,740,272** |
| smt48 | **30,968,187** | 7,520,413 | 44,708,741 |

Every one of our whole-machine W1 processes ran with Burning Blood live
(the harness default). Best against best: ours ÷ sansb 3.52 (ours with
Burning Blood live, as in the per-core 3.391; about 3.25 like for like,
scaled by the one-core no-relic ratio 3.134 / 3.391; no relic-free
whole-machine run was made), ours ÷ sts2core 0.692 (sts2core 1.44× ours,
Burning Blood live on both). Same preset, ours ÷ sansb: 3.40 (`1core`), 4.16 (`1core-smt`),
3.42 (`ccx`), 3.43 (`phys24`), 4.12 (`smt48`); ours ÷ sts2core 0.66, 0.65,
0.68, 0.67, 0.69. SMT (`smt48` ÷ `phys24`): ours 1.028, sansb 0.855,
sts2core 0.999. Whole machine ÷ one core: ours 23.5, sansb 22.7, sts2core
22.2.

W2 (sansb's fixture at A10, 240,000 playouts, CPU 20, 5 rounds):

| Engine, mode | actions/s | ns/action | playouts/s | win rate | actions/playout |
|---|---|---|---|---|---|
| ours (new), clone-and-apply per step (sansb's semantics) | 658,266 | 1,519 | 35,267 | 88.65% | 18.66 |
| ours (new), in place (2 runs × 5 passes) | 1,190,380 | 840 | 63,776 | 88.65% | 18.66 |
| sansb, `sts-sim bench` as published | 413,715 | 2,417 | 24,343 | – | 17.00 |
| sansb, our harness, its merged-action uniform player | 400,981 | 2,494 | 23,594 | 57.28% | 17.00 |
| sansb, instance player | 403,821 | 2,476 | 20,960 | 88.25% | 19.27 |
| sts2core, its merged-action player | 1,948,735 | 513 | 113,575 | 61.07% | 17.16 |
| sts2core, instance player | 1,950,219 | 513 | 105,550 | 89.30% | 18.48 |

Ratios (paired rounds): ours ÷ sansb-instance 1.625, ours ÷ sansb's own
bench 1.586, ours ÷ sts2core-instance 0.335, ours ÷ sts2core-own 0.337; ours
in place ÷ sansb-instance 2.95, ÷ sts2core-instance 0.61. Previous ÷ new on
W2: clone 1.347, in place 1.558. sansb's `sts-sim bench` and our harness's
uniform player produce the same checksum (`5d4f992689b53874`), so the
harness reproduces its benchmark.

Their own benchmarks, re-run here: sansb `eval_throughput` (random playouts
from its 594 certified real-game fixtures, CPU 20): median 120,302
transitions/s (10th percentile 66,683, 90th 338,376). sts2core `bench` (a
28-card deck against a 300-HP synthetic enemy; the player never ends its
turn by choice): 2.68 M steps/s on 1 thread, 44.4 M with 24 threads (mask
0–23), 55.3 M with 48 (mask 0–47), 3 runs each. As published by their
authors: sansb (PERF.md) 1,049,000 transitions/s on an Apple M5 Max, one
thread, best of 5, and an eval median of 302k transitions/s; sts2core
3.56–3.61 M steps/s on one thread and 54.9–56.5 M on 32, on a machine its
authors call noisy and "not a baseline".

**Per encounter, the same work.** Our baseline is 22 runs of
`site_random_bench --encounters <ID> --seeds 200 --seed-start 0 --reps 50`
(10,000 fights each, independent seeds). sansb (instance) is within 2 SE
on 19 of 22; outside: Fogmog −0.9 points (2 SE 0.84), Slimes (normal) −1.8
(1.29), Slithering Strangler −2.5 (1.09); largest actions-per-fight gap 0.86.
sts2core (instance) is within 2 SE on 14 of 22; outside: Cubex Construct
+18.0 (a content error verified against the game's code: it gives the
monster no 13 block and a fifth move the game never uses), Flyconid +13.2,
Mawler +10.4, Snapping Jaxfruit +9.1, Slithering Strangler −2.6, Slimes
+1.8, Fogmog +0.9, Overgrowth Crawlers −0.2 (2 SE 0.15); largest
actions-per-fight gap 4.82. The 2 SE is fight-level and understates
seed-level noise on encounters with randomised rosters: Slithering
Strangler's block-to-block standard deviation is 0.7 points. On Slithering
Strangler the other two agree with each other (10.3%, 10.2%) and ours is the
outlier (12.8% at seeds 0–199, 12.0% over 1,600 seeds), unexplained; on
Slimes (weak) sansb plays 0.86 more actions per fight than ours (25.27
against 24.41; sts2core 24.09), unexplained. sts2core's Flyconid, Mawler and
Snapping Jaxfruit gaps were not traced to either engine.

### The other simulators, facts

| | spire-mentor | sansb/sts2-sim | lain-wood/sts2core |
|---|---|---|---|
| Commit | `6d1918237` | `c61336cf` (12 Oct 2026) | `3b2d969d` (25 Sep 2026) |
| Game build | v0.107.1 | v0.111.0 (the only admitted build; the newest of the three, with v0.109.0–v0.110.1 between it and v0.107.1) | v0.107.1 |
| Scope | whole single-player runs, A0–A10, 3 acts in 4 act models | combat only, from fully specified start states | combat only: rules kernel, in-combat solver, deck advisor |
| Characters | all 5 | all 5; Ironclad is the largest share of its certified fights (578 of 1,196; 43–48% in each set: 280 of 597 fixtures, 1,806 of 4,205 site uploads) | effectively Ironclad only: all 79 recorded traces are Ironclad and the tables have no orbs, Osty, stars or poison (derived; its docs do not state it) |
| Content | an id for every non-deprecated C# content class: 580 card ids = the 577 C# cards + 3 legacy/internal (`GENERIC`, `PAIN`, `PARASITE`); relics 297, potions 63, monsters 121, events 69, enchantments 23 (the last three each incl. one internal id); an id does not prove the behaviour. 3 of 6 event-started fights are skipped: DenseVegetation and PunchOff give a stand-in outcome (PunchOff a placeholder `Astrolabe` relic and a potion), TheArchitect's combat is not ported | 596 cards, 22 enchantments, 88 encounters, 105 monster kinds, 64 potions, 218 powers, 299 relics; unported content refused with a typed refusal | 117 cards incl. a placeholder (112 of the 128 in its reference catalog), 105 enemies incl. 3 synthetic + an unknown placeholder, 118 relic entries of which 103 modelled, 5 of the game's 22 enchantments, 24 potion slots; Queen boss missing; A8+ values taken from the game's code (its source tier) but not yet checked against its recordings (A0–A3) |
| RNG | .NET `System.Random` hand-ported; all 12 run + 3 player streams | all 9 combat streams reproduced | own splitmix64 streams by design (shuffle, enemy, generation); random opening moves fixed |
| Validation | lockstep against `sts2.dll` run headless, compared after every action (enemy intents and card afflictions are not in the compared state, `sts2-pycli/docs/parity-suite.md` §2): full fuzz tier of random moves, Ironclad 6,104/6,104 clean (2026-06-30); Silent 5,732/5,762, Defect 5,740/5,752, Regent 5,740/5,762, Necrobinder 5,716/5,752 (2026-07-07 final state, a deferred auto-play / hot-damage tail still open; `docs/audits/2026-07-07-parity-sweep-findings.md`); run level 1,232/1,250 clean (2026-07-07; 50 seeds × A0/2/5/8/10 × 5 characters; `run_parity.py` plays a random policy, so the runs mostly end in Act 1; the deeper Act-2 carried-state check still had open divergence buckets on 2026-06-27 and reached Act 3 in 0% of runs, `parity-suite.md` §10); no full sweep since; no checksummed real-game captures yet (#739); open suspected bugs #741, #742, #759, #760, #762 | lockstep against real-game checksums: 1,196 of the author's 1,206 captured fights replay exactly and none diverges (`eval/manifest.json` census: `lockstep_ok` 1,196, no divergence classes; the other 10 are 3 truncated captures, 4 with no encounter, 2 refused event combats and 1 unpaired); 594 certified published fixtures (597 pairs, 3 refusal fixtures; selected from the census); 4,205 certified A9/A10 site uploads (a corpus of certified fights only, 550 of them fixtures) | one-step predictions on 79 own recordings: 1,565 match / 1 mismatch (a trace recorded before an enchantment fix, which cannot be re-recorded) / 9 content missing / 177 skipped; per turn 236 / 2 / 51 / 84; enemy next move 353/415 (85%) |
| Clairvoyance | deployed searches (run AI, browser hint, real-game advisor) use belief-draw since #246 (July 2026): the draw order is hidden and every combat random stream reseeded; `MctsConfig`'s default is still clairvoyant | yes, by design: the exact solver and the site's reviews search the true state incl. draw order and RNG, to show what was achievable in hindsight (`python/README.md`: the product goal is a post-hoc review); a draw-blind lookahead (reshuffled draw pile, reseeded streams) exists, as an example program, for its coach's offer reviews | partly: a patched STS2MCP exports the true draw-pile order, used as known until the next reshuffle; `bestline` is explicitly clairvoyant (documented as an upper bound) |
| Licence | MIT | MIT | MIT |
| Rust code lines (cloc 1.98, code only, in-file tests included) | 217,781 (`sts2-core/src` + `sts2-content/src`; 7,069 `#[test]` attributes under `sts2-rs/crates`; about 138k of its 330k raw lines in test modules, ~42%; search lives in other crates); 365,739 (all 12 crates) | 419,659 (`v0.111.0/engine/src`, incl. its solver; about 4,677 `#[test]` functions, about 280k–292k of its 507k raw lines in test modules, 55–58% depending on the heuristic) | 32,366 (`src/`, 29 files, incl. its solver, advisor binaries and tests; `lib.rs` is about 9.5k lines of tests) |
| Bindings | PyO3 (`sts2_rs`), WASM (workbench) | WASM (browser engine); no PyO3 | none; zero dependencies |
| State | mutated in place; cloned per fight / per search node | new state per step (`apply_action_into`) | 3,880-byte `Copy` state, `step(s, a) -> State` |
| Legal actions | one per card instance per target (our MCTS merges identical card plays itself before expanding, `canonicalize_actions` in `sts2-mcts`) | identical cards and identical targets merged | identical cards merged |
| Build | rustc 1.96.0, fat LTO, cgu 1, unwind | rustc 1.97.1 (its pin), thin LTO, cgu 1, abort | rustc 1.96.0 (it pins none; its authors use 1.97.1 on Windows), fat LTO, cgu 1, abort |

Test shares are a heuristic count: lines from a `#[cfg(test)]` module to the
end of its file, plus `*test*.rs` files. sansb's and sts2core's directories
include their solvers; ours keeps search in separate crates, counted in the
365,739.

Clairvoyance is a property of a search, not of an engine: every engine here
steps a concrete state, ours included. No benchmark in this document's
cross-engine section runs a search, so it affects none of those numbers.

sts2core's README calls its L3 deck advisor's absolute death rates
untrustworthy and only paired differences trustworthy; that is a caveat
about the advisor, not about the rules kernel measured here.

Published figures of the other two come from their authors' machines
(sansb: an Apple M5 Max; sts2core: a Windows PC), so only the re-runs are
comparable with the rest of this document.

## Identity checks

All 111 checks pass. The main ones:

- **W1, ours**: every one-core run of both engines (14) and every
  `--relics none` run: 110,000 fights, 35,646 wins, 2,417,920 actions,
  digest `c78f3428460d718b`, spawn mode `run`, 0 anomalies. Every process of
  every scaling preset, both engines: 440,000 fights, 143,036 wins,
  9,667,495 actions, digest `fd781f2d768f425d`.
- **W2, ours**: clone and in place, both engines, every scaling process:
  240,000 playouts, 4,479,594 transitions, 212,766 wins, digest
  `792d3d36f6eab175`.
- **External engines**: one digest per player in all rounds and all
  scaling processes; the instance-player digests equal the values
  registered before the measurement (sansb `dda07e263c74afe9`, sts2core
  `2575e7b84eb83af7`); W2 checksums, transitions and wins equal their
  references.
- **Searches**: advisor digests identical on both engines in every run (one
  thread `a2833dcde062`, eight threads `7c30afa17608`); the hint's position
  fingerprint and first move identical at all five budgets in every native,
  Node and Chromium run of both engines, and in every process of the hint
  scaling runs.
- **The shipped game**: no cell forced; A0's 66 fights equal the earlier
  reference rows step for step; cold and warm give identical steps and
  outcomes on 220 fights; one digest per group of cells that play the same
  fights (A `84d2299ced4ea73d`; B `322937e47db75350`; C1–C3
  `03f186279b8e44e6`; C4–C6 `6cc4b6fddedf7d15`); 0 start-state mismatches;
  one setup fingerprint; all workers finished.
- **Training**: the nine one-core runs produced byte-identical records; every
  full run loaded the expected extension in every worker; new plain and new
  PGO play the identical 960 games; the previous engine's two runs agree with
  each other and differ from the new engine only through the dropped game
  (A4: 84 runs against 85, 64 wins in both).

## Method

- **Window**: 14 October 2026, 10:59:56–16:32:40 UTC, on a shared
  multi-user host. The training replays behind "typical iteration" ran
  16:38–18:10 UTC (see Caveats).
- **Quiet-host gate** before every timed stage: 1-minute load below 1.5, the
  whole machine below 3% busy over 5 s (`/proc/stat`), and the stage's own
  CPUs plus their SMT siblings below 5% busy over 2 s
  (`scripts/site-bench/cpu_busy.py`); wait up to 30 minutes, never force.
  58 gate passes: load median 1.38 [1.01, 1.49], busy median 2% [1, 3].
  The whole-machine runs (ours, the other engines and the game's C cells)
  used the whole-host part only, because ambient load from other users on
  single CPUs kept the per-CPU check from passing for the game's cells.
  Maximum per-CPU busy at the gate for C1–C6: 22%, 67%, 26%, 47%, 19%, 16%.
  The training stages (one-core runs and full iterations) checked only the
  whole-host busy share (≤ 3%, `QUIET_MAX` in `training/measure.sh`); the
  1-minute load at those 15 gate passes was 1.09–7.3.
- **Pinning**: one-core runs on CPU 20 (CPU 44 idle); WASM on CPUs 18–23;
  the advisor on CPU 20 (one thread), 6–17 or 6–13 (eight); the game's
  one-core cells on CPUs 20 and 44; scaling presets as listed.
- **Interleaving**: previous and new alternate ABBA: 7 pairs for W1, 2 for
  W2 (× 5 passes), 5 per search runtime, 4 per advisor cell; the three
  training builds in reversed order every round (3 rounds one-core, 2 full
  iterations); cross-engine W1 and W2 rotate the order of all six runs every
  round (7 and 5 rounds). Scaling: 3 repeats per preset, each with a fresh
  one-process baseline.
- **Statistics**: medians over runs; speed-ups as the median of the paired
  ratios (and the ratio of medians, which agrees).
- **Builds**: both engines exported with `git archive` and built in separate
  target directories with identical flags and no environment overrides
  (`RUSTFLAGS` empty, no `sccache`). WASM: the `wasm-release` profile,
  wasm-bindgen 0.2.120 `--target web`, `wasm-opt -O3`. The Python extension:
  the same release profile as a plain cdylib (equivalent to `maturin
  develop --release`), and for PGO the `just sim-engine-pgo` recipe.
- **The #762 harness fix.** Before this measurement, `site_random_bench` and
  `xsim_toadpoles` built monsters through the `sts2-cli` scenario builder,
  which gave every monster spawn position index 0 (the back Toadpole opened
  with SPIKEN instead of WHIRL) and rolled HP from the stream the roster
  randomiser had just read (a front Axe Ruby Raider always had 20 HP). Both
  harnesses now rebuild each monster's AI with the run path's spawn position
  and roster arguments, re-roll HP with `roll_unique_max_hps` on the
  combat's `niche` stream, and reseed the legacy combat RNG, as
  `run_bridge.rs` does; a checker found 0 mismatches against run-path start
  states over 1,320 states. `--legacy-spawn` keeps the old builder and
  reproduces the earlier identities byte for byte (2,200 × 50: 35,607 wins,
  digest `b00b9c215952899c`). The aggregate W1 statistics moved by +0.04
  points of win rate and −0.005 actions per fight. W2 now runs at A10
  (`--ascension 10`, the default).

  The scenario builder itself was fixed afterwards (GH #762): it now spawns
  through `run_bridge::spawn_roster`, the helper the run path uses, so the
  harnesses no longer rebuild anything. Their default output is unchanged
  byte for byte, and `--legacy-spawn` (now `examples/common/legacy_spawn.rs`,
  which rebuilds the old builder's monsters) still reproduces the pre-fix
  identities byte for byte.

## Reproduce

Paths are relative to the repository root unless marked. Each command
prints one JSON line (or a text block for `hint_bench` and sts2core's
`bench`) with its identity fields; compare those before timings.

```sh
# Our harnesses (from sts2-rs/)
cargo build --release -p sts2-bench --example site_random_bench --example xsim_toadpoles
cargo build --release -p sts2-workbench --example hint_bench

# W1, one core (from sts2-rs/); --relics none for the relic-free run,
# --legacy-spawn for the pre-#762 builder
perf stat -e cycles:u,instructions:u,task-clock,context-switches,cpu-migrations -- \
    taskset -c 20 target/release/examples/site_random_bench \
    --seeds 2200 --seed-start 0 --reps 50 --relics burning-blood --mix round-robin

# W2, one core (from sts2-rs/)
taskset -c 20 target/release/examples/xsim_toadpoles --mode both --passes 5 --ascension 10

# Browser hint, native (from sts2-rs/)
taskset -c 20 target/release/examples/hint_bench BENCH1 7
```

Whole machine (`scale_runner.py --show-topology` lists the presets: `1core`,
`1core-smt`, `ccx`, `ccx-smt`, `phys12`, `phys24`, `smt48`):

```sh
B=sts2-rs/target/release/examples
python3 -I scripts/site-bench/scale_runner.py --preset 1core --core 20 --perf --parser json \
    --label w1-1core --out-dir out/w1/1core -- $B/site_random_bench --seeds 2200 --reps 200
for p in 1core-smt ccx ccx-smt phys12 phys24 smt48; do
  python3 -I scripts/site-bench/scale_runner.py --preset $p --core 20 --perf --parser json \
      --baseline out/w1/1core/summary.json --label w1-$p --out-dir out/w1/$p -- \
      $B/site_random_bench --seeds 2200 --reps 200
done
# W2: --identity clone.digest,clone.wins,transitions,playouts -- $B/xsim_toadpoles --mode clone --passes 1 --ascension 10
# hint: --parser hint_bench -- sts2-rs/target/release/examples/hint_bench BENCH1 31
```

WASM (build as `sts2-pycli/scripts/workbench_static.py wasm` does:
`cargo build --locked --profile wasm-release --target wasm32-unknown-unknown
-p sts2-workbench-wasm`, then `wasm-bindgen --target web` and `wasm-opt
-O3`):

```sh
taskset -c 18-23 node scripts/site-bench/wasm_hint_bench.mjs <pkg dir> BENCH1 7
# serve chrome_bench.html, chrome_bench_page.js and pkg/ from one directory, then:
PW_ROOT=sts2-pycli/web taskset -c 18-23 node scripts/site-bench/chrome_driver.mjs \
    http://127.0.0.1:<port>/chrome_bench.html BENCH1 7
```

Advisor (`<mod dir>` holds the built `sts2_rs.abi3.so`):

```sh
taskset -c 20 python -I scripts/site-bench/advisor_bench.py --mod-dir <mod dir> --threads 1 --sims 8000 --reps 7
taskset -c 6-17 python -I scripts/site-bench/advisor_bench.py --mod-dir <mod dir> --threads 8 --sims 8000 --reps 7
taskset -c 6-13 python -I scripts/site-bench/advisor_bench.py --mod-dir <mod dir> --threads 8 --sims 8000 --reps 7 --pin-workers 6-13
```

The shipped game (needs the game's assemblies, `docs/bring-your-own-game.md`,
and a Release build of `sts2-pycli/src/Sts2Headless`):

```sh
export STS2_HEADLESS_BIN=<release build>/Sts2Headless STS2_GAME_DIR=<game assemblies>
# A0: a fresh process per fight
taskset -c 20,44 python -I scripts/site-bench/cs_random_fights.py --pycli sts2-pycli --per-encounter 3 --out A0.jsonl
# untimed, once: per-seed saves for seeds 0..8447
python -I scripts/site-bench/cs_fights_pool.py prep --pycli sts2-pycli --saves <saves dir> --n-seeds 8448 --prep-jobs 40
# A, B16 and C5 (B2/B4/B8, C1-C4 and C6: see the table above for workers and placement)
python -I scripts/site-bench/cs_fights_pool.py pool --pycli sts2-pycli --saves <saves dir> --out-dir A \
    --workers 1 --cpu-sets 20,44 --per-worker 220 --warmup 220
python -I scripts/site-bench/cs_fights_pool.py pool --pycli sts2-pycli --saves <saves dir> --out-dir B16 \
    --workers 16 --cpu-sets 20,44 --per-worker 44 --warmup 220
python -I scripts/site-bench/cs_fights_pool.py pool --pycli sts2-pycli --saves <saves dir> --out-dir C5 \
    --workers 192 --pin core --per-worker 44 --warmup 220
```

Per-worker counts: B2 352, B4 176, B8 88, B16 44 fights (704 in total);
C1 24 × 220 (`--pin core`), C2 48 × 110 (`--pin thread`), C3 48 × 110
(`--pin none`), C4 96 × 88, C5 192 × 44, C6 384 × 22 (all `--pin core`
except C2/C3). `--ref-jsonl <A0.jsonl of an earlier run>` checks reference
fights step for step.

Training (the champion checkpoint is not in the public snapshot):

```sh
# one worker, one core
perf stat -e instructions:u,task-clock -- taskset -c 20 env OMP_NUM_THREADS=1 MKL_NUM_THREADS=1 \
    PYTHONPATH=<mod dir>:sts2-pycli/python python scripts/site-bench/ppo_collect.py Ironclad <champion.pt> roll out.txt 6 1
# a full iteration (add --allow-plain for a plain build)
python3 -I scripts/site-bench/train_iter_bench.py --label new-pgo --python <venv>/bin/python \
    --py-src sts2-pycli/python --mod-dir <mod dir> --ckpt <champion.pt> --save-dir <empty dir> \
    --gpu 1 --iterations 1 --games-per-iter 960 --procs 0
```

Other simulators (`scripts/site-bench/xsim/sansb/README.md`,
`scripts/site-bench/xsim/sts2core/README.md` build the harnesses into a
checkout of each repository at the commits above):

```sh
# sansb (run from its v0.111.0/engine directory)
scripts/site-bench/xsim/sansb/setup.sh <clone> <target>
taskset -c 20 <target>/release/examples/xsim_w1_site --seeds 2200 --seed-start 0 --reps 50 --policy instance   # or uniform
taskset -c 20 <target>/release/examples/xsim_w2_toadpoles --trajectories 240000 --policy instance              # or uniform
taskset -c 20 <target>/release/sts-sim bench --trajectories 240000
taskset -c 20 <target>/release/examples/eval_throughput <clone>/v0.111.0/eval/fights
# sts2core
taskset -c 20 <target>/release/xsim_w1_site --seeds 2200 --reps 50 --policy instance                           # or engine
taskset -c 20 <target>/release/xsim_w2_toadpoles --trajectories 240000 --policy instance --ascension 10       # or engine
taskset -c 20 <target>/release/bench; taskset -c 0-23 <target>/release/bench; taskset -c 0-47 <target>/release/bench
# whole machine: scale_runner.py as above with --reps 100 (sansb) or --reps 400 (sts2core), --policy instance
```

## Caveats

1. **Random play.** Uniformly random legal moves lose 68% of W1's fights,
   so the fights are short (22 actions). They measure the engine, not the AI.
2. **Speed-ups depend on the workload**: W1 2.25×; W2 1.35× copying per
   step, 1.56× in place; the searches about 1.9×; the advisor 1.79× (one
   thread) to 1.90× (eight); training 2.46× (plain) and 3.16× (PGO) on one
   core. Part of the training speed-up is algorithmic: the new code skips
   the search when a decision has exactly one legal move.
3. **The shipped game's numbers carry harness costs**: its CPU time includes
   the harness's state dictionary (game thread) and JSON (main thread); its
   throughput cells include the ~30 ms per-fight reset; each process's
   Python driver runs on the same CPUs; and the harness runs without the
   game's localization tables (it prints "Localization dir not found"), as
   the earlier measurement did. With the tables loaded, an indicative run
   showed the same mechanics (same digest) and 15–30% more game-thread CPU,
   which would raise every ratio.
4. **Whole-machine noise.** The whole-machine gate checks the host before,
   not during, a run, and only as a whole. Per-process spread within a
   preset reached 13% (ours `phys24`), 16–17% (ours previous `phys24`, W2
   `phys24`, sansb `phys24`) and 24–33% (sts2core `phys24`, `ccx`), with
   other users' load on unused CPUs up to 38%. Repeats agree closely
   (medians are given).
5. **Training's typical iteration** comes from one no-watchdog replay per
   build, run 16:38–18:10 UTC after the gated window, one replay at a time
   with the host about 2% busy before each (its 15-minute load still 22–26
   from an earlier job). The replays reproduce the trainer's new-engine
   collection within 0.5% and predict the previous engine's watchdog cut,
   but they are single, ungated runs. "Typical" also keeps game 703 on one
   of the 46 workers from its start, and ignores the rare outlier iterations
   a real campaign contains. "Previous = plain build" is the best-supported
   assumption: the old trainer did not record which wheel it ran on, and
   before PR #761 `just sim-engine` built a plain wheel.
6. **The searches repeat one search** per repetition on the same position,
   which warms caches in a way a real hint does not; the bias is the same
   for both engines and is the bench button's own method.
7. **One machine**, a server CPU on Linux. The advisor rows describe this
   machine, not a player's PC with the game running; "Chromium" is headless
   Chromium on this machine, not a phone or tablet.
8. **Different games, different engines.** sansb models v0.111.0, a newer
   build than ours and sts2core's v0.107.1 (v0.109.0, v0.109.1 and v0.110.1
   lie between; its `builds.json` lists them as pending). At A0 the HP bands
   of all 29 monster kinds in W1 match v0.107.1, and none of the monsters
   on its v0.110.1→v0.111.0 change list appears in W1; movesets were not
   audited, and the v0.107.1→v0.110.1 changes are not documented in its
   repository. sts2core uses its own RNG, so its fights match ours in
   distribution only. In W2, sansb's playouts run about 3–4% more actions
   from turn 3 on (19.27 against 18.66 and 18.48); turns 1–2 match exactly
   on all three; the cause is not known.
9. **Our relic-free W1 run is 7.6% slower** than the Burning Blood run for
   identical work (3.1% more instructions per action, lower IPC). The
   comparison with sansb is therefore given both ways (3.13× and 3.39×).

## Not published, and why

- **Iteration 0 × 150 as a campaign projection** (50.5 h previous, 23.8 h
  new plain, 17.3 h new PGO): about half of iteration 0 is one outlier game
  running alone, and the previous engine's iteration is watchdog-truncated.
  The typical-iteration figures replace it.
- **The previous engine's iteration-0 speed-ups** (2.92× PGO, 2.12× plain):
  not same-work ratios (the previous run finished 959 of 960 games).
- **The diag replay's quoted one-core ratios** (2.47× / 3.21×): the raw perf
  files give 2.46× / 3.16× (ratio of medians of the three task-clock reps),
  which are used.
- **Wall-clock aggregate rates** (total work ÷ first start to last end):
  recorded for every preset but not published; they charge process start
  and stragglers rather than throughput.
- **A W1 encounter intersection**: not needed, all 22 encounters run on all
  three engines.
- **The game with its localization tables loaded** (cell AL): not run in
  the measured window; see Caveat 3.
- **A tablet, phone, laptop or Windows figure**: not measurable on this
  machine.
- **`transitions_per_sec`** (criterion): an end-of-turn-only fixture with no
  cards, relics or monster AI, kept as a regression canary.

## Changes against the earlier figures

- **"~23,000× faster than the shipped game"** (published until 14 October
  2026) compared the wall time of one cold game process per fight with one
  Rust thread (now 23,409×). Of the game's 17.9 ms per action, about 44% is
  the harness's polling sleep, 44% extra cold-start CPU and 13% the game's
  warm CPU work. The site now publishes the best measured throughput (1,500×
  per core, 2,430× whole machine) and the CPU-time ratio (2,250–2,970× in
  one warm process; 954–2,970× across layouts).
- **W1 numbers** (59.0 k fights/s, 1,297,150 actions/s in
  `2026-10-14-site-performance.md`) used the pre-#762 monster builder and
  engine `beec46e14`; today's 59.4 k / 1.30 M use the run-path builder at
  `6d1918237`. The difference is within run-to-run noise and the random-play
  statistics are unchanged to 0.04 points.
- **Training**: the earlier "2.4× per worker" (one core, plain) is now 2.46×
  (plain) and 3.16× (PGO), plus whole-iteration timings.
- **The advisor on eight threads** is now published (39.6 ms on two whole L3
  domains, 39.4 ms pinned), after the CPU-mask problem described in the
  earlier document was fixed in the harness.
