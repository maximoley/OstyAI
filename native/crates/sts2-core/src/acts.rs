//! `ActConfig` POD + 3-act registry — bead `sts2-rs-7kptz` (P9.1.d).
//!
//! Mirrors `sts2-rl-agent@01c094d:sts2_env/map/acts.py`. Each act
//! carries its room-row count, weak/strong encounter rosters, elite
//! roster, boss roster, and event pool. The Python `ALL_ACTS` constant
//! is reflected here as [`all_acts`].
//!
//! ## Python → Rust act numbering
//!
//! Python uses 0-indexed acts (`ACT_0 = exordium`, `ACT_1 = Hive`,
//! `ACT_2 = Glory`); Rust's [`crate::runstate::RunState::act_index`]
//! is 1-indexed (defaults to `1` at run start, see bead
//! `sts2-rs-70anj.3.16`). The registry here keeps the 1-indexed
//! convention so `all_acts()[rs.act_index() as usize - 1]` is the
//! current act's config.
//!
//! ## Encounter / event IDs are still string-typed
//!
//! Python carries `boss_ids: list[str]`, `event_ids: list[str]`, etc.
//! The matching Rust enums (`EncounterId`, `EventId`, `BossId`) don't
//! exist at pin `01c094d`'s Rust port — see the P9 audit doc §2.1
//! "Data structures" row. To keep this bead atomic, lists stay as
//! `&'static [&'static str]`; a follow-up bead can swap them to
//! enum-typed once those identifier registries land.

/// Fallback `num_rooms` used by [`crate::runstate::RunState::generate_map`]
/// when `act_index` falls outside the populated `acts` Vec
/// (shouldn't happen at the current pin — `acts` is always
/// initialized to 3 entries — but the lookup uses
/// `.get(..)` to stay panic-free). Matches Act 1's row count
/// (Python pin `acts.py::ACT_0.num_rooms = 15`). Bead `sts2-rs-4aunm`.
pub const DEFAULT_ACT_NUM_ROOMS: i32 = 15;

/// Per-act configuration. Mirrors
/// `sts2-rl-agent@01c094d:sts2_env/map/acts.py::ActConfig`.
///
/// Lists are held as static slices so the registry constants in this
/// module can be `const fn` builders rather than allocating `Vec`s at
/// startup. The `events_visited` counter is per-run mutable state and
/// is the only non-`Copy` field; a fresh `ActConfig` from
/// [`act_1`] / [`act_2`] / [`act_3`] starts at 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActConfig {
    /// 1-indexed act number (1 = Exordium, 2 = Hive, 3 = Glory).
    /// Python's `ACT_0/1/2.act_index` is 0-indexed; the +1 happens
    /// here so callers can match against
    /// [`crate::runstate::RunState::act_index`] directly.
    pub act_index: u8,
    /// Python `num_rooms` — number of map rows for the act.
    pub num_rooms: u8,
    /// Python `num_weak_encounters` — how many of the act's weak
    /// encounter pool show up at the start of the map. Default 3
    /// (Acts 1/3); Hive/Glory each ship with 2.
    pub num_weak_encounters: u8,
    /// Python `boss_ids` — act boss pool. Map-generation picks one
    /// boss out of this pool per run.
    pub boss_ids: &'static [&'static str],
    /// Python `elite_ids` — pool of elite encounters spawned in
    /// elite-flagged rooms.
    pub elite_ids: &'static [&'static str],
    /// Python `weak_encounter_ids` — pool of "weak" (early-act)
    /// encounters in monster rooms before the elite band.
    pub weak_encounter_ids: &'static [&'static str],
    /// Python `strong_encounter_ids` — pool of "strong" (post-elite)
    /// encounters in monster rooms after the elite band.
    pub strong_encounter_ids: &'static [&'static str],
    /// Python `event_ids` — pool of events the run can roll in
    /// event rooms.
    pub event_ids: &'static [&'static str],
    /// Python `events_visited` — per-run counter incremented as the
    /// player consumes event rooms. Used by future pity-style
    /// generators (Python's `_get_next_event_id` pops visited IDs
    /// from the candidate pool). Starts at 0; the registry builders
    /// always initialize it to 0.
    pub events_visited: u32,
    /// Per-run shuffled mirror of [`Self::event_ids`]. Populated by
    /// [`crate::run_manager::RunManager::initialize_run`]'s
    /// `_generate_rooms` step (Python `run_state.py:1481-1487`) which
    /// shuffles via the `rng.up_front` stream. Empty on a fresh
    /// `ActConfig` (registry builders init to `Vec::new()`); the
    /// shuffle step populates it from `event_ids` on first call and
    /// is guarded by `RunState::rooms_generated` so subsequent
    /// `initialize_run` invocations are no-ops.
    ///
    /// Kept distinct from `event_ids` so the static-slice registry
    /// stays `const fn`-buildable; the runtime shuffle is the only
    /// per-run-mutable view consumers (the event-room handler, once
    /// it lands) should read.
    ///
    /// Bead `sts2-rs-wk1t6` (P9.2.n follow-up).
    pub event_ids_shuffled: Vec<&'static str>,
}

