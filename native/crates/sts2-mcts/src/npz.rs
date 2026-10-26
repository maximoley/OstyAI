//! Minimal NPZ (zip-of-.npy) writer for the Texel corpus pipeline.
//!
//! Bead `sts2-rs-ulbl` (vsqc.1). Texel-tuning consumes `[N, F] f32`
//! features + `[N] u8` outcomes via `numpy.load(path).items`. NPZ
//! is just a ZIP archive of `.npy` files; we hand-roll a STORED
//! (no compression) ZIP writer rather than pulling in the `zip`
//! crate — the wire format is ~60 LOC and the corpus generator
//! runs once per Texel cycle, so deflate compression isn't worth
//! the dep.
//!
//! ## Layout
//!
//! Each call to [`write_npz`] produces a file with exactly two
//! entries: `features.npy` and `outcomes.npy`. Both follow the
//! NumPy `.npy` v1.0 spec
//! (<https://numpy.org/doc/stable/reference/generated/numpy.lib.format.html>):
//! 6-byte magic `\x93NUMPY`, version `0x01 0x00`, `u16` header
//! length, padded ASCII dict header, then raw little-endian data.
//!
//! The outer ZIP uses STORED (compression method 0); CRC32 is
//! computed via a table-free bit-walk.

use std::fs::File;
use std::io::{self, Seek, Write};
use std::path::Path;

/// Write `features` (length `n_rows * n_cols`, row-major f32) and
/// `outcomes` (length `n_rows`, u8) to an NPZ at `path`. Overwrites
/// any existing file. Returns the on-disk byte size on success.
pub fn write_npz(
    path: &Path,
    features: &[f32],
    n_rows: usize,
    n_cols: usize,
    outcomes: &[u8],
) -> io::Result<u64> {
    assert_eq!(
        features.len(),
        n_rows * n_cols,
        "features length must equal n_rows * n_cols"
    );
    assert_eq!(outcomes.len(), n_rows, "outcomes length must equal n_rows");

    let mut f = File::create(path)?;

    let feat_npy = encode_npy_f32(features, &[n_rows, n_cols]);
    let out_npy = encode_npy_u8(outcomes, &[n_rows]);

    let mut central_dir: Vec<CentralDirEntry> = Vec::with_capacity(2);

    let off_feat = f.stream_position()?;
    write_local_file(
        &mut f,
        "features.npy",
        &feat_npy,
        &mut central_dir,
        off_feat,
    )?;
    let off_out = f.stream_position()?;
    write_local_file(&mut f, "outcomes.npy", &out_npy, &mut central_dir, off_out)?;

    let central_dir_offset = f.stream_position()?;
    let mut central_dir_size: u64 = 0;
    for entry in &central_dir {
        central_dir_size += write_central_dir_entry(&mut f, entry)? as u64;
    }
    write_eocd(
        &mut f,
        central_dir.len() as u16,
        central_dir_size,
        central_dir_offset,
    )?;

    let total = f.stream_position()?;
    f.flush()?;
    Ok(total)
}

/// Wide-schema NPZ writer for the Wave-6 ExIt corpus — bead
/// `sts2-rs-quuu` (qctu.6.x). Emits four arrays:
///
/// | key                   | dtype | shape              |
/// |-----------------------|-------|--------------------|
/// | `features`            | `<f4` | `[n_rows, n_cols]` |
/// | `outcomes`            | `\|u1`| `[n_rows]`         |
/// | `final_hp_norm`       | `<f4` | `[n_rows]`         |
/// | `visit_distribution`  | `<f4` | `[n_rows, n_action_cols]` |
///
/// The qctu.3 trainer detects which heads to train from which
/// arrays are present. Callers that don't have aux/policy
/// targets (qctu.2-era callers) should keep using [`write_npz`];
/// this function asserts that both aux arrays are present and
/// correctly-shaped.
// 8-arg signature is the natural shape: features+outcomes+aux_hp+
// visit_distribution each need data + (where applicable) the
// column count alongside `path`. A struct wrapper would shift the
// noise from the call site to the def site without saving any
// reader-effort, so just carve out the lint.
#[allow(clippy::too_many_arguments)]
pub fn write_npz_with_extras(
    path: &Path,
    features: &[f32],
    n_rows: usize,
    n_cols: usize,
    outcomes: &[u8],
    final_hp_norm: &[f32],
    visit_distribution: &[f32],
    n_action_cols: usize,
) -> io::Result<u64> {
    assert_eq!(
        features.len(),
        n_rows * n_cols,
        "features length must equal n_rows * n_cols"
    );
    assert_eq!(outcomes.len(), n_rows, "outcomes length must equal n_rows");
    assert_eq!(
        final_hp_norm.len(),
        n_rows,
        "final_hp_norm length must equal n_rows"
    );
    assert_eq!(
        visit_distribution.len(),
        n_rows * n_action_cols,
        "visit_distribution length must equal n_rows * n_action_cols"
    );

    let mut f = File::create(path)?;

    let feat_npy = encode_npy_f32(features, &[n_rows, n_cols]);
    let out_npy = encode_npy_u8(outcomes, &[n_rows]);
    let hp_npy = encode_npy_f32(final_hp_norm, &[n_rows]);
    let visits_npy = encode_npy_f32(visit_distribution, &[n_rows, n_action_cols]);

    let mut central_dir: Vec<CentralDirEntry> = Vec::with_capacity(4);

    let off = f.stream_position()?;
    write_local_file(&mut f, "features.npy", &feat_npy, &mut central_dir, off)?;
    let off = f.stream_position()?;
    write_local_file(&mut f, "outcomes.npy", &out_npy, &mut central_dir, off)?;
    let off = f.stream_position()?;
    write_local_file(&mut f, "final_hp_norm.npy", &hp_npy, &mut central_dir, off)?;
    let off = f.stream_position()?;
    write_local_file(
        &mut f,
        "visit_distribution.npy",
        &visits_npy,
        &mut central_dir,
        off,
    )?;

    let central_dir_offset = f.stream_position()?;
    let mut central_dir_size: u64 = 0;
    for entry in &central_dir {
        central_dir_size += write_central_dir_entry(&mut f, entry)? as u64;
    }
    write_eocd(
        &mut f,
        central_dir.len() as u16,
        central_dir_size,
        central_dir_offset,
    )?;

    let total = f.stream_position()?;
    f.flush()?;
    Ok(total)
}

