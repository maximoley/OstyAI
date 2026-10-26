//! `mcts_permutation_probe` — how much of a turn's action tree is the
//! same position reached by a different card order?
//!
//! Most card plays within one turn commute: two Defends and a Strike
//! reach the same end-of-turn state in any order. Where that holds, the
//! root children are *genuinely* tied and the search is right to score
//! them equally — but each ordering still gets its own subtree unless
//! something merges them, so the budget fragments across duplicates and
//! the options that genuinely differ (`EndTurn`, the Bash, a potion)
//! get a smaller share.
//!
//! That matters for issue #442's diagnosis: it reads near-uniform visit
//! allocation as evidence the search never commits. If a large share of
//! the children are permutations of each other, part of that flatness
//! is correct behaviour and the value-rescale target has to be computed
//! over genuinely-distinct children instead.
//!
//! ## What this measures
//!
//! 1. **Merge factor** — enumerate every legal card-play *sequence* from
//!    the root up to `max_depth` plies and count how many distinct
//!    states they reach. `sequences / distinct` is how much redundant
//!    work the tree would do without merging.
//! 2. **Pairwise classification at 2 plies** — for each unordered pair
//!    of root card plays, run both orders and classify the outcome:
//!    identical (even RNG), merges under the transposition key but not
//!    under `state_hash` (i.e. the orders differ only in RNG cursor), or
//!    genuinely different — and for the last, which observable component
//!    differs.
//! 3. **Realized TT hit rate** — a real search at the deployed budget,
//!    reporting `tt_hits / sims`. The transposition table is forced on
//!    for every search (`sts2-mcts/src/lib.rs:1395`) and keyed on the
//!    RNG-cursor-insensitive [`sts2_core::search_hash`], so this is the
//!    number the stale GH #17 A/B (0.0000 hits/sim, measured 2026-05-20
//!    against the RNG-*sensitive* `state_hash`, a month before
//!    `search_hash` existed) should be replaced by.
//!
//! Both hashes are reported because their difference is itself the
//! finding: `search_hash` is `state_hash` minus the RNG sections, so a
//! pair that merges under one and not the other is a pair the old key
//! could never have unified.
//!
//! ```sh
//! cargo run --release -p sts2-bench --example mcts_permutation_probe -- \
//!     [positions] [max_depth] [clairvoyant|beliefdraw] [tt_sims]
//! ```

#[path = "common/mod.rs"]
mod common;

use std::collections::{BTreeMap, HashSet};

use rayon::prelude::*;
use sts2_content::cards::spec_index::IRONCLAD_SPECS;
use sts2_content::cards::CardSpec;
use sts2_core::{
    apply, legal_actions, search_hash, state_hash, Action, CardUid, CombatPhase, CombatState,
    StepResult,
};
use sts2_mcts::{HiddenInfoMode, Mcts, MctsConfig};

const DECK_SIZE: usize = 24;
/// Same encounter mix as `mcts_thread_agreement` / `mcts_decision_stability`,
/// so position-level numbers stay comparable across the three harnesses.
const ENCOUNTERS: [&str; 4] = [
    "FLYCONID_NORMAL",
    "MAWLER_NORMAL",
    "BYRDONIS_ELITE",
    "VANTOM_BOSS",
];

/// Ceiling on enumerated sequences per position. A wide hand at depth 3
/// is `k·(k-1)·(k-2)` states, each a full `CombatState` clone; without a
/// cap one pathological position would dominate the run. Positions that
/// hit the cap are counted and excluded from the merge-factor means
/// rather than silently truncated.
const MAX_SEQUENCES_PER_POSITION: usize = 4096;

fn random_deck(seed: u64) -> Vec<CardSpec> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let mut rng = sts2_core::dotnet_rng::Rng::new(seed ^ 0xDEC0_DEC0_DEC0_DEC0);
    (0..DECK_SIZE)
        .map(|_| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                clippy::cast_sign_loss
            )]
            let idx = rng.next_int_exclusive(0, IRONCLAD_SPECS.len() as i32) as usize;
            IRONCLAD_SPECS[idx]()
        })
        .collect()
}

