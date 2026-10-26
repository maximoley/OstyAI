//! Cross-sim bench "W2": the fixture and loop of sansb/sts2-sim's own
//! `sts-sim bench` workload (`ironclad_toadpoles_uniform_v1`,
//! `v0.111.0/engine/src/bench.rs` and
//! `fixtures/bench_ironclad_toadpoles_uniform_v1.json`) replayed on this
//! engine, as faithfully as the engine allows. The random player is NOT
//! theirs: see "Policy" below.
//!
//! Their workload: one fixed entry state (Ironclad starter deck at 68/80 HP,
//! no relic, against two Toadpoles at 26 and 25 HP, the front one opening
//! with SPIKEN and the back one with WHIRL, turn 1 with the hand
//! Strike, Strike, Defend, Strike, Strike and the draw pile, top first,
//! Strike, Defend, Defend, Bash, Defend), at ASCENSION 10: the fixture has
//! no `player.ascension` field and sansb's admission defaults it to 10
//! (`v0.111.0/engine/src/boundary.rs:636`), so the Toadpoles hit at their A9
//! `DeadlyEnemies` values (Spike Spit 4x3, Whirl 8; C# `Toadpole.cs:49,53`)
//! and the 26 HP is an A8 `ToughEnemies` roll (22-26; 21-25 below A8,
//! `Toadpole.cs:32,34`); 240,000 playouts after 2,000
//! untimed warm-up playouts; per transition: enumerate the legal actions
//! (their `legal_actions_into`, which is DEDUPLICATED: equal hand cards
//! collapse into one play and identical monsters into one target), pick one
//! uniformly with a SplitMix64 stream seeded per playout
//! (`seed ^ i * 0x9E3779B97F4A7C15`, seed `0x5354533250455246`), then
//! clone-and-apply (`apply_action_into` returns a new state); at most 64
//! actions per playout.
//!
//! Here: the same entry (the deal is not injected: the harness searches for
//! the first shuffle seed whose natural opening deal is exactly that hand
//! and draw order; the Toadpoles' HP is set with `hp_override`), at
//! `--ascension` (default 10, the fixture's), the same counts, cap, policy
//! RNG stream and stop accounting. The Toadpoles' AI is built the way a real run
//! builds it: the `sts2-cli` scenario builder spawns through
//! `run_bridge::spawn_roster` (GH #762), so `SpawnPosition.index` is the roster
//! index, index 0 is C#'s `IsFront` toad and opens SPIKEN, index 1 opens WHIRL,
//! as `ToadpolesWeak.GenerateMonsters`. Before that fix the builder gave both
//! toads index 0, so both opened SPIKEN; `--legacy-spawn` rebuilds that
//! (`common/legacy_spawn.rs`), and with `--ascension 0` it is this harness as
//! it was before PR #761. `--mode clone` is their
//! clone-and-apply per transition; `--mode inplace` applies to the
//! playout's own copy instead (our search's pattern); `both` runs clone
//! first.
//!
//! Policy: the stream picks uniformly over THIS engine's `legal_actions`,
//! which lists one action per card instance per target (as the shipped game
//! does), not over a deduplicated list. That is a different random player
//! from `sts-sim bench`'s, so the two do NOT match in distribution: their
//! deduplicated player ends its turn far more often (mean branching 3.5 vs
//! 5.8 here) and, at A10 on 240,000 playouts, wins 57.3% with 17.0 actions
//! per playout, where this harness wins 88.65% with 18.7. Only our own
//! sansb-side harness `xsim_w2_toadpoles --policy instance`
//! (`scripts/site-bench/xsim/sansb/`), which weights each offered action by
//! the (card instance, target) pairs it stands for, is the comparable random
//! player: 88.25% wins, 19.3 actions per playout. Compare timings against
//! that one, not against `sts-sim bench`.
//!
//! Deviations that cannot be removed:
//! * rules: this engine models game build v0.107.1, theirs v0.111.0 (no
//!   Toadpole change between the two was found);
//! * the engines' RNG states differ (reshuffles, any random roll), and their
//!   legal-action lists are ordered differently, so even against the
//!   `--policy instance` harness the same policy stream picks different
//!   moves: the workloads are comparable, not identical, and never match
//!   move for move.
//!
//! Cross-sim comparison only, not a perf anchor: the fight carries zero
//! relics, which hides relic-channel dispatch (16-27% of real elite/boss
//! wall) and overweights state-clone cost about 4x; see
//! `docs/perf/2026-10-13-engine-speed-exact-design.md`.
//!
//! Output: per-mode JSON lines, then one summary JSON line (the last line,
//! with `ascension`, `spawn` and the toads' opening moves). `transitions`,
//! `wins`, `stops` and `digest` depend only on the engine's behaviour, so two
//! exact engines must print identical values.
//!
//! ```text
//! cargo build --release -p sts2-bench --example xsim_toadpoles
//! taskset -c 20 target/release/examples/xsim_toadpoles --mode both --passes 5
//! ```

