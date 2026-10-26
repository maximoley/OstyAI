//! Static content definitions: cards, powers, monsters, relics, potions, orbs.
//!
//! Each module ports the matching subdir of `sts2-rl-agent/sts2_env/`.
//!
//! ## Status
//!
//! - `cards` (bead P4-1 / sts2-rs-zcl): Ironclad starter cards (Strike,
//!   Defend, Bash) + `AscendersBane` curse. `CardSpec` is the static
//!   template type; per-combat instances live in
//!   `sts2_core::cards::Card`. Phase 6 adds the rest of the card pool
//!   plus the play interpreter.
//! - `relics` (bead P4-2 / sts2-rs-u4u): Ironclad starter relic
//!   (`BurningBlood`). `RelicSpec` is the static template type;
//!   per-run instances live in `sts2_core::Relic`. Phase 7 adds the
//!   rest of the relic pool plus the after-combat-victory /
//!   after-rest / on-card-played interpreter that fires
//!   `RelicEffect`s.

/// Content hash of THIS crate's source, stamped at build time by `build.rs`
/// (issue #15, bead `sts2-rs-cg5ci`). Compiled INTO the crate so a stale
/// `sts2-content` rlib carries its old hash. See `sts2_core::SRC_HASH`.
pub const SRC_HASH: &str = env!("STS2_CONTENT_SRC_HASH");

pub mod ascension_scaling;
pub mod cards;
pub mod enchantments;
pub mod encounters;
pub mod events;
pub mod monster_dispatch;
pub mod monsters;
pub mod names;
pub mod potions;
pub mod relics;
pub mod run_bridge;
pub mod run_callbacks;
pub mod shop;
pub mod starters;
pub mod unlocks;