/// Wave-acbj writer (bead `sts2-rs-acbj`, step 1). Emits the four
/// base arrays from [`write_npz_with_extras`] plus two per-row
/// provenance arrays consumed by `scripts/value_policy_net.py`'s
/// `--group-col` flag + per-tier loss weighting:
///
/// | NPZ key             | dtype | shape          |
/// |---------------------|-------|----------------|
/// | `features`          | `<f4` | `[n_rows, F]`  |
/// | `outcomes`          | `\|u1`| `[n_rows]`     |
/// | `final_hp_norm`     | `<f4` | `[n_rows]`     |
/// | `visit_distribution`| `<f4` | `[n_rows, A]`  |
/// | `run_id_u64`        | `<u8` | `[n_rows]`     |
/// | `encounter_tier_u8` | `\|u1`| `[n_rows]`     |
/// | `mcts_root_q`       | `<f4` | `[n_rows]`     |
/// | `mcts_q_valid`      | `\|u1`| `[n_rows]`     |
///
/// `mcts_root_q` / `mcts_q_valid` are the search-bootstrapped value
/// target + validity mask (bead `sts2-rs-wrip2`), carried here so the
/// non-embeddings `--wide` path emits the Phase-0 combat-redesign
/// reference target alongside the `run_id_u64` group key in a single
/// NPZ (`docs/corpus-target-schema.md`). They are NaN/0 when
/// `--reference-sims 0` and no action-search root Q was recorded.
///
/// `run_id_u64` is the FNV-1a 64 of `[provenance].run_id` for
/// toml-source combats and `combat_idx` for synthetic-path. `0`
/// is the unset sentinel.
/// `encounter_tier_u8` is one of `TIER_COMMON`/`TIER_ELITE`/
/// `TIER_BOSS`/`TIER_UNKNOWN` from `sts2_mcts::corpus`.
#[allow(clippy::too_many_arguments)]
pub fn write_npz_with_extras_and_provenance(
    path: &Path,
    features: &[f32],
    n_rows: usize,
    n_cols: usize,
    outcomes: &[u8],
    final_hp_norm: &[f32],
    visit_distribution: &[f32],
    n_action_cols: usize,
    run_id_u64: &[u64],
    encounter_tier_u8: &[u8],
    legal_action_mask: &[u8],
    raw_texel_score: &[f32],
    mcts_root_q: &[f32],
    mcts_q_valid: &[u8],
) -> io::Result<u64> {
    assert_eq!(features.len(), n_rows * n_cols);
    assert_eq!(outcomes.len(), n_rows);
    assert_eq!(final_hp_norm.len(), n_rows);
    assert_eq!(visit_distribution.len(), n_rows * n_action_cols);
    assert_eq!(run_id_u64.len(), n_rows);
    assert_eq!(encounter_tier_u8.len(), n_rows);
    assert_eq!(
        legal_action_mask.len(),
        n_rows * n_action_cols,
        "legal_action_mask length must equal n_rows * n_action_cols"
    );
    assert_eq!(
        raw_texel_score.len(),
        n_rows,
        "raw_texel_score length must equal n_rows (bead sts2-rs-aa09l)"
    );
    assert_eq!(
        mcts_root_q.len(),
        n_rows,
        "mcts_root_q length must equal n_rows (Phase-0 combat-redesign reference target)"
    );
    assert_eq!(
        mcts_q_valid.len(),
        n_rows,
        "mcts_q_valid length must equal n_rows"
    );

    let mut f = File::create(path)?;

    let feat_npy = encode_npy_f32(features, &[n_rows, n_cols]);
    let out_npy = encode_npy_u8(outcomes, &[n_rows]);
    let hp_npy = encode_npy_f32(final_hp_norm, &[n_rows]);
    let visits_npy = encode_npy_f32(visit_distribution, &[n_rows, n_action_cols]);
    let run_npy = encode_npy_u64(run_id_u64, &[n_rows]);
    let tier_npy = encode_npy_u8(encounter_tier_u8, &[n_rows]);
    let mask_npy = encode_npy_u8(legal_action_mask, &[n_rows, n_action_cols]);
    // Bead `sts2-rs-aa09l` (37vzt.1): per-row raw Texel score.
    let texel_npy = encode_npy_f32(raw_texel_score, &[n_rows]);
    // Phase-0 combat-redesign (2026-06-09): search-bootstrapped value
    // target + validity mask, so `--wide` (no embeddings) carries the
    // reference target next to the run_id_u64 group key.
    let q_npy = encode_npy_f32(mcts_root_q, &[n_rows]);
    let q_valid_npy = encode_npy_u8(mcts_q_valid, &[n_rows]);

    let mut central_dir: Vec<CentralDirEntry> = Vec::with_capacity(10);
    let arrays: [(&str, &[u8]); 10] = [
        ("features.npy", &feat_npy),
        ("outcomes.npy", &out_npy),
        ("final_hp_norm.npy", &hp_npy),
        ("visit_distribution.npy", &visits_npy),
        ("run_id_u64.npy", &run_npy),
        ("encounter_tier_u8.npy", &tier_npy),
        // Bead `sts2-rs-8bb9i` (svstv.2): dense legal-action mask.
        ("legal_action_mask.npy", &mask_npy),
        // Bead `sts2-rs-aa09l` (37vzt.1): per-row raw Texel score.
        ("raw_texel_score.npy", &texel_npy),
        // Phase-0 combat-redesign: reference value target + mask.
        ("mcts_root_q.npy", &q_npy),
        ("mcts_q_valid.npy", &q_valid_npy),
    ];
    for (name, data) in arrays {
        let off = f.stream_position()?;
        write_local_file(&mut f, name, data, &mut central_dir, off)?;
    }

    let central_dir_offset = f.stream_position()?;
    let mut central_dir_size: u64 = 0;
    for entry in &central_dir {
        central_dir_size += write_central_dir_entry(&mut f, entry)? as u64;
    }
    write_eocd(
        &mut f,
        central_dir.len() as u16,
        central_dir_size,
        central_dir_offset,
    )?;

    let total = f.stream_position()?;
    f.flush()?;
    Ok(total)
}

