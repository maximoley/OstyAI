//! Per-card module aggregator (Necrobinder). Rebuilt from scratch (C#-first) via
//! the character-port skill's dynamic workflow. ALL 88 cards of the C# pool are
//! now wired, in `GenerateAllCards()` (alphabetical) order. Some
//! wired cards apply powers whose listeners are not built yet — the card plays
//! and is faithful on its direct effect, but the power is currently inert (the
//! remaining power-listener bodies are the follow-up Necrobinder powers bead).
//! The Osty pet system IS wired (summon_osty / kill_osty / DieForYou redirect),
//! so the Osty cards (Bone Shards, Poke, Unleash, Sic Em, Dirge) and the Osty-
//! summon powers (SicEmPower / DevourLifePower) are live.
//! NECROBINDER_SPECS in spec_index.rs is kept in C# `GenerateAllCards()` order.
#![allow(unused_imports)]

use sts2_core::card_effects::{self, PlayCardFn};
use sts2_core::cards::CardId;

pub mod helpers;
pub mod prelude;
pub mod specs;

// ─── card modules (waves 1+2, alphabetical) ─────────────────────────────────
pub mod afterlife;
pub mod banshees_cry;
pub mod blight_strike;
pub mod bodyguard;
pub mod bone_shards;
pub mod borrowed_time;
pub mod bury;
pub mod calcify;
pub mod call_of_the_void;
pub mod capture_spirit;
pub mod cleanse;
pub mod countdown;
pub mod danse_macabre;
pub mod death_march;
pub mod deathbringer;
pub mod deaths_door;
pub mod debilitate;
pub mod defend_necrobinder;
pub mod defile;
pub mod defy;
pub mod delay;
pub mod demesne;
pub mod devour_life;
pub mod dirge;
pub mod drain_power;
pub mod dredge;
pub mod eidolon;
pub mod end_of_days;
pub mod enfeebling_touch;
pub mod eradicate;
pub mod fear;
pub mod fetch;
pub mod flatten;
pub mod forbidden_grimoire;
pub mod friendship;
pub mod glimpse_beyond;
pub mod grave_warden;
pub mod graveblast;
pub mod hang;
pub mod haunt;
pub mod high_five;
pub mod invoke;
pub mod legion_of_bone;
pub mod lethality;
pub mod melancholy;
pub mod misery;
pub mod necro_mastery;
pub mod negative_pulse;
pub mod neurosurge;
pub mod no_escape;
pub mod oblivion;
pub mod pagestorm;
pub mod parse;
pub mod poke;
pub mod protector;
pub mod pull_aggro;
pub mod pull_from_below;
pub mod putrefy;
pub mod rattle;
pub mod reanimate;
pub mod reap;
pub mod reaper_form;
pub mod reave;
pub mod right_hand_hand;
pub mod sacrifice;
pub mod scourge;
pub mod sculpting_strike;
pub mod seance;
pub mod sentry_mode;
pub mod severance;
pub mod shared_fate;
pub mod shroud;
pub mod sic_em;
pub mod sleight_of_flesh;
pub mod snap;
pub mod soul_storm;
pub mod sow;
pub mod spirit_of_ash;
pub mod spur;
pub mod squeeze;
pub mod strike_necrobinder;
pub mod the_scythe;
pub mod times_up;
pub mod transfigure;
pub mod undeath;
pub mod unleash;
pub mod veilpiercer;
pub mod wisp;

