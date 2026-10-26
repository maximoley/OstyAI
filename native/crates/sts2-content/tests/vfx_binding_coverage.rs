//! Every monster-AI move state the engine can log must resolve to exactly
//! one row of the committed VFX binding table — or be listed as a known
//! exemption.
//!
//! This exists because the two id spaces are NOT the same. C# declares
//! `new MoveState("THRASH_MOVE", ThrashMove, …)`; `create_crusher_ai()` uses
//! `"THRASH"`. Stripping the `_MOVE` suffix reconciles Crusher — but design
//! spec §1.6 flagged that as *unverified* for the other ~95 monsters. This
//! test is what turned the assumption into a checked fact. Full findings are
//! in spec §1.6 and `task-7-report.md`; the short version is three distinct
//! hazards, not one:
//!
//! 1. The `_MOVE`-suffix convention itself is bidirectional — see
//!    [`candidate_keys`]. Mechanical, no per-monster list needed.
//! 2. 18 state-alias entries, across 13 distinct monster files, spell
//!    their engine id as a genuine RENAME of the C# id (not a suffix
//!    difference) — [`Aliases`], published in the table itself.
//! 3. A few Rust `MonsterKind` variants model a C# subclass or a
//!    positional/slot variant whose move-state machine actually lives in a
//!    DIFFERENT C# class (inheritance, or straight reuse) — [`Aliases`],
//!    published in the table itself.
//!
//! `vfx_bindings.py`'s module docstring is explicit that the extractor does
//! **not** do `_MOVE`-suffix reconciliation — it stores the C# state id
//! verbatim, and "that reconciliation belongs to whatever consumes this
//! table, not to extraction." This file is that consumer, so hazards 1-3
//! live here, not in the Python extractor. (One genuine extractor bug
//! *was* found along the way — a nested-path `WithHitFx` site on
//! `VineShambler` — and that one WAS fixed in `vfx_bindings.py`, per the
//! brief's first-preference response; see its commit.)

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use sts2_content::monster_dispatch::{build_ai, MonsterSpawnArgs, SpawnPosition};
use sts2_core::monster_ai::MonsterState;
use sts2_core::MonsterKind;

/// The two alias tables this gate applies before looking a key up, read
/// from the published table's own `aliases` section rather than kept here.
///
/// They used to be two `const` arrays in this file. That made this gate a
/// liar: the OTHER consumer of the same table — the workbench renderer
/// (`sts2-pycli/web/src/effects/bindings.ts`) — applied neither, so ~30
/// engine `(monster, move)` pairs silently drew the generic slash at
/// runtime while this test reported full coverage. Publishing the tables as
/// table DATA (`extract_vfx_bindings.py`'s `_MONSTER_ALIASES` /
/// `_STATE_ALIASES`, each entry carrying the C# line it was confirmed
/// against) is what makes the two consumers agree by construction; the
/// per-entry rationale lives there now.
///
/// * `monsters`: engine monster id (`MonsterKind::python_name()`) -> the
///   decomp file stem whose `GenerateMoveStateMachine()` actually owns its
///   moves (a C# subclass with no override, or an engine-only slot variant).
/// * `states`: `<monster>:<engine state id>` -> the real C# state id, for
///   ids that are a genuine RENAME rather than a `_MOVE`-suffix difference
///   (suffixes stay mechanical — see [`candidate_keys`]). The monster half
///   is the key AFTER the monster alias is applied.
struct Aliases {
    monsters: HashMap<String, String>,
    states: HashMap<String, String>,
}

impl Aliases {
    fn monster_key(&self, kind: MonsterKind) -> &str {
        let name = kind.python_name();
        self.monsters.get(name).map_or(name, String::as_str)
    }

    fn state_id<'a>(&'a self, monster: &str, state_id: &'a str) -> &'a str {
        self.states
            .get(&format!("{monster}:{state_id}"))
            .map_or(state_id, String::as_str)
    }
}

