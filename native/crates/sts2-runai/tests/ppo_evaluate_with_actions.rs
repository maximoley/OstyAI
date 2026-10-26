// Needs the ONNX runtime (`ppo::net`), compiled out without the `ppo` feature.
#![cfg(feature = "ppo")]
//! `PpoPolicy::evaluate_with_actions` returns the masked candidate list
//! aligned with the probs, and is equivalent to `evaluate` on the
//! probs/value/argmax it shares. Gated on the committed ONNX snapshot,
//! mirroring `ppo_policy_run.rs`'s skip idiom. Uses the Neow start (a
//! non-combat, multi-option decision) so no combat/MCTS is needed.

use sts2_content::run_callbacks::{
    initialize_run_setup, new_run_manager_for_character_with_ascension,
};
use sts2_core::potions::CharacterId;
use sts2_runai::ppo::net::onnx_model_path;
use sts2_runai::ppo::policy::PpoPolicy;

#[test]
fn evaluate_with_actions_aligns_and_matches_evaluate() {
    let Some(path) = onnx_model_path() else {
        eprintln!("skip: no ONNX model in this checkout");
        return;
    };
    sts2_runai::rollout::ensure_content_installed();
    let mut rm =
        new_run_manager_for_character_with_ascension(0, CharacterId::Ironclad, 0).with_neow_start();
    initialize_run_setup(&mut rm);

    let actions = rm.get_available_actions();
    assert!(actions.len() >= 2, "Neow start should offer >=2 options");

    let policy = PpoPolicy::from_path(&path);
    let (masked, probs, value, argmax) = policy
        .evaluate_with_actions(&rm, &actions)
        .expect("Neow start is a non-combat multi-option decision");

    // Alignment: one prob per masked candidate, argmax in range.
    assert_eq!(masked.len(), probs.len());
    assert!(argmax < masked.len());
    // Every masked action was one of the offered actions.
    for a in &masked {
        assert!(
            actions.contains(a),
            "masked action {a:?} not in offered set"
        );
    }
    // Equivalence: evaluate() returns exactly the delegated triple —
    // bit-identical, not merely approximately equal.
    let (p2, v2, a2) = policy.evaluate(&rm, &actions).expect("same decision");
    assert_eq!(probs, p2);
    assert_eq!(value.to_bits(), v2.to_bits());
    assert_eq!(argmax, a2);
}
