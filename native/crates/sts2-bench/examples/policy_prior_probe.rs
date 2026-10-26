//! `policy_prior_probe` — is a learned policy prior pointing at good actions?
//!
//! `mcts_exploration_arms` scores a prior by what the *search* does with it,
//! which cannot separate two very different failures:
//!
//! * the prior is **weak** — it ranks actions better than chance, just not well
//!   enough to beat the search's own noisy values, or
//! * the prior is **misaligned** — the slot the trainer wrote a target into is
//!   not the slot [`action_to_slot`] reads at inference, so its argmax is
//!   uncorrelated (or anti-correlated) with the right answer.
//!
//! Both show up as "the arm loses". Only the second is a bug, and it is a
//! silent one: every artifact still loads, every probability is finite and
//! sums to 1, and the corpus's own `slot_collision_rate` looks healthy.
//!
//! This probe reads the prior directly on the book's positions and reports
//! top-1 agreement with the book's gold action, against the two references
//! that make the number interpretable: uniform-random over the legal actions
//! (what an uninformative prior scores) and the book's `chance_agreement`.
//!
//! ```sh
//! cargo run --release -p sts2-bench --example policy_prior_probe -- \
//!     <weights.json> [book_json] [pool_root]
//! ```

#[path = "common/mod.rs"]
mod common;

use std::path::PathBuf;

use serde::Deserialize;
use sts2_cli::scenario_to_combat_state;
use sts2_cli::sim::scenario::toml_file::load_scenario_file;
use sts2_core::{legal_actions, CombatPhase, CombatState};
use sts2_mcts::{EmbeddingValueNet, LearnedPolicyPrior, LearnedValueNet, Policy};

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

fn main() -> std::io::Result<()> {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut it = args.iter();
    let weights = PathBuf::from(
        it.next()
            .expect("usage: policy_prior_probe <weights.json> ..."),
    );
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

    // Accept either net class: the wide `LearnedValueNet` that
    // `value_policy_net.py` emits, and the `EmbeddingValueNet` that `sts2-py`
    // bundles as the shipped prior. Try wide first, fall back to embedding,
    // and report BOTH errors if neither loads — a probe that says only
    // "failed to load" for an asset of the other class sends you hunting the
    // wrong bug.
    let prior: Box<dyn Policy> = match LearnedValueNet::from_weights_path(&weights) {
        Ok(net) => Box::new(LearnedPolicyPrior::new(std::sync::Arc::new(net))),
        Err(wide_err) => match EmbeddingValueNet::from_weights_path(&weights) {
            Ok(net) => Box::new(LearnedPolicyPrior::new(std::sync::Arc::new(net))),
            Err(embed_err) => panic!(
                "{}: not loadable as either net class.\n  wide: {wide_err}\n  embedding: {embed_err}",
                weights.display()
            ),
        },
    };
    let book: Book = serde_json::from_slice(&std::fs::read(&book_path)?).expect("book parses");

    std::panic::set_hook(Box::new(|_| {}));
    let mut scored = 0_u32;
    let mut top1 = 0_u32;
    // Expected top-1 of an uninformative prior is `mean(1/k)`, not `1/mean(k)`
    // — the book's branching is skewed (5..18) and the two differ.
    let mut uniform_expectation = 0.0_f64;
    let mut mass_on_gold = 0.0_f64;
    let mut flat = 0_u32;
    // Per-character, because the shipped prior is character-GATED in
    // production (Ironclad only). A pooled number would answer a question
    // nobody is asking.
    let mut by_char: std::collections::BTreeMap<String, (u32, u32, f64)> =
        std::collections::BTreeMap::new();

    for e in &book.entries {
        let Some(state) =
            std::panic::catch_unwind(|| realize(&pool_root, e, book.max_warmup_plies))
                .ok()
                .flatten()
        else {
            continue;
        };
        let actions = legal_actions(&state);
        if actions.len() < 2 {
            continue;
        }
        let p = prior.prior(&state, &actions);
        let best = p
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map_or(0, |(i, _)| i);
        // A prior that returns the uniform fallback (every legal action mapped
        // to a zero slot) is not making a prediction; count it separately so it
        // cannot be read as either a hit or a miss.
        let spread =
            p.iter().copied().fold(f32::MIN, f32::max) - p.iter().copied().fold(f32::MAX, f32::min);
        if spread < 1e-6 {
            flat += 1;
        }
        scored += 1;
        #[allow(clippy::cast_precision_loss)]
        {
            uniform_expectation += 1.0 / actions.len() as f64;
        }
        #[allow(clippy::cast_precision_loss)]
        let row_uniform = 1.0 / actions.len() as f64;
        let entry = by_char.entry(e.character.clone()).or_insert((0, 0, 0.0));
        entry.0 += 1;
        entry.2 += row_uniform;
        for (i, a) in actions.iter().enumerate() {
            if format!("{a:?}") == e.gold_action {
                mass_on_gold += f64::from(p[i]);
                if i == best {
                    top1 += 1;
                    entry.1 += 1;
                }
            }
        }
    }

    let n = f64::from(scored.max(1));
    println!("weights: {}", weights.display());
    println!("scored:  {scored} of {} book positions", book.entries.len());
    println!("  prior top-1 == gold : {:.3}", f64::from(top1) / n);
    println!("  uniform expectation : {:.3}", uniform_expectation / n);
    println!("  book chance_agreement: {:.3}", book.chance_agreement);
    println!("  mean prior mass on gold: {:.3}", mass_on_gold / n);
    println!("  degenerate (flat) priors: {flat}");
    println!("\n  per character (top-1 vs its own uniform expectation):");
    for (ch, (n_ch, hits, unif)) in &by_char {
        let d = f64::from(*n_ch).max(1.0);
        println!(
            "    {ch:<12} n={n_ch:<4} top1={:.3}  uniform={:.3}",
            f64::from(*hits) / d,
            unif / d,
        );
    }
    println!(
        "\nRead: top-1 at or below the uniform expectation means the prior is \n\
         not ranking actions at all — suspect a slot-mapping mismatch between \n\
         the trainer's targets and `action_to_slot`, not a weak net."
    );
    Ok(())
}