// ── Act 1 (Python ACT_0 = exordium) ──────────────────────────────────

const ACT_1_BOSSES: &[&str] = &["TheLich"];
const ACT_1_ELITES: &[&str] = &["SentryAndSentry", "GremlinNob", "BookOfStabbing"];
const ACT_1_WEAK: &[&str] = &[
    "TwoLouses",
    "ThreeJawWorms",
    "SmallSlimes",
    "Cultist",
    "GremlinGang",
];
const ACT_1_STRONG: &[&str] = &[
    "BlueSlaver",
    "RedSlaver",
    "FungiBeast",
    "LooterGroup",
    "ExordiumWildlife",
    "LotOfSlimes",
];
const ACT_1_EVENTS: &[&str] = &[
    "AbyssalBaths",
    "Amalgamator",
    "BattlewornDummy",
    "BrainLeech",
    "Bugslayer",
    "ByrdonisNest",
    "ColorfulPhilosophers",
    "ColossalFlower",
    "Darv",
    "DenseVegetation",
    "DoorsOfLightAndDark",
    "DrowningBeacon",
    "GraveOfTheForgotten",
    "HungryForMushrooms",
    "InfestedAutomaton",
    "LostWisp",
    "Nonupeipe",
    "Orobas",
    "Pael",
    "PunchOff",
    "Reflections",
    "RoomFullOfCheese",
    "RoundTeaParty",
    "SapphireSeed",
    "SelfHelpBook",
    "SpiritGrafter",
    "SunkenStatue",
    "SunkenTreasury",
    "TabletOfTruth",
    "Tanx",
    "TeaMaster",
    "Tezcatara",
    "TheLegendsWereTrue",
    "ThisOrThat",
    "TinkerTime",
    "TrashHeap",
    "Trial",
    "UnrestSite",
    "Vakuu",
    "Wellspring",
    "WoodCarvings",
    "ZenWeaver",
];

/// Act 1 (Exordium) — Python `ACT_0`. 15 rooms, default 3 weak
/// encounters at map-start.
#[must_use]
pub const fn act_1() -> ActConfig {
    ActConfig {
        act_index: 1,
        num_rooms: 15,
        num_weak_encounters: 3,
        boss_ids: ACT_1_BOSSES,
        elite_ids: ACT_1_ELITES,
        weak_encounter_ids: ACT_1_WEAK,
        strong_encounter_ids: ACT_1_STRONG,
        event_ids: ACT_1_EVENTS,
        events_visited: 0,
        event_ids_shuffled: Vec::new(),
    }
}

// ── Act 2 (Python ACT_1 = Hive) ──────────────────────────────────────

