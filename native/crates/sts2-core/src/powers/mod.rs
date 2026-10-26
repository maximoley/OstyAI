// HAND-MAINTAINED — edit directly. Add a `pub mod` line for a new character.
//! Power discriminant + per-character listener bodies.
//!
//! `PowerKind` lives in `../power_ids_generated.rs` and is hand-maintained
//! against the v0.107.1 C# decompilation — 272 variants as of the assertion
//! at the bottom of this file. Bead sts2-rs-pmf (P6-1b) originally generated
//! *this* aggregator from `powers.toml` via a `scripts/regen-powers-mod.py`
//! that no longer exists; it was retired because a rerun re-added the
//! deliberately deleted `necrobinder` module and rewrote the variant-count
//! assertion from 272 back to the Python pin's 267, discarding the
//! provenance comments that justify every bump. `powers.toml` survives as a
//! provenance table (it lists each character's exclusive `PowerKind`s);
//! nothing reads it. Listener bodies (`on_card_played`,
//! `modify_damage_additive`, ...) live in per-character submodules below;
//! each downstream P6 pool bead (P6-3..P6-7) edits its own character file
//! without touching this aggregator.
//!
//! ## Variant history
//!
//! * Bead sts2-rs-5if (VTHIN-2): `Vulnerable`.
//! * Bead sts2-rs-6w8 (P2-2): the remaining "core" pipeline powers
//!   (Weak, Frail, Strength, Dexterity, Artifact, Intangible, Thorns,
//!   Poison, `FlameBarrier`).
//! * Bead sts2-rs-9v1 (P5-2): 22 monster-side variants.
//! * Bead sts2-rs-8c0 (P6-2): full enum auto-generated from the Python pin.
//! * Bead sts2-rs-pmf (P6-1b): per-character listener submodules + manifest.
//!
//! ## Aliases
//!
//! Python's `PowerId` enum has zero `ENEMY_`-prefixed members; owner side
//! is encoded by which creature owns the `Power` instance, not by a
//! discriminant variant. Block lives on `Creature.block` (no `BLOCK`
//! variant). See the historical notes in the commit log for `8c0`/`pmf`.

#[path = "../power_ids_generated.rs"]
mod power_ids_generated;
pub use power_ids_generated::PowerKind;

// P6-3 (sts2-rs-le3)
pub mod ironclad;
// P6-7 (sts2-rs-ltq)
pub mod colorless;
// sts2-rs-jxz (umbrella; jxz.1 stars-resource first slice)
pub mod regent;
// necrobinder powers (sts2-rs-bt9) removed for a from-scratch C#-first
// re-port (was Python-pin-derived). Re-add `pub mod necrobinder;` when ported.
//
// silent (P6-4 `sts2-rs-l2c`) and defect (P6-5 `sts2-rs-l7s`) removed in
// GH #666. Both were scaffolds whose helpers the dispatcher was supposed to
// key on and never did: every function in them had zero callers outside its
// own file, and the only outside mentions were COMMENTS — several of them
// retractions saying the stub described the wrong mechanic. They were also
// a second, wrong source of truth (`blur_preserves_block` returned `false`
// while Blur was implemented and flashing), and they descended from
// `sts2-rl-agent@01c094d:sts2_env/powers/*.py`, which this repo no longer
// treats as authoritative. The listeners themselves live where they always
// did — the per-`PowerKind` match arms in `hooks.rs`.
//
// `ironclad` and `colorless` have no dead helpers. `regent` is the same
// vintage but is NOT a scaffold: 15 of its functions are called from
// `hooks.rs`, `lib.rs` and `state_dict.rs`. Its ~21 unreferenced helpers are
// left for a decision of their own.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_names_match_python_core() {
        assert_eq!(PowerKind::Strength.python_name(), "STRENGTH");
        assert_eq!(PowerKind::Vulnerable.python_name(), "VULNERABLE");
        assert_eq!(PowerKind::Weak.python_name(), "WEAK");
        assert_eq!(PowerKind::Dexterity.python_name(), "DEXTERITY");
        assert_eq!(PowerKind::Frail.python_name(), "FRAIL");
        assert_eq!(PowerKind::Intangible.python_name(), "INTANGIBLE");
        assert_eq!(PowerKind::Thorns.python_name(), "THORNS");
        assert_eq!(PowerKind::FlameBarrier.python_name(), "FLAME_BARRIER");
        assert_eq!(PowerKind::Poison.python_name(), "POISON");
        assert_eq!(PowerKind::Artifact.python_name(), "ARTIFACT");
        assert_eq!(PowerKind::Minion.python_name(), "MINION");
        assert_eq!(PowerKind::Ritual.python_name(), "RITUAL");
        assert_eq!(
            PowerKind::BattlewornDummyTimeLimit.python_name(),
            "BATTLEWORN_DUMMY_TIME_LIMIT",
        );
        assert_eq!(PowerKind::BackAttackLeft.python_name(), "BACK_ATTACK_LEFT");
    }

    #[test]
    fn every_variant_has_python_name() {
        for &k in PowerKind::all() {
            let name = k.python_name();
            assert!(!name.is_empty(), "empty Python name for {k:?}");
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
                "non upper-snake-case Python name for {k:?}: {name}",
            );
        }
    }

    #[test]
    fn all_has_expected_variant_count() {
        // Bumped 268 → 271 (bead `sts2-rs-44897`) after sibling power
        // additions landed (predecessor bump bf818b93 went 265 → 268).
        // Bumped 271 → 273 for the hand-appended C#-sourced `BorrowedTime`
        // variant (Necrobinder Borrowed Time power; no Python counterpart).
        // Bumped 273 → 274 for the hand-appended C#-sourced `SicEmPower` and
        // `DevourLifePower` Osty-summon powers (Necrobinder Sic Em / Devour
        // Life; no Python counterparts).
        // Bumped 274 → 275 for the hand-appended C#-sourced `Tainted` power
        // (v0.107.1 Tainted keyword applied by VitalSpark; no Python
        // counterpart; bead `…wb8lk.4`).
        // Bumped 275 → 276 for the hand-appended C#-sourced `WitheringPresence`
        // power (v0.107.1 Aeonglass Act-3 boss; no Python counterpart; bead
        // `…wb8lk.2`).
        // Bumped 276 → 273: removed `Hunger` / `Scrutiny` / `Grasp` (Doormaker
        // phase-marker powers, deleted in v0.107.1 with the Doormaker boss;
        // bead `…wb8lk.2`).
        // Bumped 273 → 273: removed `DrumOfBattle` (v0.107.1 deleted
        // `DrumOfBattlePower`; DrumOfBattle is now a 1-cost Skill with an
        // `AfterCardExhausted` energy gain; bead `…wb8lk.13`).
        // This assertion is the canonical low-cost guard that anyone
        // adding/removing a PowerKind variant must update the count.
        // Beta adds Ambergris and HyperbeamFocusDown.
        assert_eq!(PowerKind::all().len(), 274);
    }
}
