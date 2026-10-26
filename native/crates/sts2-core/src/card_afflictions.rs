//! Per-card afflictions (Devoured, Bound, Entangled, …).
//!
//! Mirrors C# `MegaCrit.Sts2.Core.Models.Afflictions/` (the 8
//! sub-types `Bound` / `Devoured` / `Entangled` / `Galvanized` /
//! `Hexed` / `Ringing` / `Smog` / `Weighted`) collapsed into a flat
//! `CardAfflictionKind` enum + a single per-instance state record
//! (`stacks` + the Devoured-specific `applied_exhaust` bool). One
//! affliction per card max — mirrors `CardModel.Affliction :
//! AfflictionModel?` (`CardModel.cs:515`).
//!
//! Scope + decomp: `docs/audits/2026-05-29-7n7tb-card-affliction-port-scope.md`.
//! Bead `sts2-rs-96546` (7n7tb sub-1). Sub-2 `sts2-rs-cd1zg` wires
//! Devoured's AppliedExhaust round-trip on top of this primitive;
//! sub-3 `sts2-rs-9l5c2` wires HungerPower's `AfterApplied` /
//! `AfterCardEnteredCombat` / `AfterRemoved` hooks against this
//! surface.

use crate::cards::CardType;

/// One of the eight C# affliction sub-types at pin v0.103.2.
// `PartialOrd, Ord`: the MCTS `canonicalize_actions` dedup key sorts on this
// (it is part of the `EdgeAction::PlayCard` identity tuple). Fieldless enum, so
// the derived order is declaration order — arbitrary but stable, which is all
// the sort needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CardAfflictionKind {
    /// `MegaCrit.Sts2.Core.Models.Afflictions/Bound.cs`.
    Bound,
    /// `MegaCrit.Sts2.Core.Models.Afflictions/Devoured.cs` —
    /// Doormaker's HungerPower applies this to player Attack/Skill
    /// cards. The `AppliedExhaust` flag (see
    /// [`CardAffliction::applied_exhaust`]) tracks whether the
    /// affliction-application force-added the `Exhaust` keyword so
    /// the revert path on clear can strip it.
    Devoured,
    /// `MegaCrit.Sts2.Core.Models.Afflictions/Entangled.cs`.
    Entangled,
    /// `MegaCrit.Sts2.Core.Models.Afflictions/Galvanized.cs`.
    Galvanized,
    /// `MegaCrit.Sts2.Core.Models.Afflictions/Hexed.cs`.
    Hexed,
    /// `MegaCrit.Sts2.Core.Models.Afflictions/Ringing.cs`.
    Ringing,
    /// `MegaCrit.Sts2.Core.Models.Afflictions/Smog.cs`.
    Smog,
    /// `MegaCrit.Sts2.Core.Models.Afflictions/Weighted.cs`.
    Weighted,
    /// `MegaCrit.Sts2.Core.Models.Afflictions/Tainted.cs` (v0.107.1) —
    /// `VitalSparkPower` (Infested Prism) applies this to the player's Skill
    /// cards. Stackable, **Skill-only** (`CanAfflictCardType` returns true only
    /// for `CardType.Skill`). The behavior lives in
    /// [`crate::PowerKind::Tainted`] (`TaintedPower`): playing a Tainted card
    /// stacks `TaintedPower` on the player.
    Tainted,
}

/// Per-card affliction state — the in-engine model of one
/// `AfflictionModel` instance attached to a `Card`.
///
/// Sub-type-specific state at this pin is rare: only Devoured carries
/// an `applied_exhaust` bool. Future per-affliction state can be
/// folded into this struct as additional optional fields — keeping
/// `CardAffliction` `Copy` is the design goal so it lives inline on
/// `Card` with no allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CardAffliction {
    pub kind: CardAfflictionKind,
    /// Stack count. Mirrors `AfflictionModel._amount`.
    pub stacks: i32,
    /// `Devoured.AppliedExhaust` (`Devoured.cs:9-22`) — set by
    /// [`crate::cards::Card::afflict`] when the application also
    /// installed the `Exhaust` keyword on the card, so the
    /// `clear`/`AfterRemoved` path can revert it. Unused for all
    /// non-Devoured variants. Sub-bead `sts2-rs-cd1zg` (7n7tb sub-2).
    pub applied_exhaust: bool,
}