// ─── dispatch POOL (C# pool order) ──────────────────────────────────────────
const POOL: &[(CardId, PlayCardFn)] = &[
    (CardId::Afterlife, afterlife::play_afterlife as PlayCardFn),
    (
        CardId::BansheesCry,
        banshees_cry::play_banshees_cry as PlayCardFn,
    ),
    (
        CardId::BlightStrike,
        blight_strike::play_blight_strike as PlayCardFn,
    ),
    (CardId::Bodyguard, bodyguard::play_bodyguard as PlayCardFn),
    (
        CardId::BoneShards,
        bone_shards::play_bone_shards as PlayCardFn,
    ),
    (
        CardId::BorrowedTime,
        borrowed_time::play_borrowed_time as PlayCardFn,
    ),
    (CardId::Bury, bury::play_bury as PlayCardFn),
    (CardId::CalcifyCard, calcify::play_calcify as PlayCardFn),
    (
        CardId::CallOfTheVoid,
        call_of_the_void::play_call_of_the_void as PlayCardFn,
    ),
    (
        CardId::CaptureSpirit,
        capture_spirit::play_capture_spirit as PlayCardFn,
    ),
    (CardId::Cleanse, cleanse::play_cleanse as PlayCardFn),
    (
        CardId::CountdownCard,
        countdown::play_countdown as PlayCardFn,
    ),
    (
        CardId::DanseMacabre,
        danse_macabre::play_danse_macabre as PlayCardFn,
    ),
    (
        CardId::DeathMarch,
        death_march::play_death_march as PlayCardFn,
    ),
    (
        CardId::Deathbringer,
        deathbringer::play_deathbringer as PlayCardFn,
    ),
    (
        CardId::DeathsDoor,
        deaths_door::play_deaths_door as PlayCardFn,
    ),
    (
        CardId::DebilitateCard,
        debilitate::play_debilitate as PlayCardFn,
    ),
    (
        CardId::DefendNecrobinder,
        defend_necrobinder::play_defend_necrobinder as PlayCardFn,
    ),
    (CardId::Defile, defile::play_defile as PlayCardFn),
    (CardId::Defy, defy::play_defy as PlayCardFn),
    (CardId::Delay, delay::play_delay as PlayCardFn),
    (CardId::Demesne, demesne::play_demesne as PlayCardFn),
    (
        CardId::DevourLifeCard,
        devour_life::play_devour_life as PlayCardFn,
    ),
    (CardId::Dirge, dirge::play_dirge as PlayCardFn),
    (
        CardId::DrainPower,
        drain_power::play_drain_power as PlayCardFn,
    ),
    (CardId::Dredge, dredge::play_dredge as PlayCardFn),
    (CardId::Eidolon, eidolon::play_eidolon as PlayCardFn),
    (
        CardId::EndOfDays,
        end_of_days::play_end_of_days as PlayCardFn,
    ),
    (
        CardId::EnfeeblingTouch,
        enfeebling_touch::play_enfeebling_touch as PlayCardFn,
    ),
    (CardId::Eradicate, eradicate::play_eradicate as PlayCardFn),
    (CardId::Fear, fear::play_fear as PlayCardFn),
    (CardId::Fetch, fetch::play_fetch as PlayCardFn),
    (CardId::Flatten, flatten::play_flatten as PlayCardFn),
    (
        CardId::ForbiddenGrimoire,
        forbidden_grimoire::play_forbidden_grimoire as PlayCardFn,
    ),
    (
        CardId::Friendship,
        friendship::play_friendship as PlayCardFn,
    ),
    (
        CardId::GlimpseBeyond,
        glimpse_beyond::play_glimpse_beyond as PlayCardFn,
    ),
    (
        CardId::GraveWarden,
        grave_warden::play_grave_warden as PlayCardFn,
    ),
    (
        CardId::Graveblast,
        graveblast::play_graveblast as PlayCardFn,
    ),
    (CardId::Hang, hang::play_hang as PlayCardFn),
    (CardId::Haunt, haunt::play_haunt as PlayCardFn),
    (CardId::HighFive, high_five::play_high_five as PlayCardFn),
    (CardId::Invoke, invoke::play_invoke as PlayCardFn),
    (
        CardId::LegionOfBone,
        legion_of_bone::play_legion_of_bone as PlayCardFn,
    ),
    (
        CardId::LethalityCard,
        lethality::play_lethality as PlayCardFn,
    ),
    (
        CardId::Melancholy,
        melancholy::play_melancholy as PlayCardFn,
    ),
    (CardId::Misery, misery::play_misery as PlayCardFn),
    (
        CardId::NecroMasteryCard,
        necro_mastery::play_necro_mastery as PlayCardFn,
    ),
    (
        CardId::NegativePulse,
        negative_pulse::play_negative_pulse as PlayCardFn,
    ),
    (
        CardId::Neurosurge,
        neurosurge::play_neurosurge as PlayCardFn,
    ),
    (CardId::NoEscape, no_escape::play_no_escape as PlayCardFn),
    (CardId::Oblivion, oblivion::play_oblivion as PlayCardFn),
    (CardId::Pagestorm, pagestorm::play_pagestorm as PlayCardFn),
    (CardId::Parse, parse::play_parse as PlayCardFn),
    (CardId::Poke, poke::play_poke as PlayCardFn),
    (CardId::Protector, protector::play_protector as PlayCardFn),
    (CardId::PullAggro, pull_aggro::play_pull_aggro as PlayCardFn),
    (
        CardId::PullFromBelow,
        pull_from_below::play_pull_from_below as PlayCardFn,
    ),
    (CardId::Putrefy, putrefy::play_putrefy as PlayCardFn),
    (CardId::Rattle, rattle::play_rattle as PlayCardFn),
    (CardId::Reanimate, reanimate::play_reanimate as PlayCardFn),
    (CardId::Reap, reap::play_reap as PlayCardFn),
    (
        CardId::ReaperForm,
        reaper_form::play_reaper_form as PlayCardFn,
    ),
    (CardId::Reave, reave::play_reave as PlayCardFn),
    (
        CardId::RightHandHand,
        right_hand_hand::play_right_hand_hand as PlayCardFn,
    ),
    (CardId::Sacrifice, sacrifice::play_sacrifice as PlayCardFn),
    (CardId::Scourge, scourge::play_scourge as PlayCardFn),
    (
        CardId::SculptingStrike,
        sculpting_strike::play_sculpting_strike as PlayCardFn,
    ),
    (CardId::Seance, seance::play_seance as PlayCardFn),
    (
        CardId::SentryMode,
        sentry_mode::play_sentry_mode as PlayCardFn,
    ),
    (CardId::Severance, severance::play_severance as PlayCardFn),
    (
        CardId::SharedFate,
        shared_fate::play_shared_fate as PlayCardFn,
    ),
    (CardId::Shroud, shroud::play_shroud as PlayCardFn),
    (CardId::SicEm, sic_em::play_sic_em as PlayCardFn),
    (
        CardId::SleightOfFlesh,
        sleight_of_flesh::play_sleight_of_flesh as PlayCardFn,
    ),
    (CardId::Snap, snap::play_snap as PlayCardFn),
    (CardId::SoulStorm, soul_storm::play_soul_storm as PlayCardFn),
    (CardId::Sow, sow::play_sow as PlayCardFn),
    (
        CardId::SpiritOfAsh,
        spirit_of_ash::play_spirit_of_ash as PlayCardFn,
    ),
    (CardId::Spur, spur::play_spur as PlayCardFn),
    (CardId::Squeeze, squeeze::play_squeeze as PlayCardFn),
    (
        CardId::StrikeNecrobinder,
        strike_necrobinder::play_strike_necrobinder as PlayCardFn,
    ),
    (CardId::TheScythe, the_scythe::play_the_scythe as PlayCardFn),
    (CardId::TimesUp, times_up::play_times_up as PlayCardFn),
    (
        CardId::Transfigure,
        transfigure::play_transfigure as PlayCardFn,
    ),
    (CardId::Undeath, undeath::play_undeath as PlayCardFn),
    (CardId::Unleash, unleash::play_unleash as PlayCardFn),
    (
        CardId::Veilpiercer,
        veilpiercer::play_veilpiercer as PlayCardFn,
    ),
    (CardId::Wisp, wisp::play_wisp as PlayCardFn),
];

pub use helpers::necrobinder_starter_deck;

/// Late-effect handlers fired by `card_effects::fire_late_effects` after every
/// card play (C# `AfterCardPlayedLate`). Right Hand Hand returns to hand from
/// the discard pile whenever the player spends ≥2 energy on a card.
const LATE_EFFECTS: &[(CardId, card_effects::LateEffectFn)] = &[(
    CardId::RightHandHand,
    right_hand_hand::right_hand_hand_late_effect as card_effects::LateEffectFn,
)];

/// Install every Necrobinder card handler + late-effect. Idempotent.
pub fn install_pool() {
    card_effects::register_many(POOL);
    card_effects::register_many_late_effects(LATE_EFFECTS);
}
