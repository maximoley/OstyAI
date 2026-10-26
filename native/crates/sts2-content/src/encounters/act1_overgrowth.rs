//! Act 1 encounter registry — bead `sts2-rs-k923`.
//!
//! Mirrors `sts2_env/encounters/act1.py` (pin `01c094d`):
//!
//! - 4 weak encounters
//! - 12 normal encounters
//! - 3 elite encounters
//! - 3 boss encounters (Vantom, Ceremonial Beast, The Kin)
//!
//! Two Python encounters (`slimes_weak`, `ruby_raiders_normal`) draw
//! a random sub-selection at spawn-time; for Phase 3 we materialize a
//! canonical fixed permutation so the static-table contract holds.
//! The deferred randomization is tracked under the act1 surface
//! umbrella (`sts2-rs-78ks`).

use crate::encounters::{EncounterDef, EncounterRoster, EncounterTier};
use crate::monster_dispatch::MonsterSpawnArgs;
use sts2_core::MonsterKind;

/// Every Act 1 encounter, in the canonical Python ordering
/// (weak → normal → elite → boss). Iteration order is the printed
/// order for `sts2-cli list encounters --filter act1`.
pub static ACT1_ENCOUNTERS: &[EncounterDef] = &[
    // ── Weak ──────────────────────────────────────────────────────
    EncounterDef {
        id: "SHRINKER_BEETLE_WEAK",
        display: "Shrinker Beetle",
        monsters: &[(MonsterKind::ShrinkerBeetle, MonsterSpawnArgs::None)],
        tier: EncounterTier::Weak,
        randomize: None,
    },
    EncounterDef {
        id: "FUZZY_WURM_CRAWLER_WEAK",
        display: "Fuzzy Wurm Crawler",
        monsters: &[(MonsterKind::FuzzyWurmCrawler, MonsterSpawnArgs::None)],
        tier: EncounterTier::Weak,
        randomize: None,
    },
    EncounterDef {
        id: "NIBBITS_WEAK",
        display: "Nibbit",
        monsters: &[(
            MonsterKind::Nibbit,
            MonsterSpawnArgs::Nibbit {
                is_alone: true,
                is_front: true,
            },
        )],
        tier: EncounterTier::Weak,
        randomize: None,
    },
    // `slimes_weak`: Python samples 2-of-2 small slimes + 1-of-2 medium
    // (`sts2_env/encounters/act1.py:63-72`). The `monsters` slice holds
    // the canonical (Leaf-S + Twig-S + Leaf-M) permutation as the
    // no-RNG fallback; `randomize` wires the spawn-time draw. Bead
    // `sts2-rs-6inu`.
    EncounterDef {
        id: "SLIMES_WEAK",
        display: "Slimes",
        monsters: &[
            (MonsterKind::LeafSlimeS, MonsterSpawnArgs::None),
            (MonsterKind::TwigSlimeS, MonsterSpawnArgs::None),
            (MonsterKind::LeafSlimeM, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Weak,
        randomize: Some(randomize_slimes_weak),
    },
    // ── Normal ────────────────────────────────────────────────────
    EncounterDef {
        id: "CUBEX_CONSTRUCT_NORMAL",
        display: "Cubex Construct",
        monsters: &[(MonsterKind::CubexConstruct, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    // `FLYCONID_NORMAL`: C# at
    // `MegaCrit.Sts2.Core.Models.Encounters/FlyconidNormal.cs`
    // spawns `[Rng.NextItem(_mediumSlimes), Flyconid]` where
    // `_mediumSlimes = [LeafSlimeM, TwigSlimeM]`. Prior Rust shipped
    // only Flyconid — dropped the secondary entirely. Bead
    // `sts2-rs-ogcv5` (parity-fuzz INIT diff, 5/100 trials).
    EncounterDef {
        id: "FLYCONID_NORMAL",
        display: "Medium Slime + Flyconid",
        monsters: &[
            (MonsterKind::LeafSlimeM, MonsterSpawnArgs::None),
            (MonsterKind::Flyconid, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Normal,
        randomize: Some(randomize_flyconid_normal),
    },
    EncounterDef {
        id: "FOGMOG_NORMAL",
        display: "Fogmog",
        monsters: &[(MonsterKind::Fogmog, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    // `INKLETS_NORMAL`: C# decomp at
    // `MegaCrit.Sts2.Core.Models.Encounters/InkletsNormal.cs` spawns
    // THREE inklets in `(item, middle, item2)` order; the middle slot
    // carries the `MiddleInklet=true` flag which selects the RAND
    // opener — mapped to `InkletSlot::Other` here (Python pin at
    // `sts2-rl-agent@01c094d:sts2_env/encounters/act1.py:100-104`
    // spawned only 2 and is wrong; STS2 is the source of truth). Bead
    // `sts2-rs-wk96`.
    EncounterDef {
        id: "INKLETS_NORMAL",
        display: "Three Inklets",
        // C# `InkletsNormal.GenerateMonsters` sets `MiddleInklet = true` on
        // index 1 (the second spawned inklet), which opens with WHIRLWIND;
        // indices 0 and 2 open with JAB. `InkletSlot::Second` is the
        // WHIRLWIND opener (see `create_inklet_ai`), so it MUST sit at index 1.
        // (Bead `sts2-rs-zdjjx`: the prior wiring put `Other`/JAB at index 1
        // and `Second`/WHIRLWIND at index 2 — the WHIRLWIND inklet on the
        // wrong slot. The 3010t kind-only override masked it because its
        // position fallback maps index 1 → Second; native spawn exposed it.)
        monsters: &[
            (
                MonsterKind::Inklet,
                MonsterSpawnArgs::Inklet {
                    slot: crate::monsters::act1_overgrowth::InkletSlot::First,
                },
            ),
            (
                MonsterKind::Inklet,
                MonsterSpawnArgs::Inklet {
                    slot: crate::monsters::act1_overgrowth::InkletSlot::Second,
                },
            ),
            (
                MonsterKind::Inklet,
                MonsterSpawnArgs::Inklet {
                    slot: crate::monsters::act1_overgrowth::InkletSlot::Other,
                },
            ),
        ],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "MAWLER_NORMAL",
        display: "Mawler",
        monsters: &[(MonsterKind::Mawler, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "NIBBITS_NORMAL",
        display: "Two Nibbits",
        monsters: &[
            (
                MonsterKind::Nibbit,
                MonsterSpawnArgs::Nibbit {
                    is_alone: false,
                    is_front: true,
                },
            ),
            (
                MonsterKind::Nibbit,
                MonsterSpawnArgs::Nibbit {
                    is_alone: false,
                    is_front: false,
                },
            ),
        ],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    // C# `OvergrowthCrawlers.GenerateMonsters()` returns
    // `[ShrinkerBeetle, FuzzyWurmCrawler]`
    // (`MegaCrit.Sts2.Core.Models.Encounters/OvergrowthCrawlers.cs:24-30`).
    // The prior `[FuzzyWurmCrawler, Nibbit]` slice was a Python-pin
    // fabrication (Nibbit isn't in this encounter); it aborted the parity
    // harness at init and dropped the Shrinker's SHRINK debuff. q6bix fix.
    //
    // Id is `OVERGROWTH_CRAWLERS` (no `_NORMAL`): the C# model id is
    // derived from the class name `OvergrowthCrawlers`, which — unlike
    // `CubexConstructNormal`/`InkletsNormal` — carries no "Normal" suffix.
    // The prior `_NORMAL` id was a Rust-side fabrication that made the C#
    // headless `enter_room` throw `ModelNotFoundException` (sts2-rs-ztx5u).
    EncounterDef {
        id: "OVERGROWTH_CRAWLERS",
        display: "Shrinker Beetle + Fuzzy Wurm Crawler",
        monsters: &[
            (MonsterKind::ShrinkerBeetle, MonsterSpawnArgs::None),
            (MonsterKind::FuzzyWurmCrawler, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    // `ruby_raiders_normal`: Python samples 3-of-5 raider variants
    // (`sts2_env/encounters/act1.py:126-137`). The `monsters` slice
    // holds the canonical (Assassin + Axe + Brute) permutation as the
    // no-RNG fallback; `randomize` wires the spawn-time draw. Bead
    // `sts2-rs-6inu`.
    EncounterDef {
        id: "RUBY_RAIDERS_NORMAL",
        display: "Ruby Raiders",
        monsters: &[
            (MonsterKind::AssassinRubyRaider, MonsterSpawnArgs::None),
            (MonsterKind::AxeRubyRaider, MonsterSpawnArgs::None),
            (MonsterKind::BruteRubyRaider, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Normal,
        randomize: Some(randomize_ruby_raiders_normal),
    },
    // `SLIMES_NORMAL`: C# decomp at
    // `MegaCrit.Sts2.Core.Models.Encounters/SlimesNormal.cs` spawns
    // FOUR slimes: TwigSlimeM + LeafSlimeM + 2 smalls (one LeafSlimeS
    // + one TwigSlimeS in RNG-determined order via `Rng.NextBool`).
    // Python pin at
    // `sts2-rl-agent@01c094d:sts2_env/encounters/act1.py:140-145`
    // spawned only the 2 mediums and is wrong; STS2 is the source of
    // truth. Bead `sts2-rs-wk96`.
    EncounterDef {
        id: "SLIMES_NORMAL",
        display: "Two Medium + Two Small Slimes",
        monsters: &[
            (MonsterKind::TwigSlimeM, MonsterSpawnArgs::None),
            (MonsterKind::LeafSlimeM, MonsterSpawnArgs::None),
            (MonsterKind::LeafSlimeS, MonsterSpawnArgs::None),
            (MonsterKind::TwigSlimeS, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Normal,
        randomize: Some(randomize_slimes_normal),
    },
    // `SLITHERING_STRANGLER_NORMAL`: C# at
    // `MegaCrit.Sts2.Core.Models.Encounters/SlitheringStranglerNormal.cs`
    // picks a `SecondaryEnemyType` via `Rng.NextItem` ∈
    // {SnappingJaxfruit, MediumSlime, SmallSlimes}, then appends
    // SlitheringStrangler last. Branch payloads:
    //   - SnappingJaxfruit: [SnappingJaxfruit, SlitheringStrangler]
    //   - MediumSlime:      [Rng.NextItem(mediumSlimes), SlitheringStrangler]
    //   - SmallSlimes:      [Rng.NextItem(smallSlimes), Rng.NextItem(smallSlimes), SlitheringStrangler]
    // Prior Rust shipped only SlitheringStrangler. Bead
    // `sts2-rs-ogcv5` (parity-fuzz, 3/100 trials).
    EncounterDef {
        id: "SLITHERING_STRANGLER_NORMAL",
        display: "Slithering Strangler + Secondary",
        monsters: &[
            (MonsterKind::SnappingJaxfruit, MonsterSpawnArgs::None),
            (MonsterKind::SlitheringStrangler, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Normal,
        randomize: Some(randomize_slithering_strangler_normal),
    },
    // `SNAPPING_JAXFRUIT_NORMAL`: C# at
    // `MegaCrit.Sts2.Core.Models.Encounters/SnappingJaxfruitNormal.cs`
    // spawns the fixed pair `[SnappingJaxfruit, Flyconid]`. No
    // randomization — `GenerateMonsters` returns a hardcoded 2-tuple.
    // Bead `sts2-rs-ogcv5` (parity-fuzz, 5/100 trials).
    EncounterDef {
        id: "SNAPPING_JAXFRUIT_NORMAL",
        display: "Snapping Jaxfruit + Flyconid",
        monsters: &[
            (MonsterKind::SnappingJaxfruit, MonsterSpawnArgs::None),
            (MonsterKind::Flyconid, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "VINE_SHAMBLER_NORMAL",
        display: "Vine Shambler",
        monsters: &[(MonsterKind::VineShambler, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    // ── Elite ─────────────────────────────────────────────────────
    EncounterDef {
        id: "BYGONE_EFFIGY_ELITE",
        display: "Bygone Effigy",
        monsters: &[(MonsterKind::BygoneEffigy, MonsterSpawnArgs::None)],
        tier: EncounterTier::Elite,
        randomize: None,
    },
    EncounterDef {
        id: "BYRDONIS_ELITE",
        display: "Byrdonis",
        monsters: &[(MonsterKind::Byrdonis, MonsterSpawnArgs::None)],
        tier: EncounterTier::Elite,
        randomize: None,
    },
    EncounterDef {
        id: "PHROG_PARASITE_ELITE",
        display: "Phrog Parasite",
        monsters: &[(MonsterKind::PhrogParasite, MonsterSpawnArgs::None)],
        tier: EncounterTier::Elite,
        randomize: None,
    },
    // ── Boss ──────────────────────────────────────────────────────
    EncounterDef {
        id: "VANTOM_BOSS",
        display: "Vantom",
        monsters: &[(MonsterKind::Vantom, MonsterSpawnArgs::None)],
        tier: EncounterTier::Boss,
        randomize: None,
    },
    EncounterDef {
        id: "CEREMONIAL_BEAST_BOSS",
        display: "Ceremonial Beast",
        monsters: &[(MonsterKind::CeremonialBeast, MonsterSpawnArgs::None)],
        tier: EncounterTier::Boss,
        randomize: None,
    },
    // C# `TheKinBoss.GenerateMonsters()` returns, in order:
    //   slot1      = KinFollower with `StartsWithDance = true` (opens on
    //                POWER_DANCE → Strength(2)),
    //   slot2      = KinFollower (default → opens on QUICK_SLASH),
    //   leaderSlot = KinPriest.
    // So the DANCING follower is `monsters[0]` and the priest is LAST.
    // The prior Rust order ([Priest, Follower, dancer]) put the priest at
    // index 0 and the dancer at index 2 — both wrong vs C# spawn order,
    // which desynced monster indexing (targeting / MCTS) and left the
    // turn-0 monster[0] without the dance Strength (sts2-rs-ztx5u). The
    // `Third` slot is the `StartsWithDance` stand-in (see `KinFollowerSlot`).
    EncounterDef {
        id: "THE_KIN_BOSS",
        display: "The Kin",
        monsters: &[
            (
                MonsterKind::KinFollower,
                MonsterSpawnArgs::KinFollower {
                    slot: crate::monsters::act1_overgrowth::KinFollowerSlot::Third,
                },
            ),
            (
                MonsterKind::KinFollower,
                MonsterSpawnArgs::KinFollower {
                    slot: crate::monsters::act1_overgrowth::KinFollowerSlot::First,
                },
            ),
            (MonsterKind::KinPriest, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Boss,
        randomize: None,
    },
];

/// Lookup helper: resolve an Act-1 encounter id (`UPPER_SNAKE`).
/// Case-insensitive: `MAWLER_NORMAL` and `mawler_normal` both match.
#[must_use]
pub fn find(id: &str) -> Option<&'static EncounterDef> {
    ACT1_ENCOUNTERS
        .iter()
        .find(|e| e.id.eq_ignore_ascii_case(id))
}

// ---------------------------------------------------------------------------
// Spawn-time randomizers (bead `sts2-rs-6inu`)
// ---------------------------------------------------------------------------

// C# candidate arrays, in their exact declaration order — `NextItem`
// indexes into these, so the ORDER is part of the RNG contract.
//   `_smallSlimes = [LeafSlimeS, TwigSlimeS]`
//   `_mediumSlimes = [LeafSlimeM, TwigSlimeM]`
const SMALL_SLIMES: [MonsterKind; 2] = [MonsterKind::LeafSlimeS, MonsterKind::TwigSlimeS];
const MEDIUM_SLIMES: [MonsterKind; 2] = [MonsterKind::LeafSlimeM, MonsterKind::TwigSlimeM];

/// `SLIMES_WEAK`: C# `SlimesWeak.GenerateMonsters` (decomp):
/// ```text
/// list2 = _smallSlimes.ToList()       // [LeafSlimeS, TwigSlimeS]
/// a = Rng.NextItem(list2); list2.Remove(a)
/// b = Rng.NextItem(list2)             // 1-elt list → STILL reads
/// return [a, Rng.NextItem(_mediumSlimes), b]   // medium in the MIDDLE
/// ```
/// Three reads, in that order; the medium sits at slot 1, the two
/// (distinct) smalls at slots 0 and 2. The prior Python-pin port used
/// `shuffle` + medium-last, which mismatched both the draw count and the
/// slot order. Bead M3b roster-parity.
pub fn randomize_slimes_weak(rng: &mut sts2_core::dotnet_rng::Rng) -> EncounterRoster {
    let mut smalls: Vec<MonsterKind> = SMALL_SLIMES.to_vec();
    let a_idx = rng.next_int_exclusive(0, smalls.len() as i32) as usize;
    let a = smalls.remove(a_idx);
    let b = crate::encounters::next_item(rng, &smalls);
    let medium = crate::encounters::next_item(rng, &MEDIUM_SLIMES);
    let mut out = EncounterRoster::new();
    out.push((a, MonsterSpawnArgs::None));
    out.push((medium, MonsterSpawnArgs::None));
    out.push((b, MonsterSpawnArgs::None));
    out
}

/// `SLIMES_NORMAL`: C# `SlimesNormal.GenerateMonsters`:
/// ```text
/// flag = Rng.NextBool()
/// m1 = flag ? LeafSlimeS : TwigSlimeS
/// m2 = flag ? TwigSlimeS : LeafSlimeS
/// return [TwigSlimeM, LeafSlimeM, m1, m2]
/// ```
/// A single `NextBool` read decides the two small slots' order. The
/// prior port used `shuffle`, which reads the same single int but maps
/// it to the OPPOSITE permutation (`shuffle` swaps on val 0; `NextBool`
/// is `val == 0`), so it diverged on composition. Bead M3b roster-parity.
pub fn randomize_slimes_normal(rng: &mut sts2_core::dotnet_rng::Rng) -> EncounterRoster {
    let flag = rng.next_bool();
    let (s1, s2) = if flag {
        (MonsterKind::LeafSlimeS, MonsterKind::TwigSlimeS)
    } else {
        (MonsterKind::TwigSlimeS, MonsterKind::LeafSlimeS)
    };
    let mut out = EncounterRoster::new();
    out.push((MonsterKind::TwigSlimeM, MonsterSpawnArgs::None));
    out.push((MonsterKind::LeafSlimeM, MonsterSpawnArgs::None));
    out.push((s1, MonsterSpawnArgs::None));
    out.push((s2, MonsterSpawnArgs::None));
    out
}

/// `RUBY_RAIDERS_NORMAL`: C# `RubyRaidersNormal.GenerateMonsters` loops
/// three times, each iteration rebuilding the still-valid pool (every
/// raider has cap 1, so a pick is removed) and drawing `Rng.NextItem`
/// over it — a 3-of-5 selection WITHOUT replacement that preserves the
/// dict's insertion order `[Axe, Assassin, Brute, Crossbow, Tracker]`.
/// Three reads (`NextInt(0,5)`, `(0,4)`, `(0,3)`). The prior port used a
/// full `shuffle` (a different algorithm, 4 reads, wrong pool order).
/// Bead M3b roster-parity.
pub fn randomize_ruby_raiders_normal(rng: &mut sts2_core::dotnet_rng::Rng) -> EncounterRoster {
    let mut remaining: Vec<MonsterKind> = vec![
        MonsterKind::AxeRubyRaider,
        MonsterKind::AssassinRubyRaider,
        MonsterKind::BruteRubyRaider,
        MonsterKind::CrossbowRubyRaider,
        MonsterKind::TrackerRubyRaider,
    ];
    let mut out = EncounterRoster::new();
    for _ in 0..3 {
        let idx = rng.next_int_exclusive(0, remaining.len() as i32) as usize;
        out.push((remaining.remove(idx), MonsterSpawnArgs::None));
    }
    out
}

/// `FLYCONID_NORMAL`: C# `FlyconidNormal.GenerateMonsters` spawns
/// `[Rng.NextItem(_mediumSlimes), Flyconid]` — one read over
/// `[LeafSlimeM, TwigSlimeM]`. Bead `sts2-rs-ogcv5`; the composition was
/// already faithful — M3b only moved the read onto the per-encounter Rng.
pub fn randomize_flyconid_normal(rng: &mut sts2_core::dotnet_rng::Rng) -> EncounterRoster {
    let medium = crate::encounters::next_item(rng, &MEDIUM_SLIMES);
    let mut out = EncounterRoster::new();
    out.push((medium, MonsterSpawnArgs::None));
    out.push((MonsterKind::Flyconid, MonsterSpawnArgs::None));
    out
}

/// `SLITHERING_STRANGLER_NORMAL`: C# at
/// `MegaCrit.Sts2.Core.Models.Encounters/SlitheringStranglerNormal.cs`
/// picks a 3-way branch (uniform `Rng.NextItem` over the
/// `SecondaryEnemyType` enum `{SnappingJaxfruit, MediumSlime,
/// SmallSlimes}`), draws sub-picks inside the branch, then appends
/// SlitheringStrangler last. Bead `sts2-rs-ogcv5`.
pub fn randomize_slithering_strangler_normal(
    rng: &mut sts2_core::dotnet_rng::Rng,
) -> EncounterRoster {
    let mut out = EncounterRoster::new();
    match rng.next_int_exclusive(0, 3) {
        0 => {
            // SecondaryEnemyType.SnappingJaxfruit
            out.push((MonsterKind::SnappingJaxfruit, MonsterSpawnArgs::None));
        }
        1 => {
            // SecondaryEnemyType.MediumSlime
            out.push((
                crate::encounters::next_item(rng, &MEDIUM_SLIMES),
                MonsterSpawnArgs::None,
            ));
        }
        _ => {
            // SecondaryEnemyType.SmallSlimes — two independent picks
            // (with replacement, per the C# back-to-back `Rng.NextItem`).
            let s1 = crate::encounters::next_item(rng, &SMALL_SLIMES);
            let s2 = crate::encounters::next_item(rng, &SMALL_SLIMES);
            out.push((s1, MonsterSpawnArgs::None));
            out.push((s2, MonsterSpawnArgs::None));
        }
    }
    out.push((MonsterKind::SlitheringStrangler, MonsterSpawnArgs::None));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monster_dispatch::{build_ai, SpawnPosition};

    /// Every encounter in the registry must build a non-empty
    /// `MonsterAi` for each of its monsters via `build_ai`. Pins the
    /// dispatch contract for the Phase 4 CLI surface.
    #[test]
    fn every_act1_encounter_builds_to_nonempty_ai() {
        assert!(!ACT1_ENCOUNTERS.is_empty());
        for enc in ACT1_ENCOUNTERS {
            assert!(
                !enc.monsters.is_empty(),
                "encounter '{}' has no monsters",
                enc.id
            );
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

    /// Bead `sts2-rs-ot78`: the encounter-side
    /// [`EncounterDef::pressure_threshold`] and the eval-side
    /// `sts2_core::monster_pressure_threshold` lookup must agree for
    /// any encounter that sets a custom threshold. Today that's just
    /// `VANTOM_BOSS` (threshold 5, keyed on `MonsterKind::Vantom`).
    #[test]
    fn vantom_pressure_threshold_matches_eval_side_lookup() {
        let vantom = ACT1_ENCOUNTERS
            .iter()
            .find(|e| e.id == "VANTOM_BOSS")
            .expect("VANTOM_BOSS encounter row");
        assert_eq!(vantom.pressure_threshold(), 5);
        assert_eq!(
            vantom.pressure_threshold(),
            sts2_core::monster_pressure_threshold(MonsterKind::Vantom),
        );
        // Defaults stay at 99 — guards against a future content edit
        // forgetting to wire the eval-side mirror.
        for enc in ACT1_ENCOUNTERS.iter().filter(|e| e.id != "VANTOM_BOSS") {
            assert_eq!(
                enc.pressure_threshold(),
                99,
                "encounter '{}' grew a custom pressure_threshold without a sts2_core mirror update",
                enc.id,
            );
        }
    }

    /// Count check pinned to the Python pin (4 weak + 12 normal + 3
    /// elite + 3 boss = 22). Guards against accidental row drift.
    #[test]
    fn act1_encounter_counts_match_python_pin() {
        let weak = ACT1_ENCOUNTERS
            .iter()
            .filter(|e| e.tier == EncounterTier::Weak)
            .count();
        let normal = ACT1_ENCOUNTERS
            .iter()
            .filter(|e| e.tier == EncounterTier::Normal)
            .count();
        let elite = ACT1_ENCOUNTERS
            .iter()
            .filter(|e| e.tier == EncounterTier::Elite)
            .count();
        let boss = ACT1_ENCOUNTERS
            .iter()
            .filter(|e| e.tier == EncounterTier::Boss)
            .count();
        assert_eq!(weak, 4, "weak count drift");
        assert_eq!(normal, 12, "normal count drift");
        assert_eq!(elite, 3, "elite count drift");
        assert_eq!(boss, 3, "boss count drift");
        assert_eq!(ACT1_ENCOUNTERS.len(), 22);
    }

    /// Ids must be unique (`UPPER_SNAKE`) — guards against copy/paste.
    #[test]
    fn act1_encounter_ids_are_unique() {
        let mut seen: Vec<&str> = Vec::new();
        for enc in ACT1_ENCOUNTERS {
            assert!(
                !seen.contains(&enc.id),
                "duplicate encounter id: {}",
                enc.id
            );
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

    #[test]
    fn find_is_case_insensitive() {
        assert!(find("MAWLER_NORMAL").is_some());
        assert!(find("mawler_normal").is_some());
        assert!(find("Mawler_Normal").is_some());
        assert!(find("not_a_real_encounter").is_none());
    }

    // ─── Spawn-time randomizers (bead `sts2-rs-6inu`) ────────────────

    use std::collections::HashSet;

    /// `OVERGROWTH_CRAWLERS` composition: C#
    /// `OvergrowthCrawlers.GenerateMonsters()` returns exactly
    /// `[ShrinkerBeetle, FuzzyWurmCrawler]`
    /// (`MegaCrit.Sts2.Core.Models.Encounters/OvergrowthCrawlers.cs:24-30`).
    /// The prior Rust slice `[FuzzyWurmCrawler, Nibbit]` was a Python-pin
    /// fabrication — Nibbit is not in this encounter and ShrinkerBeetle was
    /// missing, which aborted the parity harness at init (HARNESS_INIT) and
    /// dropped the Shrinker's SHRINK debuff. q6bix coverage-sweep fix.
    #[test]
    fn overgrowth_crawlers_normal_matches_csharp_roster() {
        let enc = find("OVERGROWTH_CRAWLERS").expect("encounter registered");
        let roster: Vec<MonsterKind> = enc.monsters.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            roster,
            vec![MonsterKind::ShrinkerBeetle, MonsterKind::FuzzyWurmCrawler],
            "OvergrowthCrawlers.GenerateMonsters() = [ShrinkerBeetle, FuzzyWurmCrawler]"
        );
    }

    /// `THE_KIN_BOSS` spawn order mirrors C# `TheKinBoss.GenerateMonsters()`:
    /// the dancing follower (slot1, `StartsWithDance`) is `monsters[0]`, the
    /// normal follower (slot2) is `monsters[1]`, and the KinPriest
    /// (leaderSlot) is LAST. The dancing follower opens on POWER_DANCE
    /// (`KinFollowerSlot::Third` stand-in); the prior Rust order put the
    /// priest first and the dancer last (sts2-rs-ztx5u).
    #[test]
    fn inklets_normal_middle_inklet_is_index_1() {
        use crate::monsters::act1_overgrowth::InkletSlot;
        let enc = find("INKLETS_NORMAL").expect("encounter registered");
        let slots: Vec<InkletSlot> = enc
            .monsters
            .iter()
            .map(|(_, args)| match args {
                MonsterSpawnArgs::Inklet { slot } => *slot,
                other => panic!("INKLETS_NORMAL monster carried non-Inklet args: {other:?}"),
            })
            .collect();
        // C# `InkletsNormal` sets MiddleInklet=true on index 1; the WHIRLWIND
        // opener (`InkletSlot::Second`) must be exactly that slot. Indices 0
        // and 2 are plain JAB openers (`First` / `Other`). Bead `sts2-rs-zdjjx`.
        assert_eq!(
            slots,
            vec![InkletSlot::First, InkletSlot::Second, InkletSlot::Other],
            "the WHIRLWIND (Second) inklet must sit at index 1, the C# MiddleInklet"
        );
    }

    #[test]
    fn the_kin_boss_matches_csharp_spawn_order() {
        use crate::monsters::act1_overgrowth::KinFollowerSlot;
        let enc = find("THE_KIN_BOSS").expect("encounter registered");
        let roster: Vec<MonsterKind> = enc.monsters.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            roster,
            vec![
                MonsterKind::KinFollower,
                MonsterKind::KinFollower,
                MonsterKind::KinPriest,
            ],
            "TheKinBoss.GenerateMonsters() = [follower(dance), follower, priest]"
        );
        // The first follower carries the StartsWithDance stand-in slot.
        assert!(
            matches!(
                enc.monsters[0].1,
                MonsterSpawnArgs::KinFollower {
                    slot: KinFollowerSlot::Third
                }
            ),
            "monsters[0] must be the dancing follower (slot1, StartsWithDance)"
        );
    }

    /// `SLIMES_WEAK` randomizer: always returns 3 enemies in C# slot
    /// order `[smallA, medium, smallB]` — the medium sits in the MIDDLE
    /// (per `SlimesWeak.GenerateMonsters`), the two distinct smalls flank
    /// it. Bead M3b roster-parity (was `sts2-rs-6inu`).
    #[test]
    fn vyh_6inu_slimes_weak_randomizer_shape_invariants() {
        let small_set: HashSet<MonsterKind> = [MonsterKind::LeafSlimeS, MonsterKind::TwigSlimeS]
            .into_iter()
            .collect();
        let medium_set: HashSet<MonsterKind> = [MonsterKind::LeafSlimeM, MonsterKind::TwigSlimeM]
            .into_iter()
            .collect();
        let mut seen_small_perms: HashSet<(MonsterKind, MonsterKind)> = HashSet::new();
        let mut seen_medium: HashSet<MonsterKind> = HashSet::new();
        for seed in 0..64u64 {
            let mut rng = sts2_core::dotnet_rng::Rng::new(seed);
            let roster = randomize_slimes_weak(&mut rng);
            assert_eq!(roster.len(), 3, "slimes_weak always returns 3");
            assert!(small_set.contains(&roster[0].0), "slot 0 is a small");
            assert!(medium_set.contains(&roster[1].0), "slot 1 is the medium");
            assert!(small_set.contains(&roster[2].0), "slot 2 is a small");
            assert_ne!(roster[0].0, roster[2].0, "the two smalls must be distinct");
            seen_small_perms.insert((roster[0].0, roster[2].0));
            seen_medium.insert(roster[1].0);
        }
        // Across 64 seeds we expect both small permutations and both
        // medium variants to surface — locks the randomizer against
        // a degenerate identity.
        assert_eq!(seen_small_perms.len(), 2);
        assert_eq!(seen_medium.len(), 2);
    }

    /// `RUBY_RAIDERS_NORMAL` randomizer: always returns 3 distinct
    /// raiders drawn from the 5-entry pool. Bead `sts2-rs-6inu`.
    #[test]
    fn vyh_6inu_ruby_raiders_randomizer_shape_invariants() {
        let pool: HashSet<MonsterKind> = [
            MonsterKind::AssassinRubyRaider,
            MonsterKind::AxeRubyRaider,
            MonsterKind::BruteRubyRaider,
            MonsterKind::CrossbowRubyRaider,
            MonsterKind::TrackerRubyRaider,
        ]
        .into_iter()
        .collect();
        let mut seen_picks: HashSet<MonsterKind> = HashSet::new();
        for seed in 0..64u64 {
            let mut rng = sts2_core::dotnet_rng::Rng::new(seed);
            let roster = randomize_ruby_raiders_normal(&mut rng);
            assert_eq!(roster.len(), 3);
            let ids: HashSet<MonsterKind> = roster.iter().map(|(k, _)| *k).collect();
            assert_eq!(ids.len(), 3, "3-of-5 sample must be distinct");
            assert!(ids.is_subset(&pool));
            seen_picks.extend(ids);
        }
        // Across 64 seeds every raider should surface at least once.
        assert_eq!(
            seen_picks.len(),
            5,
            "all 5 raiders should appear across 64 seeds; saw {seen_picks:?}",
        );
    }

    /// `EncounterDef::roster` routes through the randomizer when set
    /// and falls back to the canonical slice when not. Bead
    /// `sts2-rs-6inu`.
    #[test]
    fn vyh_6inu_encounterdef_roster_routes_correctly() {
        let mut rng = sts2_core::dotnet_rng::Rng::new(42_i32);
        let slimes = find("SLIMES_WEAK").expect("registered");
        let roster = slimes.roster(&mut rng);
        assert_eq!(roster.len(), 3);
        // Static fallback returns the canonical permutation regardless
        // of randomizer presence.
        let canonical = slimes.canonical_roster();
        assert_eq!(canonical.len(), 3);
        assert_eq!(canonical[0].0, MonsterKind::LeafSlimeS);
        assert_eq!(canonical[1].0, MonsterKind::TwigSlimeS);
        assert_eq!(canonical[2].0, MonsterKind::LeafSlimeM);

        // A non-randomized encounter ignores the rng and returns its
        // static slice.
        let mawler = find("MAWLER_NORMAL").expect("registered");
        assert!(mawler.randomize.is_none());
        let roster_static = mawler.roster(&mut rng);
        assert_eq!(roster_static.len(), 1);
        assert_eq!(roster_static[0].0, MonsterKind::Mawler);
    }

    /// `randomize_slimes_normal` returns the 4-monster roster
    /// `[TwigSlimeM, LeafSlimeM, small, small]` for every seed; the two small
    /// slots are a permutation of the two smalls. Kills the
    /// `-> Default::default()` mutant (which would return an empty roster) and
    /// the `flag` branch identity by requiring both small orderings to appear.
    #[test]
    fn slimes_normal_roster_shape_and_both_orders() {
        let mut seen_small_perms: HashSet<(MonsterKind, MonsterKind)> = HashSet::new();
        for seed in 0..64u64 {
            let mut rng = sts2_core::dotnet_rng::Rng::new(seed);
            let roster = randomize_slimes_normal(&mut rng);
            assert_eq!(roster.len(), 4, "slimes_normal always returns 4 monsters");
            assert_eq!(roster[0].0, MonsterKind::TwigSlimeM);
            assert_eq!(roster[1].0, MonsterKind::LeafSlimeM);
            assert_ne!(roster[2].0, roster[3].0, "the two smalls are distinct");
            seen_small_perms.insert((roster[2].0, roster[3].0));
        }
        assert_eq!(
            seen_small_perms.len(),
            2,
            "both small orderings must surface"
        );
    }

    /// `randomize_flyconid_normal` returns the 2-monster roster
    /// `[medium_slime, Flyconid]`; the medium is one of the two medium slimes.
    /// Kills the `-> Default::default()` mutant (empty roster).
    #[test]
    fn flyconid_normal_roster_shape() {
        let medium_set: HashSet<MonsterKind> = [MonsterKind::LeafSlimeM, MonsterKind::TwigSlimeM]
            .into_iter()
            .collect();
        let mut seen_medium: HashSet<MonsterKind> = HashSet::new();
        for seed in 0..64u64 {
            let mut rng = sts2_core::dotnet_rng::Rng::new(seed);
            let roster = randomize_flyconid_normal(&mut rng);
            assert_eq!(roster.len(), 2, "flyconid_normal always returns 2 monsters");
            assert!(
                medium_set.contains(&roster[0].0),
                "slot 0 is a medium slime"
            );
            assert_eq!(roster[1].0, MonsterKind::Flyconid, "slot 1 is the Flyconid");
            seen_medium.insert(roster[0].0);
        }
        assert_eq!(seen_medium.len(), 2, "both medium variants must surface");
    }

    /// `randomize_slithering_strangler_normal` has three branches selected by
    /// `Rng.NextItem` over `{SnappingJaxfruit, MediumSlime, SmallSlimes}`,
    /// each appending SlitheringStrangler last. Over a seed sweep all three
    /// secondary shapes must surface:
    ///  - arm 0 (Jaxfruit): roster `[SnappingJaxfruit, Strangler]`,
    ///  - arm 1 (MediumSlime): roster `[medium, Strangler]` with NO Jaxfruit,
    ///  - arm _ (SmallSlimes): roster `[small, small, Strangler]`.
    /// Requiring the Jaxfruit shape kills `delete match arm 0`; requiring the
    /// medium-slime length-2 shape kills `delete match arm 1` (deleting either
    /// arm collapses that input into the small-slimes `_` branch).
    #[test]
    fn slithering_strangler_normal_all_three_branches_surface() {
        let medium_set: HashSet<MonsterKind> = [MonsterKind::LeafSlimeM, MonsterKind::TwigSlimeM]
            .into_iter()
            .collect();
        let small_set: HashSet<MonsterKind> = [MonsterKind::LeafSlimeS, MonsterKind::TwigSlimeS]
            .into_iter()
            .collect();
        let mut saw_jaxfruit = false;
        let mut saw_medium = false;
        let mut saw_smalls = false;
        for seed in 0..256u64 {
            let mut rng = sts2_core::dotnet_rng::Rng::new(seed);
            let roster = randomize_slithering_strangler_normal(&mut rng);
            // Strangler is always appended last.
            assert_eq!(roster.last().unwrap().0, MonsterKind::SlitheringStrangler);
            let secondaries: Vec<MonsterKind> =
                roster[..roster.len() - 1].iter().map(|(k, _)| *k).collect();
            match secondaries.as_slice() {
                [MonsterKind::SnappingJaxfruit] => saw_jaxfruit = true,
                [m] if medium_set.contains(m) => saw_medium = true,
                [a, b] if small_set.contains(a) && small_set.contains(b) => saw_smalls = true,
                other => panic!("unexpected secondary roster: {other:?}"),
            }
        }
        assert!(saw_jaxfruit, "arm 0 (SnappingJaxfruit) must surface");
        assert!(saw_medium, "arm 1 (single MediumSlime) must surface");
        assert!(saw_smalls, "arm _ (two SmallSlimes) must surface");
    }
}