impl CardAffliction {
    /// Mirrors C# `AfflictionModel.CanAfflictCardType(CardType)`.
    /// Devoured restricts to Attack/Skill (`Devoured.cs:26-33`); the
    /// remaining 7 sub-types accept any card type at this pin.
    #[must_use]
    pub const fn can_afflict_card_type(kind: CardAfflictionKind, ct: CardType) -> bool {
        match kind {
            CardAfflictionKind::Devoured => matches!(ct, CardType::Attack | CardType::Skill),
            // C# `Tainted.CanAfflictCardType` (`Tainted.cs:18-21`): Skill only.
            CardAfflictionKind::Tainted => matches!(ct, CardType::Skill),
            CardAfflictionKind::Bound
            | CardAfflictionKind::Entangled
            | CardAfflictionKind::Galvanized
            | CardAfflictionKind::Hexed
            | CardAfflictionKind::Ringing
            | CardAfflictionKind::Smog
            | CardAfflictionKind::Weighted => true,
        }
    }

    /// Construct a fresh affliction with no `applied_exhaust` side
    /// effect recorded. The Devoured round-trip (sub-bead
    /// [`sts2-rs-cd1zg`]) uses [`Self::devoured_with_exhaust`]
    /// instead, which records the keyword-application side effect.
    #[must_use]
    pub const fn new(kind: CardAfflictionKind, stacks: i32) -> Self {
        Self {
            kind,
            stacks,
            applied_exhaust: false,
        }
    }

    /// Construct a `Devoured` affliction recording whether the
    /// caller also force-added the `Exhaust` keyword to the card
    /// (mirrors `Devoured.AppliedExhaust = true` after the
    /// `CardCmd.ApplyKeyword` call in `HungerPower.Afflict`,
    /// `HungerPower.cs:82-87`). The clear path on the bead-cd1zg
    /// helper reverts the keyword iff this is set.
    #[must_use]
    pub const fn devoured_with_exhaust(stacks: i32, applied_exhaust: bool) -> Self {
        Self {
            kind: CardAfflictionKind::Devoured,
            stacks,
            applied_exhaust,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devoured_restricts_to_attack_or_skill() {
        assert!(CardAffliction::can_afflict_card_type(
            CardAfflictionKind::Devoured,
            CardType::Attack
        ));
        assert!(CardAffliction::can_afflict_card_type(
            CardAfflictionKind::Devoured,
            CardType::Skill
        ));
        assert!(!CardAffliction::can_afflict_card_type(
            CardAfflictionKind::Devoured,
            CardType::Power
        ));
    }

    #[test]
    fn tainted_restricts_to_skill_only() {
        // C# `Tainted.CanAfflictCardType` (`Tainted.cs:18-21`): Skill only.
        assert!(CardAffliction::can_afflict_card_type(
            CardAfflictionKind::Tainted,
            CardType::Skill
        ));
        assert!(!CardAffliction::can_afflict_card_type(
            CardAfflictionKind::Tainted,
            CardType::Attack
        ));
        assert!(!CardAffliction::can_afflict_card_type(
            CardAfflictionKind::Tainted,
            CardType::Power
        ));
    }

    #[test]
    fn non_devoured_afflicts_any_card_type() {
        for kind in [
            CardAfflictionKind::Bound,
            CardAfflictionKind::Entangled,
            CardAfflictionKind::Galvanized,
            CardAfflictionKind::Hexed,
            CardAfflictionKind::Ringing,
            CardAfflictionKind::Smog,
            CardAfflictionKind::Weighted,
        ] {
            for ct in [CardType::Attack, CardType::Skill, CardType::Power] {
                assert!(
                    CardAffliction::can_afflict_card_type(kind, ct),
                    "{kind:?} should accept {ct:?}",
                );
            }
        }
    }

    #[test]
    fn devoured_with_exhaust_records_flag() {
        let d = CardAffliction::devoured_with_exhaust(3, true);
        assert_eq!(d.kind, CardAfflictionKind::Devoured);
        assert_eq!(d.stacks, 3);
        assert!(d.applied_exhaust);

        let d2 = CardAffliction::devoured_with_exhaust(1, false);
        assert!(!d2.applied_exhaust);
    }

    #[test]
    fn new_defaults_applied_exhaust_false() {
        let a = CardAffliction::new(CardAfflictionKind::Bound, 2);
        assert!(!a.applied_exhaust);
        assert_eq!(a.stacks, 2);
    }
}