use std::time::Instant;

use clap::Parser;

use sts2_cli::sim::scenario::{MonsterAiArgs, MonsterSpawnArgs, MonsterSpec};
use sts2_cli::sim::{build_starter_state_from_monsters_with_ascension, PlayerClass};
use sts2_core::cards::CardId;
use sts2_core::combat_loop::{start_combat, start_player_turn};
use sts2_core::{apply, legal_actions, CardUid, CombatState, MonsterKind};

#[path = "common/legacy_spawn.rs"]
mod legacy_spawn;

#[derive(Parser, Debug)]
struct Args {
    /// Timed playouts per pass (theirs: 240,000).
    #[arg(long, default_value_t = 240_000)]
    playouts: u32,
    /// Action cap per playout (theirs: 64).
    #[arg(long, default_value_t = 64)]
    max_actions: u32,
    /// Policy seed (theirs: 0x5354533250455246).
    #[arg(long, default_value_t = 0x5354_5332_5045_5246)]
    seed: u64,
    /// Untimed warm-up playouts before each mode (theirs: 2,000).
    #[arg(long, default_value_t = 2_000)]
    warmup: u32,
    /// Timed passes per mode (their CLI runs one; their floor script keeps
    /// the best of 5).
    #[arg(long, default_value_t = 1)]
    passes: u32,
    /// `clone` (theirs: clone-and-apply per transition), `inplace`, or `both`.
    #[arg(long, default_value = "both")]
    mode: String,
    /// `all` (every content registry, as the run AI and `site_random_bench`)
    /// or `cards` (card registry only, as this harness did before 2026-10-14).
    #[arg(long, default_value = "all")]
    content: String,
    /// Shuffle seed of the entry state. Default: search from 0 for the
    /// first seed that deals the fixture's hand and draw order.
    #[arg(long)]
    entry_seed: Option<u64>,
    /// Smoke size: 5,000 playouts, 200 warm-up.
    #[arg(long)]
    quick: bool,
    /// Ascension of the fight. Default 10: the fixture's (it elides the
    /// field, which sansb's admission reads as A10).
    #[arg(long, default_value_t = 10)]
    ascension: u8,
    /// Rebuild the pre-#762 scenario builder's Toadpoles (both on slot 0's AI,
    /// so both open SPIKEN), for auditing the pre-fix harness.
    #[arg(long)]
    legacy_spawn: bool,
}

const S: CardId = CardId::StrikeIronclad;
const D: CardId = CardId::DefendIronclad;
const B: CardId = CardId::Bash;
/// The fixture's turn-1 hand, in draw order.
const HAND: [CardId; 5] = [S, S, D, S, S];
/// The fixture's draw pile, top (next draw) first.
const DRAW: [CardId; 5] = [S, D, D, B, D];
const TOAD_HP: [u32; 2] = [26, 25];

fn build(seed: u64, ascension: u8, legacy_spawn: bool) -> CombatState {
    let monsters: Vec<MonsterSpec> = (0..2)
        .map(|idx| MonsterSpec {
            kind: MonsterKind::Toadpole,
            hp_override: Some(TOAD_HP[idx]),
            // Index 0 is C#'s `IsFront = true` (opens SPIKEN), index 1 the
            // back toad (opens WHIRL), as `ToadpolesWeak.GenerateMonsters`.
            // The Toadpole AI reads only the spawn index (its roster index);
            // `ai_args` reaches Nibbits only.
            ai_args: MonsterAiArgs {
                is_alone: false,
                is_front: idx == 0,
            },
            args: MonsterSpawnArgs::None,
            slot: None,
        })
        .collect();
    let specs = monsters.clone();
    let mut s = build_starter_state_from_monsters_with_ascension(
        seed,
        monsters,
        PlayerClass::Ironclad,
        Some(68),
        Some(80),
        None,
        ascension,
    );
    if legacy_spawn {
        legacy_spawn::respawn_legacy(&mut s, &specs, seed);
    }
    let _ = start_combat(&mut s);
    let _ = start_player_turn(&mut s);
    s
}