fn base_config(sims: u32, seed: u64, belief: bool) -> MctsConfig {
    let common = MctsConfig {
        max_simulations: sims,
        seed,
        // The advisor's depth, not `MctsConfig::default()`'s 10 — at 10 a
        // rollout never reaches a terminal and every downstream number is
        // noise for the wrong reason.
        max_rollout_depth: 200,
        ..MctsConfig::default()
    };
    if belief {
        MctsConfig {
            hidden_info: HiddenInfoMode::BeliefDraw,
            c_puct: 2.5,
            draw_widening_c: 2.0,
            ..common
        }
    } else {
        common
    }
}

/// The card each legal `PlayCard` action would play, paired with the
/// action itself. Targeting is held fixed at the first legal target for
/// a given hand index: this probe asks about *ordering*, and letting the
/// target vary would conflate "different order" with "different play".
fn play_options(state: &CombatState) -> Vec<(CardUid, Action)> {
    let mut seen: HashSet<u8> = HashSet::new();
    let mut out = Vec::new();
    for action in legal_actions(state) {
        if let Action::PlayCard { hand_index, .. } = action {
            if !seen.insert(hand_index) {
                continue;
            }
            if let Some(uid) = state.player.hand.get(hand_index as usize) {
                out.push((*uid, action));
            }
        }
    }
    out
}

/// Play the card identified by `uid`, re-resolving its hand index in
/// `state` first — indices shift as cards leave the hand, so a stored
/// `Action::PlayCard { hand_index }` from a previous position would play
/// the wrong card.
///
/// Returns `None` when the line cannot continue: the card is no longer
/// in hand (drawn away, discarded, exhausted by the other play), it is
/// no longer affordable, the play opened a pending choice, or combat
/// ended. Those are genuine non-commutativity, counted separately rather
/// than folded into the "differs" bucket.
fn play_by_uid(state: &CombatState, uid: CardUid) -> Option<CombatState> {
    let idx = state.player.hand.iter().position(|u| *u == uid)?;
    #[allow(clippy::cast_possible_truncation)]
    let idx = idx as u8;
    let action = legal_actions(state)
        .into_iter()
        .find(|a| matches!(a, Action::PlayCard { hand_index, .. } if *hand_index == idx))?;
    let mut next = state.clone();
    match apply(&mut next, action) {
        StepResult::Advanced => {}
        _ => return None,
    }
    if next.is_over || next.phase != CombatPhase::PlayerPlayPhase {
        return None;
    }
    Some(next)
}

