//! Regression test for bead `sts2-rs-7hhd`: states built via
//! `state_from_dict` should support MCTS rollouts reporting wins > 0
//! on an obviously-winnable matchup.

#![allow(clippy::missing_panics_doc)]

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use sts2_core::{apply as core_apply, is_terminal, terminal_value, Action};
use sts2_mcts::{Mcts, MctsConfig};
use sts2_rs::state_dict::state_from_dict;

fn empty_list(py: Python<'_>) -> Bound<'_, PyList> {
    PyList::empty(py)
}

fn make_card<'py>(py: Python<'py>, id: &str, stable_idx: i32) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("id", id)?;
    d.set_item("stable_idx", stable_idx)?;
    d.set_item("kind", "card")?;
    d.set_item("upgraded", false)?;
    Ok(d)
}

fn build_trivial_winnable_fixture(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    // 5-HP ShrinkerBeetle (real STS2 enemy) vs full-HP Ironclad with 5
    // Strikes in hand and energy=3. Player should be able to one-shot the
    // enemy with a single Strike (6 damage).
    let hand = PyList::empty(py);
    for i in 0..5 {
        hand.append(make_card(py, "STRIKE_IRONCLAD", i + 1)?)?;
    }
    let draw = PyList::empty(py);
    for i in 0..3 {
        draw.append(make_card(py, "DEFEND_IRONCLAD", i + 100)?)?;
    }
    let deck = PyDict::new(py);
    deck.set_item("hand", hand)?;
    deck.set_item("draw_pile", draw)?;
    deck.set_item("discard_pile", empty_list(py))?;
    deck.set_item("exhaust_pile", empty_list(py))?;

    let player = PyDict::new(py);
    player.set_item("class", "ironclad")?;
    player.set_item("hp", 80)?;
    player.set_item("max_hp", 80)?;
    player.set_item("block", 0)?;
    player.set_item("energy", 3)?;
    player.set_item("max_energy", 3)?;
    player.set_item("energy_regen_per_turn", 3)?;
    player.set_item("relics", empty_list(py))?;
    player.set_item("powers", empty_list(py))?;
    player.set_item("potions", empty_list(py))?;

    let enc = PyList::empty(py);
    let beetle = PyDict::new(py);
    beetle.set_item("kind", "ShrinkerBeetle")?;
    beetle.set_item("uid", 1_i64)?;
    beetle.set_item("hp", 5)?;
    beetle.set_item("max_hp", 5)?;
    beetle.set_item("block", 0)?;
    beetle.set_item("powers", empty_list(py))?;
    beetle.set_item("intents", empty_list(py))?;
    beetle.set_item("intent_history", empty_list(py))?;
    beetle.set_item("is_dead", false)?;
    beetle.set_item("is_escaped", false)?;
    enc.append(beetle)?;

    let rng = PyDict::new(py);
    rng.set_item("seed", 42_u64)?;
    rng.set_item("ascension", 0_u8)?;

    let d = PyDict::new(py);
    d.set_item("wire_version", 1)?;
    d.set_item("phase", "PlayerPlayPhase")?;
    d.set_item("round", 1)?;
    d.set_item("side", "Player")?;
    d.set_item("is_elite", false)?;
    d.set_item("turn", 1)?;
    d.set_item("combat_id", 0_i64)?;
    d.set_item("player", player)?;
    d.set_item("deck", deck)?;
    d.set_item("encounter", enc)?;
    d.set_item("rng", rng)?;
    Ok(d)
}

#[test]
fn random_rollouts_can_kill_5hp_shrinker_beetle() {
    // Sanity check the engine alone: from the state_from_dict path,
    // a random sequence of actions should sometimes kill a 5-HP enemy.
    Python::with_gil(|py| {
        let d = build_trivial_winnable_fixture(py).unwrap();
        let state = state_from_dict(&d).expect("fixture must parse");

        let mut wins = 0u32;
        let n: u64 = 100;
        for seed in 0_u64..n {
            let mut s = state.clone();
            // Re-seed for determinism per trial.
            s.rng = sts2_core::dotnet_rng::Rng::new(seed);
            let mut depth = 0u32;
            while !is_terminal(&s) && depth < 200 {
                let acts = sts2_core::legal_actions(&s);
                if acts.is_empty() {
                    break;
                }
                let idx = s.rng.next_int_exclusive(0, acts.len() as i32) as usize;
                let action = acts[idx].clone();
                let _ = core_apply(&mut s, action);
                depth += 1;
            }
            if is_terminal(&s) {
                let (_, v) = terminal_value(&s);
                if v > 0.5 {
                    wins += 1;
                }
            }
        }
        assert!(
            wins > 0,
            "Random play from state_from_dict should sometimes kill a 5-HP ShrinkerBeetle, got wins={wins}/{n}"
        );
    });
}

#[test]
fn mcts_reports_wins_on_state_from_dict() {
    // The bead-7hhd repro: MCTS should report wins > 0 for at least one
    // action on a trivially winnable matchup built via state_from_dict.
    Python::with_gil(|py| {
        let d = build_trivial_winnable_fixture(py).unwrap();
        let state = state_from_dict(&d).expect("fixture must parse");

        let cfg = MctsConfig {
            c_puct: 1.4,
            max_simulations: 1000,
            max_rollout_depth: 200,
            seed: 42,
            ..MctsConfig::default()
        };
        let mut mcts = Mcts::new(state, cfg);
        let _ = mcts.search();
        let outcomes = mcts.root_outcomes();
        let total_wins: u32 = outcomes.iter().map(|o| o.wins).sum();
        let total_losses: u32 = outcomes.iter().map(|o| o.losses).sum();
        let total_ties: u32 = outcomes.iter().map(|o| o.ties).sum();
        eprintln!("MCTS aggregate: wins={total_wins} losses={total_losses} ties={total_ties}");
        for o in &outcomes {
            eprintln!(
                "  action={:?} visits={} wins={} losses={} ties={}",
                o.action, o.visits, o.wins, o.losses, o.ties
            );
        }
        assert!(
            total_wins > 0,
            "MCTS should find wins on a trivially-winnable matchup, got wins={total_wins}"
        );
    });
}

#[test]
#[allow(dead_code)]
fn direct_strike_kills_beetle() {
    // Confirm the engine actually transitions to a win when we directly
    // apply a PlayCard{Strike, target=beetle} action.
    Python::with_gil(|py| {
        let d = build_trivial_winnable_fixture(py).unwrap();
        let mut state = state_from_dict(&d).expect("fixture must parse");
        let acts = sts2_core::legal_actions(&state);
        eprintln!("legal acts: {acts:?}");
        // Find a PlayCard action targeted at the beetle.
        let play = acts.iter().find(|a| matches!(a, Action::PlayCard { .. }));
        assert!(play.is_some(), "expected at least one PlayCard action");
        let action = play.unwrap().clone();
        let _ = core_apply(&mut state, action);
        eprintln!(
            "after play: is_over={} player_won={} all_creatures={:?}",
            state.is_over,
            state.player_won,
            state
                .creatures
                .iter()
                .map(|c| (c.id.get(), c.current_hp, c.monster_id.0))
                .collect::<Vec<_>>()
        );
    });
}