/// Narrow-schema provenance writer (bead `sts2-rs-e2bjk`). Emits
/// the two base arrays from [`write_npz`] plus the two per-row
/// provenance arrays consumed by `scripts/texel_tune.py`'s
/// `--group-split` flag:
///
/// | NPZ key             | dtype | shape         |
/// |---------------------|-------|---------------|
/// | `features`          | `<f4` | `[n_rows, F]` |
/// | `outcomes`          | `\|u1`| `[n_rows]`    |
/// | `run_id_u64`        | `<u8` | `[n_rows]`    |
/// | `encounter_tier_u8` | `\|u1`| `[n_rows]`    |
///
/// Strict superset of the 2-array NPZ — pre-e2bjk consumers that
/// only look at `features` + `outcomes` still work.
#[allow(clippy::too_many_arguments)]
pub fn write_npz_narrow_with_provenance(
    path: &Path,
    features: &[f32],
    n_rows: usize,
    n_cols: usize,
    outcomes: &[u8],
    run_id_u64: &[u64],
    encounter_tier_u8: &[u8],
) -> io::Result<u64> {
    assert_eq!(features.len(), n_rows * n_cols);
    assert_eq!(outcomes.len(), n_rows);
    assert_eq!(run_id_u64.len(), n_rows);
    assert_eq!(encounter_tier_u8.len(), n_rows);

    let mut f = File::create(path)?;

    let feat_npy = encode_npy_f32(features, &[n_rows, n_cols]);
    let out_npy = encode_npy_u8(outcomes, &[n_rows]);
    let run_npy = encode_npy_u64(run_id_u64, &[n_rows]);
    let tier_npy = encode_npy_u8(encounter_tier_u8, &[n_rows]);

    let mut central_dir: Vec<CentralDirEntry> = Vec::with_capacity(4);
    let arrays: [(&str, &[u8]); 4] = [
        ("features.npy", &feat_npy),
        ("outcomes.npy", &out_npy),
        ("run_id_u64.npy", &run_npy),
        ("encounter_tier_u8.npy", &tier_npy),
    ];
    for (name, data) in arrays {
        let off = f.stream_position()?;
        write_local_file(&mut f, name, data, &mut central_dir, off)?;
    }

    let central_dir_offset = f.stream_position()?;
    let mut central_dir_size: u64 = 0;
    for entry in &central_dir {
        central_dir_size += write_central_dir_entry(&mut f, entry)? as u64;
    }
    write_eocd(
        &mut f,
        central_dir.len() as u16,
        central_dir_size,
        central_dir_offset,
    )?;

    let total = f.stream_position()?;
    f.flush()?;
    Ok(total)
}

