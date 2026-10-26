//! Act 2 (Hive) encounter registry — bead `sts2-rs-6ynt.10`.
//!
//! Mirrors `sts2_env/encounters/act2.py` (pin `01c094d`):
//!
//! - 4 weak encounters
//! - 11 normal encounters
//! - 3 elite encounters
//! - 3 boss encounters (The Insatiable, Knowledge Demon, Kaiser Crab)
//!
//! Slot-arg monsters (Exoskeleton / Decimillipede / Infested Prism /
//! Chomper) carry explicit [`MonsterSpawnArgs`] payloads so the
//! dispatch table picks the canonical starting move without relying on
//! positional fallback.

use crate::encounters::{EncounterDef, EncounterTier};
use crate::monster_dispatch::MonsterSpawnArgs;
use crate::monsters::act2_hive::{ExoskeletonSlot, InfestedPrismSlot, MyteSlot};
use sts2_core::MonsterKind;

/// Every Act 2 encounter, in the canonical Python ordering
/// (weak → normal → elite → boss).
pub static ACT2_ENCOUNTERS: &[EncounterDef] = &[
    // ── Weak ──────────────────────────────────────────────────────
    EncounterDef {
        id: "THIEVING_HOPPER_WEAK",
        display: "Thieving Hopper",
        monsters: &[(MonsterKind::ThievingHopper, MonsterSpawnArgs::None)],
        tier: EncounterTier::Weak,
        randomize: None,
    },
    EncounterDef {
        id: "TUNNELER_WEAK",
        display: "Tunneler",
        monsters: &[(MonsterKind::Tunneler, MonsterSpawnArgs::None)],
        tier: EncounterTier::Weak,
        randomize: None,
    },
    EncounterDef {
        id: "BOWLBUGS_WEAK",
        // C# `BowlbugsWeak.GenerateMonsters` = `[BowlbugRock "odd",
        // Rng.NextItem(Bugs) "even"]` where `Bugs = [BowlbugEgg,
        // BowlbugNectar]` — a fixed Rock plus one randomized bug. The
        // prior Rust table was a fixed `[Egg, Nectar]` (wrong: missing the
        // Rock, no roll). `monsters` keeps the canonical no-RNG fallback.
        // Bead M3b roster-parity.
        display: "Bowlbugs (Rock + 1 of Egg/Nectar)",
        monsters: &[
            (MonsterKind::BowlbugRock, MonsterSpawnArgs::None),
            (MonsterKind::BowlbugEgg, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Weak,
        randomize: Some(randomize_bowlbugs_weak),
    },
    EncounterDef {
        id: "EXOSKELETONS_WEAK",
        // C# `ExoskeletonsWeak.GenerateMonsters` is a FIXED roster of
        // THREE Exoskeletons in slots first/second/third (no Rng). The
        // prior Rust table carried only two, so the niche HP sweep saw
        // one fewer roll. Bead M3b roster-parity (roster-composition gap).
        display: "Exoskeletons",
        monsters: &[
            (
                MonsterKind::Exoskeleton,
                MonsterSpawnArgs::Exoskeleton {
                    slot: ExoskeletonSlot::First,
                },
            ),
            (
                MonsterKind::Exoskeleton,
                MonsterSpawnArgs::Exoskeleton {
                    slot: ExoskeletonSlot::Second,
                },
            ),
            (
                MonsterKind::Exoskeleton,
                MonsterSpawnArgs::Exoskeleton {
                    slot: ExoskeletonSlot::Third,
                },
            ),
        ],
        tier: EncounterTier::Weak,
        randomize: None,
    },
    // ── Normal ────────────────────────────────────────────────────
    EncounterDef {
        id: "BOWLBUGS_NORMAL",
        // C# `BowlbugsNormal.GenerateMonsters` fronts a fixed BowlbugRock
        // ("first"), then draws TWO distinct workers from
        // `_workerValidCounts.Keys = [BowlbugEgg, BowlbugSilk,
        // BowlbugNectar]` (all cap 1) via `Rng.NextItem` without
        // replacement (slots "middle"/"last"). The prior Rust table was a
        // fixed `[Egg, Rock, Silk]` (wrong order + composition).
        // `monsters` keeps the canonical no-RNG fallback. Bead M3b
        // roster-parity.
        display: "Bowlbugs (Rock + 2 of Egg/Silk/Nectar)",
        monsters: &[
            (MonsterKind::BowlbugRock, MonsterSpawnArgs::None),
            (MonsterKind::BowlbugEgg, MonsterSpawnArgs::None),
            (MonsterKind::BowlbugSilk, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Normal,
        randomize: Some(randomize_bowlbugs_normal),
    },
    EncounterDef {
        id: "CHOMPERS_NORMAL",
        display: "Two Chompers",
        monsters: &[
            (
                MonsterKind::Chomper,
                MonsterSpawnArgs::Chomper {
                    scream_first: false,
                },
            ),
            (
                MonsterKind::Chomper,
                MonsterSpawnArgs::Chomper { scream_first: true },
            ),
        ],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "EXOSKELETONS_NORMAL",
        // C# `ExoskeletonsNormal.GenerateMonsters` is a FIXED roster of
        // FOUR Exoskeletons in slots first/second/third/fourth (no Rng);
        // the "fourth" picks its starter move via a `RandomBranchState`
        // off the per-monster move Rng at telegraph time, NOT niche, so
        // the HP sweep stays aligned. The prior Rust table carried only
        // three. Bead M3b roster-parity (roster-composition gap).
        display: "Four Exoskeletons",
        monsters: &[
            (
                MonsterKind::Exoskeleton,
                MonsterSpawnArgs::Exoskeleton {
                    slot: ExoskeletonSlot::First,
                },
            ),
            (
                MonsterKind::Exoskeleton,
                MonsterSpawnArgs::Exoskeleton {
                    slot: ExoskeletonSlot::Second,
                },
            ),
            (
                MonsterKind::Exoskeleton,
                MonsterSpawnArgs::Exoskeleton {
                    slot: ExoskeletonSlot::Third,
                },
            ),
            (
                MonsterKind::Exoskeleton,
                MonsterSpawnArgs::Exoskeleton {
                    slot: ExoskeletonSlot::Fourth,
                },
            ),
        ],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "HUNTER_KILLER_NORMAL",
        display: "Hunter Killer",
        monsters: &[(MonsterKind::HunterKiller, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "LOUSE_PROGENITOR_NORMAL",
        display: "Louse Progenitor",
        monsters: &[(MonsterKind::LouseProgenitor, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "MYTES_NORMAL",
        // C# `MytesNormal.GenerateMonsters` returns exactly TWO Mytes in
        // slots "first"/"second" (`MytesNormal.cs`). The prior Rust table
        // spawned three slotless Mytes, so all three opened at TOXIC_MOVE
        // and dumped 6 Toxic into hand on turn 1 (the "first" Myte alone
        // casts TOXIC = 2 Toxic; "second" opens at SUCK). Bead
        // `sts2-rs-ybtmu`.
        display: "Two Mytes",
        monsters: &[
            (
                MonsterKind::Myte,
                MonsterSpawnArgs::Myte {
                    slot: MyteSlot::First,
                },
            ),
            (
                MonsterKind::Myte,
                MonsterSpawnArgs::Myte {
                    slot: MyteSlot::Second,
                },
            ),
        ],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "OVICOPTER_NORMAL",
        display: "Ovicopter",
        monsters: &[(MonsterKind::Ovicopter, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "SLUMBERING_BEETLE_NORMAL",
        // C# `SlumberingBeetleNormal.GenerateMonsters` is a FIXED roster
        // of [BowlbugRock "first", BowlbugSilk "second", SlumberingBeetle
        // "third"] (no Rng) — the beetle slumbers behind two worker
        // escorts. The prior Rust table spawned the beetle alone. Bowlbug
        // escorts take `None` args (matching `BOWLBUGS_NORMAL`). Bead M3b
        // roster-parity (roster-composition gap).
        display: "Slumbering Beetle (Rock + Silk escorts)",
        monsters: &[
            (MonsterKind::BowlbugRock, MonsterSpawnArgs::None),
            (MonsterKind::BowlbugSilk, MonsterSpawnArgs::None),
            (MonsterKind::SlumberingBeetle, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "SPINY_TOAD_NORMAL",
        display: "Spiny Toad",
        monsters: &[(MonsterKind::SpinyToad, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "THE_OBSCURA_NORMAL",
        display: "The Obscura",
        monsters: &[(MonsterKind::TheObscura, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "TUNNELER_NORMAL",
        // C# `TunnelerNormal.GenerateMonsters` returns a Chomper (with
        // `ScreamFirst = true`) in slot 0 followed by a Tunneler — NOT two
        // Tunnelers (tags: Burrower + Chomper). The prior Rust roster of
        // two slotless Tunnelers was a fabrication; the real fight opens
        // with the Chomper SCREECHing (3 status cards) beside the burrowing
        // Tunneler. Bead `sts2-rs-ybtmu`.
        display: "Chomper and Tunneler",
        monsters: &[
            (
                MonsterKind::Chomper,
                MonsterSpawnArgs::Chomper { scream_first: true },
            ),
            (MonsterKind::Tunneler, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    // ── Elite ─────────────────────────────────────────────────────
    // C# `DecimillipedeElite.GenerateMonsters` returns the three distinct
    // positional segment classes (`segment1`=Front, `segment2`=Middle,
    // `segment3`=Back), emitting ids `DECIMILLIPEDE_SEGMENT_{FRONT,MIDDLE,
    // BACK}`. Spawning the matching Rust variants (rather than the generic
    // `DecimillipedeSegment`) keeps the native parity roster aligned with
    // C#'s kinds, so the sweep no longer falls back to the kind-only
    // override path (which dropped each segment's Reattach power). The
    // Front/Middle/Back kinds are visual-only wrappers that reuse the same
    // moveset + `starter_idx` arg, the same `[40, 46]` HP roll, and now the
    // same Reattach(25) on-spawn buff. Bead `sts2-rs-8mpo1`.
    //
    // C# rolls the segments' opening moves: `num2 = base.Rng.NextInt(3)`, then
    // Front=`num2`, Middle=`(num2+1)%3`, Back=`(num2+2)%3`
    // (`DecimillipedeElite.cs:42-44`). The static `monsters` slice keeps the
    // `idx=0` fallback (0/1/2) for no-RNG callers; the gameplay path draws the
    // base via `randomize_decimillipede_elite`. Without the draw Rust pinned
    // num2=0, so the BULK (Strength+2) segment landed on the wrong member and
    // the per-encounter RNG cursor was one read short (Regent:1021 — Strength(2)
    // on Middle instead of Back; sts2-rs-RP30).
    EncounterDef {
        id: "DECIMILLIPEDE_ELITE",
        display: "Decimillipede",
        monsters: &[
            (
                MonsterKind::DecimillipedeSegmentFront,
                MonsterSpawnArgs::DecimillipedeSegment { starter_idx: 0 },
            ),
            (
                MonsterKind::DecimillipedeSegmentMiddle,
                MonsterSpawnArgs::DecimillipedeSegment { starter_idx: 1 },
            ),
            (
                MonsterKind::DecimillipedeSegmentBack,
                MonsterSpawnArgs::DecimillipedeSegment { starter_idx: 2 },
            ),
        ],
        tier: EncounterTier::Elite,
        randomize: Some(randomize_decimillipede_elite),
    },
    EncounterDef {
        id: "ENTOMANCER_ELITE",
        display: "Entomancer",
        monsters: &[(MonsterKind::Entomancer, MonsterSpawnArgs::None)],
        tier: EncounterTier::Elite,
        randomize: None,
    },
    EncounterDef {
        id: "INFESTED_PRISMS_ELITE",
        display: "Infested Prisms",
        // C# `InfestedPrismsElite.GenerateMonsters` returns a
        // `ReadOnlySingleElementList` — ONE InfestedPrism (the prism spawns
        // its infested minions during combat; the plural name is the fight,
        // not the starting roster). The prior Rust table spawned two, so the
        // starting roster + downstream energy diverged. Bead `sts2-rs-ybtmu`.
        monsters: &[(
            MonsterKind::InfestedPrism,
            MonsterSpawnArgs::InfestedPrism {
                slot: InfestedPrismSlot::First,
            },
        )],
        tier: EncounterTier::Elite,
        randomize: None,
    },
    // ── Boss ──────────────────────────────────────────────────────
    EncounterDef {
        id: "THE_INSATIABLE_BOSS",
        display: "The Insatiable",
        monsters: &[(MonsterKind::TheInsatiable, MonsterSpawnArgs::None)],
        tier: EncounterTier::Boss,
        randomize: None,
    },
    EncounterDef {
        id: "KNOWLEDGE_DEMON_BOSS",
        display: "Knowledge Demon",
        monsters: &[(MonsterKind::KnowledgeDemon, MonsterSpawnArgs::None)],
        tier: EncounterTier::Boss,
        randomize: None,
    },
    EncounterDef {
        id: "KAISER_CRAB_BOSS",
        display: "Kaiser Crab (Crusher + Rocket)",
        monsters: &[
            (MonsterKind::Crusher, MonsterSpawnArgs::None),
            (MonsterKind::Rocket, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Boss,
        randomize: None,
    },
];

// ---------------------------------------------------------------------------
// Spawn-time randomizers (M3b roster-parity)
// ---------------------------------------------------------------------------

/// `BOWLBUGS_WEAK`: C# `BowlbugsWeak.GenerateMonsters` returns
/// `[BowlbugRock "odd", Rng.NextItem(Bugs) "even"]` where
/// `Bugs = [BowlbugEgg, BowlbugNectar]` — one read over the two bugs.
pub fn randomize_bowlbugs_weak(
    rng: &mut sts2_core::dotnet_rng::Rng,
) -> crate::encounters::EncounterRoster {
    const BUGS: [MonsterKind; 2] = [MonsterKind::BowlbugEgg, MonsterKind::BowlbugNectar];
    let pick = crate::encounters::next_item(rng, &BUGS);
    let mut out = crate::encounters::EncounterRoster::new();
    out.push((MonsterKind::BowlbugRock, MonsterSpawnArgs::None));
    out.push((pick, MonsterSpawnArgs::None));
    out
}

/// `BOWLBUGS_NORMAL`: C# `BowlbugsNormal.GenerateMonsters` fronts a fixed
/// BowlbugRock, then draws TWO distinct workers from
/// `_workerValidCounts.Keys = [BowlbugEgg, BowlbugSilk, BowlbugNectar]`
/// (all cap 1) via `Rng.NextItem` WITHOUT replacement — two reads
/// (`NextInt(0,3)`, `NextInt(0,2)`) over the shrinking, insertion-ordered
/// worker list.
pub fn randomize_bowlbugs_normal(
    rng: &mut sts2_core::dotnet_rng::Rng,
) -> crate::encounters::EncounterRoster {
    let mut remaining: Vec<MonsterKind> = vec![
        MonsterKind::BowlbugEgg,
        MonsterKind::BowlbugSilk,
        MonsterKind::BowlbugNectar,
    ];
    let mut out = crate::encounters::EncounterRoster::new();
    out.push((MonsterKind::BowlbugRock, MonsterSpawnArgs::None));
    for _ in 0..2 {
        let idx = rng.next_int_exclusive(0, remaining.len() as i32) as usize;
        out.push((remaining.remove(idx), MonsterSpawnArgs::None));
    }
    out
}

/// `DECIMILLIPEDE_ELITE`: C# `DecimillipedeElite.GenerateMonsters`
/// (`DecimillipedeElite.cs:42-44`) draws `num2 = base.Rng.NextInt(3)` then sets
/// the three positional segments' `StarterMoveIdx` to `num2`, `(num2+1)%3`,
/// `(num2+2)%3` (Front, Middle, Back). `DecimillipedeSegment` maps the index to
/// the opener (`StarterMoveIdx % 3`: 0=WRITHE, 1=BULK, 2=CONSTRICT). One read
/// over the per-encounter `Rng`, BEFORE the per-creature HP rolls (which run off
/// the separate `niche` stream), so this does not shift any segment's HP.
pub fn randomize_decimillipede_elite(
    rng: &mut sts2_core::dotnet_rng::Rng,
) -> crate::encounters::EncounterRoster {
    const SEGMENTS: [MonsterKind; 3] = [
        MonsterKind::DecimillipedeSegmentFront,
        MonsterKind::DecimillipedeSegmentMiddle,
        MonsterKind::DecimillipedeSegmentBack,
    ];
    let base = rng.next_int_exclusive(0, 3);
    let mut out = crate::encounters::EncounterRoster::new();
    for (offset, &kind) in SEGMENTS.iter().enumerate() {
        out.push((
            kind,
            MonsterSpawnArgs::DecimillipedeSegment {
                starter_idx: ((base + offset as i32) % 3) as u8,
            },
        ));
    }
    out
}

/// Lookup helper: resolve an Act-2 encounter id (`UPPER_SNAKE`).
/// Case-insensitive.
#[must_use]
pub fn find(id: &str) -> Option<&'static EncounterDef> {
    ACT2_ENCOUNTERS
        .iter()
        .find(|e| e.id.eq_ignore_ascii_case(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monster_dispatch::{build_ai, SpawnPosition};

    #[test]
    fn every_act2_encounter_builds_to_nonempty_ai() {
        assert!(!ACT2_ENCOUNTERS.is_empty());
        for enc in ACT2_ENCOUNTERS {
            assert!(!enc.monsters.is_empty(), "encounter '{}' empty", enc.id);
            for (idx, (kind, args)) in enc.monsters.iter().enumerate() {
                let pos = SpawnPosition {
                    alone: enc.monsters.len() == 1,
                    front: idx == 0,
                    back: idx == enc.monsters.len() - 1,
                    index: idx as u8,
                };
                let ai = build_ai(*kind, *args, pos);
                assert!(
                    !ai.current_move().intents.is_empty(),
                    "encounter '{}' kind {:?} has empty starting intents",
                    enc.id,
                    kind
                );
            }
        }
    }

    /// Pin counts to the Python pin (4 weak + 11 normal + 3 elite + 3
    /// boss = 21).
    #[test]
    fn act2_encounter_counts_match_python_pin() {
        let weak = ACT2_ENCOUNTERS
            .iter()
            .filter(|e| e.tier == EncounterTier::Weak)
            .count();
        let normal = ACT2_ENCOUNTERS
            .iter()
            .filter(|e| e.tier == EncounterTier::Normal)
            .count();
        let elite = ACT2_ENCOUNTERS
            .iter()
            .filter(|e| e.tier == EncounterTier::Elite)
            .count();
        let boss = ACT2_ENCOUNTERS
            .iter()
            .filter(|e| e.tier == EncounterTier::Boss)
            .count();
        assert_eq!(weak, 4, "weak count drift");
        assert_eq!(normal, 11, "normal count drift");
        assert_eq!(elite, 3, "elite count drift");
        assert_eq!(boss, 3, "boss count drift");
        assert_eq!(ACT2_ENCOUNTERS.len(), 21);
    }

    /// `randomize_decimillipede_elite` must mirror C#
    /// `DecimillipedeElite.cs:42-44`: Front=`num2`, Middle=`(num2+1)%3`,
    /// Back=`(num2+2)%3` off ONE `NextInt(3)`, regardless of the rolled base.
    /// Probe many seeds so every base (0/1/2) is exercised.
    #[test]
    fn decimillipede_starter_idx_rotates_off_one_draw() {
        use sts2_core::dotnet_rng::Rng;
        let mut saw_base = [false; 3];
        for seed in 0..64i32 {
            let mut rng = Rng::with_name(seed, "DECIMILLIPEDE_ELITE");
            let roster = randomize_decimillipede_elite(&mut rng);
            assert_eq!(roster.len(), 3, "seed {seed}: roster size");
            let idxs: Vec<u8> = roster
                .iter()
                .map(|(_, args)| match args {
                    MonsterSpawnArgs::DecimillipedeSegment { starter_idx } => *starter_idx,
                    other => panic!("seed {seed}: unexpected args {other:?}"),
                })
                .collect();
            // Positional kinds match the C# segment1/2/3 order.
            assert_eq!(roster[0].0, MonsterKind::DecimillipedeSegmentFront);
            assert_eq!(roster[1].0, MonsterKind::DecimillipedeSegmentMiddle);
            assert_eq!(roster[2].0, MonsterKind::DecimillipedeSegmentBack);
            // Front = base; Middle/Back are +1/+2 mod 3.
            let base = idxs[0];
            assert!(base < 3, "seed {seed}: base out of range");
            assert_eq!(idxs[1], (base + 1) % 3, "seed {seed}: middle offset");
            assert_eq!(idxs[2], (base + 2) % 3, "seed {seed}: back offset");
            saw_base[base as usize] = true;
        }
        assert!(
            saw_base.iter().all(|&b| b),
            "expected all three bases across the seed sweep, saw {saw_base:?}"
        );
    }

    /// Bead `sts2-rs-ybtmu`: C# `InfestedPrismsElite.GenerateMonsters`
    /// returns a single-element list — the starting roster is ONE prism
    /// (minions spawn during combat). The prior table spawned two.
    #[test]
    fn infested_prisms_elite_starts_with_one_prism() {
        let enc = find("INFESTED_PRISMS_ELITE").expect("registered");
        assert_eq!(enc.monsters.len(), 1, "C# spawns exactly one InfestedPrism");
        assert_eq!(enc.monsters[0].0, sts2_core::MonsterKind::InfestedPrism);
    }

    #[test]
    fn act2_encounter_ids_are_unique() {
        let mut seen: Vec<&str> = Vec::new();
        for enc in ACT2_ENCOUNTERS {
            assert!(!seen.contains(&enc.id), "dup: {}", enc.id);
            assert!(
                enc.id
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                "encounter id is not UPPER_SNAKE: {}",
                enc.id
            );
            seen.push(enc.id);
        }
    }

    /// `randomize_bowlbugs_weak` returns the 2-monster roster
    /// `[BowlbugRock, worker]` where the worker is one of the two bugs. Kills
    /// the `-> Default::default()` mutant (empty roster) and pins both worker
    /// variants surfacing across a seed sweep.
    #[test]
    fn bowlbugs_weak_roster_shape() {
        use std::collections::HashSet;
        let bugs: HashSet<MonsterKind> = [MonsterKind::BowlbugEgg, MonsterKind::BowlbugNectar]
            .into_iter()
            .collect();
        let mut seen_worker: HashSet<MonsterKind> = HashSet::new();
        for seed in 0..64u64 {
            let mut rng = sts2_core::dotnet_rng::Rng::new(seed);
            let roster = randomize_bowlbugs_weak(&mut rng);
            assert_eq!(roster.len(), 2, "bowlbugs_weak always returns 2 monsters");
            assert_eq!(roster[0].0, MonsterKind::BowlbugRock, "slot 0 is the rock");
            assert!(bugs.contains(&roster[1].0), "slot 1 is a worker bug");
            seen_worker.insert(roster[1].0);
        }
        assert_eq!(seen_worker.len(), 2, "both worker bugs must surface");
    }

    /// `randomize_bowlbugs_normal` fronts a fixed BowlbugRock then draws TWO
    /// distinct workers from `[BowlbugEgg, BowlbugSilk, BowlbugNectar]` without
    /// replacement. Kills the `-> Default::default()` mutant (empty roster) and
    /// pins the 3-monster shape + distinct-worker invariant; all three workers
    /// surface across the sweep.
    #[test]
    fn bowlbugs_normal_roster_shape() {
        use std::collections::HashSet;
        let workers: HashSet<MonsterKind> = [
            MonsterKind::BowlbugEgg,
            MonsterKind::BowlbugSilk,
            MonsterKind::BowlbugNectar,
        ]
        .into_iter()
        .collect();
        let mut seen_workers: HashSet<MonsterKind> = HashSet::new();
        for seed in 0..64u64 {
            let mut rng = sts2_core::dotnet_rng::Rng::new(seed);
            let roster = randomize_bowlbugs_normal(&mut rng);
            assert_eq!(roster.len(), 3, "bowlbugs_normal always returns 3 monsters");
            assert_eq!(roster[0].0, MonsterKind::BowlbugRock, "slot 0 is the rock");
            assert!(workers.contains(&roster[1].0), "slot 1 is a worker");
            assert!(workers.contains(&roster[2].0), "slot 2 is a worker");
            assert_ne!(roster[1].0, roster[2].0, "the two workers are distinct");
            seen_workers.insert(roster[1].0);
            seen_workers.insert(roster[2].0);
        }
        assert_eq!(seen_workers.len(), 3, "all three workers must surface");
    }
}
