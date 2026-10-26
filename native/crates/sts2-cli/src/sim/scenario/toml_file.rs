//! `sts2-rs-g2ex` (cliext D1): TOML scenario file (`--scenario <path>`).
//!
//! Deserialize a TOML scenario description that bundles player class +
//! deck + relics + potions + monsters into a single declarative file.
//! CLI flags take precedence over file values (CLI > file > defaults);
//! the merge happens in `main::run_combat` after parsing.
//!
//! ## File format
//!
//! ```toml
//! schema = 1                    # forward-compat hook
//!
//! [player]
//! class = "Ironclad"            # optional — defaults to CLI default
//! hp = 80                       # optional
//! deck = ["Strike", "Bash+"]    # optional inline deck
//!
//! relics = ["BurningBlood"]     # optional starter-relic list
//! potions = ["Fire Potion"]     # optional starter potions
//!
//! [[monsters]]
//! kind = "Nibbit"
//! offset_phase = false
//! hp = 40                       # optional HP override
//!
//! [[monsters]]
//! kind = "Nibbit"
//! offset_phase = true
//! ```
//!
//! `schema` is required. Accepted values:
//!
//! - **`schema = 1`** — original shape (the docblock above).
//! - **`schema = 2`** — superset of 1. Adds an optional `[provenance]`
//!   block (informational, used by sts2_run_scraper's fixture emitter
//!   to tag the source run) and an optional `[player.relic_counters]`
//!   table that seeds non-zero `Relic.counter` values at load time.
//!
//! Schemas above the current max are rejected.
//!
//! ```toml
//! schema = 2
//!
//! [provenance]
//! run_id     = "00e4b4d4-fc6c-42ad-a363-fea1dfe39c87"
//! build_id   = "v0.101.0"
//! floor      = 7
//! act        = "ACT.UNDERDOCKS"
//! encounter  = "ENCOUNTER.SLUDGE_SPINNER_WEAK"
//! room_type  = "monster"
//! player_outcome = "won"
//! deck_perfect   = true
//! relics_perfect = true
//! # Optional:
//! # player_turns_taken = 4
//! # seed = "SYDDKWZUC8"
//!
//! [player]
//! class  = "Ironclad"
//! deck   = ["Strike", "Bash+"]
//! relics = ["BurningBlood", "JossPaper"]
//!
//! [player.relic_counters]
//! JOSS_PAPER = 4    # JossPaper's CardsExhausted counter
//!
//! [[monsters]]
//! kind = "SludgeSpinner"
//! ```
//!
//! ## Deferred (bead `sts2-rs-kbi1`)
//!
//! Schema=2 also reserves these card-token shapes — the parser does
//! not yet recognize them, but `sts2_run_scraper`'s emitter produces
//! them. Until kbi1 lands, schema=2 fixtures containing them will
//! fail at deck resolution (unknown card), not at TOML parse:
//!
//! - `Foo+N`       — upgrade level N (N ≥ 2; the existing `Foo+`
//!   covers N = 1).
//! - `Foo@KIND`    — base + enchantment kind (e.g. `STRIKE_IRONCLAD@GOOPY`).
//! - `Foo+@KIND` / `Foo+N@KIND` — combinations of the above.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sts2_core::MonsterKind;

use super::encounter::{MonsterAiArgs, MonsterSpec};
use crate::sim::PlayerClass;

/// Highest schema version the loader accepts. Schemas 1..=SCHEMA_VERSION
/// are valid; schemas above this are rejected. The loader is
/// backward-compatible: every schema-2 field is optional (or
/// `#[serde(default)]`), so a schema-1 file parses cleanly under the
/// schema-2 deserializer.
pub const SCHEMA_VERSION: u32 = 2;

/// Raw deserialized scenario file. Field names mirror the TOML keys
/// verbatim; `#[serde(default)]` keeps every section optional so a
/// minimal `schema = 1` + `[[monsters]]` block is a valid file.
#[derive(Debug, Deserialize)]
pub struct ScenarioFile {
    pub schema: u32,
    /// Schema=2 informational block. Stored verbatim; the loader
    /// itself does not branch on these fields. Consumers (e.g. parity
    /// harnesses) can read it to tag results with the source run.
    /// `#[allow(dead_code)]` because the binary doesn't currently
    /// consume the field — the parity / replay harness will once it
    /// lands.
    #[allow(dead_code)]
    #[serde(default)]
    pub provenance: Option<Provenance>,
    #[serde(default)]
    pub player: Option<PlayerSection>,
    /// Top-level relics list. Also accepted as `[player] relics =
    /// [...]` for callers who lay the TOML out with relics inside the
    /// `[player]` table — the merge step prefers the top-level field
    /// but falls back to the player-table field.
    #[serde(default)]
    pub relics: Vec<String>,
    #[serde(default)]
    pub potions: Vec<String>,
    #[serde(default)]
    pub monsters: Vec<MonsterEntry>,
}

