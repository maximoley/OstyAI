//! Gate A1 (spec Phase 2 §4a): Rust featurization must reproduce the
//! Python encoder EXACTLY on every fixture row. This is the load-bearing
//! gate — and the mechanism that keeps future small encoder tweaks safe.
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use sts2_runai::ppo::encoder::{featurize_with, Featurized, N_CONT};
use sts2_runai::ppo::features::Features;
use sts2_runai::ppo::obs::{ActionDesc, PpoObs};

#[derive(Deserialize)]
struct FeatJson {
    obs_cat: Vec<Vec<i64>>,
    obs_cont: Vec<Vec<f64>>,
    obs_sec: Vec<i64>,
    act_cat: Vec<Vec<i64>>,
    act_cont: Vec<Vec<f64>>,
    act_sec: Vec<i64>,
    n_actions: usize,
}

#[derive(Deserialize)]
struct Row {
    seed: i64,
    decision: i64,
    surface: String,
    /// `encoder.py::fine_surface` of this row's actions — the finer key the
    /// state token actually carries. Recorded by the dump script (GH #399) so
    /// the fixture can assert deck-mutation coverage; `default` keeps older
    /// fixtures (dumped before the field existed) loadable.
    #[serde(default)]
    fine_surface: String,
    /// `STS2_PPO_FEATURES` names the row was RECORDED at (`ppo.schema`).
    /// Absent = the default schema, which is what every row dumped before
    /// the mechanism existed carries — so the fixture pins the OFF path
    /// even when the runner's own environment has a feature enabled.
    #[serde(default)]
    features: Vec<String>,
    obs: PpoObs,
    actions: Vec<ActionDesc>,
    feat: FeatJson,
}

/// Deck-mutation surfaces became reachable in GH #399. They are sparse (a few
/// decisions per RUN), so the dump script deliberately walks extra seeds to
/// collect them — assert here that the committed fixture still carries them,
/// or a future regen that quietly loses them would leave these encoder
/// branches unpinned again without anything going red.
const REQUIRED_FINE_SURFACES: [&str; 4] = [
    "card-remove",
    "card-upgrade",
    "card-transform",
    "card-enchant",
];

/// GH #399 Task 5b: `UpgradeDeckCard{idx}` indexes the engine's
/// `Card::is_upgradable()` subset, which drops curses / statuses / Quest
/// cards. On a deck holding none of those it coincides with the `!upgraded`
/// scan the encoder used to do, so an all-A0 fixture pins the upgrade branch
/// only where the defect is invisible. The dump script walks ascension-5 seeds
/// (Ascender's Bane from floor 1) to guarantee at least one row where they
/// diverge, and asserts the same predicate as this one.
///
/// NOT a length test: a row can hold a non-upgradable card and still featurize
/// identically to the pre-fix code, if every excluded card sits AFTER all the
/// offered ones (curse last, upgrade taken before any acquisition) — then the
/// two spaces agree on every offer position and the shorter length changes
/// nothing. The real condition is POSITIONAL: some excluded master-deck index
/// must sit before some offered one. Mirrors
/// `dump_parity_fixtures.py::_upgrade_index_spaces_differ` and the assertion
/// in `sts2-pycli/tests/test_ppo_encoder_index_spaces.py`.
fn upgrade_index_spaces_differ(row: &Row) -> bool {
    let Some(offered) = row.obs.upgrade_select_deck_indices.as_ref() else {
        return false;
    };
    let excluded = row
        .obs
        .deck
        .iter()
        .enumerate()
        .filter(|(i, (_, up))| !up && !offered.contains(&(*i as i64)))
        .map(|(i, _)| i as i64);
    match (offered.iter().max(), excluded.min()) {
        (Some(last_offered), Some(first_excluded)) => *last_offered > first_excluded,
        _ => false,
    }
}

fn fixture() -> Option<String> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ppo_encoder_parity.jsonl");
    fs::read_to_string(p).ok()
}

/// The default fixture AND every per-feature supplement beside it.
fn fixtures() -> Vec<String> {
    sts2_runai::ppo::test_fixture_paths("ppo_encoder_parity")
        .iter()
        .filter_map(|p| fs::read_to_string(p).ok())
        .collect()
}

