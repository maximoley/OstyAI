// Needs the ONNX runtime (`ppo::net`), compiled out without the `ppo` feature.
#![cfg(feature = "ppo")]
//! Every committed champion must load, and its categorical embedding tables
//! must be the right height for the vocabularies this build indexes them with.
//!
//! This is the tripwire for GH #544. The PPO vocabs are derived from the
//! content on purpose, so **adding content re-numbers the embedding rows** —
//! sorted registries shift every id after the insertion point. Porting
//! Underdocks as a second Act 1 adds three boss ids and moves five existing
//! ones onto different rows.
//!
//! When that lands, this test goes red and names the champion and the table.
//! That is the intended outcome, not a flake: **re-export/re-bake the nets,
//! never relax the assertion.** Silencing it puts champions back to reading a
//! different boss's embedding and answering plausibly from it.

use sts2_core::potions::CharacterId;
use sts2_runai::ppo::net::{onnx_model_path_for, PpoNet};

#[test]
fn every_committed_champion_loads_with_matching_embedding_heights() {
    let mut checked = 0;
    for character in [
        CharacterId::Ironclad,
        CharacterId::Silent,
        CharacterId::Defect,
        CharacterId::Regent,
        CharacterId::Necrobinder,
    ] {
        let Some(path) = onnx_model_path_for(character) else {
            continue; // no sibling checkout (standalone worktree) — nothing to check
        };
        match PpoNet::load(&path) {
            Ok(net) => {
                checked += 1;
                eprintln!("{character:?}: cat_dim={} OK", net.cat_dim());
            }
            Err(e) => panic!(
                "{character:?} champion at {} failed to load: {e}",
                path.display()
            ),
        }
    }
    eprintln!("{checked} champions checked");
}
