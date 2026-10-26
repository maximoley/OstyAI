//! Every C# `Flash()` site is ported to Rust or listed as unported BY NAME.
//!
//! Spec: `docs/superpowers/specs/2026-08-29-workbench-vfx-creature-events-design.md`
//! §1.5.
//!
//! C# `PowerModel.Flash()` reaches `NCreature.OnPowerFlashed`
//! (`NCreature.cs:925`), which spawns `power_flash_vfx` on the power's owner.
//! There are 90 such calls across 89 power classes, and the engine must
//! record `EventLogEntry::PowerFlashed` at each.
//!
//! This gate exists because the failure mode is SILENCE: a power that should
//! flash and does not looks exactly like a power that correctly did nothing.
//! The same reasoning produced `vfx_binding_coverage` in sub-project 2, and
//! that gate is what caught a missing `VineShambler` site.
//!
//! The table is regenerated with `just extract-power-flash-sites` (sts2-wiki),
//! which PRESERVES the `ported`/`rust` fields of sites that still exist and
//! DROPS rows whose C# site disappeared — so a vanished site moves the count
//! here rather than passing quietly.

use std::collections::HashSet;
use std::path::PathBuf;

use sts2_core::PowerKind;

/// How many sites have no Rust counterpart yet.
///
/// A LEDGER, not a target: shrinking it is the work of the porting tasks, and
/// every decrement should arrive with the Rust site that justifies it. It is
/// asserted exactly (not as an upper bound) so that a site silently
/// disappearing from the C# — or a re-extraction quietly un-porting one —
/// fails here instead of reducing coverage unnoticed.
///
/// 10 -> 7 in GH #635, which landed the three `AfterModifyingDamageAmount`
/// arms (`HardToKillPower:23`, `IntangiblePower:61`, `SlowPower:48`) together —
/// they share one selection rule, so they were never separable.
///
/// 7 -> 6 in GH #634, which taught `hooks::block_clear_preventer` to report
/// WHICH listener vetoed the block clear, so `BlurPower:35` could fire on the
/// preventer alone rather than on any retained block.
///
/// 6 -> 5 in GH #636, which ported `HelloWorldPower.BeforeHandDraw`'s
/// Common-rarity card generation so the flash at `HelloWorldPower:24` had a
/// real effect to ride.
///
/// 5 -> 4 in GH #637, which ported `ReboundPower`'s Discard -> draw-top
/// redirect, its stack decrement and its turn-end removal, so the flash at
/// `ReboundPower:36` had one.
///
/// 4 -> 3 in GH #639, which ported `TrashToTreasurePower`'s per-stack random
/// orb channel on a player-generated Status, so the flash at
/// `TrashToTreasurePower:20` had one. Of the three left, two
/// (`GravityPower:58`, `MagicBombPower:25`) are dead classes and wontfix
/// (#640), so the reachable denominator is 89, not 91.
const EXPECTED_UNPORTED: usize = 3;

#[derive(Debug)]
struct Site {
    power: String,
    cs_class: String,
    cs_line: u64,
    ported: bool,
    rust: Option<String>,
    reason: Option<String>,
}

fn table_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../sts2-wiki/data/v0.107.1/power_flash_sites.json")
}

/// The table is extracted from the game (`just extract-power-flash-sites`)
/// and not published; every test below skips when it is absent.
macro_rules! skip_without_table {
    () => {
        if !table_path().is_file() {
            eprintln!("skipping: {} not present", table_path().display());
            return;
        }
    };
}

fn load_sites() -> Vec<Site> {
    let path = table_path();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read {}: {e} — regenerate it with `just extract-power-flash-sites` (sts2-wiki)",
            path.display()
        )
    });
    let json: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));

    json["sites"]
        .as_array()
        .expect("`sites` must be an array")
        .iter()
        .map(|s| Site {
            power: s["power"]
                .as_str()
                .expect("power must be a string")
                .to_string(),
            cs_class: s["cs_class"]
                .as_str()
                .expect("cs_class must be a string")
                .to_string(),
            cs_line: s["cs_line"].as_u64().expect("cs_line must be a number"),
            ported: s["ported"].as_bool().expect("ported must be a bool"),
            rust: s["rust"].as_str().map(str::to_string),
            reason: s["reason"].as_str().map(str::to_string),
        })
        .collect()
}

