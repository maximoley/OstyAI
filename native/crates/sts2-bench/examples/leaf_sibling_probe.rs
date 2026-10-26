//! `leaf_sibling_probe` — does a leaf evaluator rank sibling combat states
//! the way a 50k-sim oracle ranks the actions that lead to them?
//!
//! D2 Part 3 screen S1 (GH #453,
//! `docs/experiments/2026-08-19-d2-part3-learned-guidance.md`). The leaf's
//! only job inside the search is ranking sibling subtrees, so the cheapest
//! falsifiable question about any leaf candidate — Texel, TexelWide, or a
//! learned net — is asked here directly: for each book position, apply each
//! legal action to a clone, evaluate the child with the leaf, and score the
//! argmax against the book's double-oracle gold action. A learned value
//! leaf that cannot beat `TexelWide` on this probe cannot help the search
//! anywhere, and the campaign dies before any win-rate compute (the D1
//! prerequisite pattern, #452).
//!
//! Caveat by construction: each child is a SINGLE realization of the
//! action's stochastic effects (the clone's own RNG), so absolute agreement
//! undershoots what the search achieves by averaging over determinizations.
//! Every candidate carries the same handicap, so the comparison between
//! leaves is fair; only cross-probe comparisons of absolute numbers are not.
//!
//! ```sh
//! cargo run --release -p sts2-bench --example leaf_sibling_probe -- \
//!     [book_json] [pool_root] [learned_weights.json ...]
//! ```

#[path = "common/mod.rs"]
mod common;

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use sts2_cli::scenario_to_combat_state;
use sts2_cli::sim::scenario::toml_file::load_scenario_file;
use sts2_core::{apply, legal_actions, CombatPhase, CombatState};
use sts2_mcts::{EmbeddingValueNet, LeafEval, LearnedValueNet};

