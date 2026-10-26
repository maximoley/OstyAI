//! Epoch-gated card unlocks — the engine's stand-in for C# `UnlockState`.
//!
//! GH #367. Every character card pool overrides
//! `CardPoolModel.FilterThroughEpochs(UnlockState, cards)` to prune the cards
//! belonging to epochs the save has not revealed, and `GetUnlockedCards`
//! (`CardPoolModel.cs:83-96`) runs that filter before ANY consumer sees the
//! pool — combat generation, card rewards and shop stock alike.
//!
//! sts2-rs had no `UnlockState` concept at all. The implicit assumption was
//! "headless `UnlockState.all`", which is right for the C# oracle (it builds
//! one) and wrong for a live save mid-timeline. The visible symptom was a
//! `combat_card_generation` cursor drift of exactly **mirror = world + 1** on
//! every turn transition where CROSSBOW was owned (37/37 end_turn records on
//! the 2026-08-03 live Regent A1 corpus): `UnstableShuffle` reads `len - 1`
//! ints, so the read count *measures* the pool size, and Regent5Epoch holds
//! exactly one Attack — 29 vs 28. The untyped potion pools drifted by +3, the
//! full epoch size, confirming it independently (Rust 79 vs 76).
//!
//! The registry is process-level, which is the staging the issue calls for:
//! the save's true unlock state is inference at this point, so the live arm
//! installs what it can infer and a wire-carried version follows. The default
//! is EMPTY — nothing locked, i.e. `UnlockState.all` — so the oracle, every
//! training run and every existing test are bit-identical to before.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;

use sts2_core::cards::CardId;

/// A timeline epoch that gates three cards behind a reveal.
///
/// The full set that any `FilterThroughEpochs` override consults: three per
/// character pool (positions 2/5/7) plus five colorless. Verified against
/// `MegaCrit.Sts2.Core.Timeline.Epochs/` and each
/// `Models.CardPools/*CardPool.cs`'s `IsEpochRevealed<…>` calls in v0.107.1 —
/// a closed set on both sides, so this table is a static audit rather than
/// anything that needs sampling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Epoch {
    Ironclad2,
    Ironclad5,
    Ironclad7,
    Silent2,
    Silent5,
    Silent7,
    Defect2,
    Defect5,
    Defect7,
    Regent2,
    Regent5,
    Regent7,
    Necrobinder2,
    Necrobinder5,
    Necrobinder7,
    Colorless1,
    Colorless2,
    Colorless3,
    Colorless4,
    Colorless5,
}

impl Epoch {
    /// Every epoch, in declaration order.
    pub const ALL: [Self; 20] = [
        Self::Ironclad2,
        Self::Ironclad5,
        Self::Ironclad7,
        Self::Silent2,
        Self::Silent5,
        Self::Silent7,
        Self::Defect2,
        Self::Defect5,
        Self::Defect7,
        Self::Regent2,
        Self::Regent5,
        Self::Regent7,
        Self::Necrobinder2,
        Self::Necrobinder5,
        Self::Necrobinder7,
        Self::Colorless1,
        Self::Colorless2,
        Self::Colorless3,
        Self::Colorless4,
        Self::Colorless5,
    ];