/// Schema=2 `[provenance]` block. Every field is optional so a
/// minimal `[provenance]` table (or its complete absence) is valid.
/// The loader does not act on these values — they are passed through
/// for downstream consumers that want to attribute scenario outcomes
/// back to the source .run file.
#[derive(Debug, Default, Clone, Deserialize, PartialEq, Eq)]
pub struct Provenance {
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub build_id: Option<String>,
    #[serde(default)]
    pub floor: Option<u32>,
    #[serde(default)]
    pub act: Option<String>,
    #[serde(default)]
    pub encounter: Option<String>,
    #[serde(default)]
    pub room_type: Option<String>,
    /// `"won" | "lost" | "unknown"`. Not enum-typed deliberately —
    /// future shapes (`"abandoned"`, `"timed_out"`) shouldn't be a
    /// parse error for the harness.
    #[serde(default)]
    pub player_outcome: Option<String>,
    #[serde(default)]
    pub player_turns_taken: Option<u32>,
    /// Seed is a base32-style string in STS2 (`"SYDDKWZUC8"`), not an
    /// integer.
    #[serde(default)]
    pub seed: Option<String>,
    /// `true` iff the scraper's deck reconstruction round-tripped
    /// against the source run's final-state snapshot. Useful for
    /// filtering / stratifying replay-harness results.
    #[serde(default)]
    pub deck_perfect: Option<bool>,
    #[serde(default)]
    pub relics_perfect: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct PlayerSection {
    #[serde(default)]
    pub class: Option<String>,
    /// Ascension level recorded in the source run. The engine is
    /// ascension-identity at the pin (`CLAUDE.md`) so the field is
    /// informational here — used by `corpus_gen --ascension-min` to
    /// stratify the fixture pool toward harder source runs, not by
    /// the engine itself. Bead `sts2-rs-t4tn` (acbj.next-iter).
    /// `corpus_gen` reads ascension via its private `FixtureRelicPlayer`
    /// shape, so this public field is parse-only today.
    #[serde(default)]
    #[allow(dead_code)]
    // Consumed cross-binary by corpus_gen --ascension-min via the public TOML schema.
    pub ascension: Option<u32>,
    /// Player HP at scenario start. `i32` (not `u32`) so overkill-death
    /// dumps from `sts2log` — where the final blow took HP below 0 and
    /// the run-scraper emitted e.g. `hp = -6` — parse cleanly. Callers
    /// that need an unsigned value clamp via `hp.max(0) as u32`.
    /// (Bead `sts2-rs-ppuk`.)
    #[serde(default)]
    pub hp: Option<i32>,
    /// Player's `max_hp` independent of `hp` (current HP). Without
    /// this, the loader collapsed both into a single value, starting
    /// a wounded run-fixture at `hp/hp` instead of `hp/max_hp` —
    /// silently diverging on heal-to-max effects. (Bead `sts2-rs-f3g0`.)
    #[serde(default)]
    pub max_hp: Option<u32>,
    /// Inline deck list — same `Card` / `Card+` token shape as the
    /// `--deck` flag.
    #[serde(default)]
    pub deck: Vec<String>,
    /// Relic list nested under `[player]`. TOML's section-scoping
    /// rules make a `relics = [...]` line that follows the `[player]`
    /// header parse as `player.relics`; accepting it here lets the
    /// spec's example file shape (relics declared after the player
    /// section without an intervening top-level header) round-trip.
    #[serde(default)]
    pub relics: Vec<String>,
    /// Same rationale as [`PlayerSection::relics`].
    #[serde(default)]
    pub potions: Vec<String>,
    /// Schema=2 `[player.relic_counters]` table — `relic_token =
    /// counter_value`. Seeded into `Relic.counter` at `seed_relics`
    /// time (see `super::relics::seed_relics_with_counters`). Missing
    /// or zero counters fall through to the default-0 path.
    #[serde(default)]
    pub relic_counters: BTreeMap<String, i32>,
}

#[derive(Debug, Deserialize)]
pub struct MonsterEntry {
    pub kind: String,
    /// Legacy: `offset_phase` was a Sentry-only field that was removed
    /// in bead `sts2-rs-v1u5` alongside the STS1 Sentry stub. The
    /// field is preserved on deserialize so existing scenario TOMLs
    /// don't fail to parse — it is now silently ignored.
    #[serde(default)]
    pub offset_phase: bool,
    #[serde(default)]
    pub hp: Option<u32>,
    #[serde(default)]
    pub is_alone: Option<bool>,
    #[serde(default)]
    pub is_front: Option<bool>,
}

/// Failure modes for [`load_scenario_file`].
#[derive(Debug)]
pub enum ScenarioFileError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        message: String,
    },
    UnsupportedSchema {
        found: u32,
        expected: u32,
    },
    UnknownPlayerClass(String),
    UnknownMonsterKind(String),
    NoMonsters,
}

impl std::fmt::Display for ScenarioFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "could not read scenario '{}': {source}", path.display())
            }
            Self::Parse { path, message } => write!(
                f,
                "could not parse scenario '{}': {message}",
                path.display()
            ),
            Self::UnsupportedSchema { found, expected } => {
                write!(f, "unsupported schema {found}; expected {expected}")
            }
            Self::UnknownPlayerClass(s) => {
                write!(
                    f,
                    "unknown player.class '{s}' (expected: Ironclad, Silent, \
                     Defect, Regent, Necrobinder)"
                )
            }
            Self::UnknownMonsterKind(s) => write!(
                f,
                "unknown monster kind '{s}' (expected an STS2 act1 / \
                 act1_weak kind — see `sts2-cli list monsters` for the \
                 full roster; STS1 stubs were removed in sts2-rs-ar9u)"
            ),
            Self::NoMonsters => write!(f, "scenario must contain at least one [[monsters]] entry"),
        }
    }
}

impl std::error::Error for ScenarioFileError {}

