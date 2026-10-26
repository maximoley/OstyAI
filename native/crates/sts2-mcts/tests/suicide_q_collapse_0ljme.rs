//! Diagnose-only repro for bead `sts2-rs-0ljme`: MCTS suicide pattern
//! at lethal-HP. With `TerminalEvalMode::RichBoth` (default since
//! `sts2-rs-v6bo2`), a Bloodletting child whose post-play state is
//! `is_over=true, player_won=false` should back up
//! `-1.0 + 0.5 * leaf_eval.evaluate(&terminal_state)` per
//! `crates/sts2-mcts/src/lib.rs:1724-1730`, landing in the
//! `[-1.45, -0.55]` band documented at the
//! `MAX_LEAF_VALUE_ABS = 1.5` invariant (`lib.rs:389`).
//!
//! The fight-replay batches (`target/fight-replays-mistakes/`,
//! 2026-05-21 sims=50000 depth=20 c_puct=1.0 seed=1) reported
//! Q ~= -0.887 on the suicide child -- 0.5+ above the expected band
//! and only 0.003 better than the EndTurn alternative, enough to
//! flip selection.
//!
//! This file is the diagnostic harness: it builds the hand-rigged
//! HP=1 scenario, runs MCTS, prints every `RootOutcome`, probes
//! `evaluate_in_progress` directly on the post-Bloodletting terminal
//! state, and pins observations the follow-up fix bead can target.
//!
//! No fix lives here -- see the follow-up bead filed alongside this
//! test for the actual remediation.

mod common;

use common::build_handful_ex;
use sts2_core::cards::{CardId, TargetType};
use sts2_core::{evaluate_in_progress, Action, PotionKind};
use sts2_mcts::{LeafEval, Mcts, MctsConfig, TerminalEvalMode};

/// Build the lethal-HP fixture used by the bead: HP=1, hand=[Defend,
/// Bloodletting, Strike] vs a Cultist with `Attack { damage: 6 }`
/// intent. Bloodletting is the suicide line -- at HP=1 the 3-HP
/// self-damage drops HP to -2, `apply_damage_to_player` flips
/// `is_over=true, player_won=false`. EndTurn is the safe(r) line --
/// the cultist's 6-damage attack also kills the player, so the
/// position is lost either way, but the relative magnitudes determine
/// whether MCTS picks the *immediate* suicide or the *delayed* one.
fn lethal_hp_fixture() -> sts2_core::CombatState {
    build_handful_ex(
        1,
        &[
            (CardId::DefendIronclad, 0, 5, 1, TargetType::Self_),
            (CardId::Bloodletting, 0, 0, 0, TargetType::Self_),
            (CardId::StrikeIronclad, 6, 0, 1, TargetType::Enemy),
        ],
        /* cultist_hp= */ 50,
        /* player_hp= */ 1,
        sts2_core::Intent::Attack { damage: 6 },
        &[] as &[PotionKind],
        /* energy= */ 3,
    )
}

/// Bead `sts2-rs-0ljme`: probe `evaluate_in_progress` directly on a
/// HP=0 terminal-loss state so we can compute the *expected* loss-side
/// backup value (`-1.0 + 0.5 * leaf`) before running MCTS.
///
/// `evaluate_in_progress` does NOT short-circuit on `state.is_over`
/// (the only `is_over` guards are in `legal_actions` /
/// `terminal_value`). So the Texel HCE evaluates the feature vector
/// of the dead-player state as-is.
#[test]
fn probe_leaf_eval_on_terminal_loss_state() {
    sts2_core::card_effects::ensure_installed();
    sts2_content::cards::dispatch::install_default_registry();

    let mut state = lethal_hp_fixture();
    state.player.current_hp = 0;
    state.is_over = true;
    state.player_won = false;

    let leaf = evaluate_in_progress(&state);
    let backup = -1.0_f32 + 0.5 * leaf;
    eprintln!(
        "[0ljme] terminal-loss state: leaf_eval={leaf:.4}, \
         RichBoth backup = -1.0 + 0.5*leaf = {backup:.4} \
         (expected band: [-1.45, -0.55] per lib.rs:1717)"
    );

    let in_band = (-1.45..=-0.55).contains(&backup);
    eprintln!("[0ljme] backup in documented loss band [-1.45, -0.55]? {in_band}");
}