/// Wave-6 generalist-net writer (bead `sts2-rs-uqnh`). Emits the
/// four base arrays from [`write_npz_with_extras`] plus the six
/// per-slot id arrays consumed by
/// `scripts/value_policy_net_generalist.py`. All id arrays are
/// row-major `[n_rows, slot_max]` with dtype `<u2`; `0` is the NULL
/// sentinel, non-null ids are `(rust_enum_discriminant as u16) + 1`.
///
/// Column-count assertions match the per-array `slot_max` carried
/// in the [`crate::corpus::WideArraysWithEmbeddings`] bundle — the
/// caller is expected to pass the bundle's `slot_max` constants
/// alongside the data slices.
// 14-arg signature mirrors the natural shape: 4 base arrays + 6 id
// arrays each with their stride; struct wrapper would just push the
// noise from call-site to def-site.
#[allow(clippy::too_many_arguments)]
pub fn write_npz_with_embeddings(
    path: &Path,
    features: &[f32],
    n_rows: usize,
    n_cols: usize,
    outcomes: &[u8],
    final_hp_norm: &[f32],
    visit_distribution: &[f32],
    n_action_cols: usize,
    card_ids_hand: &[u16],
    hand_slot_max: usize,
    card_ids_draw: &[u16],
    card_ids_discard: &[u16],
    card_ids_exhaust: &[u16],
    pile_slot_max: usize,
    monster_slot_ids: &[u16],
    enemy_slot_max: usize,
    relic_set_ids: &[u16],
    relic_slot_max: usize,
    card_feats_hand: &[f32],
    card_feats_draw: &[f32],
    card_feats_discard: &[f32],
    card_feats_exhaust: &[f32],
    card_feat_dim: usize,
    mcts_root_q: &[f32],
    mcts_q_valid: &[u8],
    legal_action_mask: &[u8],
    run_id_u64: &[u64],
    encounter_tier_u8: &[u8],
) -> io::Result<u64> {
    assert_eq!(features.len(), n_rows * n_cols);
    assert_eq!(run_id_u64.len(), n_rows);
    assert_eq!(encounter_tier_u8.len(), n_rows);
    assert_eq!(outcomes.len(), n_rows);
    assert_eq!(final_hp_norm.len(), n_rows);
    assert_eq!(visit_distribution.len(), n_rows * n_action_cols);
    assert_eq!(card_ids_hand.len(), n_rows * hand_slot_max);
    assert_eq!(card_ids_draw.len(), n_rows * pile_slot_max);
    assert_eq!(card_ids_discard.len(), n_rows * pile_slot_max);
    assert_eq!(card_ids_exhaust.len(), n_rows * pile_slot_max);
    assert_eq!(monster_slot_ids.len(), n_rows * enemy_slot_max);
    assert_eq!(relic_set_ids.len(), n_rows * relic_slot_max);
    assert_eq!(card_feats_hand.len(), n_rows * card_feat_dim);
    assert_eq!(card_feats_draw.len(), n_rows * card_feat_dim);
    assert_eq!(card_feats_discard.len(), n_rows * card_feat_dim);
    assert_eq!(card_feats_exhaust.len(), n_rows * card_feat_dim);
    assert_eq!(mcts_root_q.len(), n_rows);
    assert_eq!(mcts_q_valid.len(), n_rows);
    assert_eq!(
        legal_action_mask.len(),
        n_rows * n_action_cols,
        "legal_action_mask length must equal n_rows * n_action_cols"
    );

    let mut f = File::create(path)?;

    let feat_npy = encode_npy_f32(features, &[n_rows, n_cols]);
    let out_npy = encode_npy_u8(outcomes, &[n_rows]);
    let hp_npy = encode_npy_f32(final_hp_norm, &[n_rows]);
    let visits_npy = encode_npy_f32(visit_distribution, &[n_rows, n_action_cols]);
    let hand_npy = encode_npy_u16(card_ids_hand, &[n_rows, hand_slot_max]);
    let draw_npy = encode_npy_u16(card_ids_draw, &[n_rows, pile_slot_max]);
    let disc_npy = encode_npy_u16(card_ids_discard, &[n_rows, pile_slot_max]);
    let exh_npy = encode_npy_u16(card_ids_exhaust, &[n_rows, pile_slot_max]);
    let mons_npy = encode_npy_u16(monster_slot_ids, &[n_rows, enemy_slot_max]);
    let rel_npy = encode_npy_u16(relic_set_ids, &[n_rows, relic_slot_max]);
    // Lever A stage (b): per-pile summed per-card static features.
    let cf_hand_npy = encode_npy_f32(card_feats_hand, &[n_rows, card_feat_dim]);
    let cf_draw_npy = encode_npy_f32(card_feats_draw, &[n_rows, card_feat_dim]);
    let cf_disc_npy = encode_npy_f32(card_feats_discard, &[n_rows, card_feat_dim]);
    let cf_exh_npy = encode_npy_f32(card_feats_exhaust, &[n_rows, card_feat_dim]);
    // Bead `sts2-rs-wrip2`: TD-style value targets.
    let q_npy = encode_npy_f32(mcts_root_q, &[n_rows]);
    let q_valid_npy = encode_npy_u8(mcts_q_valid, &[n_rows]);
    // Bead `sts2-rs-8bb9i` (svstv.2): dense legal-action mask.
    let mask_npy = encode_npy_u8(legal_action_mask, &[n_rows, n_action_cols]);
    // Phase 2: provenance for leak-free group splits in the embeddings path.
    let run_npy = encode_npy_u64(run_id_u64, &[n_rows]);
    let tier_npy = encode_npy_u8(encounter_tier_u8, &[n_rows]);

    let mut central_dir: Vec<CentralDirEntry> = Vec::with_capacity(19);
    let arrays: [(&str, &[u8]); 19] = [
        ("features.npy", &feat_npy),
        ("outcomes.npy", &out_npy),
        ("final_hp_norm.npy", &hp_npy),
        ("visit_distribution.npy", &visits_npy),
        ("card_ids_hand.npy", &hand_npy),
        ("card_ids_draw.npy", &draw_npy),
        ("card_ids_discard.npy", &disc_npy),
        ("card_ids_exhaust.npy", &exh_npy),
        ("monster_slot_ids.npy", &mons_npy),
        ("relic_set_ids.npy", &rel_npy),
        ("card_feats_hand.npy", &cf_hand_npy),
        ("card_feats_draw.npy", &cf_draw_npy),
        ("card_feats_discard.npy", &cf_disc_npy),
        ("card_feats_exhaust.npy", &cf_exh_npy),
        ("mcts_root_q.npy", &q_npy),
        ("mcts_q_valid.npy", &q_valid_npy),
        ("legal_action_mask.npy", &mask_npy),
        ("run_id_u64.npy", &run_npy),
        ("encounter_tier_u8.npy", &tier_npy),
    ];
    for (name, data) in arrays {
        let off = f.stream_position()?;
        write_local_file(&mut f, name, data, &mut central_dir, off)?;
    }

    let central_dir_offset = f.stream_position()?;
    let mut central_dir_size: u64 = 0;
    for entry in &central_dir {
        central_dir_size += write_central_dir_entry(&mut f, entry)? as u64;
    }
    write_eocd(
        &mut f,
        central_dir.len() as u16,
        central_dir_size,
        central_dir_offset,
    )?;

    let total = f.stream_position()?;
    f.flush()?;
    Ok(total)
}