/// Parse + validate a TOML scenario at `path`. Validates schema version
/// and the basic shape (≥1 monster, known class/kind names) but leaves
/// downstream deck/relic/potion name resolution to the existing
/// resolvers in sibling modules.
pub fn load_scenario_file(path: &Path) -> Result<ScenarioFile, ScenarioFileError> {
    let text = std::fs::read_to_string(path).map_err(|source| ScenarioFileError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let parsed: ScenarioFile = toml::from_str(&text).map_err(|e| ScenarioFileError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    // Schemas 1..=SCHEMA_VERSION are accepted (schema=2 is a superset
    // of schema=1; the deserializer's `#[serde(default)]` fields absorb
    // the missing schema=1 sections). Schemas above the current max
    // are rejected so we surface unknown features rather than silently
    // dropping them.
    if parsed.schema == 0 || parsed.schema > SCHEMA_VERSION {
        return Err(ScenarioFileError::UnsupportedSchema {
            found: parsed.schema,
            expected: SCHEMA_VERSION,
        });
    }
    if parsed.monsters.is_empty() {
        return Err(ScenarioFileError::NoMonsters);
    }
    // Eager validation of class + monster kinds so failures surface at
    // load time rather than after the banner prints.
    if let Some(p) = &parsed.player {
        if let Some(c) = &p.class {
            parse_player_class(c)?;
        }
    }
    for m in &parsed.monsters {
        parse_monster_kind(&m.kind)?;
    }
    Ok(parsed)
}

/// Resolve a player-class name to the `PlayerClass` enum.
/// Case-insensitive; accepts all 5 STS2 character names. Defect /
/// Regent / Necrobinder are placeholders at the engine layer (P6
/// hasn't wired their starter HP/deck/relics yet) — see
/// `PlayerClass` for the placeholder fallback policy. Bead
/// `sts2-rs-5m6k`: unblocks 565 sim-error fixtures whose corpus
/// runs are Defect/Regent/Necrobinder by letting them parse and
/// surface the real per-class content gaps one layer deeper.
pub fn parse_player_class(s: &str) -> Result<PlayerClass, ScenarioFileError> {
    match s.trim().to_ascii_lowercase().as_str() {
        "ironclad" => Ok(PlayerClass::Ironclad),
        "silent" => Ok(PlayerClass::Silent),
        "defect" => Ok(PlayerClass::Defect),
        "regent" => Ok(PlayerClass::Regent),
        "necrobinder" => Ok(PlayerClass::Necrobinder),
        _ => Err(ScenarioFileError::UnknownPlayerClass(s.to_string())),
    }
}

/// Resolve a monster-kind name to the `MonsterKind` enum.
/// Case-insensitive; accepts the dual-form (kebab/snake/space/PascalCase)
/// shapes the rest of the CLI already accepts via `normalize_alias`.
/// Cliext Phase 4 (`sts2-rs-90n5`) wired all 34 then-active monsters;
/// bead `sts2-rs-ar9u` dropped the 5 STS1 scaffolding stubs (`cultist`
/// / `jawworm` / `acidslimes` / `greenlouse` / `sentry`), leaving the
/// 29 STS2-native kinds (6 `act1_weak` + 22 Act-1 + `Nibbit`).
pub fn parse_monster_kind(s: &str) -> Result<MonsterKind, ScenarioFileError> {
    let needle: String = s
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let kind = match needle.as_str() {
        // STS2 act1_weak.
        // The five STS1-name scaffolding stubs (cultist / jawworm /
        // acidslimes / greenlouse / sentry) that used to live here
        // were removed by bead `sts2-rs-ar9u` — they're not STS2
        // content, and being reachable from TOML files silently
        // misled users designing STS2 sweeps.
        "nibbit" => MonsterKind::Nibbit,
        "shrinkerbeetle" => MonsterKind::ShrinkerBeetle,
        "fuzzywurmcrawler" => MonsterKind::FuzzyWurmCrawler,
        "leafslimes" => MonsterKind::LeafSlimeS,
        "twigslimes" => MonsterKind::TwigSlimeS,
        "leafslimem" => MonsterKind::LeafSlimeM,
        "twigslimem" => MonsterKind::TwigSlimeM,
        // STS2 act1 (sts2-rs-90n5).
        "cubexconstruct" => MonsterKind::CubexConstruct,
        "flyconid" => MonsterKind::Flyconid,
        "eyewithteeth" => MonsterKind::EyeWithTeeth,
        "fogmog" => MonsterKind::Fogmog,
        "inklet" => MonsterKind::Inklet,
        "mawler" => MonsterKind::Mawler,
        "vineshambler" => MonsterKind::VineShambler,
        "slitheringstrangler" => MonsterKind::SlitheringStrangler,
        "snappingjaxfruit" => MonsterKind::SnappingJaxfruit,
        "assassinrubyraider" => MonsterKind::AssassinRubyRaider,
        "axerubyraider" => MonsterKind::AxeRubyRaider,
        "bruterubyraider" => MonsterKind::BruteRubyRaider,
        "crossbowrubyraider" => MonsterKind::CrossbowRubyRaider,
        "trackerrubyraider" => MonsterKind::TrackerRubyRaider,
        "bygoneeffigy" => MonsterKind::BygoneEffigy,
        "byrdonis" => MonsterKind::Byrdonis,
        "phrogparasite" => MonsterKind::PhrogParasite,
        "parafright" => MonsterKind::Parafright,
        "vantom" => MonsterKind::Vantom,
        "ceremonialbeast" => MonsterKind::CeremonialBeast,
        "kinpriest" => MonsterKind::KinPriest,
        "kinfollower" => MonsterKind::KinFollower,
        "wriggler" => MonsterKind::Wriggler,
        // STS2 act2 (sts2-rs-6ynt.10).
        "thievinghopper" => MonsterKind::ThievingHopper,
        "tunneler" => MonsterKind::Tunneler,
        "bowlbugegg" => MonsterKind::BowlbugEgg,
        "bowlbugnectar" => MonsterKind::BowlbugNectar,
        "bowlbugrock" => MonsterKind::BowlbugRock,
        "bowlbugsilk" => MonsterKind::BowlbugSilk,
        "exoskeleton" => MonsterKind::Exoskeleton,
        "chomper" => MonsterKind::Chomper,
        "hunterkiller" => MonsterKind::HunterKiller,
        "louseprogenitor" => MonsterKind::LouseProgenitor,
        "myte" => MonsterKind::Myte,
        "toughegg" => MonsterKind::ToughEgg,
        "ovicopter" => MonsterKind::Ovicopter,
        "slumberingbeetle" => MonsterKind::SlumberingBeetle,
        "spinytoad" => MonsterKind::SpinyToad,
        "theobscura" => MonsterKind::TheObscura,
        "decimillipedesegment" => MonsterKind::DecimillipedeSegment,
        "entomancer" => MonsterKind::Entomancer,
        "infestedprism" => MonsterKind::InfestedPrism,
        "theinsatiable" => MonsterKind::TheInsatiable,
        "knowledgedemon" => MonsterKind::KnowledgeDemon,
        "crusher" => MonsterKind::Crusher,
        "rocket" => MonsterKind::Rocket,
        // STS2 act3 (sts2-rs-6ynt.10).
        "devotedsculptor" => MonsterKind::DevotedSculptor,
        "scrollofbiting" => MonsterKind::ScrollOfBiting,
        "turretoperator" => MonsterKind::TurretOperator,
        "axebot" => MonsterKind::Axebot,
        "zapbot" => MonsterKind::Zapbot,
        "stabbot" => MonsterKind::Stabbot,
        "guardbot" => MonsterKind::Guardbot,
        "noisebot" => MonsterKind::Noisebot,
        "fabricator" => MonsterKind::Fabricator,
        "frogknight" => MonsterKind::FrogKnight,
        "globehead" => MonsterKind::GlobeHead,
        "owlmagistrate" => MonsterKind::OwlMagistrate,
        "slimedberserker" => MonsterKind::SlimedBerserker,
        "thelost" => MonsterKind::TheLost,
        "theforgotten" => MonsterKind::TheForgotten,
        "flailknight" => MonsterKind::FlailKnight,
        "mysteriousknight" => MonsterKind::MysteriousKnight,
        "livingshield" => MonsterKind::LivingShield,
        "magiknight" => MonsterKind::MagiKnight,
        "spectralknight" => MonsterKind::SpectralKnight,
        "mechaknight" => MonsterKind::MechaKnight,
        "osty" => MonsterKind::Osty,
        "soulnexus" => MonsterKind::SoulNexus,
        // Aeonglass replaced Doormaker in v0.107.1.
        "aeonglass" => MonsterKind::Aeonglass,
        "queen" => MonsterKind::Queen,
        "testsubject" => MonsterKind::TestSubject,
        // Replay-harness gap drain (sts2-rs-c02m). All 22 below were
        // added to `MonsterKind` + factory + `monster_dispatch` by
        // earlier act2/act3/act4 sweeps but never wired into this
        // resolver — 201 of 202 sim-errors from the 2026-05-16 sts2log
        // corpus were "unknown monster kind 'X'" for X already enumerated.
        "decimillipedesegmentfront" => MonsterKind::DecimillipedeSegmentFront,
        "corpseslug" => MonsterKind::CorpseSlug,
        "sludgespinner" => MonsterKind::SludgeSpinner,
        "punchconstruct" => MonsterKind::PunchConstruct,
        "seapunk" => MonsterKind::Seapunk,
        "toadpole" => MonsterKind::Toadpole,
        "torchheadamalgam" => MonsterKind::TorchHeadAmalgam,
        "phantasmalgardener" => MonsterKind::PhantasmalGardener,
        "terroreel" => MonsterKind::TerrorEel,
        "sewerclam" => MonsterKind::SewerClam,
        "soulfysh" => MonsterKind::SoulFysh,
        "skulkingcolony" => MonsterKind::SkulkingColony,
        "fossilstalker" => MonsterKind::FossilStalker,
        "waterfallgiant" => MonsterKind::WaterfallGiant,
        "livingfog" => MonsterKind::LivingFog,
        "calcifiedcultist" => MonsterKind::CalcifiedCultist,
        "battlefriendv1" => MonsterKind::BattleFriendV1,
        "twotailedrat" => MonsterKind::TwoTailedRat,
        "gremlinmerc" => MonsterKind::GremlinMerc,
        "hauntedship" => MonsterKind::HauntedShip,
        "lagavulinmatriarch" => MonsterKind::LagavulinMatriarch,
        "fakemerchantmonster" => MonsterKind::FakeMerchantMonster,
        // Wave-C tail (sts2-rs-qds8). Already wired in core + dispatch
        // (via earlier sweeps) but never added to this resolver; cleared
        // the final 29/954 sim-error fixtures in the 2026-05-16 corpus.
        "decimillipedesegmentmiddle" => MonsterKind::DecimillipedeSegmentMiddle,
        "decimillipedesegmentback" => MonsterKind::DecimillipedeSegmentBack,
        "battlefriendv2" => MonsterKind::BattleFriendV2,
        "battlefriendv3" => MonsterKind::BattleFriendV3,
        "dampcultist" => MonsterKind::DampCultist,
        // ywr4 coverage-guard sweep (sts2-rs-ywr4). Adds the 15 remaining
        // MonsterKind::ALL_KNOWN variants that hadn't been wired here yet —
        // surfaced by parse_monster_kind_covers_every_all_known_variant.
        // Mix of boss / adversary / test-harness scaffolding kinds; all
        // already had python_name + dispatch arms from earlier sweeps.
        "architect" => MonsterKind::Architect,
        "byrdpip" => MonsterKind::Byrdpip,
        "paelslegion" => MonsterKind::PaelsLegion,
        "theadversarymkone" => MonsterKind::TheAdversaryMkOne,
        "theadversarymktwo" => MonsterKind::TheAdversaryMkTwo,
        "theadversarymkthree" => MonsterKind::TheAdversaryMkThree,
        "bigdummy" => MonsterKind::BigDummy,
        "onehpmonster" => MonsterKind::OneHpMonster,
        "tenhpmonster" => MonsterKind::TenHpMonster,
        "singleattackmovemonster" => MonsterKind::SingleAttackMoveMonster,
        "multiattackmovemonster" => MonsterKind::MultiAttackMoveMonster,
        "densevegetationwriggler" => MonsterKind::DenseVegetationWriggler,
        "sneakygremlin" => MonsterKind::SneakyGremlin,
        "fatgremlin" => MonsterKind::FatGremlin,
        "gasbomb" => MonsterKind::GasBomb,
        _ => return Err(ScenarioFileError::UnknownMonsterKind(s.to_string())),
    };
    Ok(kind)
}

/// Serialize a resolved scenario (monsters + optional deck + relics +
/// potions + player class/hp) back to a strict-subset TOML string that
/// round-trips through [`load_scenario_file`] + the merge step in
/// `main::run_combat`. Cliext E1 (`sts2-rs-92sq`) — the
/// `--print-scenario` flag.
///
/// The output emits `schema = 1` and exactly the four sections
/// (`[player]` / top-level `relics` / top-level `potions` /
/// `[[monsters]]`) that the loader understands. Field order is fixed so
/// the output is diff-friendly.
///
/// Inputs:
/// - `class`: resolved [`PlayerClass`] (always emitted as `[player]
///   class = "..."`).
/// - `hp`: explicit player HP if the caller passed `--hp`; `None` keeps
///   the field absent so the loader falls back to the engine default.
/// - `deck_cards`: serialized deck tokens (`"Strike"`, `"Bash+"`, ...).
///   Empty → no `deck =` field emitted. Caller is responsible for
///   formatting upgraded-card markers.
/// - `relic_pascal_names`: starter relics in `PascalCase` form.
/// - `potion_pascal_names`: starter potions in `PascalCase` form.
/// - `monsters`: the resolved monster list as
///   `(PascalCase kind, offset_phase, hp_override)` triples.
#[must_use]
pub fn render_scenario_toml(
    class: PlayerClass,
    hp: Option<u32>,
    deck_cards: &[String],
    relic_pascal_names: &[String],
    potion_pascal_names: &[String],
    monsters: &[(String, bool, Option<u32>)],
) -> String {
    let mut out = String::new();
    out.push_str("schema = 1\n\n");
    out.push_str("[player]\n");
    let class_name = match class {
        PlayerClass::Ironclad => "Ironclad",
        PlayerClass::Silent => "Silent",
        PlayerClass::Defect => "Defect",
        PlayerClass::Regent => "Regent",
        PlayerClass::Necrobinder => "Necrobinder",
    };
    out.push_str(&format!("class = \"{class_name}\"\n"));
    if let Some(h) = hp {
        out.push_str(&format!("hp = {h}\n"));
    }
    if !deck_cards.is_empty() {
        out.push_str("deck = [");
        for (i, c) in deck_cards.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push('"');
            out.push_str(c);
            out.push('"');
        }
        out.push_str("]\n");
    }
    out.push('\n');

    if !relic_pascal_names.is_empty() {
        out.push_str("relics = [");
        for (i, r) in relic_pascal_names.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push('"');
            out.push_str(r);
            out.push('"');
        }
        out.push_str("]\n");
    }
    if !potion_pascal_names.is_empty() {
        out.push_str("potions = [");
        for (i, p) in potion_pascal_names.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push('"');
            out.push_str(p);
            out.push('"');
        }
        out.push_str("]\n");
    }
    if !relic_pascal_names.is_empty() || !potion_pascal_names.is_empty() {
        out.push('\n');
    }

    for (kind, offset_phase, hp_override) in monsters {
        out.push_str("[[monsters]]\n");
        out.push_str(&format!("kind = \"{kind}\"\n"));
        out.push_str(&format!("offset_phase = {offset_phase}\n"));
        if let Some(h) = hp_override {
            out.push_str(&format!("hp = {h}\n"));
        }
        out.push('\n');
    }
    out
}

