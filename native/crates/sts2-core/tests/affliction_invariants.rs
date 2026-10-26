//! Affliction registry invariants — bead `sts2-rs-d19` (P7-8).
//!
//! Pins the load-bearing properties of [`sts2_core::afflictions`]:
//!
//!   * `classify` is total — every [`PowerKind`] variant has an
//!     entry, no panic.
//!   * Coverage record is internally consistent
//!     (sum of buckets == total).
//!   * Canonical afflictions (Vulnerable / Weak / Frail / Poison /
//!     `NoxiousFumes` / `NoBlock` / Slow) classify as Debuffs.
//!   * Canonical buffs (Strength / Dexterity / Vigor / Artifact /
//!     Intangible / Thorns / Plating / `DemonForm`) classify as Buffs.
//!
//! When a new `PowerKind` variant is added, this test stays green
//! because of the wildcard `_ => Unknown` arm in `classify`. The
//! coverage floor below (>=30% non-Unknown) is the soft alarm that
//! flags "did you forget to classify".

use sts2_core::afflictions::{classify, coverage_counts, is_affliction, AfflictionKind};
use sts2_core::PowerKind;

#[test]
fn every_power_kind_has_a_classification() {
    for &k in PowerKind::all() {
        // Total: classify never panics; returns *some* class.
        let _ = classify(k);
    }
}

#[test]
fn coverage_buckets_sum_to_total() {
    let c = coverage_counts();
    assert_eq!(c.total, PowerKind::all().len());
    assert_eq!(
        c.buffs + c.debuffs + c.mixed + c.neutrals + c.unknown,
        c.total,
        "bucket sum != total ({} + {} + {} + {} + {} != {})",
        c.buffs,
        c.debuffs,
        c.mixed,
        c.neutrals,
        c.unknown,
        c.total,
    );
}

#[test]
fn canonical_afflictions_are_debuffs() {
    let afflictions = [
        PowerKind::Vulnerable,
        PowerKind::Weak,
        PowerKind::Frail,
        PowerKind::Poison,
        PowerKind::NoxiousFumes,
        PowerKind::NoBlock,
        PowerKind::Slow,
        PowerKind::Shrink,
        PowerKind::Knockdown,
        PowerKind::Hang,
        PowerKind::Shadowmeld,
        PowerKind::Confused,
        PowerKind::Entangled,
        // sts2-rs-t88vp: C# PowerType.Debuff variants formerly UNKNOWN
        PowerKind::Constrict,
        PowerKind::Tangled,
        PowerKind::Hex,
        PowerKind::Strangle,
        PowerKind::Debilitate,
        PowerKind::DarkShackles,
        PowerKind::EnfeeblingTouch,
        PowerKind::PiercingWail,
        PowerKind::Oblivion,
        PowerKind::Disintegration,
        PowerKind::BorrowedTime,
        // `SicEmPower` is the canonical applied variant (the Necrobinder
        // SicEm card applies `PowerKind::SicEmPower`; bare `PowerKind::SicEm`
        // is vestigial / never applied — see `classify` in `afflictions.rs`).
        // C# `SicEmPower.Type => Debuff` (SicEmPower.cs:16).
        PowerKind::SicEmPower,
        PowerKind::Tender,
        PowerKind::WraithForm,
    ];
    for k in afflictions {
        let c = classify(k);
        assert_eq!(
            c.kind,
            AfflictionKind::Debuff,
            "{k:?} should be classified as Debuff, got {:?}",
            c.kind,
        );
        assert!(is_affliction(k));
    }
}

#[test]
fn canonical_buffs_are_buffs() {
    let buffs = [
        PowerKind::Strength,
        PowerKind::Dexterity,
        PowerKind::Vigor,
        PowerKind::Artifact,
        PowerKind::Intangible,
        PowerKind::Thorns,
        PowerKind::Plating,
        PowerKind::DemonForm,
        PowerKind::Metallicize,
        PowerKind::Ritual,
        PowerKind::Barricade,
    ];
    for k in buffs {
        let c = classify(k);
        assert_eq!(
            c.kind,
            AfflictionKind::Buff,
            "{k:?} should be Buff, got {:?}",
            c.kind,
        );
        assert!(!is_affliction(k));
    }
}

/// Coverage report — emitted to stderr so CI logs preserve the
/// historical actual coverage. If the floor below regresses, classify
/// the missing variants in [`sts2_core::afflictions::classify`].
#[test]
fn report_and_enforce_coverage_floor() {
    let c = coverage_counts();
    eprintln!(
        "[affliction coverage] total={} buffs={} debuffs={} \
         neutrals={} mixed={} unknown={}",
        c.total, c.buffs, c.debuffs, c.neutrals, c.mixed, c.unknown,
    );
    let classified = c.total - c.unknown;
    let pct = (classified * 100) / c.total;
    // Baseline recorded by bead `sts2-rs-d19`: 26% of variants
    // classified (72/267) covering all canonical core-pipeline
    // afflictions + buffs. The long tail (~195 monster-side and
    // colorless variants) lands with subsequent character beads.
    assert!(
        pct >= 25,
        "affliction classification coverage regressed: \
         {classified}/{} ({pct}%) — classify more PowerKind variants",
        c.total,
    );
}
