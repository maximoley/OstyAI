// Tests in this module construct legacy scenarios via
// `Scenario::from_legacy_args(EncounterKind::*)`; Phase 4
// (`sts2-rs-90n5`) deprecated that path but kept it alive for
// existing fixtures. Suppress in-file deprecation warnings.
#![allow(deprecated)]

//! Starter-relic resolution + `state.relics` seeding — bead
//! `sts2-rs-q3tv` (P9 / cliext C3).
//!
//! Three input paths feed [`RelicInput`]:
//!
//! 1. `--relic <Id,Id,...>` — inline comma-separated.
//! 2. `--relic-preset <name>` — `none` / `ironclad-starter` /
//!    `silent-starter` / `kitchen-sink`.
//! 3. `--relic-file <path>` — one `RelicId` per line.
//!
//! Each input is resolved to a `Vec<RelicId>` via C1's
//! [`sts2_content::names::relic_id_from_str`] (dual-form
//! `PascalCase` / `"Display Form"`) and seeded into `state.relics`
//! BEFORE `combat_loop::start_combat`. The Phase-7 dispatchers
//! (`hooks::dispatch_before_combat_start` +
//! `relic_effects::dispatch_before_combat_start`) walk every
//! registered listener at start-of-combat; listeners gate themselves
//! on `state.relics.iter().any(|r| r.kind == kind)`, so the relic
//! POD MUST be present before that dispatch fires.
//!
//! ## Audit: do `on_combat_start` handlers mutate `state.relics`?
//!
//! Reviewed `crates/sts2-content/src/relics/*.rs` and
//! `crates/sts2-core/src/relic_effects.rs` for `state.relics.push /
//! remove / retain / clear` at the pin `01c094d` + current HEAD:
//!
//! - All `state.relics.push(...)` occurrences are inside `#[cfg(test)]`
//!   test fixtures (e.g. `event_reward.rs::push_relic`, `hooks.rs`
//!   listener-iteration tests). NONE are inside a registered
//!   `before_combat_start` / `after_combat_start` listener body.
//! - Production listeners ITERATE `state.relics` (gate by `kind`) but
//!   never mutate the list.
//!
//! Conclusion: a single seed step before `combat_loop::start_combat`
//! suffices. No re-seed step is required after `start_combat`.

use std::path::PathBuf;

use sts2_content::names::{
    is_lenient_skip_relic_token, pascal_display, relic_id_from_str, suggest_relic_id,
};
use sts2_core::relics::RelicId;
use sts2_core::{CombatState, Relic, RelicKind};

use std::num::NonZeroU32;

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// Source of the starter-relic list passed on the CLI. Exactly one of
/// the three variants is populated per invocation; the clap arg group
/// enforces mutual exclusivity at parse time.
#[derive(Debug, Clone, Default)]
pub enum RelicInput {
    /// `--relic BurningBlood,Anchor` — comma-separated literal list.
    Inline(Vec<String>),
    /// `--relic-preset ironclad-starter` — symbolic name.
    Preset(String),
    /// `--relic-file relics.txt` — one `RelicId` per line.
    File(PathBuf),
    /// No relic flag supplied — `state.relics` stays empty.
    #[default]
    None,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Failure modes for [`resolve_relics`]. Display impl emits a
/// CLI-friendly one-liner (potentially multi-line for did-you-mean).
#[derive(Debug)]
pub enum RelicError {
    /// `--relic SomeId` did not match any `RelicId`, dual-form or
    /// otherwise. The included Vec is the top-3 did-you-mean
    /// suggestions in canonical `PascalCase`.
    UnknownRelic {
        input: String,
        suggestions: Vec<String>,
    },
    /// `--relic-preset foo` is not in the supported preset set.
    UnknownPreset {
        input: String,
        supported: Vec<&'static str>,
    },
    /// `--relic-file path` could not be opened.
    FileRead {
        path: PathBuf,
        source: std::io::Error,
    },
    /// One of the relic-pool members has no `RelicKind` variant yet
    /// (the runtime POD uses `RelicKind`, which at pin `01c094d` is
    /// `#[non_exhaustive]` and only covers relics with a `RelicSpec`
    /// factory in `sts2-content`).
    NoRuntimeKind { input: String, relic_id: RelicId },
}

impl std::fmt::Display for RelicError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownRelic { input, suggestions } => {
                write!(f, "unknown relic `{input}`")?;
                if !suggestions.is_empty() {
                    write!(f, " — did you mean: {}", suggestions.join(", "))?;
                }
                Ok(())
            }
            Self::UnknownPreset { input, supported } => {
                write!(
                    f,
                    "unknown --relic-preset `{input}` (supported: {})",
                    supported.join(", ")
                )
            }
            Self::FileRead { path, source } => {
                write!(f, "--relic-file `{}` read error: {source}", path.display())
            }
            Self::NoRuntimeKind { input, relic_id } => write!(
                f,
                "relic `{input}` (RelicId::{relic_id:?}) has no runtime RelicKind yet — \
                 not seedable until its content-side pool bead lands"
            ),
        }
    }
}

