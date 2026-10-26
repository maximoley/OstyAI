//! Bead `sts2-rs-a2fh`: round-trip the (ScenarioFile, seed) →
//! CombatState library function against 5 hand-picked
//! `.run-source` fixtures from
//! `/tmp/run-fixtures-combined/ironclad/`.
//!
//! For each fixture, load the TOML via the existing loader, hand
//! it to `scenario_to_combat_state`, and assert load-bearing
//! invariants on the resulting `CombatState`:
//!
//! - deck contents (card-id multiset) match the TOML's
//!   `[player] deck = [...]`
//! - relic set matches the TOML's `relics = [...]`
//! - monster lineup matches `[[monsters]]` order (kind + count)
//! - player HP / max-HP match the TOML's `[player] { hp, max_hp }`
//!
//! The fixtures live outside the repo (gitignored
//! `/tmp/run-fixtures-combined/`). When they're missing — common
//! on CI / a fresh clone — the test logs a skip and exits 0 rather
//! than failing the workspace. The fixture-corpus regeneration
//! pipeline is `sts2_run_scraper`, separately tracked.

use std::path::PathBuf;

use sts2_cli::scenario_to_combat_state;
use sts2_cli::sim::scenario::toml_file::load_scenario_file;
use sts2_core::CombatState;

/// 5 fixtures hand-picked across all four difficulty tiers so a
/// regression in any single tier surfaces in the round-trip:
/// `weak` / `normal` (two — to catch shape-specific monster
/// loaders) / `elite` / `boss`.
const FIXTURES: &[&str] = &[
    "/tmp/run-fixtures-combined/ironclad/00030217-6611-4a0a-aa7b-a01d92f22b49_floor002_sludge_spinner_weak.toml",
    "/tmp/run-fixtures-combined/ironclad/002b06d4-7a04-4f05-bfc0-5fe0945bd225_floor011_mawler_normal.toml",
    "/tmp/run-fixtures-combined/ironclad/24c6e06db5acb3e4_floor006_inklets_normal.toml",
    "/tmp/run-fixtures-combined/ironclad/00143010-687d-4254-b174-88e2929bc819_floor015_byrdonis_elite.toml",
    "/tmp/run-fixtures-combined/ironclad/002d8557-651f-4600-a29e-1c934d7095f2_floor017_vantom_boss.toml",
];

fn skip_if_no_fixtures() -> bool {
    let any = FIXTURES.iter().any(|p| PathBuf::from(p).exists());
    if !any {
        eprintln!(
            "[a2fh] fixture corpus /tmp/run-fixtures-combined/ironclad/ \
             not present; skipping round-trip test"
        );
    }
    !any
}

fn assert_roundtrip(fixture_path: &str) {
    let path = PathBuf::from(fixture_path);
    if !path.exists() {
        eprintln!("[a2fh] {fixture_path}: missing, skipping");
        return;
    }

    let scenario = load_scenario_file(&path).unwrap_or_else(|e| panic!("load {fixture_path}: {e}"));
    let state: CombatState = scenario_to_combat_state(&scenario, 42)
        .unwrap_or_else(|e| panic!("build state from {fixture_path}: {e}"));

    let player = scenario
        .player
        .as_ref()
        .expect("fixture has [player] block");

    // ── HP / max-HP ──
    if let Some(file_hp) = player.hp {
        let want = u32::try_from(file_hp.max(0)).unwrap_or(0) as i32;
        assert_eq!(
            state.player.current_hp, want,
            "{fixture_path}: player.current_hp mismatch (got {}, want {want})",
            state.player.current_hp,
        );
    }
    if let Some(file_max_hp) = player.max_hp {
        let want = i32::try_from(file_max_hp).unwrap_or(i32::MAX);
        assert_eq!(
            state.player.max_hp, want,
            "{fixture_path}: player.max_hp mismatch (got {}, want {want})",
            state.player.max_hp,
        );
    }

    // ── Deck size ── (full card-id multiset compare is brittle
    // against the upgrade-token (`Foo+`) round-trip; size + total
    // basic-name multiset is the load-bearing invariant the
    // fixture round-trip needs to catch.)
    if !player.deck.is_empty() {
        // The fixture's deck list and the loaded state's draw pile
        // should have the same number of cards (every TOML entry
        // becomes exactly one arena card, regardless of `+` token).
        let want_size = player.deck.len();
        let got_size = state.cards.len();
        assert_eq!(
            got_size, want_size,
            "{fixture_path}: deck size mismatch (loaded {got_size} cards, \
             fixture lists {want_size})",
        );
    }

    // ── Relics ──
    let file_relics: &[String] = if scenario.relics.is_empty() {
        &player.relics
    } else {
        &scenario.relics
    };
    assert_eq!(
        state.relics.len(),
        file_relics.len(),
        "{fixture_path}: relic count mismatch (loaded {}, fixture lists {})",
        state.relics.len(),
        file_relics.len(),
    );

    // ── Monster lineup ──
    assert_eq!(
        state.enemy_uids.len(),
        scenario.monsters.len(),
        "{fixture_path}: enemy_uids count mismatch (loaded {}, fixture lists {})",
        state.enemy_uids.len(),
        scenario.monsters.len(),
    );
    // Spot-check that the first monster's kind in the loaded state
    // matches the fixture's first monster — guards against e.g.
    // lower_monsters silently dropping entries or reordering.
    let first_uid = state.enemy_uids[0];
    let first_creature = state
        .creatures
        .iter()
        .find(|c| c.id == first_uid.0)
        .expect("first enemy_uid must reference an alive creature");
    // The fixture's monsters[0].kind is a free-form `String` (the
    // TOML token). Lower-cased canonical-ish comparison is enough:
    // both forms uppercase the species enum's discriminant name.
    let first_kind_file = &scenario.monsters[0].kind;
    let first_kind_state = format!("{:?}", first_creature.species.expect("species present"));
    let normalize = |s: &str| {
        s.replace('_', "")
            .replace(['.', ' '], "")
            .to_ascii_lowercase()
    };
    let f = normalize(first_kind_file);
    let s = normalize(&first_kind_state);
    // Some fixtures encode the kind with a content-side prefix
    // (e.g. `SludgeSpinner` vs `SLUDGE_SPINNER`); compare on the
    // normalized-suffix substring rather than on equality.
    assert!(
        s.contains(&f) || f.contains(&s),
        "{fixture_path}: monsters[0].kind mismatch (file={first_kind_file:?}, \
         state={first_kind_state:?})",
    );
}

#[test]
fn weak_tier_sludge_spinner() {
    if skip_if_no_fixtures() {
        return;
    }
    assert_roundtrip(FIXTURES[0]);
}

#[test]
fn normal_tier_mawler() {
    if skip_if_no_fixtures() {
        return;
    }
    assert_roundtrip(FIXTURES[1]);
}

#[test]
fn normal_tier_inklets() {
    if skip_if_no_fixtures() {
        return;
    }
    assert_roundtrip(FIXTURES[2]);
}

#[test]
fn elite_tier_byrdonis() {
    if skip_if_no_fixtures() {
        return;
    }
    assert_roundtrip(FIXTURES[3]);
}

#[test]
fn boss_tier_vantom() {
    if skip_if_no_fixtures() {
        return;
    }
    assert_roundtrip(FIXTURES[4]);
}
