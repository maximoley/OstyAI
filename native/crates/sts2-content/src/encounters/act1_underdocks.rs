//! Act 1 ALTERNATE (Underdocks) encounter registry — GH #544.
//!
//! The second act model at `Index => 0`. Transcribed from
//! `Underdocks.GenerateAllEncounters()` (v0.107.1) and each encounter class's
//! `GenerateMonsters()` override — **not** from the Python pin, which never
//! modelled this act.
//!
//! - 4 weak, 10 normal, 3 elite, 3 boss (Overgrowth is 4 / 12 / 3 / 3).
//! - Two encounters draw at spawn time off the per-encounter `Rng`
//!   (`CORPSE_SLUGS_*` and `TWO_TAILED_RATS_NORMAL` both roll a starter-move
//!   rotation); they expose a [`EncounterRandomizer`]. Their static
//!   `monsters` slice is the rotation-0 permutation, kept as the no-RNG
//!   fallback exactly as the other act tables do.
//! - Three encounters spawn FEWER monsters than they end up fighting:
//!   `GREMLIN_MERC_NORMAL` (the Fat/Sneaky gremlins arrive via
//!   `SurprisePower`), `LIVING_FOG_NORMAL` (gas bombs are summoned) and
//!   `TWO_TAILED_RATS_NORMAL` (`CALL_FOR_BACKUP`). The table lists only what
//!   `GenerateMonsters` returns, which is what combat starts with.
//!
//! The run-level draw order for this act lives in
//! [`sts2_core::run_room_gen`]'s `ACT1_UNDERDOCKS`; this table is the
//! composition side, keyed by the same `UPPER_SNAKE` encounter ids.

use crate::encounters::{EncounterDef, EncounterRoster, EncounterTier};
use crate::monster_dispatch::MonsterSpawnArgs;
use crate::monsters::act1_underdocks::TwoTailedRatSlot;
use sts2_core::MonsterKind;

