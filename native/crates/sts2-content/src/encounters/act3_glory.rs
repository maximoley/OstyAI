//! Act 3 (Glory) encounter registry — bead `sts2-rs-6ynt.10`.
//!
//! Mirrors `sts2_env/encounters/act3.py` (pin `01c094d`):
//!
//! - 3 weak encounters
//! - 9 normal encounters
//! - 3 elite encounters
//! - 3 boss encounters (Doormaker, Queen, Test Subject)
//!
//! The Python pin modeled `KNIGHTS_ELITE` as a random 2-of-3 sub-selection,
//! but C# `KnightsElite.GenerateMonsters` is a deterministic fixed-3 roster
//! (Flail / Spectral / Magi). C# is authoritative, so the encounter now uses
//! a static fixed roster with no randomizer. Bead `sts2-rs-ybtmu`.

use crate::encounters::{EncounterDef, EncounterRoster, EncounterTier};
use crate::monster_dispatch::MonsterSpawnArgs;
use sts2_core::MonsterKind;

/// Every Act 3 encounter, in the canonical Python ordering
/// (weak → normal → elite → boss).
pub static ACT3_ENCOUNTERS: &[EncounterDef] = &[
    // ── Weak ──────────────────────────────────────────────────────
    EncounterDef {
        id: "DEVOTED_SCULPTOR_WEAK",
        display: "Devoted Sculptor",
        monsters: &[(MonsterKind::DevotedSculptor, MonsterSpawnArgs::None)],
        tier: EncounterTier::Weak,
        randomize: None,
    },
    // Port-pin: C# `ScrollsOfBitingWeak.cs::GenerateMonsters` rotates the
    // 3 scrolls' starter moves — `idx = Rng.NextInt(3)`, then `idx`,
    // `(idx+1)%3`, `(idx+2)%3`. `ScrollOfBiting.cs` maps the index to the
    // opener (0=CHOMP, 1=CHEW, 2=MORE_TEETH). The gameplay path draws the base
    // via `randomize_scrolls_of_biting_weak`; the static `monsters` slice keeps
    // the `idx=0` fallback (0/1/2) for no-RNG callers. Bead `sts2-rs-ybtmu`.
    EncounterDef {
        id: "SCROLLS_OF_BITING_WEAK",
        display: "Three Scrolls of Biting",
        monsters: &[
            (
                MonsterKind::ScrollOfBiting,
                MonsterSpawnArgs::ScrollOfBiting { starter_idx: 0 },
            ),
            (
                MonsterKind::ScrollOfBiting,
                MonsterSpawnArgs::ScrollOfBiting { starter_idx: 1 },
            ),
            (
                MonsterKind::ScrollOfBiting,
                MonsterSpawnArgs::ScrollOfBiting { starter_idx: 2 },
            ),
        ],
        tier: EncounterTier::Weak,
        randomize: Some(randomize_scrolls_of_biting_weak),
    },
    EncounterDef {
        id: "TURRET_OPERATOR_WEAK",
        display: "Turret Operator",
        // C# `TurretOperatorWeak.cs:15-16`: roster is LivingShield FIRST,
        // then TurretOperator. Rust previously spawned only the
        // TurretOperator, dropping the LivingShield and shifting the HP-roll
        // RNG for the encounter. Bead `sts2-rs-34y0l`.
        monsters: &[
            (MonsterKind::LivingShield, MonsterSpawnArgs::None),
            (MonsterKind::TurretOperator, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Weak,
        randomize: None,
    },
    // ── Normal ────────────────────────────────────────────────────
    // `…wb8lk.11`: v0.107.1 `AxebotsNormal.cs:19-22` spawns a SINGLE Axebot
    // (front slot); v0.103.2 spawned two (front + back). The Axebot's HP and
    // moveset were buffed correspondingly (see `monsters.rs` / `act3_glory`).
    EncounterDef {
        id: "AXEBOTS_NORMAL",
        display: "Axebot",
        monsters: &[(MonsterKind::Axebot, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "CONSTRUCT_MENAGERIE_NORMAL",
        display: "Construct Menagerie (Punch Construct + Cubex x2)",
        // Mirrors `sts2_env/encounters/act3.py:70-77`
        // (`setup_construct_menagerie_normal`). Wired by `sts2-rs-551m`.
        monsters: &[
            (MonsterKind::PunchConstruct, MonsterSpawnArgs::None),
            (MonsterKind::CubexConstruct, MonsterSpawnArgs::None),
            (MonsterKind::CubexConstruct, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "FABRICATOR_NORMAL",
        display: "Fabricator",
        monsters: &[(MonsterKind::Fabricator, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "FROG_KNIGHT_NORMAL",
        display: "Frog Knight",
        monsters: &[(MonsterKind::FrogKnight, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "GLOBE_HEAD_NORMAL",
        display: "Globe Head",
        monsters: &[(MonsterKind::GlobeHead, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "OWL_MAGISTRATE_NORMAL",
        display: "Owl Magistrate",
        monsters: &[(MonsterKind::OwlMagistrate, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    // Port-pin: C# `ScrollsOfBitingNormal.cs::GenerateMonsters` rotates
    // the 4 scrolls' starter moves — `idx = Rng.NextInt(3)`, then `idx`,
    // `(idx+1)%3`, `(idx+2)%3`, with the 4th scroll fixed at idx 2.
    // `ScrollOfBiting.cs` maps the index to the opener (0=CHOMP, 1=CHEW,
    // 2=MORE_TEETH). The gameplay path draws the base via
    // `randomize_scrolls_of_biting_normal`; the static `monsters` slice keeps
    // the `idx=0` fallback (0/1/2 + fixed 2) for no-RNG callers. Bead
    // `sts2-rs-ybtmu`.
    EncounterDef {
        id: "SCROLLS_OF_BITING_NORMAL",
        display: "Four Scrolls of Biting",
        monsters: &[
            (
                MonsterKind::ScrollOfBiting,
                MonsterSpawnArgs::ScrollOfBiting { starter_idx: 0 },
            ),
            (
                MonsterKind::ScrollOfBiting,
                MonsterSpawnArgs::ScrollOfBiting { starter_idx: 1 },
            ),
            (
                MonsterKind::ScrollOfBiting,
                MonsterSpawnArgs::ScrollOfBiting { starter_idx: 2 },
            ),
            (
                MonsterKind::ScrollOfBiting,
                MonsterSpawnArgs::ScrollOfBiting { starter_idx: 2 },
            ),
        ],
        tier: EncounterTier::Normal,
        randomize: Some(randomize_scrolls_of_biting_normal),
    },
    EncounterDef {
        id: "SLIMED_BERSERKER_NORMAL",
        display: "Slimed Berserker",
        monsters: &[(MonsterKind::SlimedBerserker, MonsterSpawnArgs::None)],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    EncounterDef {
        id: "THE_LOST_AND_FORGOTTEN_NORMAL",
        display: "The Lost + The Forgotten",
        monsters: &[
            (MonsterKind::TheLost, MonsterSpawnArgs::None),
            (MonsterKind::TheForgotten, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Normal,
        randomize: None,
    },
    // ── Elite ─────────────────────────────────────────────────────
    // `KNIGHTS_ELITE`: C# `KnightsElite.GenerateMonsters` (`KnightsElite.cs:39-47`)
    // spawns a FIXED three-knight roster — FlailKnight ("first"),
    // SpectralKnight ("second"), MagiKnight ("third") — with NO RNG. The prior
    // Rust port (`rng.sample(knight_creators, 2)` from the Python pin
    // `act3.py:134-139`, bead `sts2-rs-6inu`) was a fabrication: it dropped one
    // knight and randomized the pair. C# is authoritative → fixed 3 in source
    // order, `randomize: None`. Bead `sts2-rs-ybtmu`.
    EncounterDef {
        id: "KNIGHTS_ELITE",
        display: "Knights (Flail + Spectral + Magi)",
        monsters: &[
            (MonsterKind::FlailKnight, MonsterSpawnArgs::None),
            (MonsterKind::SpectralKnight, MonsterSpawnArgs::None),
            (MonsterKind::MagiKnight, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Elite,
        randomize: None,
    },
    EncounterDef {
        id: "MECHA_KNIGHT_ELITE",
        display: "Mecha Knight",
        monsters: &[(MonsterKind::MechaKnight, MonsterSpawnArgs::None)],
        tier: EncounterTier::Elite,
        randomize: None,
    },
    EncounterDef {
        id: "SOUL_NEXUS_ELITE",
        display: "Soul Nexus",
        monsters: &[(MonsterKind::SoulNexus, MonsterSpawnArgs::None)],
        tier: EncounterTier::Elite,
        randomize: None,
    },
    // ── Boss ──────────────────────────────────────────────────────
    // C# `AeonglassBoss.GenerateMonsters` (v0.107.1) returns a SINGLE
    // Aeonglass (`AeonglassBoss.cs:35-38`), `RoomType.Boss`. Replaces the
    // removed Doormaker. Bead `sts2-rs-v0107-content-port-wb8lk.2`.
    EncounterDef {
        id: "AEONGLASS_BOSS",
        display: "Aeonglass",
        monsters: &[(MonsterKind::Aeonglass, MonsterSpawnArgs::None)],
        tier: EncounterTier::Boss,
        randomize: None,
    },
    EncounterDef {
        id: "QUEEN_BOSS",
        // C# `QueenBoss.GenerateMonsters` is a FIXED pair
        // `[TorchHeadAmalgam "amalgam", Queen "queen"]` — the Queen fights
        // beside her amalgam escort (her `HasAmalgamDied` listener keys off
        // the amalgam's death, see `act3_glory::create_queen_ai`). The prior
        // Rust table spawned the Queen alone, dropping the amalgam + its
        // niche HP roll. Bead M3b roster-parity (roster-composition gap).
        display: "Queen (+ Torch Head Amalgam)",
        monsters: &[
            (MonsterKind::TorchHeadAmalgam, MonsterSpawnArgs::None),
            (MonsterKind::Queen, MonsterSpawnArgs::None),
        ],
        tier: EncounterTier::Boss,
        randomize: None,
    },
    EncounterDef {
        id: "TEST_SUBJECT_BOSS",
        display: "Test Subject",
        monsters: &[(MonsterKind::TestSubject, MonsterSpawnArgs::None)],
        tier: EncounterTier::Boss,
        randomize: None,
    },
];

/// Lookup helper: resolve an Act-3 encounter id (`UPPER_SNAKE`).
/// Case-insensitive.
#[must_use]
pub fn find(id: &str) -> Option<&'static EncounterDef> {
    ACT3_ENCOUNTERS
        .iter()
        .find(|e| e.id.eq_ignore_ascii_case(id))
}

// `KNIGHTS_ELITE` previously had a `randomize_knights_elite` spawn-time
// randomizer (2-of-3 sample, bead `sts2-rs-6inu`). Removed under
// `sts2-rs-ybtmu`: C# `KnightsElite.GenerateMonsters` is a deterministic
// fixed-3 roster, so the encounter now uses the static `monsters` slice with
// `randomize: None` (see the table entry above).

// ---------------------------------------------------------------------------
// Scrolls-of-Biting starter-move rotation (bead `sts2-rs-ybtmu`)
// ---------------------------------------------------------------------------

/// `ScrollsOfBitingWeak.GenerateMonsters` (`ScrollsOfBitingWeak.cs:18-32`):
/// draws `idx = base.Rng.NextInt(3)` and assigns the three scrolls'
/// `StarterMoveIdx` to `idx`, `(idx+1)%3`, `(idx+2)%3`. `ScrollOfBiting`
/// maps the index to the opener (0=CHOMP, 1=CHEW, 2=MORE_TEETH).
///
/// The static `monsters` slice keeps the `idx=0` fallback (0/1/2) for no-RNG
/// callers (`sts2-cli list encounters`, the parity coverage harness's
/// `build_scenario_state_impl`, which does not yet call `roster()` — tracked
/// by the harness-infra bead); the gameplay path (`run_callbacks::enter_room`)
/// invokes this randomizer with the per-encounter RNG, drawing exactly one
/// `NextInt(3)` as C# does.
pub fn randomize_scrolls_of_biting_weak(rng: &mut sts2_core::dotnet_rng::Rng) -> EncounterRoster {
    let idx = rng.next_int_exclusive(0, 3);
    let mut out = EncounterRoster::new();
    for offset in 0..3 {
        out.push((
            MonsterKind::ScrollOfBiting,
            MonsterSpawnArgs::ScrollOfBiting {
                starter_idx: ((idx + offset) % 3) as u8,
            },
        ));
    }
    out
}

/// `ScrollsOfBitingNormal.GenerateMonsters` (`ScrollsOfBitingNormal.cs:16-33`):
/// like the weak variant — `idx = base.Rng.NextInt(3)` then `idx`,
/// `(idx+1)%3`, `(idx+2)%3` — but with a FOURTH scroll fixed at
/// `StarterMoveIdx = 2`. See `randomize_scrolls_of_biting_weak` for the
/// static-fallback vs gameplay-randomizer split.
pub fn randomize_scrolls_of_biting_normal(rng: &mut sts2_core::dotnet_rng::Rng) -> EncounterRoster {
    let idx = rng.next_int_exclusive(0, 3);
    let mut out = EncounterRoster::new();
    for offset in 0..3 {
        out.push((
            MonsterKind::ScrollOfBiting,
            MonsterSpawnArgs::ScrollOfBiting {
                starter_idx: ((idx + offset) % 3) as u8,
            },
        ));
    }
    out.push((
        MonsterKind::ScrollOfBiting,
        MonsterSpawnArgs::ScrollOfBiting { starter_idx: 2 },
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monster_dispatch::{build_ai, SpawnPosition};

    #[test]
    fn every_act3_encounter_builds_to_nonempty_ai() {
        assert!(!ACT3_ENCOUNTERS.is_empty());
        for enc in ACT3_ENCOUNTERS {
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

    /// Pin counts to the Python pin (3 weak + 9 normal + 3 elite + 3
    /// boss = 18).
    #[test]
    fn act3_encounter_counts_match_python_pin() {
        let weak = ACT3_ENCOUNTERS
            .iter()
            .filter(|e| e.tier == EncounterTier::Weak)
            .count();
        let normal = ACT3_ENCOUNTERS
            .iter()
            .filter(|e| e.tier == EncounterTier::Normal)
            .count();
        let elite = ACT3_ENCOUNTERS
            .iter()
            .filter(|e| e.tier == EncounterTier::Elite)
            .count();
        let boss = ACT3_ENCOUNTERS
            .iter()
            .filter(|e| e.tier == EncounterTier::Boss)
            .count();
        assert_eq!(weak, 3, "weak count drift");
        assert_eq!(normal, 9, "normal count drift");
        assert_eq!(elite, 3, "elite count drift");
        assert_eq!(boss, 3, "boss count drift");
        assert_eq!(ACT3_ENCOUNTERS.len(), 18);
    }

    #[test]
    fn act3_encounter_ids_are_unique() {
        let mut seen: Vec<&str> = Vec::new();
        for enc in ACT3_ENCOUNTERS {
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

    /// `KNIGHTS_ELITE` is a deterministic fixed-3 roster (Flail / Spectral /
    /// Magi, C# source order) with no randomizer. Bead `sts2-rs-ybtmu`.
    #[test]
    fn ybtmu_knights_elite_fixed_three_roster() {
        let knights = find("KNIGHTS_ELITE").expect("registered");
        assert!(
            knights.randomize.is_none(),
            "KNIGHTS_ELITE must not randomize"
        );
        let ids: Vec<MonsterKind> = knights.monsters.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            ids,
            vec![
                MonsterKind::FlailKnight,
                MonsterKind::SpectralKnight,
                MonsterKind::MagiKnight,
            ],
            "fixed roster must match C# GenerateMonsters order",
        );
    }

    fn scroll_idx(args: &MonsterSpawnArgs) -> u8 {
        match args {
            MonsterSpawnArgs::ScrollOfBiting { starter_idx } => *starter_idx,
            other => panic!("expected ScrollOfBiting args, got {other:?}"),
        }
    }

    /// `SCROLLS_OF_BITING_WEAK` rotation: 3 scrolls at `idx`, `(idx+1)%3`,
    /// `(idx+2)%3` for a single `NextInt(3)` draw. Bead `sts2-rs-ybtmu`.
    #[test]
    fn ybtmu_scrolls_weak_rotation_invariants() {
        let mut seen_bases: std::collections::HashSet<u8> = std::collections::HashSet::new();
        for seed in 0..64_i32 {
            let mut rng = sts2_core::dotnet_rng::Rng::new(seed);
            let roster = randomize_scrolls_of_biting_weak(&mut rng);
            assert_eq!(roster.len(), 3);
            let idxs: Vec<u8> = roster.iter().map(|(_, a)| scroll_idx(a)).collect();
            assert!(idxs[0] < 3, "base idx in range");
            assert_eq!(idxs[1], (idxs[0] + 1) % 3, "second scroll = (idx+1)%3");
            assert_eq!(idxs[2], (idxs[0] + 2) % 3, "third scroll = (idx+2)%3");
            assert!(
                roster
                    .iter()
                    .all(|(k, _)| *k == MonsterKind::ScrollOfBiting),
                "all scrolls",
            );
            seen_bases.insert(idxs[0]);
        }
        assert_eq!(seen_bases.len(), 3, "all 3 bases appear across 64 seeds");
    }

    /// `SCROLLS_OF_BITING_NORMAL` rotation: 3 rotated scrolls + a 4th fixed at
    /// idx 2. Bead `sts2-rs-ybtmu`.
    #[test]
    fn ybtmu_scrolls_normal_rotation_invariants() {
        for seed in 0..64_i32 {
            let mut rng = sts2_core::dotnet_rng::Rng::new(seed);
            let roster = randomize_scrolls_of_biting_normal(&mut rng);
            assert_eq!(roster.len(), 4);
            let idxs: Vec<u8> = roster.iter().map(|(_, a)| scroll_idx(a)).collect();
            assert!(idxs[0] < 3);
            assert_eq!(idxs[1], (idxs[0] + 1) % 3);
            assert_eq!(idxs[2], (idxs[0] + 2) % 3);
            assert_eq!(idxs[3], 2, "4th scroll fixed at idx 2");
        }
    }

    /// Both SCROLLS rows route their randomizer through `EncounterDef::roster`
    /// and consume exactly one encounter-RNG int (the `NextInt(3)` base),
    /// matching C#. The static fallback stays the `idx=0` permutation.
    #[test]
    fn ybtmu_scrolls_encounterdef_roster_routes() {
        for id in ["SCROLLS_OF_BITING_WEAK", "SCROLLS_OF_BITING_NORMAL"] {
            let enc = find(id).expect("registered");
            assert!(enc.randomize.is_some(), "{id} must randomize");
            let mut rng = sts2_core::dotnet_rng::Rng::new(11_i32);
            let roster = enc.roster(&mut rng);
            assert_eq!(roster.len(), enc.monsters.len());
            // Static fallback is the idx=0 permutation (0/1/2[/2]).
            let static_idxs: Vec<u8> = enc.monsters.iter().map(|(_, a)| scroll_idx(a)).collect();
            assert_eq!(static_idxs[0], 0, "{id} static fallback base 0");
        }
    }
}