/// The candidate table keys for one engine `(monster, state_id)` pair, most
/// likely first.
///
/// There are two hypotheses, both directly grounded in the engine's own
/// logged id (never a guess):
///
/// 1. **`"{state_id}_MOVE"`** — the Crusher pattern: C# declares
///    `"THRASH_MOVE"`, the Rust factory strips the suffix to `"THRASH"`.
///    Verified as the majority behavior across the roster.
/// 2. **`"{state_id}"` as-is** — a real, sizeable minority where the
///    reconciliation in (1) does not apply, for one of two underlying
///    reasons (both confirmed by reading the C# `MoveState(...)`
///    constructor directly, not inferred from the id's shape):
///    - The Rust factory did NOT strip anything — its own id already
///      carries `_MOVE` (or `_MOVE_2`, `_MOVE_1`, …), matching C# exactly
///      (e.g. `Aeonglass:EBB_MOVE`, `Queen:ENRAGE_MOVE`,
///      `HunterKiller:BITE_MOVE`). Appending a second `_MOVE` here would
///      only ever break the join.
///    - C# itself never used the `_MOVE` suffix for this state at all
///      (e.g. `AxeRubyRaider.cs` declares `"SWING_1"`, `"BIG_SWING"` —
///      no suffix anywhere), and the Rust factory carries the identical
///      bare id.
///
/// Both branches of (2) collapse to the same lookup ("try the bare id"),
/// so this is one mechanical two-candidate rule rather than a per-monster
/// special case — an explicit alias list keyed on every affected monster
/// was tried first and rejected: it grew to ~55 entries with no rule other
/// than "the bare id already matches", which is exactly what trying
/// candidate (2) directly gives for free, with no invented casing step
/// (contrast the published state aliases, which stay a hand list because
/// those entries are genuine renames with no shared rule at all).
///
/// `bindings.ts`'s `ownerKeyCandidates` builds the same two keys, in the
/// same order, from the same table.
fn candidate_keys(monster: &str, state_id: &str) -> [String; 2] {
    [
        format!("{monster}:{state_id}_MOVE"),
        format!("{monster}:{state_id}"),
    ]
}