/// Every Underdocks encounter, in the canonical weak → normal → elite →
/// boss order the other act tables use.
pub static ACT1_UNDERDOCKS_ENCOUNTERS: &[EncounterDef] = &[
    // ── Weak ──────────────────────────────────────────────────────
    // C# `CorpseSlugsWeak.GenerateMonsters` builds two CorpseSlugs and then
    // calls `CorpseSlug.EnsureCorpseSlugsStartWithDifferentMoves(list, Rng)`,
    // which draws ONE `NextInt(3)` and walks the slugs from it. See
    // `randomize_corpse_slugs_weak`.
    EncounterDef {
        id: "CORPSE_SLUGS_WEAK",
        display: "Two Corpse Slugs",
        monsters: &[
            (
                MonsterKind::CorpseSlug,
                MonsterSpawnArgs::CorpseSlug { starter_idx: 0 },
            ),
            (
                MonsterKind::CorpseSlug,
                MonsterSpawnArgs::CorpseSlug { starter_idx: 1 },
            ),
        ],
        tier: EncounterTier::Weak,
        randomize: Some(randomize_corpse_slugs_weak),
    },
    EncounterDef {
        id: "SEAPUNK_WEAK",
        display: "Seapunk",
        monsters: &[(MonsterKind::Seapunk, MonsterSpawnArgs::None)],
        tier: EncounterTier::Weak,
        randomize: None,
    },
    EncounterDef {
        id: "SLUDGE_SPINNER_WEAK",
        display: "Sludge Spinner",
        monsters: &[(MonsterKind::SludgeSpinner, MonsterSpawnArgs::None)],
        tier: EncounterTier::Weak,
        randomize: None,
    },
    // C# `ToadpolesWeak.GenerateMonsters` sets `IsFront = true` on the first
    // toadpole and `false` on the second — no RNG. `MonsterKind::Toadpole`'s
    // dispatch arm derives the slot from the spawn index, so the positional
    // fallback already produces First/Second here.
    EncounterDef {
        id: "TOADPOLES_WEAK",
        display: "Two Toadpoles",
        monsters: &[
            (MonsterKind::Toadpole, MonsterSpawnArgs::None),
            (MonsterKind::Toadpole, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Weak,
        randomize: None,
    },
    // ── Normal ────────────────────────────────────────────────────
    EncounterDef {
        id: "CORPSE_SLUGS_NORMAL",
        display: "Three Corpse Slugs",
        monsters: &[
            (
                MonsterKind::CorpseSlug,
                MonsterSpawnArgs::CorpseSlug { starter_idx: 0 },
            ),
            (
                MonsterKind::CorpseSlug,
                MonsterSpawnArgs::CorpseSlug { starter_idx: 1 },
            ),
            (
                MonsterKind::CorpseSlug,
                MonsterSpawnArgs::CorpseSlug { starter_idx: 2 },
            ),
        ],
        tier: EncounterTier::Normal,
        randomize: Some(randomize_corpse_slugs_normal),
    },
    EncounterDef {
        id: "CULTISTS_NORMAL",
        display: "Calcified + Damp Cultist",
        monsters: &[
            (MonsterKind::CalcifiedCultist, MonsterSpawnArgs::None),
            (MonsterKind::DampCultist, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "FOSSIL_STALKER_NORMAL",
        display: "Fossil Stalker",
        monsters: &[(MonsterKind::FossilStalker, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    // Only the merc spawns. `SurprisePower` adds the Fat and Sneaky gremlins
    // mid-combat (`surprise_after_death_spawn`), so listing them here would
    // start the fight with monsters C# has not created yet.
    EncounterDef {
        id: "GREMLIN_MERC_NORMAL",
        display: "Gremlin Merc",
        monsters: &[(MonsterKind::GremlinMerc, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "HAUNTED_SHIP_NORMAL",
        display: "Haunted Ship",
        monsters: &[(MonsterKind::HauntedShip, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    // `LivingFogNormal` declares six slots (five bombs + the fog) but
    // `GenerateMonsters` returns only the fog; the `GasBomb`s are summoned.
    EncounterDef {
        id: "LIVING_FOG_NORMAL",
        display: "Living Fog",
        monsters: &[(MonsterKind::LivingFog, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    // `PunchConstruct.StartsWithFastPunch` defaults to false and this
    // encounter does not set it (only the `PunchOff` event does), so the
    // construct opens on READY.
    EncounterDef {
        id: "PUNCH_CONSTRUCT_NORMAL",
        display: "Punch Construct",
        monsters: &[(
            MonsterKind::PunchConstruct,
            MonsterSpawnArgs::PunchConstruct {
                starts_with_fast_punch: false,
            },
        )],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    // Order matters: C# returns `[CalcifiedCultist, Seapunk]` even though
    // `AllPossibleMonsters` lists the Seapunk first.
    EncounterDef {
        id: "SEAPUNK_NORMAL",
        display: "Calcified Cultist + Seapunk",
        monsters: &[
            (MonsterKind::CalcifiedCultist, MonsterSpawnArgs::None),
            (MonsterKind::Seapunk, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "SEWER_CLAM_NORMAL",
        display: "Sewer Clam",
        monsters: &[(MonsterKind::SewerClam, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    // C# rolls ONE `Rng.NextInt(3)` and hands the three rats
    // `n`, `n+1`, `n+2` (mod 3) as `StarterMoveIndex`. See
    // `randomize_two_tailed_rats_normal`.
    EncounterDef {
        id: "TWO_TAILED_RATS_NORMAL",
        display: "Three Two-Tailed Rats",
        monsters: &[
            (
                MonsterKind::TwoTailedRat,
                MonsterSpawnArgs::TwoTailedRat {
                    slot: TwoTailedRatSlot::First,
                },
            ),
            (
                MonsterKind::TwoTailedRat,
                MonsterSpawnArgs::TwoTailedRat {
                    slot: TwoTailedRatSlot::Second,
                },
            ),
            (
                MonsterKind::TwoTailedRat,
                MonsterSpawnArgs::TwoTailedRat {
                    slot: TwoTailedRatSlot::Third,
                },
            ),
        ],
        tier: EncounterTier::Normal,
        randomize: Some(randomize_two_tailed_rats_normal),
    },
    // ── Elite ─────────────────────────────────────────────────────
    EncounterDef {
        id: "PHANTASMAL_GARDENERS_ELITE",
        display: "Four Phantasmal Gardeners",
        monsters: &[
            (MonsterKind::PhantasmalGardener, MonsterSpawnArgs::None),
            (MonsterKind::PhantasmalGardener, MonsterSpawnArgs::None),
            (MonsterKind::PhantasmalGardener, MonsterSpawnArgs::None),
            (MonsterKind::PhantasmalGardener, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Elite,
        randomize: None,
    },
    EncounterDef {
        id: "SKULKING_COLONY_ELITE",
        display: "Skulking Colony",
        monsters: &[(MonsterKind::SkulkingColony, MonsterSpawnArgs::None)],
        tier: EncounterTier::Elite,
        randomize: None,
    },
    EncounterDef {
        id: "TERROR_EEL_ELITE",
        display: "Terror Eel",
        monsters: &[(MonsterKind::TerrorEel, MonsterSpawnArgs::None)],
        tier: EncounterTier::Elite,
        randomize: None,
    },
    // ── Boss ──────────────────────────────────────────────────────
    EncounterDef {
        id: "LAGAVULIN_MATRIARCH_BOSS",
        display: "Lagavulin Matriarch",
        monsters: &[(MonsterKind::LagavulinMatriarch, MonsterSpawnArgs::None)],
        tier: EncounterTier::Boss,
        randomize: None,
    },
    EncounterDef {
        id: "SOUL_FYSH_BOSS",
        display: "Soul Fysh",
        monsters: &[(MonsterKind::SoulFysh, MonsterSpawnArgs::None)],
        tier: EncounterTier::Boss,
        randomize: None,
    },
    EncounterDef {
        id: "WATERFALL_GIANT_BOSS",
        display: "Waterfall Giant",
        monsters: &[(MonsterKind::WaterfallGiant, MonsterSpawnArgs::None)],
        tier: EncounterTier::Boss,
        randomize: None,
    },
];

/// Lookup helper: resolve an Underdocks encounter id (`UPPER_SNAKE`).
/// Case-insensitive, matching the sibling act tables.
#[must_use]
pub fn find(id: &str) -> Option<&'static EncounterDef> {
    ACT1_UNDERDOCKS_ENCOUNTERS
        .iter()
        .find(|e| e.id.eq_ignore_ascii_case(id))
}

// ---------------------------------------------------------------------------
// Spawn-time randomizers
// ---------------------------------------------------------------------------

/// `CorpseSlug.EnsureCorpseSlugsStartWithDifferentMoves` (v0.107.1
/// `CorpseSlug.cs:137-146`):
///
/// [decompiled C# omitted — see the cited path]
///
/// One read, then consecutive indices from it. `StarterMoveIdx % 3` picks
/// WHIP_SLAP / GLOMP / GOOP, and the three moves chain in a fixed cycle, so
/// the offset genuinely changes what the player faces on turn 1 — unlike the
/// Two-Tailed Rats, these slugs are NOT interchangeable across the rotation
/// once the player starts killing them in order.
fn corpse_slug_roster(rng: &mut sts2_core::dotnet_rng::Rng, count: u8) -> EncounterRoster {
    let base = rng.next_int_exclusive(0, 3);
    let mut out = EncounterRoster::new();
    for i in 0..count {
        // C# increments a plain `int` and takes `% 3` per slug.
        let starter_idx = ((base + i32::from(i)) % 3) as u8;
        out.push((
            MonsterKind::CorpseSlug,
            MonsterSpawnArgs::CorpseSlug { starter_idx },
        ));
    }
    out
}

/// `CORPSE_SLUGS_WEAK` — two slugs, one `NextInt(3)`.
pub fn randomize_corpse_slugs_weak(rng: &mut sts2_core::dotnet_rng::Rng) -> EncounterRoster {
    corpse_slug_roster(rng, 2)
}

/// `CORPSE_SLUGS_NORMAL` — three slugs, one `NextInt(3)`.
pub fn randomize_corpse_slugs_normal(rng: &mut sts2_core::dotnet_rng::Rng) -> EncounterRoster {
    corpse_slug_roster(rng, 3)
}

/// `TwoTailedRatsNormal.GenerateMonsters` (v0.107.1):
///
/// [decompiled C# omitted — see the cited path]
///
/// One read. The three rats are otherwise identical, so the rotation offset
/// permutes indistinguishable actors and does not change the fight — but the
/// READ is not optional: skipping it would leave the per-encounter stream one
/// cursor short of C# for everything drawn after it.
pub fn randomize_two_tailed_rats_normal(rng: &mut sts2_core::dotnet_rng::Rng) -> EncounterRoster {
    let base = rng.next_int_exclusive(0, 3);
    let mut out = EncounterRoster::new();
    for i in 0..3 {
        let slot = match (base + i) % 3 {
            0 => TwoTailedRatSlot::First,
            1 => TwoTailedRatSlot::Second,
            _ => TwoTailedRatSlot::Third,
        };
        out.push((
            MonsterKind::TwoTailedRat,
            MonsterSpawnArgs::TwoTailedRat { slot },
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table must carry exactly the 20 ids
    /// `Underdocks.GenerateAllEncounters` lists, with the tiers its
    /// `RoomType` / `IsWeak` overrides imply. The run-level generator picks
    /// ids from its own copy of that list, so a mismatch here surfaces as
    /// "unknown encounter id" mid-run rather than at startup.
    #[test]
    fn table_matches_the_csharp_generate_all_encounters_list() {
        let mut ids: Vec<&str> = ACT1_UNDERDOCKS_ENCOUNTERS.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            [
                "CORPSE_SLUGS_NORMAL",
                "CORPSE_SLUGS_WEAK",
                "CULTISTS_NORMAL",
                "FOSSIL_STALKER_NORMAL",
                "GREMLIN_MERC_NORMAL",
                "HAUNTED_SHIP_NORMAL",
                "LAGAVULIN_MATRIARCH_BOSS",
                "LIVING_FOG_NORMAL",
                "PHANTASMAL_GARDENERS_ELITE",
                "PUNCH_CONSTRUCT_NORMAL",
                "SEAPUNK_NORMAL",
                "SEAPUNK_WEAK",
                "SEWER_CLAM_NORMAL",
                "SKULKING_COLONY_ELITE",
                "SLUDGE_SPINNER_WEAK",
                "SOUL_FYSH_BOSS",
                "TERROR_EEL_ELITE",
                "TOADPOLES_WEAK",
                "TWO_TAILED_RATS_NORMAL",
                "WATERFALL_GIANT_BOSS",
            ]
        );
        let count = |t: EncounterTier| {
            ACT1_UNDERDOCKS_ENCOUNTERS
                .iter()
                .filter(|e| e.tier == t)
                .count()
        };
        assert_eq!(count(EncounterTier::Weak), 4);
        assert_eq!(count(EncounterTier::Normal), 10);
        assert_eq!(count(EncounterTier::Elite), 3);
        assert_eq!(count(EncounterTier::Boss), 3);
    }

    /// Every id in this table must agree with the run-level generator's
    /// `ACT1_UNDERDOCKS` rows on tier. The generator picks WHICH encounter
    /// each room is; this table says WHAT spawns. If one calls
    /// `TOADPOLES_WEAK` weak and the other calls it normal, the room the
    /// player walks into stops matching the room the map promised.
    #[test]
    fn tiers_agree_with_the_run_level_generator() {
        for enc in ACT1_UNDERDOCKS_ENCOUNTERS {
            let is_boss = enc.tier == EncounterTier::Boss;
            let in_boss_pool = sts2_core::run_room_gen::all_boss_encounter_ids().contains(&enc.id);
            assert_eq!(
                is_boss, in_boss_pool,
                "{} tier/boss-pool disagreement",
                enc.id
            );
        }
    }

    /// Each randomizer consumes exactly ONE read, and the rotation walks
    /// consecutively from it. Getting the read COUNT wrong desyncs the
    /// per-encounter stream; getting the rotation wrong changes turn 1.
    #[test]
    fn randomizers_consume_one_read_and_rotate_consecutively() {
        type Randomizer = fn(&mut sts2_core::dotnet_rng::Rng) -> EncounterRoster;
        for (count, f) in [
            (2usize, randomize_corpse_slugs_weak as Randomizer),
            (3usize, randomize_corpse_slugs_normal as Randomizer),
        ] {
            for seed in 0..12 {
                let mut rng = sts2_core::dotnet_rng::Rng::with_name(seed, "corpse_slugs");
                let roster = f(&mut rng);
                assert_eq!(rng.counter(), 1, "one NextInt(3), seed {seed}");
                assert_eq!(roster.len(), count);
                let idxs: Vec<u8> = roster
                    .iter()
                    .map(|(_, a)| match a {
                        MonsterSpawnArgs::CorpseSlug { starter_idx } => *starter_idx,
                        other => panic!("unexpected spawn args {other:?}"),
                    })
                    .collect();
                for w in idxs.windows(2) {
                    assert_eq!(w[1], (w[0] + 1) % 3, "consecutive rotation, got {idxs:?}");
                }
            }
        }
    }

    /// The rat rotation is a permutation of all three starters — never a
    /// repeat — for every offset the single read can produce.
    #[test]
    fn two_tailed_rats_rotate_through_all_three_starters() {
        let mut seen_offsets = std::collections::BTreeSet::new();
        for seed in 0..40 {
            let mut rng = sts2_core::dotnet_rng::Rng::with_name(seed, "two_tailed_rats_normal");
            let roster = randomize_two_tailed_rats_normal(&mut rng);
            assert_eq!(rng.counter(), 1, "one NextInt(3)");
            assert_eq!(roster.len(), 3);
            let slots: Vec<TwoTailedRatSlot> = roster
                .iter()
                .map(|(_, a)| match a {
                    MonsterSpawnArgs::TwoTailedRat { slot } => *slot,
                    other => panic!("unexpected spawn args {other:?}"),
                })
                .collect();
            let distinct: std::collections::BTreeSet<_> =
                slots.iter().map(|s| format!("{s:?}")).collect();
            assert_eq!(distinct.len(), 3, "all three starters, got {slots:?}");
            seen_offsets.insert(format!("{:?}", slots[0]));
        }
        assert_eq!(
            seen_offsets.len(),
            3,
            "40 seeds should exercise every rotation offset, saw {seen_offsets:?}"
        );
    }

    #[test]
    fn find_is_case_insensitive() {
        assert!(find("toadpoles_weak").is_some());
        assert!(find("TOADPOLES_WEAK").is_some());
        assert!(find("SLIMES_WEAK").is_none(), "that is an Overgrowth id");
    }
}