/// Encode a `u16` slice as an `.npy` v1.0 byte vector with dtype
/// `<u2` (little-endian u16) and the given shape. Wave-6 generalist
/// id arrays consume this (bead `sts2-rs-uqnh`).
pub fn encode_npy_u16(data: &[u16], shape: &[usize]) -> Vec<u8> {
    let header = npy_header("<u2", shape);
    let mut buf = Vec::with_capacity(header.len() + data.len() * 2);
    buf.extend_from_slice(&header);
    for v in data {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    buf
}

/// Encode an `f32` slice as an `.npy` v1.0 byte vector with dtype
/// `<f4` (little-endian f32) and the given shape.
pub fn encode_npy_f32(data: &[f32], shape: &[usize]) -> Vec<u8> {
    let header = npy_header("<f4", shape);
    let mut buf = Vec::with_capacity(header.len() + data.len() * 4);
    buf.extend_from_slice(&header);
    for v in data {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    buf
}

/// Encode a `u64` slice as an `.npy` v1.0 byte vector with dtype
/// `<u8` (little-endian u64) and the given shape. Used for the
/// `run_id_u64` provenance column emitted by
/// [`write_npz_with_extras_and_provenance`] (bead `sts2-rs-acbj`).
pub fn encode_npy_u64(data: &[u64], shape: &[usize]) -> Vec<u8> {
    let header = npy_header("<u8", shape);
    let mut buf = Vec::with_capacity(header.len() + data.len() * 8);
    buf.extend_from_slice(&header);
    for v in data {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    buf
}

/// Encode a `u8` slice as an `.npy` v1.0 byte vector with dtype
/// `|u1` and the given shape.
pub fn encode_npy_u8(data: &[u8], shape: &[usize]) -> Vec<u8> {
    let header = npy_header("|u1", shape);
    let mut buf = Vec::with_capacity(header.len() + data.len());
    buf.extend_from_slice(&header);
    buf.extend_from_slice(data);
    buf
}

/// Build the NPY v1.0 header (magic + version + u16 length +
/// padded ASCII dict). Total length is a multiple of 64; the
/// final byte before the data is `\n`.
fn npy_header(dtype: &str, shape: &[usize]) -> Vec<u8> {
    let shape_repr = if shape.len() == 1 {
        format!("({},)", shape[0])
    } else {
        let parts: Vec<String> = shape.iter().map(ToString::to_string).collect();
        format!("({})", parts.join(", "))
    };
    let header_body =
        format!("{{'descr': '{dtype}', 'fortran_order': False, 'shape': {shape_repr}, }}");
    const MAGIC: &[u8] = b"\x93NUMPY";
    const VERSION: &[u8] = &[0x01, 0x00];
    let preamble_len = MAGIC.len() + VERSION.len() + 2; // 2 = u16 header_len
    let unpadded = preamble_len + header_body.len() + 1; // +1 trailing \n
    let padded_total = unpadded.div_ceil(64) * 64;
    let pad = padded_total - unpadded;
    let header_len = (header_body.len() + pad + 1) as u16;

    let mut buf = Vec::with_capacity(padded_total);
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(VERSION);
    buf.extend_from_slice(&header_len.to_le_bytes());
    buf.extend_from_slice(header_body.as_bytes());
    buf.resize(buf.len() + pad, b' ');
    buf.push(b'\n');
    buf
}

// ── ZIP STORED writer ───────────────────────────────────────────────────────

struct CentralDirEntry {
    filename: String,
    crc32: u32,
    size: u32,
    local_header_offset: u32,
}

fn write_local_file<W: Write + Seek>(
    w: &mut W,
    name: &str,
    data: &[u8],
    central_dir: &mut Vec<CentralDirEntry>,
    offset: u64,
) -> io::Result<()> {
    let crc = crc32(data);
    let name_bytes = name.as_bytes();
    let data_len = u32::try_from(data.len()).expect("npy entry < 4 GiB");
    let name_len = u16::try_from(name_bytes.len()).expect("filename < 64 KiB");
    let offset_u32 = u32::try_from(offset).expect("npz file < 4 GiB");

    // Local file header (signature + 26 byte fields) + filename + data.
    w.write_all(&0x0403_4b50u32.to_le_bytes())?; // signature
    w.write_all(&20u16.to_le_bytes())?; // version needed
    w.write_all(&0u16.to_le_bytes())?; // flags
    w.write_all(&0u16.to_le_bytes())?; // method (STORED)
    w.write_all(&0u16.to_le_bytes())?; // last mod time
    w.write_all(&0u16.to_le_bytes())?; // last mod date
    w.write_all(&crc.to_le_bytes())?;
    w.write_all(&data_len.to_le_bytes())?; // compressed size
    w.write_all(&data_len.to_le_bytes())?; // uncompressed size
    w.write_all(&name_len.to_le_bytes())?;
    w.write_all(&0u16.to_le_bytes())?; // extra field length
    w.write_all(name_bytes)?;
    w.write_all(data)?;

    central_dir.push(CentralDirEntry {
        filename: name.to_string(),
        crc32: crc,
        size: data_len,
        local_header_offset: offset_u32,
    });
    Ok(())
}

fn write_central_dir_entry<W: Write>(w: &mut W, e: &CentralDirEntry) -> io::Result<usize> {
    let name_bytes = e.filename.as_bytes();
    let name_len = u16::try_from(name_bytes.len()).expect("filename < 64 KiB");
    w.write_all(&0x0201_4b50u32.to_le_bytes())?; // central-dir signature
    w.write_all(&20u16.to_le_bytes())?; // version made by
    w.write_all(&20u16.to_le_bytes())?; // version needed
    w.write_all(&0u16.to_le_bytes())?; // flags
    w.write_all(&0u16.to_le_bytes())?; // method (STORED)
    w.write_all(&0u16.to_le_bytes())?; // last mod time
    w.write_all(&0u16.to_le_bytes())?; // last mod date
    w.write_all(&e.crc32.to_le_bytes())?;
    w.write_all(&e.size.to_le_bytes())?; // compressed
    w.write_all(&e.size.to_le_bytes())?; // uncompressed
    w.write_all(&name_len.to_le_bytes())?;
    w.write_all(&0u16.to_le_bytes())?; // extra
    w.write_all(&0u16.to_le_bytes())?; // comment
    w.write_all(&0u16.to_le_bytes())?; // disk number
    w.write_all(&0u16.to_le_bytes())?; // internal attrs
    w.write_all(&0u32.to_le_bytes())?; // external attrs
    w.write_all(&e.local_header_offset.to_le_bytes())?;
    w.write_all(name_bytes)?;
    Ok(46 + name_bytes.len())
}

fn write_eocd<W: Write>(
    w: &mut W,
    entries: u16,
    central_dir_size: u64,
    central_dir_offset: u64,
) -> io::Result<()> {
    let cd_size = u32::try_from(central_dir_size).expect("central dir < 4 GiB");
    let cd_offset = u32::try_from(central_dir_offset).expect("central dir < 4 GiB");
    w.write_all(&0x0605_4b50u32.to_le_bytes())?; // EOCD signature
    w.write_all(&0u16.to_le_bytes())?; // disk number
    w.write_all(&0u16.to_le_bytes())?; // disk-with-central-dir
    w.write_all(&entries.to_le_bytes())?; // entries on this disk
    w.write_all(&entries.to_le_bytes())?; // total entries
    w.write_all(&cd_size.to_le_bytes())?;
    w.write_all(&cd_offset.to_le_bytes())?;
    w.write_all(&0u16.to_le_bytes())?; // comment length
    Ok(())
}

// ── CRC32 (IEEE polynomial, table-free bit-walk) ───────────────────────────

/// Bit-walk CRC-32-IEEE. Slow but a one-shot per file; the corpus
/// generator's hot path is MCTS search, not CRC.
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = !0;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vectors() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"abc"), 0x352441C2);
        // "123456789" canonical CRC-32-IEEE test vector.
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn npy_f32_header_format_matches_numpy_spec() {
        let body = encode_npy_f32(&[1.0, 2.0, 3.0, 4.0], &[2, 2]);
        assert_eq!(&body[..6], b"\x93NUMPY");
        assert_eq!(&body[6..8], &[0x01, 0x00]);
        let header_len = u16::from_le_bytes([body[8], body[9]]);
        let header =
            std::str::from_utf8(&body[10..10 + header_len as usize]).expect("ascii header");
        assert!(header.contains("'descr': '<f4'"));
        assert!(header.contains("'fortran_order': False"));
        assert!(header.contains("'shape': (2, 2)"));
        assert_eq!((10 + header_len as usize) % 64, 0);
        assert_eq!(body.len(), 10 + header_len as usize + 16);
    }

    #[test]
    fn npy_u8_round_trip_bytes() {
        let body = encode_npy_u8(&[0, 1, 0, 1, 0], &[5]);
        let header_len = u16::from_le_bytes([body[8], body[9]]);
        let payload_off = 10 + header_len as usize;
        assert_eq!(&body[payload_off..], &[0, 1, 0, 1, 0]);
    }

    #[test]
    fn write_npz_produces_valid_zip_signature_and_eocd() {
        let dir = std::env::temp_dir().join("sts2-rs-ulbl-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("smoke.npz");
        let features: Vec<f32> = (0..12).map(|i| i as f32 * 0.1).collect();
        let outcomes: Vec<u8> = vec![1, 0];
        let n = write_npz(&path, &features, 2, 6, &outcomes).expect("write succeeds");
        assert!(n > 0);
        let raw = std::fs::read(&path).expect("read back");
        // Local file header magic at offset 0.
        assert_eq!(&raw[..4], &0x0403_4b50u32.to_le_bytes());
        // EOCD signature appears in the file.
        let eocd_sig = 0x0605_4b50u32.to_le_bytes();
        assert!(
            raw.windows(4).any(|w| w == eocd_sig),
            "EOCD signature must appear in the file"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_npz_with_extras_produces_four_entries() {
        let dir = std::env::temp_dir().join("sts2-rs-quuu-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("smoke-extras.npz");
        let features: Vec<f32> = (0..160).map(|i| i as f32 * 0.01).collect();
        let outcomes: Vec<u8> = vec![1, 0];
        let final_hp: Vec<f32> = vec![0.8, 0.2];
        let visits: Vec<f32> = (0..1280).map(|i| (i as f32) * 0.001).collect();
        let n = write_npz_with_extras(&path, &features, 2, 80, &outcomes, &final_hp, &visits, 640)
            .expect("write succeeds");
        assert!(n > 0);
        let raw = std::fs::read(&path).expect("read back");
        // Four local-file-header signatures should appear, one per
        // member entry.
        let sig = 0x0403_4b50u32.to_le_bytes();
        let count = (0..raw.len() - 4)
            .filter(|i| raw[*i..*i + 4] == sig)
            .count();
        assert_eq!(count, 4, "expected 4 .npy entries, got {count}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_npz_with_embeddings_produces_twelve_entries_including_td_targets() {
        // Bead sts2-rs-wrip2: the embeddings NPZ writer emits 12
        // arrays — the 10 pre-wrip2 ones plus the new mcts_root_q
        // and mcts_q_valid TD-target columns. Round-trip pins the
        // central-directory entry count.
        let dir = std::env::temp_dir().join("sts2-rs-wrip2-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("smoke-wrip2.npz");
        let n_rows = 2usize;
        let n_cols = 8usize;
        let hand_max = 4usize;
        let pile_max = 6usize;
        let enemy_max = 3usize;
        let relic_max = 5usize;
        let action_cols = 7usize;
        let features: Vec<f32> = (0..n_rows * n_cols).map(|i| i as f32).collect();
        let outcomes: Vec<u8> = vec![1, 0];
        let final_hp: Vec<f32> = vec![0.9, 0.1];
        let visits: Vec<f32> = vec![0.0; n_rows * action_cols];
        let hand: Vec<u16> = vec![0; n_rows * hand_max];
        let draw: Vec<u16> = vec![0; n_rows * pile_max];
        let discard: Vec<u16> = vec![0; n_rows * pile_max];
        let exhaust: Vec<u16> = vec![0; n_rows * pile_max];
        let enemies: Vec<u16> = vec![0; n_rows * enemy_max];
        let relics: Vec<u16> = vec![0; n_rows * relic_max];
        let card_feat_dim = 3usize;
        let cf_hand: Vec<f32> = vec![0.0; n_rows * card_feat_dim];
        let cf_draw: Vec<f32> = vec![0.0; n_rows * card_feat_dim];
        let cf_discard: Vec<f32> = vec![0.0; n_rows * card_feat_dim];
        let cf_exhaust: Vec<f32> = vec![0.0; n_rows * card_feat_dim];
        let mcts_root_q: Vec<f32> = vec![0.42, f32::NAN];
        let mcts_q_valid: Vec<u8> = vec![1, 0];
        // Bead `sts2-rs-8bb9i`: legal_action_mask is a flat u8 buffer
        // with the same shape as visit_distribution (`n_rows *
        // n_action_cols`).
        let legal_mask: Vec<u8> = vec![0; n_rows * action_cols];
        // Phase 2: provenance arrays now ride the embeddings emit too.
        let run_id_u64: Vec<u64> = vec![111, 222];
        let tier_u8: Vec<u8> = vec![0, 1];
        let bytes = write_npz_with_embeddings(
            &path,
            &features,
            n_rows,
            n_cols,
            &outcomes,
            &final_hp,
            &visits,
            action_cols,
            &hand,
            hand_max,
            &draw,
            &discard,
            &exhaust,
            pile_max,
            &enemies,
            enemy_max,
            &relics,
            relic_max,
            &cf_hand,
            &cf_draw,
            &cf_discard,
            &cf_exhaust,
            card_feat_dim,
            &mcts_root_q,
            &mcts_q_valid,
            &legal_mask,
            &run_id_u64,
            &tier_u8,
        )
        .expect("write succeeds");
        assert!(bytes > 0);
        let raw = std::fs::read(&path).expect("read back");
        let sig = 0x0403_4b50u32.to_le_bytes();
        let count = (0..raw.len() - 4)
            .filter(|i| raw[*i..*i + 4] == sig)
            .count();
        assert_eq!(
            count, 19,
            "expected 19 .npy entries (17 + run_id_u64 + encounter_tier_u8)"
        );
        assert!(
            raw.windows(b"run_id_u64.npy".len())
                .any(|w| w == b"run_id_u64.npy"),
            "run_id_u64.npy filename must appear in the archive"
        );
        assert!(
            raw.windows(b"legal_action_mask.npy".len())
                .any(|w| w == b"legal_action_mask.npy"),
            "legal_action_mask.npy filename must appear in the archive"
        );
        // Verify the two new entry names land in the central
        // directory by searching the raw bytes for their filenames.
        assert!(
            raw.windows(b"mcts_root_q.npy".len())
                .any(|w| w == b"mcts_root_q.npy"),
            "mcts_root_q.npy filename must appear in the archive"
        );
        assert!(
            raw.windows(b"mcts_q_valid.npy".len())
                .any(|w| w == b"mcts_q_valid.npy"),
            "mcts_q_valid.npy filename must appear in the archive"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_npz_with_extras_and_provenance_produces_ten_entries() {
        // Bead `sts2-rs-acbj` (step 1): provenance writer round-trip.
        // Ten members: features, outcomes, final_hp_norm,
        // visit_distribution, run_id_u64, encounter_tier_u8,
        // legal_action_mask, raw_texel_score, and the Phase-0
        // combat-redesign mcts_root_q + mcts_q_valid reference target.
        let dir = std::env::temp_dir().join("sts2-rs-acbj-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("smoke-acbj.npz");
        let features: Vec<f32> = (0..160).map(|i| i as f32 * 0.01).collect();
        let outcomes: Vec<u8> = vec![1, 0];
        let final_hp: Vec<f32> = vec![0.8, 0.2];
        let visits: Vec<f32> = (0..1280).map(|i| (i as f32) * 0.001).collect();
        let run_ids: Vec<u64> = vec![0x1111_2222_3333_4444, 0xdead_beef_dead_beef];
        let tiers: Vec<u8> = vec![2, 1];
        let legal_mask: Vec<u8> = vec![0; 2 * 640];
        let raw_texel: Vec<f32> = vec![0.123, -0.456];
        let mcts_root_q: Vec<f32> = vec![0.5, -0.5];
        let mcts_q_valid: Vec<u8> = vec![1, 1];
        let n = write_npz_with_extras_and_provenance(
            &path,
            &features,
            2,
            80,
            &outcomes,
            &final_hp,
            &visits,
            640,
            &run_ids,
            &tiers,
            &legal_mask,
            &raw_texel,
            &mcts_root_q,
            &mcts_q_valid,
        )
        .expect("write succeeds");
        assert!(n > 0);
        let raw = std::fs::read(&path).expect("read back");
        let sig = 0x0403_4b50u32.to_le_bytes();
        let count = (0..raw.len() - 4)
            .filter(|i| raw[*i..*i + 4] == sig)
            .count();
        assert_eq!(
            count, 10,
            "expected 10 .npy entries (8 + Phase-0 mcts_root_q/mcts_q_valid), got {count}"
        );
        let raw_str_mask = String::from_utf8_lossy(&raw);
        assert!(raw_str_mask.contains("legal_action_mask.npy"));
        // Each declared name appears verbatim in the central
        // directory; spot-check the provenance + Phase-0 target keys.
        let raw_str = String::from_utf8_lossy(&raw);
        assert!(raw_str.contains("run_id_u64.npy"));
        assert!(raw_str.contains("encounter_tier_u8.npy"));
        assert!(raw_str.contains("mcts_root_q.npy"));
        assert!(raw_str.contains("mcts_q_valid.npy"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_npz_narrow_with_provenance_produces_four_entries() {
        // Bead sts2-rs-e2bjk: narrow-path provenance writer round-trip.
        // Four members: features, outcomes, run_id_u64,
        // encounter_tier_u8.
        let dir = std::env::temp_dir().join("sts2-rs-e2bjk-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("smoke-e2bjk.npz");
        // 27-col narrow schema × 2 rows.
        let features: Vec<f32> = (0..54).map(|i| i as f32 * 0.01).collect();
        let outcomes: Vec<u8> = vec![1, 0];
        let run_ids: Vec<u64> = vec![0xcafe_d00d_cafe_d00d, 0xbeef_dead_beef_dead];
        let tiers: Vec<u8> = vec![2, 1];
        let n =
            write_npz_narrow_with_provenance(&path, &features, 2, 27, &outcomes, &run_ids, &tiers)
                .expect("write succeeds");
        assert!(n > 0);
        let raw = std::fs::read(&path).expect("read back");
        let sig = 0x0403_4b50u32.to_le_bytes();
        let count = (0..raw.len() - 4)
            .filter(|i| raw[*i..*i + 4] == sig)
            .count();
        assert_eq!(count, 4, "expected 4 .npy entries, got {count}");
        let raw_str = String::from_utf8_lossy(&raw);
        assert!(raw_str.contains("features.npy"));
        assert!(raw_str.contains("outcomes.npy"));
        assert!(raw_str.contains("run_id_u64.npy"));
        assert!(raw_str.contains("encounter_tier_u8.npy"));
        let _ = std::fs::remove_file(&path);
    }
}