/// Engine `(monster, state)` pairs that can never produce a hit effect, so
/// having no binding row is correct rather than a gap. Each reason names
/// what was actually checked in the C# method body — a buff move can still
/// carry `WithHitFx` for a self-target flourish, so "it's a buff" alone is
/// not evidence. Keyed on the key this test actually looks up (i.e.
/// post-monster-alias ([`Aliases`]) — an entry keyed `"FlailKnight:…"`
/// exempts BOTH `MonsterKind::FlailKnight` and `MonsterKind::MysteriousKnight`,
/// and one keyed `"DecimillipedeSegment:…"` exempts all four Decimillipede
/// variants, since they all resolve to the same lookup key).
const EXEMPT: &[(&str, &str)] = &[
    // ── pure buff/debuff/defend, no hit-vfx call in C# ──
    ("FrogKnight:FOR_THE_QUEEN_MOVE", "pure self-buff, no hit-vfx call in C#"),
    ("Nibbit:HISS_MOVE", "pure self-buff (Strength), no hit-vfx call in C#"),
    (
        "FuzzyWurmCrawler:INHALE_MOVE",
        "pure self-buff (Strength), no hit-vfx call in C#",
    ),
    ("CubexConstruct:CHARGE_UP_MOVE", "pure self-buff, no hit-vfx call in C#"),
    (
        "Flyconid:VULNERABLE_SPORES_MOVE",
        "pure debuff (Vulnerable), no hit-vfx call in C#",
    ),
    ("Mawler:ROAR_MOVE", "pure debuff (Vulnerable), no hit-vfx call in C#"),
    (
        "SlitheringStrangler:CONSTRICT_MOVE",
        "pure debuff (Constrict), no hit-vfx call in C#",
    ),
    ("BruteRubyRaider:ROAR_MOVE", "pure self-buff, no hit-vfx call in C#"),
    ("CrossbowRubyRaider:RELOAD_MOVE", "pure Defend, no hit-vfx call in C#"),
    ("Vantom:PREPARE_MOVE", "pure self-buff, no hit-vfx call in C#"),
    ("CeremonialBeast:STAMP_MOVE", "pure self-buff (Plow), no hit-vfx call in C#"),
    ("Fogmog:ILLUSION_MOVE", "Summon, no hit-vfx call in C#"),
    ("Noisebot:NOISE_MOVE", "StatusCard, no hit-vfx call in C#"),
    ("Rocket:RECHARGE_MOVE_MOVE", "pure Sleep, no hit-vfx call in C#"),
    (
        "SlimedBerserker:VOMIT_ICHOR_MOVE_MOVE",
        "StatusCard, no hit-vfx call in C#",
    ),
    (
        "WaterfallGiant:SIPHON_MOVE",
        "pure Heal+self-buff, no hit-vfx call in C#",
    ),
    ("KinPriest:RITUAL_MOVE_MOVE", "pure self-buff, no hit-vfx call in C#"),
    ("KinFollower:POWER_DANCE_MOVE", "pure self-buff, no hit-vfx call in C#"),
    ("BowlbugNectar:BUFF_MOVE", "pure self-buff, no hit-vfx call in C#"),
    (
        "BowlbugSilk:TOXIC_SPIT_MOVE",
        "pure debuff (Weak), no hit-vfx call in C#",
    ),
    ("Exoskeleton:ENRAGE_MOVE", "pure self-buff, no hit-vfx call in C#"),
    (
        "HunterKiller:TENDERIZING_GOOP_MOVE_MOVE",
        "pure debuff (Tender), no hit-vfx call in C#",
    ),
    (
        "LouseProgenitor:CURL_AND_GROW_MOVE_MOVE",
        "pure Defend+self-buff, no hit-vfx call in C#",
    ),
    (
        "Entomancer:PHEROMONE_SPIT_MOVE",
        "pure self-buff, no hit-vfx call in C#",
    ),
    (
        "TheInsatiable:SALIVATE_MOVE",
        "pure self-buff, no hit-vfx call in C#",
    ),
    (
        "KnowledgeDemon:CURSE_OF_KNOWLEDGE_MOVE",
        "pure debuff (Weak), no hit-vfx call in C#",
    ),
    ("Crusher:ADAPT_MOVE", "pure self-buff, no hit-vfx call in C#"),
    ("Rocket:CHARGE_UP_MOVE_MOVE", "pure self-buff, no hit-vfx call in C#"),
    (
        "ScrollOfBiting:MORE_TEETH_MOVE",
        "pure self-buff, no hit-vfx call in C#",
    ),
    ("TurretOperator:RELOAD_MOVE_MOVE", "pure self-buff, no hit-vfx call in C#"),
    (
        "Axebot:BOOT_UP_MOVE",
        "pure Defend+self-buff, no hit-vfx call in C#",
    ),
    ("Guardbot:GUARD_MOVE", "pure Defend, no hit-vfx call in C#"),
    ("Fabricator:FABRICATE_MOVE", "Summon, no hit-vfx call in C#"),
    (
        "SlimedBerserker:LEECHING_HUG_MOVE_MOVE",
        "pure debuff+self-buff, no hit-vfx call in C#",
    ),
    (
        "TheLost:DEBILITATING_SMOG_MOVE",
        "pure debuff+self-buff, no hit-vfx call in C#",
    ),
    (
        "TheForgotten:MIASMA_MOVE",
        "pure debuff+Defend+self-buff, no hit-vfx call in C#",
    ),
    // Also exempts MysteriousKnight (shares this row via the monster alias).
    ("FlailKnight:WAR_CHANT_MOVE", "pure self-buff, no hit-vfx call in C#"),
    ("MagiKnight:DAMPEN_MOVE", "pure debuff (Dampen), no hit-vfx call in C#"),
    ("MagiKnight:PREP_MOVE", "pure Defend, no hit-vfx call in C#"),
    ("SpectralKnight:HEX_MOVE", "pure debuff (Hex), no hit-vfx call in C#"),
    (
        "MechaKnight:WINDUP_MOVE",
        "pure Defend+self-buff, no hit-vfx call in C#",
    ),
    (
        "Aeonglass:INCREASING_INTENSITY_MOVE_MOVE",
        "StatusCard+self-buff, no hit-vfx call in C#",
    ),
    (
        "Queen:BURN_BRIGHT_FOR_ME_MOVE_MOVE",
        "pure self-buff+Defend, no hit-vfx call in C#",
    ),
    ("Queen:ENRAGE_MOVE_MOVE", "pure self-buff, no hit-vfx call in C#"),
    (
        "Queen:PUPPET_STRINGS_MOVE_MOVE",
        "CardDebuffIntent, no hit-vfx call in C#",
    ),
    (
        "Queen:YOURE_MINE_MOVE_MOVE",
        "pure debuff (Frail/Weak/Vulnerable), no hit-vfx call in C#; real C# id is `YOU_ARE_MINE_MOVE`",
    ),
    (
        "TestSubject:BURNING_GROWL_MOVE",
        "StatusCard+self-buff, no hit-vfx call in C#",
    ),
    ("FakeMerchantMonster:ENRAGE_MOVE", "pure self-buff, no hit-vfx call in C#"),
    (
        "PunchConstruct:READY_MOVE",
        "pure Defend, no hit-vfx call in C#",
    ),
    ("CorpseSlug:GOOP_MOVE", "pure debuff (Frail), no hit-vfx call in C#"),
    (
        "Seapunk:BUBBLE_BURP_MOVE",
        "pure self-buff+Defend, no hit-vfx call in C#",
    ),
    (
        "CalcifiedCultist:INCANTATION_MOVE",
        "pure self-buff (Ritual), no hit-vfx call in C#",
    ),
    (
        "DampCultist:INCANTATION_MOVE",
        "pure self-buff (Ritual), no hit-vfx call in C#",
    ),
    ("SewerClam:PRESSURIZE_MOVE", "pure self-buff, no hit-vfx call in C#"),
    (
        "TwoTailedRat:SCREECH_MOVE",
        "pure debuff (Frail), no hit-vfx call in C#",
    ),
    (
        "PhantasmalGardener:ENLARGE_MOVE",
        "pure self-buff, no hit-vfx call in C#",
    ),
    (
        "WaterfallGiant:PRESSURIZE_MOVE",
        "pure self-buff (SteamEruption), no hit-vfx call in C#",
    ),
    (
        "SoulFysh:FADE_MOVE",
        "pure self-buff (Intangible), no hit-vfx call in C#",
    ),
    (
        "LagavulinMatriarch:SOUL_SIPHON_MOVE",
        "pure debuff+self-buff, no hit-vfx call in C#",
    ),
    (
        "ShrinkerBeetle:SHRINK_MOVE",
        "pure debuff-strong, no hit-vfx call in C#; real C# id is `SHRINKER_MOVE`",
    ),
    (
        "SpinyToad:SPINES_MOVE",
        "pure self-buff (Thorns), no hit-vfx call in C#; real C# id is `PROTRUDING_SPIKES_MOVE`",
    ),
    // ── Attack intent, but the C# move only chains `.WithAttackerFx` (a
    // sound cue) — no `.WithHitFx`/`.WithHitVfxNode` override at all, so it
    // relies on `DamageCmd.Attack`'s own default hit visual rather than a
    // table row. The renderer's fallback chain (not this table) already
    // reproduces that default. ──
    (
        "SnappingJaxfruit:ENERGY_ORB_MOVE_MOVE",
        "Attack intent, C# chains no WithHitFx/WithHitVfxNode override",
    ),
    (
        "LouseProgenitor:POUNCE_MOVE_MOVE",
        "Attack intent, C# chains no WithHitFx/WithHitVfxNode override",
    ),
    (
        "SlimedBerserker:FURIOUS_PUMMELING_MOVE_MOVE",
        "MultiAttack intent, C# chains no WithHitFx/WithHitVfxNode override",
    ),
    (
        "SlimedBerserker:SMOTHER_MOVE_MOVE",
        "Attack intent, C# chains no WithHitFx/WithHitVfxNode override",
    ),
    (
        "SpectralKnight:SOUL_FLAME_MOVE",
        "MultiAttack intent, C# chains no WithHitFx/WithHitVfxNode override",
    ),
    (
        "SoulNexus:DRAIN_LIFE_MOVE_MOVE",
        "Attack intent, C# chains no WithHitFx/WithHitVfxNode override",
    ),
    (
        "TestSubject:MULTI_CLAW_MOVE",
        "MultiAttack intent, C# chains no WithHitFx/WithHitVfxNode override",
    ),
    (
        "TestSubject:PHASE3_LACERATE_MOVE",
        "MultiAttack intent, C# chains no WithHitFx/WithHitVfxNode override",
    ),
    (
        "WaterfallGiant:EXPLODE_MOVE",
        "DeathBlow (self-destruct), C# chains no WithHitFx/WithHitVfxNode override",
    ),
    // ── Sleep / stun / escape / cycle-reset / silent, no hit-vfx call ──
    (
        "OwlMagistrate:JUDICIAL_FLIGHT_MOVE",
        "escape move, no hit-vfx call in C#",
    ),
    // NOTE: no `OwlMagistrate:VERDICT_MOVE` entry — `VERDICT` (OwlMagistrate.cs:98)
    // is a real `SingleAttackIntent` move with a `WithHitFx` call and a
    // bare-id table row (`OwlMagistrate:VERDICT`); an earlier draft of
    // this list mischaracterized it as a no-intent cycle-reset. It never
    // needed an exemption — `candidate_keys`'s bare-id form resolves it —
    // and `DEAD_EXEMPT_ENTRIES_MUST_BE_CONSULTED` below is what caught
    // this one being both dead and wrong.
    ("Wriggler:SPAWNED_MOVE_MOVE", "pure Stun, no hit-vfx call in C#"),
    ("ThievingHopper:ESCAPE_MOVE_MOVE", "Escape intent, no hit-vfx call in C#"),
    (
        "ThievingHopper:FLUTTER_MOVE_MOVE",
        "pure self-buff (Flutter), no hit-vfx call in C#",
    ),
    (
        "Tunneler:BURROW_MOVE_MOVE",
        "pure self-buff+Defend (burrow), no hit-vfx call in C#",
    ),
    ("Chomper:SCREECH_MOVE", "StatusCard, no hit-vfx call in C#"),
    ("Myte:TOXIC_MOVE_MOVE", "StatusCard, no hit-vfx call in C#"),
    ("ToughEgg:HATCH_MOVE_MOVE", "Summon, no hit-vfx call in C#"),
    ("Ovicopter:LAY_EGGS_MOVE_MOVE", "Summon, no hit-vfx call in C#"),
    (
        "Ovicopter:NUTRITIONAL_PASTE_MOVE_MOVE",
        "pure self-buff, no hit-vfx call in C#",
    ),
    ("SlumberingBeetle:SNORE_MOVE_MOVE", "pure Sleep, no hit-vfx call in C#"),
    ("TheObscura:ILLUSION_MOVE_MOVE", "Summon, no hit-vfx call in C#"),
    (
        "TheObscura:SAIL_MOVE_MOVE",
        "pure self-buff (`WailMove`), no hit-vfx call in C#",
    ),
    // Also exempts DecimillipedeSegmentFront/Middle/Back (share this row
    // via the monster alias).
    ("DecimillipedeSegment:DEAD_MOVE", "no intent, no hit-vfx call in C#"),
    ("DecimillipedeSegment:REATTACH_MOVE", "pure Heal, no hit-vfx call in C#"),
    (
        "Osty:HAUNT_MOVE",
        "C#'s ONLY state is a bare `NOTHING_MOVE` self-loop with no intent \
         argument at all — no hit-vfx call possible",
    ),
    (
        "TestSubject:RESPAWN_MOVE",
        "pure Heal+self-buff, no hit-vfx call in C#",
    ),
    // Test-only synthetic monsters and silent bosses — C# `MoveState`
    // bodies (`NothingMove`/`PokeMove`) never call
    // `WithHitFx`/`WithHitVfxNode`/`Create` at all.
    ("Architect:NOTHING_MOVE", "silent monster, no hit-vfx call in C#"),
    ("Byrdpip:NOTHING_MOVE_MOVE", "silent monster, no hit-vfx call in C#"),
    ("PaelsLegion:NOTHING_MOVE_MOVE", "silent monster, no hit-vfx call in C#"),
    ("BattleFriendV1:NOTHING_MOVE_MOVE", "silent monster, no hit-vfx call in C#"),
    ("BattleFriendV2:NOTHING_MOVE_MOVE", "silent monster, no hit-vfx call in C#"),
    ("BattleFriendV3:NOTHING_MOVE_MOVE", "silent monster, no hit-vfx call in C#"),
    ("BigDummy:NOTHING_MOVE", "test fixture, no hit-vfx call in C#"),
    ("OneHpMonster:NOTHING_MOVE", "test fixture, no hit-vfx call in C#"),
    ("TenHpMonster:NOTHING_MOVE", "test fixture, no hit-vfx call in C#"),
    (
        "SingleAttackMoveMonster:POKE_MOVE",
        "test fixture, no hit-vfx call in C#",
    ),
    (
        "MultiAttackMoveMonster:POKE_MOVE",
        "test fixture, no hit-vfx call in C#",
    ),
    ("Toadpole:SPIKEN_MOVE_MOVE", "pure self-buff (Thorns), no hit-vfx call in C#"),
    ("SneakyGremlin:SPAWNED_MOVE", "pure Stun, no hit-vfx call in C#"),
    ("FatGremlin:FLEE_MOVE", "Escape intent, no hit-vfx call in C#"),
    ("FatGremlin:SPAWNED_MOVE", "pure Stun, no hit-vfx call in C#"),
    ("TwoTailedRat:CALL_FOR_BACKUP_MOVE", "Summon, no hit-vfx call in C#"),
    ("TerrorEel:STUN_MOVE_MOVE", "pure Stun, no hit-vfx call in C#"),
    ("WaterfallGiant:ABOUT_TO_BLOW_MOVE", "pure Stun, no hit-vfx call in C#"),
    ("LagavulinMatriarch:SLEEP_MOVE", "pure Sleep, no hit-vfx call in C#"),
    // ── Stunned-turn overlays. `CreatureCmd.Stun(creature, stunMove, ...)`
    // (`CreatureCmd.cs:885-906`) wraps EVERY stunned turn, of every kind, in
    // a shared closure that unconditionally spawns `NStunnedVfx` on the
    // creature and adds it to its VFX container — BEFORE running the
    // caller-supplied `stunMove` body. So "the move body calls no
    // hit-vfx" is true but incomplete for these: there IS a real overlay,
    // it's just not attached by the move at all, and not reachable from a
    // move-keyed table by construction — it comes from a shared command
    // wrapper, fires for the same visual reason regardless of which power
    // or move triggered it, and lives outside the `Cards/*.cs` /
    // `Monsters/*.cs` files this extractor walks. Design spec's own
    // out-of-scope list assigns exactly this kind of thing — "block broken,
    // blocked text, power applied/removed, monster-death dissolve" — to
    // sub-project 3's creature-event VFX; a stun overlay belongs in that
    // same bucket, not this move-binding table.
    //
    // Every entry below cites its own trigger and file:line; NONE of them
    // needs to repeat the shared-wrapper explanation above — that repeated
    // paragraph is exactly what review round 2 flagged as needing factoring
    // out. This is a CLOSED set, swept exhaustively (not found one-by-one):
    // `grep -rn "CreatureCmd.Stun(" decompiled/v0.107.1/` returns 12 call
    // sites total. Two are not real calls / not move-keyed at all
    // (`Creature.cs:517` is an XML `<see cref>` doc reference; `Whistle.cs:33`
    // is a CARD stunning an arbitrary target via the argument-less overload,
    // not tied to any one `MonsterKind`'s own state). Of the other 10, four
    // use the argument-less/string-only overload (`ImbalancedPower.cs:24`'s
    // non-`BowlbugRock` branch, `PlowPower.cs:49`'s non-`CeremonialBeast`
    // branch, `ShriekPower.cs:30`) — these re-wrap WHATEVER move the
    // creature already had in flight, not a specific named method, so they
    // don't correspond to any one state id either. The remaining SEVEN pass
    // a specific monster method as `stunMove`, and each of those seven is
    // exactly one exemption below — the two found in round 1 beyond the
    // four the reviewer originally named, plus this round's `CeremonialBeast`.
    (
        "ThievingHopper:STUNNED_MOVE_MOVE",
        "Stun intent; reached via `FlutterPower.AfterDamageReceived` -> \
         `CreatureCmd.Stun(Owner, StunnedMove, nextState)` \
         (FlutterPower.cs:48) — see act2_hive.rs:1038",
    ),
    (
        "CorpseSlug:STUNNED_MOVE",
        "runtime-synthesized, no static MoveState(...) at all — \
         `RavenousPower.AfterDeath` -> `CreatureCmd.Stun(Owner, \
         StunnedMove)` (RavenousPower.cs:32); see act1_underdocks.rs:361",
    ),
    (
        "LagavulinMatriarch:WAKE_UP_MOVE_MOVE",
        "runtime-synthesized, no static MoveState(...) at all — \
         `AsleepPower.AfterDamageReceived` -> `CreatureCmd.Stun(Owner, \
         WakeUpMove, \"SLASH_MOVE\")` (AsleepPower.cs:33); see \
         act1_underdocks.rs:2392",
    ),
    (
        "SlumberingBeetle:WAKE_UP_MOVE_MOVE",
        "runtime-synthesized, no static MoveState(...) at all \
         (`WakeUpMove`, SlumberingBeetle.cs:96, is a plain method) — \
         `SlumberPower.AfterDamageReceived` -> `CreatureCmd.Stun(Owner, \
         WakeUpMove, \"ROLL_OUT_MOVE\")` (SlumberPower.cs:29)",
    ),
    (
        "Tunneler:DIZZY_MOVE_MOVE",
        "Stun intent; reached via `BurrowedPower.AfterBlockBroken` -> \
         `CreatureCmd.Stun(Owner, StillDizzyMove, \"BITE_MOVE\")` \
         (BurrowedPower.cs:32)",
    ),
    (
        "BowlbugRock:DIZZY_MOVE",
        "Stun intent; `ImbalancedPower.AfterDamageGiven`'s `BowlbugRock` \
         branch flags `IsOffBalance`, and the monster's own `DizzyMove` \
         then calls `CreatureCmd.Stun(Creature, DizzyMove)` \
         (BowlbugRock.cs:100)",
    ),
    (
        "CeremonialBeast:STUN_MOVE",
        "Stun intent; reached via `PlowPower.AfterDamageReceived`'s \
         `CeremonialBeast` branch -> `CreatureCmd.Stun(Owner, StunnedMove, \
         BeastCryState.StateId)` (PlowPower.cs:45)",
    ),
    // ── BygoneEffigy: two Sleep states and a Buff wake, all confirmed
    // no-hit-vfx-call; state-id renames noted for anyone chasing them. ──
    (
        "BygoneEffigy:INITIAL_SLEEP_MOVE",
        "pure Sleep, no hit-vfx call in C# (`InitialSleepMove`, real id \
         `SLEEP_MOVE`)",
    ),
    (
        "BygoneEffigy:SLEEP_MOVE",
        "pure Sleep, no hit-vfx call in C# (`SleepMove`, real id \
         `SLEEP_MOVE_2`)",
    ),
    ("BygoneEffigy:WAKE_MOVE", "pure self-buff, no hit-vfx call in C#"),
];

