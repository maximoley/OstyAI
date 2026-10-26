// Needs the ONNX runtime (`ppo::net`), compiled out without the `ppo` feature.
#![cfg(feature = "ppo")]
//! tract loads the committed ONNX and produces sane shapes (Phase 2 §2).
use sts2_runai::ppo::net::PpoNet;

/// The action-slot budget every committed champion must be exported at
/// (GH #315). `A_MAX=96` bound at a ~96-card deck and panicked a full
/// run mid-flight on a shop card-removal decision offering 103 options;
/// 160 moves the binder to `T_MAX=320` (`2·deck + 68 ≤ 320` → deck ≈
/// 126). Asserted at load so a stale, un-re-exported model checked back
/// in cannot silently reintroduce the cliff.
const REQUIRED_A_MAX: usize = 160;
/// The token budget the champions are exported at. Unchanged by #315 —
/// raising it costs O(T²) attention on every non-combat PPO decision.
const REQUIRED_T_MAX: usize = 320;

#[test]
fn tract_loads_and_runs_the_exported_model() {
    let Some(path) = sts2_runai::ppo::net::onnx_model_path() else {
        eprintln!("skip: no ONNX model (set STS2_RUNAI_PPO or export it350)");
        return;
    };
    let net = PpoNet::load(&path).expect("tract must accept the exported graph");
    let (t_max, a_max) = (net.t_max(), net.a_max());
    // One real state token + one valid action slot; everything else pad.
    // (Real pad ids come from Task 4's vocab; for a shape smoke any
    // in-range ids work — zeros are in-range for every embedding table.)
    let cat = vec![0i64; t_max * net.cat_dim()];
    let cont = vec![0f32; t_max * net.cont_dim()];
    let sec = vec![0i64; t_max];
    let mut pad_mask = vec![true; t_max];
    pad_mask[0] = false;
    pad_mask[1] = false;
    let mut act_mask = vec![false; a_max];
    act_mask[0] = true;
    let (logits, value) = net
        .forward_raw(&cat, &cont, &sec, &pad_mask, 1, &act_mask)
        .expect("forward");
    assert_eq!(logits.len(), a_max);
    assert!(logits[0].is_finite());
    assert!(value.is_finite());
}

/// GH #315 regression: every committed champion must carry the widened
/// action budget. This is the guard that a re-export was actually run
/// for each character, not just for the one the default resolver picks.
#[test]
fn every_committed_champion_carries_the_widened_budget() {
    use sts2_core::potions::CharacterId::{Defect, Ironclad, Necrobinder, Regent, Silent};

    let mut checked = 0;
    // Every character has a champion, so the list is the whole roster. It used
    // to omit one that had no card pool and therefore no champion.
    for character in [Ironclad, Silent, Defect, Regent, Necrobinder] {
        let Some(path) = sts2_runai::ppo::net::onnx_model_path_for(character) else {
            eprintln!("skip {character:?}: no committed champion on this checkout");
            continue;
        };
        let net = PpoNet::load(&path).unwrap_or_else(|e| panic!("load {character:?}: {e}"));
        assert_eq!(
            net.a_max(),
            REQUIRED_A_MAX,
            "{character:?} champion {} was exported at a_max={} — re-export it with \
             `export_onnx.py --a-max {REQUIRED_A_MAX}` (GH #315)",
            path.display(),
            net.a_max(),
        );
        assert_eq!(
            net.t_max(),
            REQUIRED_T_MAX,
            "{character:?} champion {} was exported at t_max={}",
            path.display(),
            net.t_max(),
        );
        checked += 1;
    }
    assert!(
        checked > 0,
        "no committed champions found — this test would pass vacuously"
    );
}