/// Why two states reached by opposite orderings are not the same state.
/// Ordered most-specific first; the first matching label is reported.
fn difference_label(a: &CombatState, b: &CombatState) -> &'static str {
    if a.phase != b.phase {
        return "phase";
    }
    if a.is_over != b.is_over {
        return "over";
    }
    if a.round != b.round {
        return "round";
    }
    // Piles first, but a pile whose contents match as a multiset and differ
    // only in sequence CANNOT be why the hashes disagree — `pile_mix` folds
    // each pile through an order-invariant commutative sum. Such a pile is
    // recorded and skipped so the scan keeps looking for the real cause;
    // reporting it would blame the one difference that provably does not
    // reach the hash.
    let mut order_only: Option<&'static str> = None;
    for (label, pa, pb) in [
        ("hand", a.player.hand.as_slice(), b.player.hand.as_slice()),
        ("draw", a.player.draw.as_slice(), b.player.draw.as_slice()),
        (
            "discard",
            a.player.discard.as_slice(),
            b.player.discard.as_slice(),
        ),
        (
            "exhaust",
            a.player.exhaust.as_slice(),
            b.player.exhaust.as_slice(),
        ),
        ("play", a.player.play.as_slice(), b.player.play.as_slice()),
    ] {
        if pa == pb {
            continue;
        }
        let (mut sa, mut sb) = (pa.to_vec(), pb.to_vec());
        sa.sort_unstable();
        sb.sort_unstable();
        if sa == sb {
            order_only.get_or_insert(match label {
                "hand" => "pile_order_only:hand",
                "draw" => "pile_order_only:draw",
                "discard" => "pile_order_only:discard",
                "exhaust" => "pile_order_only:exhaust",
                _ => "pile_order_only:play",
            });
            continue;
        }
        return match label {
            "hand" => "pile_content:hand",
            "draw" => "pile_content:draw",
            "discard" => "pile_content:discard",
            "exhaust" => "pile_content:exhaust",
            _ => "pile_content:play",
        };
    }
    if a.player.energy != b.player.energy {
        return "energy";
    }
    if a.player.block != b.player.block {
        return "block";
    }
    if a.player.current_hp != b.player.current_hp {
        return "player_hp";
    }
    if a.player.powers != b.player.powers {
        return "player_powers";
    }
    if a.player.potions != b.player.potions {
        return "potions";
    }
    if a.player.orbs != b.player.orbs {
        return "orbs";
    }
    if a.player != b.player {
        return "player_other";
    }
    if a.creatures != b.creatures {
        return "enemies";
    }
    if a.powers != b.powers {
        return "powers_arena";
    }
    if a.cards != b.cards {
        return "cards_arena";
    }
    // Every observable field this scan knows about matches, yet the hashes
    // disagree. If a pile differed in order only, that is the standing
    // suspect and the pile hash is not as order-blind as documented;
    // otherwise the hash is keyed on something not enumerated here.
    order_only.unwrap_or("hash_differs_no_field_found")
}

/// Enumerate every card-play sequence from `root` up to `max_depth`
/// plies, returning per-depth `(sequences, distinct_search_hash,
/// distinct_state_hash)`. `None` if the position blew the sequence cap.
fn merge_profile(root: &CombatState, max_depth: usize) -> Option<Vec<(usize, usize, usize)>> {
    let mut per_depth: Vec<(usize, HashSet<u64>, HashSet<u64>)> = (0..max_depth)
        .map(|_| (0, HashSet::new(), HashSet::new()))
        .collect();
    let mut frontier = vec![root.clone()];
    let mut total = 0usize;

    for slot in &mut per_depth {
        let mut next_frontier = Vec::new();
        for state in &frontier {
            for (uid, _) in play_options(state) {
                let Some(child) = play_by_uid(state, uid) else {
                    continue;
                };
                total += 1;
                if total > MAX_SEQUENCES_PER_POSITION {
                    return None;
                }
                slot.0 += 1;
                slot.1.insert(search_hash(&child));
                slot.2.insert(state_hash(&child));
                next_frontier.push(child);
            }
        }
        frontier = next_frontier;
        if frontier.is_empty() {
            break;
        }
    }

    Some(
        per_depth
            .into_iter()
            .map(|(seqs, sh, st)| (seqs, sh.len(), st.len()))
            .collect(),
    )
}

/// Card ids behind a pair of hand uids, ordered so `(a, b)` and `(b, a)`
/// produce the same key — commutation is a property of the unordered pair.
fn card_pair_key(state: &CombatState, ui: CardUid, uj: CardUid) -> Option<(u32, u32)> {
    let id = |u: CardUid| -> Option<u32> {
        let idx = u.0.get() as usize - 1;
        state.cards.get(idx).map(|c| c.card_id as u32)
    };
    let (a, b) = (id(ui)?, id(uj)?);
    Some(if a <= b { (a, b) } else { (b, a) })
}