fn table_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../sts2-wiki/data/v0.111.0/vfx_bindings.json")
}

fn load_json() -> serde_json::Value {
    let path = table_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
}

fn load_table(json: &serde_json::Value) -> HashSet<String> {
    json["bindings"]
        .as_array()
        .expect("bindings must be an array")
        .iter()
        .filter(|b| b["kind"] == "move")
        .map(|b| {
            b["owner"]
                .as_str()
                .expect("owner must be a string")
                .to_string()
        })
        .collect()
}

/// Reads one `aliases` sub-table: `{ "<key>": { "<field>": "...", "reason":
/// "..." } }`. A missing section is a hard failure, not an empty map — the
/// published table not carrying these means the renderer is not applying
/// them either, which is exactly the silent state this gate exists to catch.
fn load_alias_map(json: &serde_json::Value, section: &str, field: &str) -> HashMap<String, String> {
    json["aliases"][section]
        .as_object()
        .unwrap_or_else(|| {
            panic!(
                "vfx_bindings.json has no `aliases.{section}` object — regenerate it \
                 with `just extract-vfx-bindings` (sts2-wiki)"
            )
        })
        .iter()
        .map(|(key, value)| {
            let target = value[field]
                .as_str()
                .unwrap_or_else(|| panic!("aliases.{section}[{key}] has no `{field}` string"));
            (key.clone(), target.to_string())
        })
        .collect()
}

