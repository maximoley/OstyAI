//! `mcts_corpus` — corpus generator for bead `sts2-rs-yuyc`.
//!
//! Builds the canonical 50-state A/B corpus (20 common + 20 elite +
//! 10 boss, all Ironclad starter deck, all from
//! `sts2_content::encounters::act1_overgrowth::ACT1_ENCOUNTERS`) and persists it
//! to `crates/sts2-bench/data/mcts_corpus.json`.
//!
//! Re-run after touching the corpus generator or the encounter
//! registry. Cheap (<1s on this hardware).
//!
//! ```sh
//! cargo run --release --example mcts_corpus
//! ```

#[path = "common/mod.rs"]
mod common;

use common::{
    build_default_corpus, git_short_sha, realize_state, write_json_pretty, Corpus, CorpusEntry,
};

fn main() -> std::io::Result<()> {
    let mut entries: Vec<CorpusEntry> = build_default_corpus();
    assert_eq!(entries.len(), 50, "corpus size invariant: 20 + 20 + 10");

    // Realize each state so the `state_hash` field is populated.
    // We discard the actual `CombatState` — the A/B harness rebuilds
    // it from the corpus entry at run time.
    for entry in &mut entries {
        let _ = realize_state(entry);
    }

    let corpus = Corpus {
        version: 1,
        git_sha: git_short_sha(),
        generator: "sts2-bench/examples/mcts_corpus.rs (sts2-rs-yuyc)".to_string(),
        entries,
    };

    let path = common::data_path("mcts_corpus.json");
    write_json_pretty(&path, &corpus)?;
    eprintln!(
        "[mcts_corpus] wrote {} entries to {}",
        corpus.entries.len(),
        path.display()
    );
    Ok(())
}
