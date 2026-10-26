//! Pins that `mcts_play_to_terminal` actually threads the DEPLOYED honest
//! search config into `MctsConfig`. Regression guard for the autopsy bug:
//! the builder used to inherit `MctsConfig::default()`, i.e. Clairvoyant +
//! draw_widening_c 1.0, silently replaying a leaky search.

use sts2_mcts::{HiddenInfoMode, MctsConfig};
// Absolute (`::`) path: `use sts2_rs::sts2_rs` below binds the local name
// `sts2_rs` to the `#[pymodule]` entry point (required by
// `append_to_inittab!`), which shadows the crate name for any *relative*
// `sts2_rs::…` path written after it in this file — an unqualified second
// import collides and silently fails to resolve `parse_hidden_info` /
// `play_to_terminal_config`.
use ::sts2_rs::{parse_hidden_info, play_to_terminal_config};

#[test]
fn deployed_triple_reaches_the_config() {
    let hidden = parse_hidden_info("beliefdraw", "mcts_play_to_terminal").unwrap();
    let cfg = play_to_terminal_config(4000, 200, 7, 2.5, hidden, 2.0, 0);
    assert_eq!(cfg.hidden_info, HiddenInfoMode::BeliefDraw);
    assert!((cfg.draw_widening_c - 2.0).abs() < f32::EPSILON);
    assert!((cfg.c_puct - 2.5).abs() < f32::EPSILON);
    assert_eq!(cfg.max_simulations, 4000);
    assert_eq!(cfg.max_rollout_depth, 200);
}

#[test]
fn search_triple_defaults_match_mcts_config_for_existing_callers() {
    // boss_only.py / boss_sweep_parallel.py pass neither new param, so the
    // clairvoyant/1.0/sqrt(2) triple they inherit must still equal
    // `MctsConfig::default()`.
    //
    // Scope: this pins the SEARCH TRIPLE only, not the whole config. It is
    // deliberately NOT a claim that those callers are byte-identical overall
    let hidden = parse_hidden_info("clairvoyant", "mcts_play_to_terminal").unwrap();
    let cfg = play_to_terminal_config(1000, 200, 0, std::f32::consts::SQRT_2, hidden, 1.0, 0);
    let d = MctsConfig::default();
    assert_eq!(cfg.hidden_info, d.hidden_info);
    assert!((cfg.draw_widening_c - d.draw_widening_c).abs() < f32::EPSILON);
    assert!((cfg.c_puct - d.c_puct).abs() < f32::EPSILON);
}

#[test]
fn seed_is_decorrelated_per_action() {
    let hidden = HiddenInfoMode::BeliefDraw;
    let a = play_to_terminal_config(1000, 200, 5, 2.5, hidden, 2.0, 0);
    let b = play_to_terminal_config(1000, 200, 5, 2.5, hidden, 2.0, 1);
    assert_ne!(a.seed, b.seed, "each action must get its own search seed");
}

#[test]
fn unknown_hidden_info_is_rejected() {
    let err = parse_hidden_info("beliefdrawww", "mcts_play_to_terminal");
    assert!(err.is_err(), "a typo must not silently fall back");
}