/// Per-position pairwise result at 2 plies, plus per-card-pair commutation
/// tallies.
///
/// The tallies answer the question that decides whether ordering can be
/// canonicalized *without* simulating: is commutation a stable property of
/// the card pair, or does it depend on the board? A static predicate — the
/// only kind that saves the state build — can only exist if a given pair
/// commutes almost always or almost never. If the same pair commutes on
/// some boards and not others, no table can predict it and the merge has
/// to stay dynamic (i.e. the transposition table, which already does it).
///
/// Thorns is the standard counterexample: attack-then-block and
/// block-then-attack differ whenever retaliation damage lands on block in
/// one order and on HP in the other, for the same two cards.
type PairTally = BTreeMap<(u32, u32), (u32, u32)>;

fn pair_profile(root: &CombatState) -> (BTreeMap<&'static str, u32>, PairTally) {
    let mut counts: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut tally: PairTally = BTreeMap::new();
    let options = play_options(root);
    for i in 0..options.len() {
        for j in (i + 1)..options.len() {
            let (ui, uj) = (options[i].0, options[j].0);
            let ab = play_by_uid(root, ui).and_then(|s| play_by_uid(&s, uj));
            let ba = play_by_uid(root, uj).and_then(|s| play_by_uid(&s, ui));
            let (Some(ab), Some(ba)) = (ab, ba) else {
                *counts.entry("incomplete").or_default() += 1;
                continue;
            };
            let commutes = search_hash(&ab) == search_hash(&ba);
            let label = if commutes {
                if state_hash(&ab) == state_hash(&ba) {
                    "identical"
                } else {
                    "merges_via_search_hash"
                }
            } else {
                difference_label(&ab, &ba)
            };
            *counts.entry(label).or_default() += 1;
            // Only completable pairs carry a commutation verdict: an
            // incomplete pair is one where the second play was not even
            // available, which is a different phenomenon.
            if let Some(key) = card_pair_key(root, ui, uj) {
                let slot = tally.entry(key).or_insert((0, 0));
                slot.0 += 1;
                slot.1 += u32::from(commutes);
            }
        }
    }
    (counts, tally)
}