#[test]
fn the_table_carries_every_known_flash_site() {
    skip_without_table!();
    let sites = load_sites();
    // Measured 2026-08-29 against `decompiled/v0.107.1`: 90 calls, 89 classes.
    assert_eq!(
        sites.len(),
        90,
        "expected 90 flash sites, found {}",
        sites.len()
    );
    let classes: HashSet<&str> = sites.iter().map(|s| s.cs_class.as_str()).collect();
    assert_eq!(classes.len(), 89);
}

#[test]
fn every_power_named_by_the_table_is_a_real_power_kind() {
    skip_without_table!();
    // The join between this table and the engine is `PowerKind::python_name()`.
    // A name that matches nothing would make its site permanently unportable
    // while looking like ordinary pending work.
    let known: HashSet<&str> = PowerKind::all().iter().map(|k| k.python_name()).collect();
    let unknown: Vec<String> = load_sites()
        .into_iter()
        .filter(|s| !known.contains(s.power.as_str()))
        .map(|s| format!("{} ({})", s.power, s.cs_class))
        .collect();
    assert!(
        unknown.is_empty(),
        "power ids matching no PowerKind: {unknown:?}"
    );
}

#[test]
fn every_ported_site_names_where_it_lives() {
    skip_without_table!();
    // "Ported" with no Rust location is unfalsifiable — nobody can check it,
    // and the count would still go down.
    let bad: Vec<String> = load_sites()
        .into_iter()
        .filter(|s| s.ported && s.rust.is_none())
        .map(|s| format!("{}:{}", s.cs_class, s.cs_line))
        .collect();
    assert!(
        bad.is_empty(),
        "marked ported but naming no Rust site: {bad:?}"
    );
}

#[test]
fn an_unported_site_is_never_also_marked_ported() {
    skip_without_table!();
    // `reason` explains why a site COULD NOT be ported. Carrying one on a
    // ported row means the row is telling two stories.
    let bad: Vec<String> = load_sites()
        .into_iter()
        .filter(|s| s.ported && s.reason.is_some())
        .map(|s| format!("{}:{}", s.cs_class, s.cs_line))
        .collect();
    assert!(
        bad.is_empty(),
        "ported sites carrying an unported reason: {bad:?}"
    );
}

#[test]
fn every_unported_site_says_why() {
    skip_without_table!();
    // The count alone is not enough. A site can sit unported for a REASON (the
    // C# hook has no Rust counterpart, the power belongs to another task's
    // slice) or because somebody forgot it existed, and those look identical
    // from `EXPECTED_UNPORTED`. That happened: `StranglePower:54` fell inside
    // Task 3's slice, was neither ported nor explained, and every gate above
    // passed — the row was silently indistinguishable from deliberate deferral.
    // Requiring a reason makes the omission say so out loud.
    let bad: Vec<String> = load_sites()
        .into_iter()
        .filter(|s| !s.ported)
        .filter(|s| s.reason.as_ref().is_none_or(|r| r.trim().is_empty()))
        .map(|s| format!("{}:{}", s.cs_class, s.cs_line))
        .collect();
    assert!(
        bad.is_empty(),
        "unported flash sites carrying no reason: {bad:?} — port the site, or \
         record WHY it could not be ported (a missing Rust counterpart, or the \
         slice it belongs to)"
    );
}

#[test]
fn unported_sites_are_listed_by_name() {
    skip_without_table!();
    let unported: Vec<String> = load_sites()
        .into_iter()
        .filter(|s| !s.ported)
        .map(|s| format!("{}:{}", s.cs_class, s.cs_line))
        .collect();
    assert_eq!(
        unported.len(),
        EXPECTED_UNPORTED,
        "unported flash sites ({}): {unported:?}",
        unported.len()
    );
}