    /// C# `<Name>Epoch.Id` — the string the save file keys reveals by.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Ironclad2 => "IRONCLAD2_EPOCH",
            Self::Ironclad5 => "IRONCLAD5_EPOCH",
            Self::Ironclad7 => "IRONCLAD7_EPOCH",
            Self::Silent2 => "SILENT2_EPOCH",
            Self::Silent5 => "SILENT5_EPOCH",
            Self::Silent7 => "SILENT7_EPOCH",
            Self::Defect2 => "DEFECT2_EPOCH",
            Self::Defect5 => "DEFECT5_EPOCH",
            Self::Defect7 => "DEFECT7_EPOCH",
            Self::Regent2 => "REGENT2_EPOCH",
            Self::Regent5 => "REGENT5_EPOCH",
            Self::Regent7 => "REGENT7_EPOCH",
            Self::Necrobinder2 => "NECROBINDER2_EPOCH",
            Self::Necrobinder5 => "NECROBINDER5_EPOCH",
            Self::Necrobinder7 => "NECROBINDER7_EPOCH",
            Self::Colorless1 => "COLORLESS1_EPOCH",
            Self::Colorless2 => "COLORLESS2_EPOCH",
            Self::Colorless3 => "COLORLESS3_EPOCH",
            Self::Colorless4 => "COLORLESS4_EPOCH",
            Self::Colorless5 => "COLORLESS5_EPOCH",
        }
    }

    /// Resolve a C# epoch id (`"REGENT5_EPOCH"`) to its variant, case-
    /// insensitively. `None` for an id this engine does not model.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|e| e.id().eq_ignore_ascii_case(id))
    }

    /// The three cards this epoch gates — C# `<Name>Epoch.Cards`.
    #[must_use]
    pub const fn cards(self) -> [CardId; 3] {
        match self {
            Self::Ironclad2 => [CardId::MoltenFist, CardId::CrueltyCard, CardId::Dominate],
            Self::Ironclad5 => [CardId::Cinder, CardId::PactsEnd, CardId::DrumOfBattleCard],
            Self::Ironclad7 => [CardId::BloodWall, CardId::TearAsunder, CardId::InfernoCard],
            Self::Silent2 => [CardId::Snakebite, CardId::BubbleBubble, CardId::Accelerant],
            Self::Silent5 => [CardId::Reflex, CardId::MasterPlanner, CardId::HandTrick],
            Self::Silent7 => [
                CardId::HiddenDaggers,
                CardId::BladeOfInk,
                CardId::PhantomBladesCard,
            ],
            Self::Defect2 => [CardId::LoopCard, CardId::NullCard, CardId::ConsumingShadow],
            Self::Defect5 => [CardId::Barrage, CardId::FlakCannon, CardId::Smokestack],
            Self::Defect7 => [CardId::Turbo, CardId::Scavenge, CardId::HelixDrill],
            Self::Regent2 => [CardId::SpoilsOfBattle, CardId::SwordSage, CardId::Furnace],
            Self::Regent5 => [CardId::Begone, CardId::Arsenal, CardId::Supermassive],
            Self::Regent7 => [CardId::Patter, CardId::HeavenlyDrill, CardId::LunarBlast],
            Self::Necrobinder2 => [CardId::Scourge, CardId::Oblivion, CardId::CountdownCard],
            Self::Necrobinder5 => [CardId::Afterlife, CardId::Sacrifice, CardId::CalcifyCard],
            Self::Necrobinder7 => [
                CardId::SculptingStrike,
                CardId::Veilpiercer,
                CardId::BansheesCry,
            ],
            Self::Colorless1 => [CardId::Automation, CardId::Entropy, CardId::Catastrophe],
            Self::Colorless2 => [CardId::EternalArmor, CardId::Jackpot, CardId::PrepTime],
            Self::Colorless3 => [CardId::Rend, CardId::BeatDown, CardId::Prowess],
            Self::Colorless4 => [CardId::Alchemize, CardId::NostalgiaCard, CardId::Scrawl],
            Self::Colorless5 => [CardId::Splash, CardId::Anointed, CardId::CalamityCard],
        }
    }
}

/// Cheap "is anything locked at all?" probe. `false` — the default — means
/// every pool takes its original memoized path with no filtering work.
static ANY_LOCKED: AtomicBool = AtomicBool::new(false);

/// Bumped on every mutation so pool caches keyed on it rebuild. Starts at 1 so
/// a cache entry can use 0 as "never populated".
static GENERATION: AtomicU64 = AtomicU64::new(1);

static LOCKED: RwLock<BTreeSet<CardId>> = RwLock::new(BTreeSet::new());