fn main() {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    let n_positions: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(180);
    let max_depth: usize = it.next().and_then(|s| s.parse().ok()).unwrap_or(3);
    let belief = !matches!(it.next().map(String::as_str), Some("clairvoyant"));
    let tt_sims: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(1000);

    eprintln!(
        "[permutation_probe] positions={n_positions} max_depth={max_depth} \
         mode={} tt_sims={tt_sims}",
        if belief { "beliefdraw" } else { "clairvoyant" }
    );

    // ── Build the position corpus ───────────────────────────────────
    // A fifth argument switches from synthesised positions to the harvested
    // on-manifold pool. It matters for the card-pair question below: the
    // synthetic mix is Ironclad on four Act-1 encounters with a uniformly
    // random 24-card deck, so most card pairs occur once or twice and the
    // board variety a "does this pair always commute?" test needs is absent.
    let mut states: Vec<CombatState> = Vec::new();
    if let Some(pool_root) = it.next() {
        let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir(pool_root)
            .expect("pool root readable")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.is_dir()
                    && p.file_name()
                        .and_then(|s| s.to_str())
                        .is_some_and(|s| s.starts_with("the-"))
            })
            .collect();
        dirs.sort();
        std::panic::set_hook(Box::new(|_| {}));
        for dir in dirs {
            let Ok(bytes) = std::fs::read(dir.join("corpus.json")) else {
                continue;
            };
            let Ok(corpus) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            let Some(entries) = corpus.get("entries").and_then(|e| e.as_array()) else {
                continue;
            };
            let per_dir = (n_positions as usize / 5).max(1);
            for entry in entries.iter().take(per_dir) {
                let (Some(path), Some(seed)) = (
                    entry.get("fixture_path").and_then(|v| v.as_str()),
                    entry.get("seed").and_then(serde_json::Value::as_u64),
                ) else {
                    continue;
                };
                let path = std::path::PathBuf::from(path);
                let built = std::panic::catch_unwind(|| {
                    let scenario =
                        sts2_cli::sim::scenario::toml_file::load_scenario_file(&path).ok()?;
                    let mut s = sts2_cli::scenario_to_combat_state(&scenario, seed).ok()?;
                    let _ = sts2_core::combat_loop::start_combat(&mut s);
                    let _ = sts2_core::combat_loop::start_player_turn(&mut s);
                    Some(s)
                })
                .ok()
                .flatten();
                if let Some(state) = built {
                    if state.phase == CombatPhase::PlayerPlayPhase
                        && !state.is_over
                        && play_options(&state).len() >= 2
                    {
                        states.push(state);
                    }
                }
            }
        }
        let _ = std::panic::take_hook();
        eprintln!(
            "[permutation_probe] loaded {} harvested positions",
            states.len()
        );
    }
    for p in 0..n_positions {
        if !states.is_empty() {
            break;
        }
        let encounter = ENCOUNTERS[p as usize % ENCOUNTERS.len()];
        let deck = random_deck(u64::from(p).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let engine_seed = u64::from(p) ^ 0xA5A5_A5A5_A5A5_A5A5;
        let mut state = common::build_ironclad_state_with_deck(encounter, engine_seed, &deck);
        common::warmup_random(&mut state, u64::from(p) ^ 0xDEAD_BEEF, 1);
        if state.phase != CombatPhase::PlayerPlayPhase {
            continue;
        }
        if legal_actions(&state).len() < 2 {
            continue;
        }
        // At least two card plays, or there is no ordering question here.
        if play_options(&state).len() < 2 {
            continue;
        }
        states.push(state);
    }
    let n = states.len();
    eprintln!("[permutation_probe] scored positions={n}");

    // ── Merge factor + pairwise classification ──────────────────────
    let results: Vec<(
        Option<Vec<(usize, usize, usize)>>,
        BTreeMap<&'static str, u32>,
        PairTally,
    )> = states
        .par_iter()
        .map(|s| {
            let (counts, tally) = pair_profile(s);
            (merge_profile(s, max_depth), counts, tally)
        })
        .collect();

    let capped = results.iter().filter(|(m, _, _)| m.is_none()).count();
    let mut depth_totals: Vec<(usize, usize, usize)> = vec![(0, 0, 0); max_depth];
    for (profile, _, _) in &results {
        let Some(profile) = profile else { continue };
        for (d, &(seqs, sh, st)) in profile.iter().enumerate() {
            depth_totals[d].0 += seqs;
            depth_totals[d].1 += sh;
            depth_totals[d].2 += st;
        }
    }

    let mut pair_totals: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut tally_totals: PairTally = BTreeMap::new();
    for (_, pairs, tally) in &results {
        for (k, v) in pairs {
            *pair_totals.entry(k).or_default() += v;
        }
        for (k, (seen, commuted)) in tally {
            let slot = tally_totals.entry(*k).or_insert((0, 0));
            slot.0 += seen;
            slot.1 += commuted;
        }
    }
    let pair_n: u32 = pair_totals.values().sum();

    // ── Realized transposition-table hit rate ───────────────────────
    // The table is forced on inside `Mcts::with_policy` regardless of
    // config, so this is the deployed behaviour, not an opt-in arm.
    let tt: Vec<(u64, u64)> = states
        .par_iter()
        .enumerate()
        .map(|(p, s)| {
            let cfg = base_config(
                tt_sims,
                (p as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
                belief,
            );
            let mut m = Mcts::new(s.clone(), cfg);
            let _ = m.search_n(tt_sims);
            (m.tt_hits(), m.nodes_allocated())
        })
        .collect();
    let tt_hits: u64 = tt.iter().map(|(h, _)| h).sum();
    let tt_nodes: u64 = tt.iter().map(|(_, a)| a).sum();
    let tt_sims_total = u64::from(tt_sims) * n as u64;

    // ── Report ──────────────────────────────────────────────────────
    println!("\n=== merge factor (card-play sequences from the root) ===");
    println!("positions={n} capped_and_excluded={capped}");
    println!(
        "{:>5}  {:>10}  {:>10}  {:>10}  {:>8}  {:>8}",
        "depth", "sequences", "distinct_s", "distinct_st", "merge_s", "merge_st"
    );
    for (d, &(seqs, sh, st)) in depth_totals.iter().enumerate() {
        if seqs == 0 {
            continue;
        }
        #[allow(clippy::cast_precision_loss)]
        let (ms, mst) = (seqs as f64 / sh as f64, seqs as f64 / st as f64);
        println!(
            "{:>5}  {seqs:>10}  {sh:>10}  {st:>10}  {ms:>8.3}  {mst:>8.3}",
            d + 1
        );
    }
    println!(
        "\nmerge_s = sequences / distinct search_hash (the transposition key).\n\
         merge_st = the same against state_hash (the RNG-sensitive key the\n\
         2026-05-20 GH #17 A/B measured). Their gap is what that A/B could\n\
         not have seen."
    );

    println!("\n=== 2-ply pairwise outcome of opposite orderings ===");
    println!("pairs={pair_n}");
    for (label, count) in &pair_totals {
        #[allow(clippy::cast_precision_loss)]
        let pct = 100.0 * f64::from(*count) / f64::from(pair_n.max(1));
        println!("{label:>24}  {count:>7}  {pct:>6.2}%");
    }

    // ── Is commutation a property of the card pair, or of the board? ──
    // Only card pairs seen enough times to have a verdict are counted; a
    // pair seen twice can look "always" by luck.
    const MIN_OBS: u32 = 5;
    let mut always = 0_u32;
    let mut never = 0_u32;
    let mut mixed = 0_u32;
    let mut mixed_obs = 0_u32;
    let mut total_obs = 0_u32;
    for (seen, commuted) in tally_totals.values() {
        if *seen < MIN_OBS {
            continue;
        }
        total_obs += seen;
        let rate = f64::from(*commuted) / f64::from(*seen);
        if rate >= 0.95 {
            always += 1;
        } else if rate <= 0.05 {
            never += 1;
        } else {
            mixed += 1;
            mixed_obs += seen;
        }
    }
    let decided = always + never + mixed;
    println!("\n=== is commutation predictable from the card pair alone? ===");
    println!(
        "card pairs with >={MIN_OBS} observations: {decided}  \
         (always {always}, never {never}, board-dependent {mixed})"
    );
    let all_obs: u32 = tally_totals.values().map(|(seen, _)| seen).sum();
    println!(
        "  coverage: these pairs carry {total_obs} of {all_obs} observations \
         ({:.1}%) across {} distinct pairs seen at all",
        100.0 * f64::from(total_obs) / f64::from(all_obs.max(1)),
        tally_totals.len()
    );
    if decided > 0 {
        println!(
            "  board-dependent share: {:.1}% of pairs, {:.1}% of observations",
            100.0 * f64::from(mixed) / f64::from(decided),
            100.0 * f64::from(mixed_obs) / f64::from(total_obs.max(1))
        );
        println!(
            "  A static table can only prune the 'always' pairs. Every\n\
             board-dependent pair must be decided by simulating, which is the\n\
             cost that avoidance was supposed to save."
        );
    }

    println!("\n=== realized transposition-table hits (deployed config) ===");
    #[allow(clippy::cast_precision_loss)]
    let per_sim = tt_hits as f64 / tt_sims_total as f64;
    #[allow(clippy::cast_precision_loss)]
    let per_node = tt_hits as f64 / (tt_hits + tt_nodes).max(1) as f64;
    println!(
        "sims={tt_sims_total} hits={tt_hits} nodes={tt_nodes} \
         hits/sim={per_sim:.4} hits/(hits+nodes)={per_node:.4}"
    );
}