/// Lower a list of [`MonsterEntry`] into the runtime [`MonsterSpec`]
/// list consumed by `scenario::starter::build_state`. Applies the
/// position fix-ups (`is_alone`, `is_front`) that
/// [`super::Scenario::from_monsters`] otherwise does, while letting
/// per-entry overrides win.
pub fn lower_monsters(entries: &[MonsterEntry]) -> Result<Vec<MonsterSpec>, ScenarioFileError> {
    let alone_default = entries.len() == 1;
    let mut out = Vec::with_capacity(entries.len());
    for (idx, m) in entries.iter().enumerate() {
        let kind = parse_monster_kind(&m.kind)?;
        // `m.offset_phase` is kept on `MonsterEntry` for back-compat
        // deserialization (bead `sts2-rs-v1u5`) but is silently ignored
        // here — the STS1 Sentry stub it controlled was removed.
        let _ = m.offset_phase;
        out.push(MonsterSpec {
            kind,
            hp_override: m.hp,
            ai_args: MonsterAiArgs {
                is_alone: m.is_alone.unwrap_or(alone_default),
                is_front: m.is_front.unwrap_or(idx == 0),
            },
            args: super::MonsterSpawnArgs::None,
            // A hand-written TOML scenario names monsters, not an encounter,
            // so there is no `Slots` list to index into (GH #552).
            slot: None,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn write_tmp(name: &str, body: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let pid = std::process::id();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        p.push(format!("sts2-g2ex-{pid}-{ts}-{name}"));
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        p
    }

    #[test]
    fn minimal_file_parses() {
        // Bead `sts2-rs-ar9u`: STS1 `Cultist` kind dropped from
        // `parse_monster_kind`; use the STS2-native `Nibbit` here.
        let path = write_tmp(
            "min.toml",
            "schema = 1\n\n[[monsters]]\nkind = \"Nibbit\"\n",
        );
        let parsed = load_scenario_file(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(parsed.schema, 1);
        assert_eq!(parsed.monsters.len(), 1);
        assert_eq!(parsed.monsters[0].kind, "Nibbit");
    }

    #[test]
    fn missing_schema_fails() {
        let path = write_tmp("noschema.toml", "[[monsters]]\nkind = \"Cultist\"\n");
        let err = load_scenario_file(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        // toml::from_str fails because `schema` has no default.
        assert!(matches!(err, ScenarioFileError::Parse { .. }));
    }

    #[test]
    fn schema_above_max_rejected() {
        let path = write_tmp(
            "schema3.toml",
            "schema = 3\n\n[[monsters]]\nkind = \"Nibbit\"\n",
        );
        let err = load_scenario_file(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        match err {
            ScenarioFileError::UnsupportedSchema { found, expected } => {
                assert_eq!(found, 3);
                assert_eq!(expected, SCHEMA_VERSION);
            }
            other => panic!("expected UnsupportedSchema, got {other:?}"),
        }
    }

    #[test]
    fn schema_zero_rejected() {
        // schema = 0 is reserved as "unset"; reject explicitly so a
        // future bump doesn't make it ambiguous.
        let path = write_tmp(
            "schema0.toml",
            "schema = 0\n\n[[monsters]]\nkind = \"Nibbit\"\n",
        );
        let err = load_scenario_file(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(matches!(err, ScenarioFileError::UnsupportedSchema { .. }));
    }

    #[test]
    fn schema_one_still_parses_under_schema_two_deserializer() {
        // schema=2 is a superset of schema=1; every schema=2 field is
        // optional, so a schema=1 file must continue to parse cleanly.
        let path = write_tmp(
            "schema1_under_2.toml",
            "schema = 1\n\n[[monsters]]\nkind = \"Nibbit\"\n",
        );
        let parsed = load_scenario_file(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(parsed.schema, 1);
        assert!(parsed.provenance.is_none());
    }

    #[test]
    fn schema_two_provenance_parses() {
        let body = "schema = 2\n\n\
            [provenance]\n\
            run_id = \"00e4b4d4-fc6c-42ad-a363-fea1dfe39c87\"\n\
            build_id = \"v0.101.0\"\n\
            floor = 7\n\
            act = \"ACT.UNDERDOCKS\"\n\
            encounter = \"ENCOUNTER.SLUDGE_SPINNER_WEAK\"\n\
            room_type = \"monster\"\n\
            player_outcome = \"won\"\n\
            player_turns_taken = 4\n\
            seed = \"SYDDKWZUC8\"\n\
            deck_perfect = true\n\
            relics_perfect = true\n\n\
            [[monsters]]\nkind = \"Nibbit\"\n";
        let path = write_tmp("schema2_prov.toml", body);
        let parsed = load_scenario_file(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(parsed.schema, 2);
        let p = parsed.provenance.as_ref().expect("provenance present");
        assert_eq!(
            p.run_id.as_deref(),
            Some("00e4b4d4-fc6c-42ad-a363-fea1dfe39c87")
        );
        assert_eq!(p.floor, Some(7));
        assert_eq!(
            p.encounter.as_deref(),
            Some("ENCOUNTER.SLUDGE_SPINNER_WEAK")
        );
        assert_eq!(p.player_outcome.as_deref(), Some("won"));
        assert_eq!(p.player_turns_taken, Some(4));
        assert_eq!(p.seed.as_deref(), Some("SYDDKWZUC8"));
        assert_eq!(p.deck_perfect, Some(true));
        assert_eq!(p.relics_perfect, Some(true));
    }

    #[test]
    fn schema_two_provenance_all_fields_optional() {
        // Empty `[provenance]` table must parse — every field defaults.
        let body = "schema = 2\n\n[provenance]\n\n[[monsters]]\nkind = \"Nibbit\"\n";
        let path = write_tmp("schema2_prov_empty.toml", body);
        let parsed = load_scenario_file(&path).unwrap();
        std::fs::remove_file(&path).ok();
        let p = parsed.provenance.expect("provenance section present");
        assert_eq!(p, Provenance::default());
    }

    #[test]
    fn schema_two_relic_counters_parses() {
        let body = "schema = 2\n\n\
            [player]\n\
            class = \"Ironclad\"\n\n\
            [player.relic_counters]\n\
            JOSS_PAPER = 4\n\
            NUNCHAKU = 7\n\n\
            [[monsters]]\nkind = \"Nibbit\"\n";
        let path = write_tmp("schema2_counters.toml", body);
        let parsed = load_scenario_file(&path).unwrap();
        std::fs::remove_file(&path).ok();
        let counters = &parsed.player.as_ref().unwrap().relic_counters;
        assert_eq!(counters.get("JOSS_PAPER").copied(), Some(4));
        assert_eq!(counters.get("NUNCHAKU").copied(), Some(7));
    }

    #[test]
    fn no_monsters_rejected() {
        let path = write_tmp("empty.toml", "schema = 1\n");
        let err = load_scenario_file(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(matches!(err, ScenarioFileError::NoMonsters));
    }

    #[test]
    fn unknown_kind_rejected() {
        let path = write_tmp(
            "unknown.toml",
            "schema = 1\n\n[[monsters]]\nkind = \"NotAMonster\"\n",
        );
        let err = load_scenario_file(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(matches!(err, ScenarioFileError::UnknownMonsterKind(_)));
    }

    /// Bead `sts2-rs-ywr4` (follow-up to `sts2-rs-23ky`): cross-crate
    /// coverage guard. Iterates the canonical `MonsterKind::ALL_KNOWN`
    /// roster const from sts2-core and asserts `parse_monster_kind`
    /// resolves each variant's PascalCase `python_name()`. Any future
    /// `MonsterKind` variant added to the enum without a matching arm
    /// in this resolver fails `cargo test` immediately, not at the
    /// next replay-harness re-run. The PascalCase round-trip is what
    /// the scenario TOML emitter produces (cf. `serialize_scenario`).
    #[test]
    fn parse_monster_kind_covers_every_all_known_variant() {
        for kind in MonsterKind::ALL_KNOWN {
            let name = kind.python_name();
            let resolved = parse_monster_kind(name).unwrap_or_else(|e| {
                panic!(
                    "parse_monster_kind({name}) failed for ALL_KNOWN variant \
                     {kind:?}: {e}. Add a lowercase arm to parse_monster_kind."
                )
            });
            assert_eq!(
                resolved, *kind,
                "parse_monster_kind({name}) resolved to {resolved:?}, expected {kind:?}"
            );
        }
    }

    /// Bead `sts2-rs-c02m`: pin every variant added to the resolver
    /// by the replay-harness gap drain (2026-05-16 sts2log corpus
    /// surfaced these 22 as `unknown monster kind 'X'` errors despite
    /// each `X` being live in `MonsterKind`, factory, and
    /// `monster_dispatch`). Catches future regressions where someone
    /// adds a `MonsterKind` variant and forgets to wire the scenario
    /// resolver.
    #[test]
    fn parse_monster_kind_drains_replay_harness_2026_05_16_gap() {
        let cases: &[(&str, MonsterKind)] = &[
            (
                "DecimillipedeSegmentFront",
                MonsterKind::DecimillipedeSegmentFront,
            ),
            ("CorpseSlug", MonsterKind::CorpseSlug),
            ("SludgeSpinner", MonsterKind::SludgeSpinner),
            ("PunchConstruct", MonsterKind::PunchConstruct),
            ("Seapunk", MonsterKind::Seapunk),
            ("Toadpole", MonsterKind::Toadpole),
            ("TorchHeadAmalgam", MonsterKind::TorchHeadAmalgam),
            ("PhantasmalGardener", MonsterKind::PhantasmalGardener),
            ("TerrorEel", MonsterKind::TerrorEel),
            ("SewerClam", MonsterKind::SewerClam),
            ("SoulFysh", MonsterKind::SoulFysh),
            ("SkulkingColony", MonsterKind::SkulkingColony),
            ("FossilStalker", MonsterKind::FossilStalker),
            ("WaterfallGiant", MonsterKind::WaterfallGiant),
            ("LivingFog", MonsterKind::LivingFog),
            ("CalcifiedCultist", MonsterKind::CalcifiedCultist),
            ("BattleFriendV1", MonsterKind::BattleFriendV1),
            ("TwoTailedRat", MonsterKind::TwoTailedRat),
            ("GremlinMerc", MonsterKind::GremlinMerc),
            ("HauntedShip", MonsterKind::HauntedShip),
            ("LagavulinMatriarch", MonsterKind::LagavulinMatriarch),
            ("FakeMerchantMonster", MonsterKind::FakeMerchantMonster),
            (
                "DecimillipedeSegmentMiddle",
                MonsterKind::DecimillipedeSegmentMiddle,
            ),
            (
                "DecimillipedeSegmentBack",
                MonsterKind::DecimillipedeSegmentBack,
            ),
            ("BattleFriendV2", MonsterKind::BattleFriendV2),
            ("BattleFriendV3", MonsterKind::BattleFriendV3),
            ("DampCultist", MonsterKind::DampCultist),
        ];
        for (input, expected) in cases {
            assert_eq!(
                parse_monster_kind(input).unwrap(),
                *expected,
                "PascalCase {input} should resolve via scenario TOML parser"
            );
        }
    }

    /// Bead `sts2-rs-5m6k`: scenario loader must accept all 5 STS2
    /// character names. Defect/Regent/Necrobinder parse to their own
    /// `PlayerClass` variants; engine-side placeholder fallback to the
    /// Ironclad starter HP/deck lives in `starter::build_state` until
    /// P6 wires those classes. Without this, 565 of 854 sim-errors in
    /// the 2026-05-17 corpus baseline were the loader rejecting
    /// non-Ironclad/Silent class strings before reaching combat-start.
    #[test]
    fn parse_player_class_accepts_all_five_characters() {
        assert_eq!(
            parse_player_class("Ironclad").unwrap(),
            PlayerClass::Ironclad
        );
        assert_eq!(parse_player_class("Silent").unwrap(), PlayerClass::Silent);
        assert_eq!(parse_player_class("Defect").unwrap(), PlayerClass::Defect);
        assert_eq!(parse_player_class("Regent").unwrap(), PlayerClass::Regent);
        assert_eq!(
            parse_player_class("Necrobinder").unwrap(),
            PlayerClass::Necrobinder
        );
        // Case-insensitive.
        assert_eq!(parse_player_class("defect").unwrap(), PlayerClass::Defect);
        assert_eq!(
            parse_player_class("NECROBINDER").unwrap(),
            PlayerClass::Necrobinder
        );
        // Unknown classes still rejected. The name here is deliberately bogus so
        // it cannot be mistaken for a real character.
        assert!(matches!(
            parse_player_class("NotAPlayerClass"),
            Err(ScenarioFileError::UnknownPlayerClass(_))
        ));
    }

    #[test]
    fn parse_monster_kind_case_insensitive() {
        assert_eq!(parse_monster_kind("Nibbit").unwrap(), MonsterKind::Nibbit);
        assert_eq!(parse_monster_kind("nibbit").unwrap(), MonsterKind::Nibbit);
        assert_eq!(
            parse_monster_kind("Shrinker Beetle").unwrap(),
            MonsterKind::ShrinkerBeetle
        );
        assert_eq!(
            parse_monster_kind("kin_priest").unwrap(),
            MonsterKind::KinPriest
        );
    }

    #[test]
    fn lower_monsters_fills_position_args() {
        // Bead `sts2-rs-ar9u`: STS1 `Sentry` kind dropped from
        // `parse_monster_kind`. Use an STS2 act1_weak kind for the
        // position-fill assertions (offset_phase only matters for the
        // ar9u-removed Sentry stub; the position-fill logic is
        // kind-agnostic).
        let entries = vec![
            MonsterEntry {
                kind: "Nibbit".into(),
                offset_phase: false,
                hp: None,
                is_alone: None,
                is_front: None,
            },
            MonsterEntry {
                kind: "ShrinkerBeetle".into(),
                offset_phase: false,
                hp: None,
                is_alone: None,
                is_front: None,
            },
        ];
        let specs = lower_monsters(&entries).unwrap();
        assert_eq!(specs.len(), 2);
        assert!(!specs[0].ai_args.is_alone);
        assert!(specs[0].ai_args.is_front);
        assert!(!specs[1].ai_args.is_front);
    }

    #[test]
    fn full_file_parses() {
        let body = r#"
schema = 1

[player]
class = "Ironclad"
hp = 80
deck = ["Strike", "Strike", "Defend", "Bash+"]

relics = ["BurningBlood", "Anchor"]
potions = ["Blood Potion", "Fire Potion"]

[[monsters]]
kind = "Nibbit"
offset_phase = false
hp = 40

[[monsters]]
kind = "Nibbit"
offset_phase = true
"#;
        let path = write_tmp("full.toml", body);
        let parsed = load_scenario_file(&path).unwrap();
        std::fs::remove_file(&path).ok();
        let player = parsed.player.as_ref().unwrap();
        assert_eq!(player.class.as_deref(), Some("Ironclad"));
        assert_eq!(player.hp, Some(80));
        assert_eq!(player.deck.len(), 4);
        // The TOML in this test puts `relics = [...]` after the
        // `[player]` header without an intervening top-level break, so
        // the relics/potions arrays parse as `player.relics` /
        // `player.potions` rather than the top-level fields.
        // `merge_scenario_into_args` accepts both shapes; the unit test
        // just verifies the values landed somewhere.
        let relics = if parsed.relics.is_empty() {
            &player.relics
        } else {
            &parsed.relics
        };
        let potions = if parsed.potions.is_empty() {
            &player.potions
        } else {
            &parsed.potions
        };
        assert_eq!(
            relics,
            &vec!["BurningBlood".to_string(), "Anchor".to_string()]
        );
        assert_eq!(
            potions,
            &vec!["Blood Potion".to_string(), "Fire Potion".to_string()]
        );
        assert_eq!(parsed.monsters.len(), 2);
        assert_eq!(parsed.monsters[0].hp, Some(40));
        assert!(parsed.monsters[1].offset_phase);
    }

    /// Bead `sts2-rs-ppuk`: overkill-death dumps from `sts2log` emit
    /// `[player] hp = -6` when the final blow took HP below 0. The
    /// scenario loader must accept the negative value (a prior `u32`
    /// schema rejected the whole file at the TOML layer); callers
    /// clamp to 0 when forwarding to the unsigned `--hp` CLI arg.
    #[test]
    fn player_hp_accepts_negative_for_overkill_death_fixtures() {
        let body = "schema = 1\n\n[player]\nclass = \"Ironclad\"\nhp = -6\n\n[[monsters]]\nkind = \"Nibbit\"\n";
        let path = write_tmp("ppuk_neghp.toml", body);
        let parsed = load_scenario_file(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(parsed.player.as_ref().unwrap().hp, Some(-6));
    }

    /// Bead `sts2-rs-f3g0`: `[player] max_hp` parses independently of
    /// `hp`. Pre-f3g0 the loader collapsed both onto `--hp`, starting
    /// wounded run-fixtures at `hp/hp` instead of `hp/max_hp`.
    #[test]
    fn player_max_hp_parses_independently_of_current_hp() {
        let body = "schema = 1\n\n[player]\nclass = \"Ironclad\"\nhp = 20\nmax_hp = 80\n\n[[monsters]]\nkind = \"Nibbit\"\n";
        let path = write_tmp("f3g0_maxhp.toml", body);
        let parsed = load_scenario_file(&path).unwrap();
        std::fs::remove_file(&path).ok();
        let p = parsed.player.as_ref().unwrap();
        assert_eq!(p.hp, Some(20));
        assert_eq!(p.max_hp, Some(80));
    }
}