impl std::error::Error for RelicError {}

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

const PRESET_NAMES: &[&str] = &["none", "ironclad-starter", "silent-starter", "kitchen-sink"];

/// Resolve a preset name to its constituent [`RelicId`]s. Returns
/// `None` (and an `UnknownPreset` from the caller) for an unrecognised
/// preset.
fn preset_to_relic_ids(name: &str) -> Option<Vec<RelicId>> {
    match name {
        "none" => Some(Vec::new()),
        "ironclad-starter" => Some(vec![RelicId::BurningBlood]),
        "silent-starter" => Some(vec![RelicId::RingOfTheSnake]),
        "kitchen-sink" => Some(vec![
            RelicId::BurningBlood,
            RelicId::CrackedCore,
            RelicId::RingOfTheSnake,
            RelicId::DivineRight,
            RelicId::BoundPhylactery,
        ]),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// Resolve a [`RelicInput`] to a flat `Vec<RelicId>`. The list MAY be
/// empty (e.g. `RelicInput::None` or `preset = "none"`).
pub fn resolve_relics(input: &RelicInput) -> Result<Vec<RelicId>, RelicError> {
    match input {
        RelicInput::None => Ok(Vec::new()),
        RelicInput::Inline(names) => names
            .iter()
            .filter(|n| !is_lenient_skip_relic_token(n))
            .map(|n| resolve_one(n))
            .collect(),
        RelicInput::Preset(name) => {
            preset_to_relic_ids(name).ok_or_else(|| RelicError::UnknownPreset {
                input: name.clone(),
                supported: PRESET_NAMES.to_vec(),
            })
        }
        RelicInput::File(path) => {
            let contents = std::fs::read_to_string(path).map_err(|e| RelicError::FileRead {
                path: path.clone(),
                source: e,
            })?;
            contents
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .filter(|l| !is_lenient_skip_relic_token(l))
                .map(resolve_one)
                .collect()
        }
    }
}

fn resolve_one(name: &str) -> Result<RelicId, RelicError> {
    relic_id_from_str(name).ok_or_else(|| RelicError::UnknownRelic {
        input: name.to_string(),
        suggestions: suggest_relic_id(name),
    })
}

// ---------------------------------------------------------------------------
// RelicId -> RelicKind mapping
// ---------------------------------------------------------------------------

/// Every `RelicKind` variant exposed by `sts2-core`, aliased to the
/// macro-derived [`RelicKind::ALL`] roster (soft-drop census wave 3,
/// 2026-07-17). This table was previously hand-maintained and drifted
/// behind the enum across at least seven beads (`pgvl`, `84ys`,
/// `pibg`, `8uvz`, `qffh`, `226v`, `nmy9`, …) — every drift wave
/// surfaced as "no runtime RelicKind" scenario-load failures for
/// relics the engine fully supported. Aliasing `ALL` makes the table
/// exhaustive by construction: a new enum variant is seedable the
/// moment it exists. (The old hand-list history is preserved in git;
/// see commit `9d0cae1c4` and earlier.)
const RELIC_KINDS: &[RelicKind] = RelicKind::ALL;

/// Map a `RelicId` to the matching `RelicKind` via python-name equality.
/// Returns `None` when the requested relic has no `RelicKind` variant
/// yet (the runtime POD won't accept it).
#[must_use]
pub fn relic_id_to_kind(id: RelicId) -> Option<RelicKind> {
    let target = id.to_python_name();
    RELIC_KINDS
        .iter()
        .copied()
        .find(|k| k.python_name() == target)
}

// ---------------------------------------------------------------------------
// Seeding
// ---------------------------------------------------------------------------

/// Push every resolved relic into `state.relics` as a `Relic` POD. The
/// per-instance `id: NonZeroU32` is assigned 1-based in input order;
/// `counter` defaults to `0` (relics that need a non-zero starting
/// counter — `EmberTea` / `BoneTea` / etc. — are documented per-relic
/// in `crates/sts2-content/src/relics/event_reward.rs` and seeding
/// them via this path is out of scope for this bead).
///
/// MUST be called BEFORE `combat_loop::start_combat`.
#[cfg_attr(not(test), allow(dead_code))] // Live in this module's test suite; reserved for production callers that don't need counter seeding.
pub fn seed_relics(state: &mut CombatState, relics: &[RelicId]) -> Result<(), RelicError> {
    seed_relics_with_counters(state, relics, &std::collections::BTreeMap::new())
}

/// Like [`seed_relics`] but seeds non-zero `Relic.counter` values from
/// a token-keyed map. Used by the schema=2 scenario loader to restore
/// counter state from `[player.relic_counters]`.
///
/// Counter keys are matched against each `RelicId::to_python_name()`
/// (the `UPPER_SNAKE_CASE` form, e.g. `"JOSS_PAPER"`). Unknown keys are
/// silently ignored — they may target relics not in the supplied list.
pub fn seed_relics_with_counters(
    state: &mut CombatState,
    relics: &[RelicId],
    counters: &std::collections::BTreeMap<String, i32>,
) -> Result<(), RelicError> {
    for (idx, &id) in relics.iter().enumerate() {
        let kind = relic_id_to_kind(id).ok_or_else(|| RelicError::NoRuntimeKind {
            input: format!("{id:?}"),
            relic_id: id,
        })?;
        let raw = u32::try_from(state.relics.len() + idx + 1).expect("relic count fits u32");
        let uid = NonZeroU32::new(raw).expect("non-zero");
        let counter = counters.get(id.to_python_name()).copied().unwrap_or(0);
        state.relics.push(Relic {
            id: uid,
            kind,
            counter,
            is_wax: false,
            is_melted: false,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Banner formatting
// ---------------------------------------------------------------------------

/// Render a resolved relic list as a comma-separated canonical
/// `Pascal Display` string (e.g. `"Burning Blood, Anchor"`). Empty
/// list renders as `"none"`.
#[must_use]
pub fn format_relics_for_banner(relics: &[RelicId]) -> String {
    if relics.is_empty() {
        return "none".to_string();
    }
    let parts: Vec<String> = relics
        .iter()
        .map(|id| pascal_display(&snake_to_pascal(id.to_python_name())))
        .collect();
    parts.join(", ")
}

/// Local `UPPER_SNAKE_CASE → PascalCase` helper. Duplicates the
/// (private) C1 helper rather than widening that module's public API
/// for one call site; if a third caller appears, promote it.
fn snake_to_pascal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut next_upper = true;
    for ch in s.chars() {
        if ch == '_' {
            next_upper = true;
        } else if next_upper {
            out.extend(ch.to_uppercase());
            next_upper = false;
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::scenario::Scenario;
    use crate::sim::{EncounterKind, PlayerClass};

    #[test]
    fn resolve_inline_dual_form() {
        let pascal = resolve_relics(&RelicInput::Inline(vec!["BurningBlood".into()])).unwrap();
        let display = resolve_relics(&RelicInput::Inline(vec!["Burning Blood".into()])).unwrap();
        assert_eq!(pascal, vec![RelicId::BurningBlood]);
        assert_eq!(display, vec![RelicId::BurningBlood]);
    }

    #[test]
    fn resolve_inline_multi() {
        let got = resolve_relics(&RelicInput::Inline(vec![
            "Anchor".into(),
            "RingOfTheSnake".into(),
        ]))
        .unwrap();
        assert_eq!(got, vec![RelicId::Anchor, RelicId::RingOfTheSnake]);
    }

    #[test]
    fn resolve_unknown_returns_suggestions() {
        let err = resolve_relics(&RelicInput::Inline(vec!["BurningBleed".into()])).unwrap_err();
        match err {
            RelicError::UnknownRelic { suggestions, .. } => {
                assert!(suggestions.iter().any(|s| s == "BurningBlood"));
            }
            other => panic!("expected UnknownRelic, got {other:?}"),
        }
    }

    #[test]
    fn preset_ironclad_starter() {
        let got = resolve_relics(&RelicInput::Preset("ironclad-starter".into())).unwrap();
        assert_eq!(got, vec![RelicId::BurningBlood]);
    }

    #[test]
    fn preset_silent_starter() {
        let got = resolve_relics(&RelicInput::Preset("silent-starter".into())).unwrap();
        assert_eq!(got, vec![RelicId::RingOfTheSnake]);
    }

    #[test]
    fn preset_kitchen_sink_covers_all_starters() {
        let got = resolve_relics(&RelicInput::Preset("kitchen-sink".into())).unwrap();
        assert!(got.contains(&RelicId::BurningBlood));
        assert!(got.contains(&RelicId::RingOfTheSnake));
        assert!(got.contains(&RelicId::CrackedCore));
        assert!(got.contains(&RelicId::DivineRight));
        assert!(got.contains(&RelicId::BoundPhylactery));
    }

    #[test]
    fn preset_none_yields_empty() {
        let got = resolve_relics(&RelicInput::Preset("none".into())).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn preset_unknown_errors() {
        let err = resolve_relics(&RelicInput::Preset("bogus".into())).unwrap_err();
        match err {
            RelicError::UnknownPreset { supported, .. } => {
                assert!(supported.contains(&"ironclad-starter"));
            }
            other => panic!("expected UnknownPreset, got {other:?}"),
        }
    }

    #[test]
    fn relic_id_to_kind_for_burning_blood() {
        assert_eq!(
            relic_id_to_kind(RelicId::BurningBlood),
            Some(RelicKind::BurningBlood)
        );
    }

    /// Beads `sts2-rs-pgvl` (7 variants) + `sts2-rs-84ys` (4 more:
    /// OrnamentalFan / GamblingChip / GremlinHorn / FrozenEgg) — the
    /// 11 RelicKind variants added to drain the sts2log gap-report
    /// §93 `NoRuntimeKind` errors must resolve via `relic_id_to_kind`.
    /// Each entry is a pure runtime-discriminant marker (no listener
    /// body) — the listener bodies stay deferred to the P7 relic-
    /// surface umbrella.
    #[test]
    fn relic_id_to_kind_drains_sts2log_gap_report_relics() {
        let cases = [
            (RelicId::ScrollBoxes, RelicKind::ScrollBoxes),
            (RelicId::LeadPaperweight, RelicKind::LeadPaperweight),
            (RelicId::ArcaneScroll, RelicKind::ArcaneScroll),
            (RelicId::NewLeaf, RelicKind::NewLeaf),
            (RelicId::PrecariousShears, RelicKind::PrecariousShears),
            (RelicId::LeafyPoultice, RelicKind::LeafyPoultice),
            (RelicId::CursedPearl, RelicKind::CursedPearl),
            (RelicId::OrnamentalFan, RelicKind::OrnamentalFan),
            (RelicId::GamblingChip, RelicKind::GamblingChip),
            (RelicId::GremlinHorn, RelicKind::GremlinHorn),
            (RelicId::FrozenEgg, RelicKind::FrozenEgg),
        ];
        for (id, expected_kind) in cases {
            assert_eq!(
                relic_id_to_kind(id),
                Some(expected_kind),
                "RelicId::{id:?} must resolve to a RelicKind",
            );
        }
    }

    /// Bead `sts2-rs-nmy9` (zs44 follow-up): InfusedCore + SturdyClamp
    /// were missing from the `RELIC_KINDS` seed table, causing 7
    /// fixtures in the Defect baseline to fail at scenario load with
    /// "no runtime RelicKind". Both RelicKind variants + python_name
    /// match arms already existed in `crates/sts2-core/src/relics.rs`.
    #[test]
    fn relic_id_to_kind_drains_nmy9_defect_blockers() {
        assert_eq!(
            relic_id_to_kind(RelicId::InfusedCore),
            Some(RelicKind::InfusedCore),
            "InfusedCore — upgraded CrackedCore for Defect starter",
        );
        assert_eq!(
            relic_id_to_kind(RelicId::SturdyClamp),
            Some(RelicKind::SturdyClamp),
            "SturdyClamp — block-retention relic",
        );
    }

    /// 2026-07-17 soft-drop census fix (wave 2): the seven census
    /// relics that were missing from the `RELIC_KINDS` seed table must
    /// resolve via `relic_id_to_kind`. StoneCalendar / UnceasingTop /
    /// CharonsAshes / Brimstone have full `relic_effects` listener
    /// bodies; OldCoin has the run-side +300-gold listener;
    /// NeowsTalisman / WingedBoots are combat-inert markers.
    #[test]
    fn relic_id_to_kind_drains_soft_drop_census_relics() {
        let cases = [
            (RelicId::StoneCalendar, RelicKind::StoneCalendar),
            (RelicId::UnceasingTop, RelicKind::UnceasingTop),
            (RelicId::CharonsAshes, RelicKind::CharonsAshes),
            (RelicId::Brimstone, RelicKind::Brimstone),
            (RelicId::OldCoin, RelicKind::OldCoin),
            (RelicId::NeowsTalisman, RelicKind::NeowsTalisman),
            (RelicId::WingedBoots, RelicKind::WingedBoots),
        ];
        for (id, expected_kind) in cases {
            assert_eq!(
                relic_id_to_kind(id),
                Some(expected_kind),
                "RelicId::{id:?} must resolve to a RelicKind",
            );
        }
    }

    /// 2026-07-17 soft-drop census fix (wave 3): the fresh-seed sweep
    /// surfaced a third tail of dropped titles. The structural fix
    /// aliases `RELIC_KINDS` to the macro-derived `RelicKind::ALL`, so
    /// each of these resolves automatically — this test pins the tail
    /// so a future regression (e.g. someone reintroducing a hand
    /// table) fails loudly. PhialHolster / NeowsBones / HeftyTablet
    /// were the last deliberate `to_relic_kind → None` deferrals and
    /// now carry combat-inert marker kinds.
    #[test]
    fn relic_id_to_kind_drains_wave3_census_relics() {
        let cases = [
            (RelicId::NutritiousOyster, RelicKind::NutritiousOyster),
            (RelicId::SilverCrucible, RelicKind::SilverCrucible),
            (RelicId::FishingRod, RelicKind::FishingRod),
            (RelicId::ToastyMittens, RelicKind::ToastyMittens),
            (RelicId::LastingCandy, RelicKind::LastingCandy),
            (RelicId::SandCastle, RelicKind::SandCastle),
            (RelicId::TungstenRod, RelicKind::TungstenRod),
            (RelicId::WhiteBeastStatue, RelicKind::WhiteBeastStatue),
            (RelicId::DelicateFrond, RelicKind::DelicateFrond),
            (RelicId::DragonFruit, RelicKind::DragonFruit),
            (RelicId::CaptainsWheel, RelicKind::CaptainsWheel),
            (RelicId::SturdyClamp, RelicKind::SturdyClamp),
            (RelicId::MoltenEgg, RelicKind::MoltenEgg),
            (RelicId::PaelsLegion, RelicKind::PaelsLegion),
            (RelicId::PaelsBlood, RelicKind::PaelsBlood),
            (RelicId::Girya, RelicKind::Girya),
            (RelicId::PaelsFlesh, RelicKind::PaelsFlesh),
            (RelicId::MummifiedHand, RelicKind::MummifiedHand),
            (RelicId::IceCream, RelicKind::IceCream),
            (RelicId::SeaGlass, RelicKind::SeaGlass),
            (RelicId::StoneHumidifier, RelicKind::StoneHumidifier),
            (RelicId::PaelsTears, RelicKind::PaelsTears),
            (RelicId::WarHammer, RelicKind::WarHammer),
            (RelicId::LoomingFruit, RelicKind::LoomingFruit),
            (RelicId::UnsettlingLamp, RelicKind::UnsettlingLamp),
            (RelicId::Kunai, RelicKind::Kunai),
            (RelicId::IntimidatingHelmet, RelicKind::IntimidatingHelmet),
            (RelicId::PrayerWheel, RelicKind::PrayerWheel),
            (RelicId::Shovel, RelicKind::Shovel),
            (RelicId::PhialHolster, RelicKind::PhialHolster),
            (RelicId::LeesWaffle, RelicKind::LeesWaffle),
            (RelicId::FrozenEgg, RelicKind::FrozenEgg),
            (RelicId::CloakClasp, RelicKind::CloakClasp),
            (RelicId::NeowsBones, RelicKind::NeowsBones),
            (RelicId::HeftyTablet, RelicKind::HeftyTablet),
        ];
        for (id, expected_kind) in cases {
            assert_eq!(
                relic_id_to_kind(id),
                Some(expected_kind),
                "RelicId::{id:?} must resolve to a RelicKind",
            );
        }
    }

    /// Structural guard (wave 3): with `RELIC_KINDS = RelicKind::ALL`
    /// and `RelicId::to_relic_kind` total, EVERY `RelicId` is seedable
    /// — `NoRuntimeKind` can only fire again if a future `RelicId`
    /// deliberately ships without a `RelicKind`.
    #[test]
    fn every_relic_id_is_seedable() {
        for &id in RelicId::all() {
            assert!(
                relic_id_to_kind(id).is_some(),
                "RelicId::{id:?} has no runtime RelicKind — seed_relics would fail",
            );
        }
    }

    #[test]
    fn seed_relics_pushes_into_state() {
        let scenario = Scenario::from_legacy_args(EncounterKind::Nibbit);
        let mut state = crate::sim::scenario::starter::build_state(
            &scenario,
            42,
            PlayerClass::Ironclad,
            None,
            None,
        );
        assert!(state.relics.is_empty());
        seed_relics(&mut state, &[RelicId::BurningBlood, RelicId::Anchor]).unwrap();
        assert_eq!(state.relics.len(), 2);
        assert_eq!(state.relics[0].kind, RelicKind::BurningBlood);
        assert_eq!(state.relics[1].kind, RelicKind::Anchor);
        // Per-instance ids are 1-based and unique.
        assert_ne!(state.relics[0].id, state.relics[1].id);
        // Counter defaults to 0 when not specified.
        assert_eq!(state.relics[0].counter, 0);
        assert_eq!(state.relics[1].counter, 0);
    }

    #[test]
    fn seed_relics_with_counters_applies_named_counter() {
        let scenario = Scenario::from_legacy_args(EncounterKind::Nibbit);
        let mut state = crate::sim::scenario::starter::build_state(
            &scenario,
            42,
            PlayerClass::Ironclad,
            None,
            None,
        );
        let mut counters = std::collections::BTreeMap::new();
        // Match by python_name (UPPER_SNAKE_CASE).
        counters.insert(RelicId::Anchor.to_python_name().to_string(), 11);
        seed_relics_with_counters(
            &mut state,
            &[RelicId::BurningBlood, RelicId::Anchor],
            &counters,
        )
        .unwrap();
        assert_eq!(state.relics[0].counter, 0); // BurningBlood not in map → 0
        assert_eq!(state.relics[1].counter, 11); // Anchor matched
    }

    #[test]
    fn seed_relics_with_counters_ignores_unknown_keys() {
        // Unknown counter keys are silently ignored — they may target
        // relics not in the relic list.
        let scenario = Scenario::from_legacy_args(EncounterKind::Nibbit);
        let mut state = crate::sim::scenario::starter::build_state(
            &scenario,
            42,
            PlayerClass::Ironclad,
            None,
            None,
        );
        let mut counters = std::collections::BTreeMap::new();
        counters.insert("NOT_A_REAL_RELIC".to_string(), 99);
        seed_relics_with_counters(&mut state, &[RelicId::BurningBlood], &counters).unwrap();
        assert_eq!(state.relics.len(), 1);
        assert_eq!(state.relics[0].counter, 0);
    }

    /// Seeding `BurningBlood` makes the relic visible to the
    /// `before_combat_start` dispatch path: after `start_combat`
    /// runs, the relic is still present in `state.relics` (no
    /// production handler removes it), so any future ownership-gated
    /// listener (`r.kind == BurningBlood`) will see it. Direct
    /// "dispatch reachable" check via the on-trigger registry: the
    /// content-side install path is idempotent and registers per-pool
    /// handlers; we exercise the seed + `start_combat` end-to-end and
    /// assert observable invariants survive.
    #[test]
    fn seeded_burning_blood_survives_start_combat() {
        sts2_content::cards::dispatch::ensure_installed();
        sts2_content::relics::dispatch::ensure_installed();

        let scenario = Scenario::from_legacy_args(EncounterKind::Nibbit);
        let mut state = crate::sim::scenario::starter::build_state(
            &scenario,
            7,
            PlayerClass::Ironclad,
            None,
            None,
        );
        seed_relics(&mut state, &[RelicId::BurningBlood]).unwrap();
        // Drive `start_combat` — this fires `dispatch_before_combat_start`.
        let _ = sts2_core::combat_loop::start_combat(&mut state);
        assert_eq!(state.relics.len(), 1);
        assert_eq!(state.relics[0].kind, RelicKind::BurningBlood);
    }

    #[test]
    fn banner_formats_pascal_display() {
        assert_eq!(format_relics_for_banner(&[]), "none");
        assert_eq!(
            format_relics_for_banner(&[RelicId::BurningBlood]),
            "Burning Blood"
        );
        assert_eq!(
            format_relics_for_banner(&[RelicId::BurningBlood, RelicId::RingOfTheSnake]),
            "Burning Blood, Ring Of The Snake"
        );
    }

    #[test]
    fn resolve_file_reads_one_per_line_skips_blanks_and_comments() {
        let dir = std::env::temp_dir();
        let path = dir.join("sts2-q3tv-relics-test.txt");
        std::fs::write(
            &path,
            "# leading comment\nBurningBlood\n\n  Anchor  \n# trailing\n",
        )
        .unwrap();
        let got = resolve_relics(&RelicInput::File(path.clone())).unwrap();
        assert_eq!(got, vec![RelicId::BurningBlood, RelicId::Anchor]);
        let _ = std::fs::remove_file(&path);
    }

    // ── sts2-rs-zhdg: NEOWS_BONES resolves via names.rs ───────────────

    #[test]
    fn resolve_inline_neows_bones_alias_resolves_to_neows_torment() {
        // Historical note: before bead `sts2-rs-wjuoi` (W9e, 2026-05-26)
        // NEOWS_BONES had no `RelicId` variant and the harness aliased
        // it to `NeowsTorment` as a lenient-skip workaround. wjuoi landed
        // the real `RelicId::NeowsBones` with its own `after_obtained`
        // (2-relic shuffle + Niche curse), so `NEOWS_BONES` now resolves
        // directly via `name_lookup` and the alias path is dead code.
        // The test still pins the CLI resolve surface for this token —
        // updated to expect the real `RelicId`.
        let input = RelicInput::Inline(vec!["NEOWS_BONES".to_string()]);
        let resolved = resolve_relics(&input).expect("NEOWS_BONES should resolve");
        assert_eq!(resolved, vec![RelicId::NeowsBones]);
    }

    // ── sts2-rs-6fc3: lenient-skip tokens drop silently in CLI path ───

    #[test]
    fn resolve_inline_hefty_tablet_skips_silently() {
        // Bead `sts2-rs-w6hdj` (W9d, 2026-05-26) landed the real
        // `RelicId::HeftyTablet` (was a lenient-skip token until then),
        // so `HEFTY_TABLET` in the input now resolves. `SILKEN_TRESS`
        // similarly gained its `RelicId` in bead
        // `sts2-rs-v0107-content-port-wb8lk.6`, so the lenient-skip cohort
        // is now just the `*_RUNE` mod-content sentinels — this test uses
        // two of them to exercise the silent-skip behaviour.
        let input = RelicInput::Inline(vec![
            "BurningBlood".to_string(),
            "HEFTY_TABLET".to_string(),
            "SINGULARITY_AI_RUNE".to_string(),
            "WATCH_OUT_GRAPEFRUIT_RUNE".to_string(),
            "Anchor".to_string(),
        ]);
        let resolved =
            resolve_relics(&input).expect("lenient-skip tokens must not fail resolution");
        assert_eq!(
            resolved,
            vec![RelicId::BurningBlood, RelicId::HeftyTablet, RelicId::Anchor]
        );
    }

    #[test]
    fn resolve_inline_only_lenient_skip_returns_empty() {
        // `HEFTY_TABLET` and `SILKEN_TRESS` resolve now (W9d /
        // `sts2-rs-v0107-content-port-wb8lk.6`); the all-skip case uses
        // two `*_RUNE` mod-content sentinels instead.
        let input = RelicInput::Inline(vec![
            "SINGULARITY_AI_RUNE".to_string(),
            "SOMETHING_RUNE".to_string(),
        ]);
        let resolved = resolve_relics(&input).expect("all-skip list must resolve to empty");
        assert!(resolved.is_empty());
    }
}