fn assert_f32_rows(kind: &str, row: &Row, got: &[[f32; N_CONT]], want: &[Vec<f64>]) {
    assert_eq!(
        got.len(),
        want.len(),
        "{kind} len @ seed {} d{}",
        row.seed,
        row.decision
    );
    for (t, (g, w)) in got.iter().zip(want).enumerate() {
        for (c, (gv, wv)) in g.iter().zip(w).enumerate() {
            let wv = *wv as f32;
            assert!(
                gv.to_bits() == wv.to_bits(),
                "{kind}[{t}][{c}] {gv} != {wv} @ seed {} d{} ({})",
                row.seed,
                row.decision,
                row.surface
            );
        }
    }
}

/// The schema a fixture row was recorded at — NOT the ambient environment.
fn row_features(row: &Row) -> Features {
    Features::parse(&row.features.join(","))
}

#[test]
fn featurization_matches_python_exactly() {
    let texts = fixtures();
    if texts.is_empty() {
        eprintln!("skip: fixture absent (run ppo.dump_parity_fixtures)");
        return;
    }
    let mut rows = 0usize;
    let mut split_upgrade_rows = 0usize;
    let mut gated_rows = 0usize;
    let mut fine_seen: BTreeSet<String> = BTreeSet::new();
    for line in texts
        .iter()
        .flat_map(|t| t.lines())
        .filter(|l| !l.trim().is_empty())
    {
        let row: Row = serde_json::from_str(line).expect("fixture row");
        // The feature fixture must EXERCISE the gated branch somewhere, or it
        // is a second copy of the default arm under a different filename.
        //
        // Counted across the file, not asserted per row. It was per-row while
        // the feature fixture was a 3-row supplement to a default-schema
        // champion; since Ironclad's champion declares `reward_upgrade`
        // (2026-08-28) that fixture carries the FULL surface walk, so most of
        // its rows legitimately have no upgraded reward — `roll_for_upgrade`
        // is `act_index * 0.25` off a 0.0 combat base, and act 1 can never
        // produce one at all.
        if !row.features.is_empty()
            && row
                .obs
                .card_reward_option_upgraded
                .as_deref()
                .is_some_and(|f| f.contains(&true))
        {
            gated_rows += 1;
        }
        if row.fine_surface == "card-upgrade" && upgrade_index_spaces_differ(&row) {
            split_upgrade_rows += 1;
        }
        // The Rust mirror of `encoder.py::fine_surface` must agree with the
        // label Python recorded. (`obs_cat[0][8]` already encodes it through
        // the vocab, but this pins the *string* — a vocab-index collision
        // could otherwise hide a mismatch.)
        if !row.fine_surface.is_empty() {
            assert_eq!(
                sts2_runai::ppo::encoder::fine_surface(&row.actions),
                row.fine_surface,
                "fine_surface @ seed {} d{}",
                row.seed,
                row.decision
            );
            fine_seen.insert(row.fine_surface.clone());
        }
        let got: Featurized = featurize_with(&row.obs, &row.actions, row_features(&row));
        assert_eq!(got.n_actions, row.feat.n_actions);
        assert_eq!(
            got.obs_cat.iter().map(|r| r.to_vec()).collect::<Vec<_>>(),
            row.feat.obs_cat,
            "obs_cat @ seed {} d{}",
            row.seed,
            row.decision
        );
        assert_eq!(got.obs_sec, row.feat.obs_sec);
        assert_eq!(
            got.act_cat.iter().map(|r| r.to_vec()).collect::<Vec<_>>(),
            row.feat.act_cat,
            "act_cat @ seed {} d{}",
            row.seed,
            row.decision
        );
        assert_eq!(got.act_sec, row.feat.act_sec);
        assert_f32_rows("obs_cont", &row, &got.obs_cont, &row.feat.obs_cont);
        assert_f32_rows("act_cont", &row, &got.act_cont, &row.feat.act_cont);
        rows += 1;
    }
    assert!(rows >= 100, "suspiciously few fixture rows: {rows}");
    // Whole-file replacement for the old per-row check above. Without it a
    // fixture dumped at a feature schema could contain zero rows that actually
    // differ from the default arm, and every featurization assertion below
    // would pass while pinning nothing about the feature.
    assert!(
        gated_rows >= 3,
        "fixture set has {gated_rows} rows that exercise a gated encoder \
         feature (need >= 3) — the walk never reached an act-2+ upgraded card \
         reward, so the ON branch is unpinned. Re-dump with \
         STS2_PPO_FEATURES set, adding seeds if needed."
    );
    let missing: Vec<&str> = REQUIRED_FINE_SURFACES
        .iter()
        .copied()
        .filter(|s| !fine_seen.contains(*s))
        .collect();
    assert!(
        missing.is_empty(),
        "fixture lacks deck-mutation rows: {missing:?} (has {fine_seen:?}) — \
         regenerate with `python -m ppo.dump_parity_fixtures`"
    );
    assert!(
        split_upgrade_rows > 0,
        "fixture has no `card-upgrade` row where the engine's `is_upgradable()` \
         offer skips a card that sits BEFORE some offered one — the Task 5b \
         branch is pinned only where the defect is invisible. Regenerate with \
         `python -m ppo.dump_parity_fixtures` (its `--asc-seeds` tail)"
    );
    println!(
        "gate A1: {rows} rows exact ({gated_rows} at a gated schema), fine \
         surfaces {fine_seen:?}, {split_upgrade_rows} split-index-space upgrade rows"
    );
}