const ACT_2_BOSSES: &[&str] = &["TheCollector", "Automaton", "Champ"];
const ACT_2_ELITES: &[&str] = &["TaskMaster", "SphericGuardian", "Snecko"];
const ACT_2_WEAK: &[&str] = &["SnakePlant", "Centurion", "ThreeByrds"];
const ACT_2_STRONG: &[&str] = &["SlaverGroup", "BookOfStabbing", "MushroomGroup"];
const ACT_2_EVENTS: &[&str] = &[
    "AbyssalBaths",
    "Amalgamator",
    "AromaOfChaos",
    "BattlewornDummy",
    "Bugslayer",
    "ByrdonisNest",
    "ColorfulPhilosophers",
    "ColossalFlower",
    "CrystalSphere",
    "Darv",
    "DenseVegetation",
    "DollRoom",
    "DoorsOfLightAndDark",
    "DrowningBeacon",
    "EndlessConveyor",
    "FakeMerchant",
    "FieldOfManSizedHoles",
    "GraveOfTheForgotten",
    "HungryForMushrooms",
    "InfestedAutomaton",
    "JungleMazeAdventure",
    "LostWisp",
    "LuminousChoir",
    "MorphicGrove",
    "Nonupeipe",
    "Orobas",
    "Pael",
    "PotionCourier",
    "PunchOff",
    "RanwidTheElder",
    "Reflections",
    "RelicTrader",
    "RoundTeaParty",
    "SapphireSeed",
    "SelfHelpBook",
    "SlipperyBridge",
    "SpiralingWhirlpool",
    "SpiritGrafter",
    "StoneOfAllTime",
    "SunkenStatue",
    "SunkenTreasury",
    "Symbiote",
    "TabletOfTruth",
    "Tanx",
    "Tezcatara",
    "TheFutureOfPotions",
    "TheLanternKey",
    "ThisOrThat",
    "TinkerTime",
    "TrashHeap",
    "Trial",
    "UnrestSite",
    "Vakuu",
    "WaterloggedScriptorium",
    "WelcomeToWongos",
    "Wellspring",
    "WhisperingHollow",
    "WoodCarvings",
    "ZenWeaver",
];

/// Act 2 (Hive) — Python `ACT_1`. 14 rooms (C# `Hive.BaseNumberOfRooms = 14`),
/// 2 weak encounters at map-start.
#[must_use]
pub const fn act_2() -> ActConfig {
    ActConfig {
        act_index: 2,
        num_rooms: 14,
        num_weak_encounters: 2,
        boss_ids: ACT_2_BOSSES,
        elite_ids: ACT_2_ELITES,
        weak_encounter_ids: ACT_2_WEAK,
        strong_encounter_ids: ACT_2_STRONG,
        event_ids: ACT_2_EVENTS,
        events_visited: 0,
        event_ids_shuffled: Vec::new(),
    }
}

// ── Act 3 (Python ACT_2 = Glory) ─────────────────────────────────────