/// Bead `sts2-rs-0ljme` repro: run MCTS on the lethal-HP fixture and
/// dump `root_outcomes()` for every legal action.
///
/// Post-fix regression test (bead `sts2-rs-pvuwv`): with the
/// terminal-loss leaf-eval guard in [`sts2_mcts::value_net::LeafEval::evaluate`]
/// active, the Bloodletting child's mean Q lands inside the
/// documented `[-1.45, -0.55]` band — the backup is now
/// `-1.0 + 0.5 * TERMINAL_LOSS_LEAF (-0.9) = -1.45` exactly on the
/// terminal-loss leaf, plus contributions from any non-terminal
/// sub-tree expansions.
#[test]
fn bloodletting_at_hp1_q_should_be_inside_loss_band() {
    let state = lethal_hp_fixture();

    let config = MctsConfig {
        c_puct: 1.0,
        max_simulations: 5_000,
        max_rollout_depth: 20,
        terminal_eval_mode: TerminalEvalMode::RichBoth,
        seed: 1,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state, config);
    let _ = mcts.search();

    let outcomes = mcts.root_outcomes();
    eprintln!("[0ljme] root_outcomes ({} children):", outcomes.len());
    for o in &outcomes {
        eprintln!(
            "[0ljme]   action={:?} visits={} W/L/T={}/{}/{} \
             value_sum={:.3} mean_value={:.4}",
            o.action, o.visits, o.wins, o.losses, o.ties, o.value_sum, o.mean_value
        );
    }

    let bloodletting = outcomes
        .iter()
        .find(|o| matches!(o.action, Action::PlayCard { hand_index: 1, .. }))
        .expect("Bloodletting (hand_index=1) should be a legal action");

    eprintln!(
        "[0ljme] Bloodletting mean_value={:.4} \
         (expected in [-1.45, -0.55] under RichBoth proven_loss backup)",
        bloodletting.mean_value
    );

    assert!(
        bloodletting.mean_value <= -0.55,
        "Bloodletting Q={:.4} is above the documented loss band ceiling \
         (-0.55) -- confirms Q-collapse bug from sts2-rs-0ljme. \
         Expected Q ~= -1.0 + 0.5*leaf for HP=0 state per lib.rs:1724-1730.",
        bloodletting.mean_value,
    );
}

/// Bead `sts2-rs-0ljme` regression probe: ALWAYS runs (not ignored)
/// and just dumps the diagnostic numbers without asserting expected
/// behavior. Lets `cargo test -- --nocapture` surface the current
/// state of the gap without breaking the suite. The follow-up fix
/// bead flips the `bloodletting_at_hp1_q_should_be_inside_loss_band`
/// test's `#[ignore]` once the fix lands.
#[test]
fn probe_bloodletting_q_observable() {
    let state = lethal_hp_fixture();

    let config = MctsConfig {
        c_puct: 1.0,
        max_simulations: 2_000,
        max_rollout_depth: 20,
        terminal_eval_mode: TerminalEvalMode::RichBoth,
        seed: 1,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(state, config);
    let _ = mcts.search();

    let outcomes = mcts.root_outcomes();
    let bloodletting_q = outcomes
        .iter()
        .find(|o| matches!(o.action, Action::PlayCard { hand_index: 1, .. }))
        .map(|o| (o.visits, o.mean_value));
    let endturn_q = outcomes
        .iter()
        .find(|o| matches!(o.action, Action::EndTurn))
        .map(|o| (o.visits, o.mean_value));

    eprintln!(
        "[0ljme probe] sims=2000 RichBoth c_puct=1.0 depth=20 \
         seed=1 -> Bloodletting={bloodletting_q:?} EndTurn={endturn_q:?}"
    );

    assert!(
        outcomes.iter().any(|o| o.visits > 0),
        "MCTS should expand at least one root child"
    );
}

/// Belt-and-braces probe: confirm `LeafEval::Texel` is the default
/// and that on a terminal-loss state it now returns the
/// `TERMINAL_LOSS_LEAF` clamp (bead `sts2-rs-pvuwv`), NOT the raw
/// `evaluate_in_progress` score. Pre-pvuwv this test asserted the
/// passthrough behavior that fed the Q-collapse bug; post-fix it
/// pins the inverted contract.
///
/// For non-terminal dispatch passthrough, see
/// `crates/sts2-mcts/src/value_net.rs::tests::leaf_eval_texel_matches_evaluate_in_progress`.
#[test]
fn leaf_eval_default_clamps_on_terminal_loss() {
    use sts2_mcts::value_net::TERMINAL_LOSS_LEAF;
    let leaf = LeafEval::default();
    let mut state = lethal_hp_fixture();
    state.player.current_hp = 0;
    state.is_over = true;
    state.player_won = false;

    let via_leaf = leaf.evaluate(&state);
    let direct = evaluate_in_progress(&state);
    assert!(
        (via_leaf - TERMINAL_LOSS_LEAF).abs() < 1e-6,
        "post-pvuwv: LeafEval::default() must clamp terminal-loss to \
         TERMINAL_LOSS_LEAF ({TERMINAL_LOSS_LEAF}) -- got via_leaf={via_leaf}, \
         unguarded direct={direct}"
    );
    assert!(
        (via_leaf - direct).abs() > 1e-3,
        "the clamp must differ from the raw evaluate_in_progress score \
         on this fixture -- otherwise the guard isn't exercised. \
         via_leaf={via_leaf}, direct={direct}"
    );
}