fn load_aliases(json: &serde_json::Value) -> Aliases {
    Aliases {
        monsters: load_alias_map(json, "monsters", "monster"),
        states: load_alias_map(json, "states", "state"),
    }
}

#[test]
fn every_engine_move_id_resolves_to_a_binding_row() {
    // Extracted from the game (`just extract-vfx-bindings`), not published.
    if !table_path().is_file() {
        eprintln!("skipping: {} not present", table_path().display());
        return;
    }
    let json = load_json();
    let table = load_table(&json);
    let aliases = load_aliases(&json);
    let exempt: HashMap<&str, &str> = EXEMPT.iter().copied().collect();

    let mut unmatched: Vec<String> = Vec::new();
    // Which `EXEMPT` keys actually got consulted (i.e. no candidate/alias
    // resolved first) AND matched — see the dead-entry check below. Records
    // the &'static str straight from `EXEMPT` (via `get_key_value`), not the
    // locally-built `exempt_key`, so the final diff is against `EXEMPT`
    // itself.
    let mut consulted_exempt: HashSet<&str> = HashSet::new();
    let mut checked = 0usize;

    for kind in MonsterKind::ALL_KNOWN {
        let ai = build_ai(*kind, MonsterSpawnArgs::None, SpawnPosition::default());
        let mut moves_for_kind = 0usize;
        for (_, state) in ai.states.iter() {
            let MonsterState::Move(mv) = state else {
                continue;
            };
            checked += 1;
            moves_for_kind += 1;
            let monster = aliases.monster_key(*kind);
            let candidates = candidate_keys(monster, mv.id);
            let exempt_key = candidates[0].clone();

            if candidates.iter().any(|k| table.contains(k)) {
                continue;
            }
            let real_id = aliases.state_id(monster, mv.id);
            if real_id != mv.id {
                let aliased = format!("{monster}:{real_id}");
                if table.contains(&aliased) {
                    continue;
                }
                unmatched.push(format!(
                    "{exempt_key} (aliased to {aliased}, which is ALSO missing)"
                ));
                continue;
            }
            if let Some((&k, _)) = exempt.get_key_value(exempt_key.as_str()) {
                consulted_exempt.insert(k);
                continue;
            }
            unmatched.push(exempt_key);
        }
        assert!(
            moves_for_kind > 0,
            "MonsterKind::{kind:?} built a state table with zero Move \
             states — a `build_ai` match arm may be silently returning an \
             empty/degenerate AI for this kind, which would make this gate \
             pass for it vacuously rather than actually checking it"
        );
    }

    assert!(checked > 0, "no monster-AI move states were walked at all");
    assert!(
        checked >= 300,
        "only {checked} move states were walked across the whole roster — \
         suspiciously low (356 as of this test's introduction, and the \
         committed table alone has 225 move rows) for a gate that claims \
         to check ~120 MonsterKinds; a dispatch or iteration bug would \
         show up here as a pass that checked almost nothing"
    );

    // Every `EXEMPT` entry must be reachable and actually needed — a dead
    // entry (one no engine state ever looks up, or one a real table row
    // resolves before `EXEMPT` is ever consulted) is invisible dead weight
    // at best and a wrong, unverified claim at worst: `OwlMagistrate:VERDICT_MOVE`
    // was exactly this — a real attack move with a real bare-id table row,
    // exempted under a fabricated "no intent" reason that nothing ever
    // caught because the bare candidate always resolved first. This
    // assertion makes that class of bug fail loudly instead of sitting
    // unnoticed.
    let mut dead_exempt: Vec<&str> = EXEMPT
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !consulted_exempt.contains(k))
        .collect();
    dead_exempt.sort_unstable();
    assert!(
        dead_exempt.is_empty(),
        "{} EXEMPT entries were never consulted by any engine state — \
         delete them (a real table row or alias already resolves the key \
         first, or no MonsterKind ever produces that state at all):\n{}",
        dead_exempt.len(),
        dead_exempt.join("\n")
    );

    unmatched.sort();
    assert!(
        unmatched.is_empty(),
        "{} engine move ids have no binding row under any candidate key. \
         Either the extractor missed the C# call site, the id needs a \
         state-alias entry in `extract_vfx_bindings.py`, or the state \
         genuinely cannot produce a hit \
         effect (add an EXEMPT entry with a one-line reason):\n{}",
        unmatched.len(),
        unmatched.join("\n")
    );
}