const ACT_3_BOSSES: &[&str] = &["AwakenedOne", "TimeEater", "DonuAndDeca"];
const ACT_3_ELITES: &[&str] = &["GiantHead", "Nemesis", "Reptomancer"];
const ACT_3_WEAK: &[&str] = &["Darkling", "OrbWalker"];
const ACT_3_STRONG: &[&str] = &["WrithingMass", "Transient", "Maw"];
const ACT_3_EVENTS: &[&str] = &[
    "AbyssalBaths",
    "Amalgamator",
    "AromaOfChaos",
    "BattlewornDummy",
    "Bugslayer",
    "ByrdonisNest",
    "ColorfulPhilosophers",
    "ColossalFlower",
    "CrystalSphere",
    "Darv",
    "DenseVegetation",
    "DoorsOfLightAndDark",
    "DrowningBeacon",
    "EndlessConveyor",
    "FakeMerchant",
    "FieldOfManSizedHoles",
    "GraveOfTheForgotten",
    "HungryForMushrooms",
    "InfestedAutomaton",
    "JungleMazeAdventure",
    "LostWisp",
    "LuminousChoir",
    "MorphicGrove",
    "Nonupeipe",
    "Orobas",
    "Pael",
    "PotionCourier",
    "PunchOff",
    "RanwidTheElder",
    "Reflections",
    "RelicTrader",
    "RoundTeaParty",
    "SapphireSeed",
    "SelfHelpBook",
    "SlipperyBridge",
    "SpiralingWhirlpool",
    "SpiritGrafter",
    "SunkenStatue",
    "SunkenTreasury",
    "Symbiote",
    "TabletOfTruth",
    "Tanx",
    "Tezcatara",
    "TheFutureOfPotions",
    "TheLanternKey",
    "ThisOrThat",
    "TinkerTime",
    "TrashHeap",
    "Trial",
    "UnrestSite",
    "Vakuu",
    "WaterloggedScriptorium",
    "Wellspring",
    "WhisperingHollow",
    "WoodCarvings",
    "ZenWeaver",
];

/// Act 3 (Glory) — Python `ACT_2`. 13 rooms (C# `Glory.BaseNumberOfRooms = 13`),
/// 2 weak encounters at map-start.
#[must_use]
pub const fn act_3() -> ActConfig {
    ActConfig {
        act_index: 3,
        num_rooms: 13,
        num_weak_encounters: 2,
        boss_ids: ACT_3_BOSSES,
        elite_ids: ACT_3_ELITES,
        weak_encounter_ids: ACT_3_WEAK,
        strong_encounter_ids: ACT_3_STRONG,
        event_ids: ACT_3_EVENTS,
        events_visited: 0,
        event_ids_shuffled: Vec::new(),
    }
}

/// Return the 3 acts in 1-indexed order. Mirrors Python `ALL_ACTS`
/// but skips the spread to a real `Vec` so callers can opt into the
/// allocation only when they want a mutable per-run snapshot.
#[must_use]
pub fn all_acts() -> [ActConfig; 3] {
    [act_1(), act_2(), act_3()]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke: each act's `act_index` matches its 1-indexed slot.
    #[test]
    fn act_indices_match_slot() {
        let acts = all_acts();
        assert_eq!(acts[0].act_index, 1);
        assert_eq!(acts[1].act_index, 2);
        assert_eq!(acts[2].act_index, 3);
    }

    /// Smoke: the Python `num_rooms` constants (15/14/13) round-trip.
    #[test]
    fn act_num_rooms_match_python_pin() {
        assert_eq!(act_1().num_rooms, 15);
        assert_eq!(act_2().num_rooms, 14);
        assert_eq!(act_3().num_rooms, 13);
    }

    /// Smoke: the Python `num_weak_encounters` constants round-trip
    /// (Act 1 = 3; Acts 2/3 = 2).
    #[test]
    fn act_num_weak_encounters_match_python_pin() {
        assert_eq!(act_1().num_weak_encounters, 3);
        assert_eq!(act_2().num_weak_encounters, 2);
        assert_eq!(act_3().num_weak_encounters, 2);
    }

    /// Smoke: boss rosters land per-act.
    #[test]
    fn act_boss_rosters_match_python_pin() {
        assert_eq!(act_1().boss_ids, &["TheLich"]);
        assert_eq!(act_2().boss_ids, &["TheCollector", "Automaton", "Champ"]);
        assert_eq!(
            act_3().boss_ids,
            &["AwakenedOne", "TimeEater", "DonuAndDeca"]
        );
    }

    /// Smoke: events_visited starts at 0 on every fresh `act_n()`.
    #[test]
    fn events_visited_starts_at_zero() {
        for cfg in all_acts() {
            assert_eq!(cfg.events_visited, 0);
        }
    }
}
