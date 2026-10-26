//! GH #604 — the search must pay to keep the loot a thief is carrying.
//!
//! The ThievingHopper steals a run-deck card on THIEVERY and, four moves
//! later, plays `Intent::Escape`. `SwipePower.BeforeDeath` gives the card
//! back if you kill it first; a flee keeps it for the rest of the run.
//!
//! The engine models an escape by zeroing the fleeing creature's HP (bead
//! `sts2-rs-8f336`), so every HP-shaped term in the combat eval reads a flee
//! as a kill — and when the thief is the last enemy standing, its flee ENDS
//! the combat as a win with the same HP, potions and round as the kill would
//! have. Before the loot charge the two lines were literally the same value
//! and the search had no reason to prefer either. This test pins that they
//! are now separated, and that `loot_weight = 0.0` puts them back together.

mod common;

use common::build_handful_ex;
use sts2_core::cards::{CardId, TargetType};
use sts2_core::{Action, Intent};
use sts2_mcts::{Mcts, MctsConfig};

const SIMS: u32 = 600;
const SEEDS: &[u64] = &[0, 1, 2];

/// One Strike, exactly lethal on a thief that is telegraphing its escape.
/// Player HP is high and the escape deals no damage, so nothing but the
/// stolen card distinguishes "kill it" from "let it go".
fn scenario(seed: u64) -> sts2_core::CombatState {
    let mut state = build_handful_ex(
        seed,
        &[(CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy)],
        6,  // enemy HP — exactly one Strike
        70, // player HP — never in danger
        Intent::Escape,
        &[],
        1, // energy — the Strike is the only affordable play
    );
    let thief = state.enemy_uids[0];
    state.record_swipe_steal(thief, CardId::Bash, false);
    state
}

fn q_gap(seed: u64, loot_weight: f32) -> (f32, f32, Action) {
    let state = scenario(seed);
    let mut mcts = Mcts::new(
        state,
        MctsConfig {
            max_simulations: SIMS,
            seed,
            loot_weight,
            ..MctsConfig::default()
        },
    );
    let picked = mcts.search();
    let kill = mcts
        .root_q_for_action(&Action::PlayCard {
            hand_index: 0,
            target: Some(0),
        })
        .expect("Strike is a root child");
    let let_it_go = mcts
        .root_q_for_action(&Action::EndTurn)
        .expect("EndTurn is a root child");
    (kill, let_it_go, picked)
}

#[test]
fn search_kills_the_thief_rather_than_let_it_flee_with_the_card() {
    for &seed in SEEDS {
        let (kill, let_it_go, picked) = q_gap(seed, MctsConfig::default().loot_weight);
        assert!(
            kill > let_it_go,
            "seed {seed}: killing the thief must back up above letting it flee \
             (kill={kill}, flee={let_it_go})"
        );
        assert!(
            matches!(picked, Action::PlayCard { hand_index: 0, .. }),
            "seed {seed}: search picked {picked:?} instead of the lethal Strike"
        );
    }
}

/// The rollback seam: with the charge off, the two lines are the same win —
/// same HP, same potions, same round — so their root Q values coincide.
/// That equality is precisely the blindness GH #604 reported.
#[test]
fn loot_blind_search_cannot_tell_the_kill_from_the_flee() {
    for &seed in SEEDS {
        let (kill, let_it_go, _) = q_gap(seed, 0.0);
        assert!(
            (kill - let_it_go).abs() < 1e-3,
            "seed {seed}: without the loot charge the kill and the flee must be \
             indistinguishable (kill={kill}, flee={let_it_go})"
        );
    }
}
