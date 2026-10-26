//! `scenario` — declarative description of a combat to be assembled
//! into a `CombatState` by [`starter::build_state`].
//!
//! Foundation laid by cliext bead `sts2-rs-r1q8` (B1: `MonsterSpec` +
//! generalized AI dispatch). Replaces the single-monster fast path
//! that previously lived inline in `sim::build_starter_state`.

// `EncounterKind` is deprecated as of cliext Phase 4
// (`sts2-rs-90n5`); the in-tree `from_legacy_args` shim and the
// existing test fixtures intentionally keep using it. Allow at module
// scope so the deprecation fires only for NEW external callers.
#![allow(deprecated)]

pub mod deck;
pub mod encounter;
pub mod export;
pub mod potions;
pub mod random;
pub mod relics;
pub mod starter;
pub mod toml_file;

// `monster_ai` / `MonsterAiArgs` are re-exported for downstream
// cliext beads (C2/C3 add CLI flags that populate these). The
// `#[allow]` keeps the binary build clean until those callers land.
// Phase 1 (`sts2-rs-tgr4`) also re-exports `MonsterSpawnArgs` +
// `SpawnPosition` so Phase 2 wiring beads (h37x/mv7c/ckj8) can build
// `MonsterSpec`s directly from CLI parse output without reaching into
// `sts2-content`.
#[allow(unused_imports)]
pub use encounter::{
    resolve_encounter, summarize_monsters, EncounterError, MonsterAiArgs, MonsterSpec, ENCOUNTERS,
};
#[allow(unused_imports)]
pub use sts2_content::monster_dispatch::{MonsterSpawnArgs, SpawnPosition};

use crate::sim::EncounterKind;

/// Top-level encounter description. v1 is monster-list only; later
/// cliext beads will extend this with relics, potions, deck mutators,
/// etc.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub monsters: Vec<MonsterSpec>,
}

impl Scenario {
    /// Build a single-monster `Scenario` from the legacy
    /// `(EncounterKind,)` CLI args. `is_alone = true`, `is_front =
    /// true`, `offset_phase = false` — i.e. the same one-shot setup
    /// the pre-r1q8 `build_starter_state` produced.
    pub fn from_legacy_args(encounter: EncounterKind) -> Self {
        Self {
            monsters: vec![MonsterSpec::new(encounter.monster_kind())],
        }
    }

    /// Convenience for tests / multi-monster callers: build a
    /// Scenario from an arbitrary `Vec<MonsterSpec>`, fixing up
    /// `is_alone` / `is_front` for each spec based on its position in
    /// the group. `offset_phase` and `hp_override` are preserved.
    ///
    /// `is_alone` is set to `monsters.len() == 1` for every entry;
    /// `is_front` is set to `idx == 0`.
    #[must_use]
    pub fn from_monsters(mut monsters: Vec<MonsterSpec>) -> Self {
        let alone = monsters.len() == 1;
        for (idx, m) in monsters.iter_mut().enumerate() {
            let is_front = idx == 0;
            m.ai_args.is_alone = alone;
            m.ai_args.is_front = is_front;
            // Phase 1 (`sts2-rs-tgr4`): migrate Nibbit's positional
            // args from the implicit `ai_args` fix-up to the
            // structured `MonsterSpawnArgs::Nibbit` form so Phase-2
            // / Phase-4 callers reading `args` directly see consistent
            // values. Caller-supplied non-None args are preserved.
            if matches!(m.args, MonsterSpawnArgs::None)
                && matches!(m.kind, sts2_core::MonsterKind::Nibbit)
            {
                m.args = MonsterSpawnArgs::Nibbit {
                    is_alone: alone,
                    is_front,
                };
            }
        }
        Self { monsters }
    }
}