/// Install the process-level locked-card set, replacing any previous one.
///
/// Prefer [`set_locked_epochs`] where the input really is "which epochs has
/// this save revealed" — locking a partial epoch is expressible but has no C#
/// counterpart. Passing an empty slice restores `UnlockState.all`.
pub fn set_locked_cards(ids: &[CardId]) {
    let set: BTreeSet<CardId> = ids.iter().copied().collect();
    let any = !set.is_empty();
    {
        let mut guard = LOCKED.write().expect("locked-card registry poisoned");
        *guard = set;
    }
    // Order matters: publish the generation bump before the fast-path flag, so
    // a reader that sees `any_locked() == true` cannot then read a stale
    // generation and serve a cache entry built without the filter.
    GENERATION.fetch_add(1, Ordering::AcqRel);
    ANY_LOCKED.store(any, Ordering::Release);
}

/// Install the locked set as "these epochs are NOT revealed" — the direct
/// analogue of C# `FilterThroughEpochs`'s `!unlockState.IsEpochRevealed<E>()`
/// arms.
pub fn set_locked_epochs(epochs: &[Epoch]) {
    let ids: Vec<CardId> = epochs.iter().flat_map(|e| e.cards()).collect();
    set_locked_cards(&ids);
}

/// Restore the default: nothing locked, i.e. `UnlockState.all`.
pub fn clear_locked_cards() {
    set_locked_cards(&[]);
}

/// True when any card is locked. A relaxed atomic load — the intended guard in
/// front of every filtering path so the unlocked default costs nothing.
#[must_use]
pub fn any_locked() -> bool {
    ANY_LOCKED.load(Ordering::Acquire)
}

/// The current registry generation, for caches that must rebuild on change.
#[must_use]
pub fn locked_generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// True when `id` is gated behind an unrevealed epoch.
#[must_use]
pub fn is_card_locked(id: CardId) -> bool {
    if !any_locked() {
        return false;
    }
    LOCKED
        .read()
        .expect("locked-card registry poisoned")
        .contains(&id)
}

/// Every currently-locked card, sorted. Diagnostic / test surface.
#[must_use]
pub fn locked_cards() -> Vec<CardId> {
    LOCKED
        .read()
        .expect("locked-card registry poisoned")
        .iter()
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 20 epochs gate 60 distinct cards — no card appears in two epochs,
    /// which is what makes "locked" a plain set rather than a multiset.
    #[test]
    fn every_epoch_gates_three_distinct_cards_and_none_overlap() {
        let mut seen: BTreeSet<CardId> = BTreeSet::new();
        for e in Epoch::ALL {
            for c in e.cards() {
                assert!(seen.insert(c), "{c:?} is gated by more than one epoch");
            }
        }
        assert_eq!(seen.len(), 60, "20 epochs x 3 cards");
    }

    #[test]
    fn epoch_ids_round_trip() {
        for e in Epoch::ALL {
            assert_eq!(Epoch::from_id(e.id()), Some(e));
        }
        assert_eq!(Epoch::from_id("regent5_epoch"), Some(Epoch::Regent5));
        assert_eq!(Epoch::from_id("NOT_AN_EPOCH"), None);
    }

    /// The default is `UnlockState.all`: nothing locked, and the fast-path
    /// probe is false so no pool does any filtering work.
    #[test]
    fn the_default_locks_nothing() {
        assert!(!any_locked());
        assert!(!is_card_locked(CardId::Supermassive));
        assert!(locked_cards().is_empty());
    }

    #[test]
    fn locking_an_epoch_locks_exactly_its_three_cards() {
        set_locked_epochs(&[Epoch::Regent5]);
        assert!(any_locked());
        for c in Epoch::Regent5.cards() {
            assert!(is_card_locked(c), "{c:?} is behind Regent5");
        }
        assert!(
            !is_card_locked(CardId::Orbit),
            "an unrelated Regent card stays available"
        );
        assert_eq!(locked_cards().len(), 3);

        clear_locked_cards();
        assert!(!any_locked());
        assert!(!is_card_locked(CardId::Supermassive));
    }

    #[test]
    fn the_generation_advances_on_every_mutation() {
        let g0 = locked_generation();
        set_locked_epochs(&[Epoch::Regent2]);
        let g1 = locked_generation();
        assert!(g1 > g0, "install bumps the generation");
        clear_locked_cards();
        assert!(locked_generation() > g1, "clear bumps it too");
    }
}