#[derive(Debug, Clone, Deserialize)]
struct Book {
    chance_agreement: f64,
    #[serde(default)]
    max_warmup_plies: u32,
    entries: Vec<BookEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct BookEntry {
    character: String,
    fixture_rel_path: String,
    seed: u64,
    state_hash: u64,
    gold_action: String,
}

fn realize(pool_root: &PathBuf, e: &BookEntry, warmup: u32) -> Option<CombatState> {
    let scenario = load_scenario_file(&pool_root.join(&e.fixture_rel_path)).ok()?;
    let mut state = scenario_to_combat_state(&scenario, e.seed).ok()?;
    let _ = sts2_core::combat_loop::start_combat(&mut state);
    let _ = sts2_core::combat_loop::start_player_turn(&mut state);
    common::warmup_plies(&mut state, e.seed, warmup);
    if state.is_over || state.phase != CombatPhase::PlayerPlayPhase {
        return None;
    }
    if sts2_core::state_hash(&state) != e.state_hash {
        return None;
    }
    Some(state)
}

/// 1-ply child value: terminal outcomes dominate any in-progress score
/// (mirroring how the search treats proven terminals), everything else is
/// the candidate leaf on the child state. `LeafEval::evaluate` already
/// short-circuits terminal LOSS to `TERMINAL_LOSS_LEAF`; the win side needs
/// the explicit clamp because `evaluate_in_progress*` would otherwise score
/// the dead-enemy state's feature vector.
fn child_value(leaf: &LeafEval, child: &CombatState) -> f32 {
    if child.is_over && child.player_won {
        return 10.0;
    }
    leaf.evaluate(child)
}

struct Candidate {
    label: String,
    leaf: LeafEval,
}

fn main() -> std::io::Result<()> {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    let book_path = PathBuf::from(
        it.next()
            .cloned()
            .unwrap_or_else(|| "data/combat_position_book.json".to_string()),
    );
    let pool_root = PathBuf::from(
        it.next()
            .cloned()
            .unwrap_or_else(|| "data/run-fixtures".to_string()),
    );

    let mut candidates = vec![
        Candidate {
            label: "texel".to_string(),
            leaf: LeafEval::Texel,
        },
        Candidate {
            label: "texelwide".to_string(),
            leaf: LeafEval::TexelWide,
        },
    ];
    // Remaining args: learned-net weight files. Accept either net class,
    // same both-errors contract as `policy_prior_probe`.
    for w in it {
        let path = PathBuf::from(w);
        let leaf = match LearnedValueNet::from_weights_path(&path) {
            Ok(net) => LeafEval::Learned(Arc::new(net)),
            Err(wide_err) => match EmbeddingValueNet::from_weights_path(&path) {
                Ok(net) => LeafEval::LearnedEmbed(Arc::new(net)),
                Err(embed_err) => panic!(
                    "{}: not loadable as either net class.\n  wide: {wide_err}\n  embedding: {embed_err}",
                    path.display()
                ),
            },
        };
        candidates.push(Candidate {
            label: path
                .file_name()
                .map_or_else(|| w.clone(), |f| f.to_string_lossy().into_owned()),
            leaf,
        });
    }

    let book: Book = serde_json::from_slice(&std::fs::read(&book_path)?).expect("book parses");

    std::panic::set_hook(Box::new(|_| {}));

    // Realize each book position once; all candidates score the same
    // positions, so per-candidate numbers stay paired.
    let mut positions: Vec<(&BookEntry, CombatState)> = Vec::new();
    for e in &book.entries {
        let Some(state) =
            std::panic::catch_unwind(|| realize(&pool_root, e, book.max_warmup_plies))
                .ok()
                .flatten()
        else {
            continue;
        };
        if legal_actions(&state).len() >= 2 {
            positions.push((e, state));
        }
    }

    println!(
        "book: {} ({} entries)",
        book_path.display(),
        book.entries.len()
    );
    println!("scored positions: {}", positions.len());
    let mut uniform_expectation = 0.0_f64;
    for (_, state) in &positions {
        #[allow(clippy::cast_precision_loss)]
        {
            uniform_expectation += 1.0 / legal_actions(state).len() as f64;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let n = (positions.len().max(1)) as f64;
    println!("uniform expectation : {:.3}", uniform_expectation / n);
    println!("book chance_agreement: {:.3}", book.chance_agreement);

    // Optional per-position dump so the pooled number can be sliced by
    // anything the book carries a per-position statistic for — e.g. joining
    // `data/book_volatility.json` on `state_hash` to ask whether the leaf is
    // better or worse than pooled on the NEAR-TIE positions, which is the
    // only subset where a tie-break would ever fire. Opt-in; absent env var
    // leaves the probe byte-identical.
    let jsonl_path = std::env::var("STS2_LEAF_PROBE_JSONL").ok();
    let mut jsonl_rows: Vec<String> = Vec::new();

    for cand in &candidates {
        let mut top1 = 0_u32;
        let mut gold_missing = 0_u32;
        let mut by_char: std::collections::BTreeMap<String, (u32, u32)> =
            std::collections::BTreeMap::new();
        for (e, state) in &positions {
            let actions = legal_actions(state);
            let mut best_i = 0_usize;
            let mut best_v = f32::MIN;
            let mut gold_i: Option<usize> = None;
            for (i, a) in actions.iter().enumerate() {
                // A child whose apply panics ranks below every scored
                // sibling — same effect as the search dropping the edge.
                let v = std::panic::catch_unwind(|| {
                    let mut child = state.clone();
                    let _ = apply(&mut child, a.clone());
                    child_value(&cand.leaf, &child)
                })
                .unwrap_or(f32::MIN);
                if v > best_v {
                    best_v = v;
                    best_i = i;
                }
                if format!("{a:?}") == e.gold_action {
                    gold_i = Some(i);
                }
            }
            let entry = by_char.entry(e.character.clone()).or_insert((0, 0));
            entry.0 += 1;
            let outcome = match gold_i {
                Some(g) if g == best_i => {
                    top1 += 1;
                    entry.1 += 1;
                    "hit"
                }
                Some(_) => "miss",
                None => {
                    gold_missing += 1;
                    "gold_not_legal"
                }
            };
            if jsonl_path.is_some() {
                jsonl_rows.push(format!(
                    "{{\"candidate\":\"{}\",\"state_hash\":{},\"character\":\"{}\",\"n_legal\":{},\"outcome\":\"{}\"}}",
                    cand.label,
                    e.state_hash,
                    e.character,
                    actions.len(),
                    outcome
                ));
            }
        }
        println!("\n=== {} ===", cand.label);
        println!("  1-ply top-1 == gold : {:.3}", f64::from(top1) / n);
        if gold_missing > 0 {
            println!("  gold action not among legal actions: {gold_missing}");
        }
        println!("  per character:");
        for (ch, (n_ch, hits)) in &by_char {
            let d = f64::from(*n_ch).max(1.0);
            println!("    {ch:<12} n={n_ch:<4} top1={:.3}", f64::from(*hits) / d);
        }
    }

    if let Some(path) = &jsonl_path {
        std::fs::write(path, jsonl_rows.join("\n") + "\n")?;
        println!("\nper-position rows -> {path} ({} lines)", jsonl_rows.len());
    }

    println!(
        "\nRead: the leaf's only job in the search is ranking sibling \n\
         subtrees. A learned candidate at or below `texelwide` here is a \n\
         kill (D2 Part 3 screen S1/S3); above it, proceed to the fork-fatal \n\
         panel before any win-rate gate."
    );
    Ok(())
}