fn ids(state: &CombatState, pile: &[CardUid]) -> Vec<CardId> {
    pile.iter()
        .map(|u| {
            state
                .cards
                .iter()
                .find(|c| c.uid == *u)
                .expect("pile uid in the card store")
                .card_id
        })
        .collect()
}

fn deals_fixture(state: &CombatState) -> bool {
    ids(state, &state.player.hand) == HAND && ids(state, &state.player.draw) == DRAW
}

/// Each monster's telegraphed move id, in roster order.
fn openers(state: &CombatState) -> Vec<&'static str> {
    state
        .enemy_uids
        .iter()
        .map(|uid| {
            state
                .creatures
                .iter()
                .find(|c| c.id == uid.0)
                .and_then(|c| c.ai.as_ref())
                .map_or("?", |ai| ai.current_move().id)
        })
        .collect()
}

/// sansb's `PolicyRng` (SplitMix64) and `trajectory_seed`, verbatim.
struct PolicyRng(u64);
impl PolicyRng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut w = self.0;
        w = (w ^ (w >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        w = (w ^ (w >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        w ^ (w >> 31)
    }
    #[allow(clippy::cast_possible_truncation)]
    fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}
fn trajectory_seed(seed: u64, i: u32) -> u64 {
    seed ^ u64::from(i).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

#[derive(Default)]
struct Tally {
    transitions: u64,
    enumerations: u64,
    legal_seen: u64,
    wins: u64,
    terminal: u64,
    budget: u64,
    no_legal: u64,
    digest: u64,
}

/// One playout, sansb's `play_one` loop (stop accounting included).
fn play_one(entry: &CombatState, seed: u64, max_actions: u32, clone: bool, t: &mut Tally) {
    let mut rng = PolicyRng(seed);
    let mut state = entry.clone();
    let mut steps = 0u64;
    let mut seen = 0u64;
    let mut stop = 1u64; // 0 terminal, 1 budget, 2 no legal action
    for _ in 0..max_actions {
        if state.is_over {
            stop = 0;
            break;
        }
        let actions = legal_actions(&state);
        t.enumerations += 1;
        seen += actions.len() as u64;
        if actions.is_empty() {
            stop = 2;
            break;
        }
        let a = actions[rng.below(actions.len())].clone();
        if clone {
            let mut next = state.clone();
            let _ = apply(&mut next, a);
            state = next;
        } else {
            let _ = apply(&mut state, a);
        }
        steps += 1;
    }
    match stop {
        0 => t.terminal += 1,
        1 => t.budget += 1,
        _ => t.no_legal += 1,
    }
    let won = state.player_won;
    t.transitions += steps;
    t.legal_seen += seen;
    t.wins += u64::from(won);
    // FNV-1a fold of (actions, legal actions seen, outcome) per playout.
    t.digest = (t.digest ^ ((seen << 24) ^ (steps << 3) ^ (stop << 1) ^ u64::from(won)))
        .wrapping_mul(0x0100_0000_01b3);
}

fn pass(entry: &CombatState, a: &Args, playouts: u32, clone: bool) -> (Tally, f64) {
    let mut t = Tally {
        digest: 0xcbf2_9ce4_8422_2325,
        ..Tally::default()
    };
    let t0 = Instant::now();
    for i in 0..playouts {
        play_one(
            entry,
            trajectory_seed(a.seed, i),
            a.max_actions,
            clone,
            &mut t,
        );
    }
    (t, t0.elapsed().as_secs_f64())
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

#[allow(clippy::cast_precision_loss)]
fn main() {
    let mut a = Args::parse();
    if a.quick {
        a.playouts = 5_000;
        a.warmup = 200;
    }
    assert!(
        a.passes >= 1 && a.playouts >= 1,
        "--passes and --playouts must be >= 1"
    );
    let modes: &[bool] = match a.mode.as_str() {
        "clone" => &[true],
        "inplace" => &[false],
        "both" => &[true, false],
        other => panic!("--mode must be clone, inplace or both, got {other:?}"),
    };
    match a.content.as_str() {
        "all" => sts2_runai::rollout::ensure_content_installed(),
        "cards" => sts2_content::cards::dispatch::ensure_installed(),
        other => panic!("--content must be all or cards, got {other:?}"),
    }

    let build_at = |s: u64| build(s, a.ascension, a.legacy_spawn);
    let (entry_seed, entry) = match a.entry_seed {
        Some(s) => (s, build_at(s)),
        None => (0..1_000_000u64)
            .map(|s| (s, build_at(s)))
            .find(|(_, st)| deals_fixture(st))
            .expect("no shuffle seed below 1,000,000 deals the fixture's opening"),
    };
    let hp: Vec<String> = entry
        .creatures
        .iter()
        .map(|c| format!("{}/{}", c.current_hp, c.max_hp))
        .collect();
    let name = |v: Vec<CardId>| format!("{v:?}");
    let spawn = if a.legacy_spawn { "legacy" } else { "run" };
    let opening = openers(&entry).join(",");
    eprintln!(
        "entry: seed={entry_seed} ascension={} spawn={spawn} openers={opening} fixture_deal={} creatures_hp={hp:?} hand={} draw={} relics={}",
        entry.ascension,
        deals_fixture(&entry),
        name(ids(&entry, &entry.player.hand)),
        name(ids(&entry, &entry.player.draw)),
        entry.relics.len()
    );

    let mut total_tx = 0u64;
    let mut total_playouts = 0u64;
    let mut total_secs = 0.0f64;
    let mut blocks = Vec::new();
    for &clone in modes {
        let label = if clone { "clone" } else { "inplace" };
        let _ = pass(&entry, &a, a.warmup, clone); // untimed warm-up
        let mut rates = Vec::new();
        let mut secs_sum = 0.0;
        let mut first: Option<Tally> = None;
        for _ in 0..a.passes {
            let (t, secs) = pass(&entry, &a, a.playouts, clone);
            rates.push(t.transitions as f64 / secs);
            secs_sum += secs;
            if let Some(f) = &first {
                assert_eq!(f.digest, t.digest, "{label}: passes did different work");
            } else {
                first = Some(t);
            }
        }
        let t = first.expect("at least one pass");
        let per_pass_tx = t.transitions;
        total_tx += per_pass_tx * u64::from(a.passes);
        total_playouts += u64::from(a.playouts) * u64::from(a.passes);
        total_secs += secs_sum;
        let best = rates.iter().copied().fold(0.0f64, f64::max);
        let med = median(&mut rates.clone());
        let playouts = f64::from(a.playouts);
        let block = format!(
            "{{\"mode\":\"{label}\",\"passes\":{},\"playouts\":{},\"transitions\":{},\"enumerations\":{},\"mean_branching\":{:.3},\"wins\":{},\"win_rate\":{:.4},\"actions_per_playout\":{:.3},\"stops\":{{\"terminal\":{},\"budget\":{},\"no_legal_actions\":{}}},\"digest\":\"{:016x}\",\"elapsed_s\":{:.4},\"tx_per_s_median\":{:.0},\"tx_per_s_best\":{:.0},\"playouts_per_s_median\":{:.1},\"tx_per_s_passes\":[{}]}}",
            a.passes,
            a.playouts,
            per_pass_tx,
            t.enumerations,
            t.legal_seen as f64 / t.enumerations.max(1) as f64,
            t.wins,
            t.wins as f64 / playouts,
            per_pass_tx as f64 / playouts,
            t.terminal,
            t.budget,
            t.no_legal,
            t.digest,
            secs_sum,
            med,
            best,
            med * playouts / per_pass_tx as f64,
            rates.iter().map(|r| format!("{r:.0}")).collect::<Vec<_>>().join(","),
        );
        println!("{block}");
        blocks.push(format!("\"{label}\":{block}"));
    }
    // Summary (last line): top-level totals cover every timed pass of every
    // mode, so transitions / elapsed_s is a valid rate for a scaling runner.
    println!(
        "{{\"bench\":\"xsim_toadpoles\",\"workload\":\"ironclad_toadpoles_uniform_v1\",\"ascension\":{},\"spawn\":\"{spawn}\",\"openers\":\"{opening}\",\"entry_seed\":{entry_seed},\"fixture_deal\":{},\"creatures_hp\":\"{}\",\"content\":\"{}\",\"mode\":\"{}\",\"max_actions\":{},\"warmup\":{},\"policy_seed\":{},\"playouts\":{total_playouts},\"transitions\":{total_tx},\"elapsed_s\":{total_secs:.4},\"tx_per_s\":{:.0},{}}}",
        entry.ascension,
        deals_fixture(&entry),
        hp.join(","),
        a.content,
        a.mode,
        a.max_actions,
        a.warmup,
        a.seed,
        total_tx as f64 / total_secs,
        blocks.join(","),
    );
}
