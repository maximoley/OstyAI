// Needs the ONNX runtime (`ppo::net`), compiled out without the `ppo` feature.
#![cfg(feature = "ppo")]
//! An out-of-range boss embedding index must NOT silently return a plausible
//! answer from the wrong row.
//!
//! Context (GH #544): the PPO boss vocab is derived from the content —
//! `sts2_vocab` (re-exported as `ppo::vocab`) builds it from `run_room_gen::all_boss_encounter_ids()`, the
//! union of every act model's `RoomKind::Boss` rows. Porting Underdocks as a
//! second Act 1 widened it from 9 ids to 12, so `vocab::bosses().size` went
//! 10 → 13 and every index derived from it moved — including the pad row,
//! which `empty_cat` sets to `bosses().size` and which every padded token
//! carries.
//!
//! The torch path fails loudly (`load_state_dict` size mismatch). This test
//! pins what the *tract/ONNX* path does, because a Gather that clamps or wraps
//! would be the dangerous case: the champion keeps answering, plausibly, from
//! the wrong boss row, and nothing in the promotion pipeline notices.
//!
//! What it actually does: `tract-core`'s `Gather` (`ops/array/gather.rs:71`)
//! resolves negatives with `index + axis_len` and then slices **unchecked** —
//! so an out-of-range index PANICS rather than returning `Err`. That is loud,
//! but note it is a panic, not the `Err` [`PpoNet::infer`]'s signature
//! suggests: a `run_arena` sweep loses that worker's labels rather than
//! reporting a clean failure.
//!
//! The table is `bosses().size + 1` rows — every boss id, an OOV row, and a
//! pad row. This test DERIVES that height rather than hardcoding it: it used
//! to say `11`, which made it fail the moment Underdocks landed even though
//! nothing it guards had changed. `PpoNet::load` refuses any graph whose
//! `emb_boss` height disagrees with the vocab, so the derived number is
//! guaranteed to be the loaded graph's actual height.

use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use serde::Deserialize;
use sts2_runai::ppo::encoder::{featurize, CAT_BOSS};
use sts2_runai::ppo::net::{onnx_model_path, PpoNet};
use sts2_runai::ppo::obs::{ActionDesc, PpoObs};

#[derive(Deserialize)]
struct Row {
    obs: PpoObs,
    actions: Vec<ActionDesc>,
}

/// The boss identity's categorical column. NOT derived from `N_CAT` — it was
/// the trailing slot only while `N_CAT == 10`; GH #545 appended `deck_op`/
/// `enchant` after it, so `N_CAT - 1` silently became the enchant column
/// instead. `encoder::CAT_BOSS` is the named column both languages agree on
/// (mirrors `schema.py::CAT_BOSS`), so this test tracks it explicitly rather
/// than re-deriving a number that has already moved once.
const BOSS_COL: usize = CAT_BOSS;

/// Rows the loaded export has: every boss id + OOV + pad. Derived, not pinned
/// — see the module docs.
fn boss_table_rows() -> i64 {
    (sts2_runai::ppo::vocab::bosses().size + 1) as i64
}

#[test]
fn out_of_range_boss_index_never_silently_answers() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ppo_encoder_parity.jsonl");
    let (Ok(text), Some(model)) = (fs::read_to_string(fixture), onnx_model_path()) else {
        eprintln!("skip: fixture or ONNX model absent");
        return;
    };
    let net = PpoNet::load(&model).expect("load");
    if net.cat_dim() <= BOSS_COL {
        eprintln!("skip: champion predates the boss input, nothing to probe");
        return;
    }
    let rows = boss_table_rows();
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .expect("a fixture row");
    let row: Row = serde_json::from_str(line).expect("row");

    let poison = |idx: i64| {
        let mut feat = featurize(&row.obs, &row.actions);
        for c in feat.obs_cat.iter_mut().chain(feat.act_cat.iter_mut()) {
            c[BOSS_COL] = idx;
        }
        feat
    };

    // The last in-range row (pad) still infers cleanly — this is the control,
    // without which "it fails" below would not distinguish a real bounds
    // reaction from a model that simply cannot run.
    assert!(
        net.infer(&poison(rows - 1)).is_ok(),
        "index {} is the pad row and must infer",
        rows - 1
    );

    // One past the table, and further out. Neither may come back Ok.
    for idx in [rows, rows + 2, 99] {
        let outcome = catch_unwind(AssertUnwindSafe(|| net.infer(&poison(idx))));
        match outcome {
            Err(_) => {}     // tract's unchecked Gather slice panicked: loud, fine.
            Ok(Err(_)) => {} // a real bounds guard returned Err: better.
            Ok(Ok((probs, value))) => panic!(
                "boss index {idx} is outside the {rows}-row table but \
                 inference returned Ok (value={value}, {} logits) — the net is \
                 answering from a wrong or wrapped embedding row. See GH #544.",
                probs.len()
            ),
        }
    }
}