/// The split-row pin must be POSITIONAL, not a length test — otherwise a
/// future regen could land a `card-upgrade` row whose excluded cards all sit
/// AFTER every offered one (curse last, upgrade taken before any acquisition),
/// which featurizes identically to the pre-fix code while keeping the assert
/// green. Take the real split row and re-point its offer at a PREFIX of the
/// non-upgraded cards: the offer is still strictly shorter than the
/// `!upgraded` scan (a length test would still call it split), but no offer
/// position can name a different card, so the helper must say false.
#[test]
fn split_row_predicate_is_positional_not_length_based() {
    let Some(text) = fixture() else {
        eprintln!("skip: fixture absent (run ppo.dump_parity_fixtures)");
        return;
    };
    let mut row: Row = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Row>(l).expect("fixture row"))
        .find(|r| r.fine_surface == "card-upgrade" && upgrade_index_spaces_differ(r))
        .expect("a split-index-space upgrade row");

    let non_upgraded: Vec<i64> = row
        .obs
        .deck
        .iter()
        .enumerate()
        .filter(|(_, (_, up))| !up)
        .map(|(i, _)| i as i64)
        .collect();
    assert!(non_upgraded.len() >= 3, "need room to drop a tail");
    let prefix = non_upgraded[..non_upgraded.len() - 2].to_vec();
    assert!(
        prefix.len() < non_upgraded.len(),
        "the rigged offer must stay SHORTER, or this proves nothing"
    );
    row.obs.upgrade_select_deck_indices = Some(prefix);
    assert!(
        !upgrade_index_spaces_differ(&row),
        "the split-row predicate regressed to a length test: every excluded \
         card sits after every offered one, so both index spaces name the \
         same card at every offer position"
    );
}

/// The fixture's dense head is A0 (only the Task 5b coverage tail is A5), so
/// the appended ascension slot is barely exercised above. Take a real A0
/// fixture obs, raise its ascension, and confirm the state token's `C_ASC`
/// slot tracks `ascension * ASC_SCALE` while every other continuous slot is
/// byte-identical to the A0 encoding — i.e. the input is purely additive in
/// the trailing column (what makes an 18-cont champion's drop of that column
/// a no-op).
#[test]
fn ascension_writes_only_the_trailing_state_slot() {
    use sts2_runai::ppo::encoder::{ASC_SCALE, C_ASC};

    let Some(text) = fixture() else {
        eprintln!("skip: fixture absent (run ppo.dump_parity_fixtures)");
        return;
    };
    let row: Row = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Row>(l).expect("fixture row"))
        .find(|r| r.obs.ascension == 0)
        .expect("an A0 fixture row");

    let base = featurize_with(&row.obs, &row.actions, row_features(&row));
    let mut obs = row.obs.clone();
    obs.ascension = 10; // A10 → C_ASC = 1.0
    let hi = featurize_with(&obs, &row.actions, row_features(&row));

    // Only the state token (index 0) slot C_ASC changes; everything else equal.
    for (t, (b, h)) in base.obs_cont.iter().zip(&hi.obs_cont).enumerate() {
        for (c, (bv, hv)) in b.iter().zip(h).enumerate() {
            if t == 0 && c == C_ASC {
                assert_eq!(bv.to_bits(), 0.0f32.to_bits(), "A0 baseline slot");
                assert_eq!(
                    hv.to_bits(),
                    ((10.0 * ASC_SCALE) as f32).to_bits(),
                    "A10 ascension slot"
                );
            } else {
                assert_eq!(bv.to_bits(), hv.to_bits(), "slot [{t}][{c}] must be stable");
            }
        }
    }
    // Action tokens never carry ascension.
    assert_eq!(base.act_cont, hi.act_cont);
}
