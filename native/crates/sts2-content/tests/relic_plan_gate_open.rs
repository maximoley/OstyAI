//! The per-combat relic plan (M01) on the cases the anchor fixtures never
//! hold: ToyBox-melted relics, GamblingChip's turn-one suspension behind a
//! melted slot, and a duplicated relic kind on the obtain-order channel.
//!
//! After the content install, the owned / equip-order / obtain-order relic
//! dispatchers read the `RelicPlan` stored on the combat's `RelicStore`
//! instead of scanning every registered handler against every relic. These
//! tests play whole random combats with such relic lists on the plan, then
//! re-register every handler of the 14 planned channels unchanged (which sends
//! each channel to its generic walk for the rest of the process) and play the
//! same combats again: every step must match. Debug builds also check each
//! planned dispatch against the generic walk as it happens.

use std::hash::{DefaultHasher, Hash, Hasher};

use sts2_core::cards::CardId;
use sts2_core::combat_loop::{start_combat, start_player_turn};
use sts2_core::potions::CharacterId;
use sts2_core::relic_effects as re;
use sts2_core::{apply, legal_actions, CardUid, ChoiceKind, CombatState, MonsterKind, Relic};
use sts2_core::{RelicKind as K, StepResult};

fn install() {
    sts2_content::cards::dispatch::install_default_registry();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();
    sts2_core::register_card_factory(sts2_content::cards::spec_index::card_pod_from_id);
}

/// `(kind, melted)` relic lists in obtain order.
const LAYOUTS: &[&[(K, bool)]] = &[
    // GamblingChip behind a melted slot, its turn-one waiters after it, and
    // a duplicated BagOfMarbles around RedMask.
    &[
        (K::ToyBox, false),
        (K::Akabeko, true),
        (K::GamblingChip, false),
        (K::MercuryHourglass, false),
        (K::BagOfMarbles, false),
        (K::RedMask, false),
        (K::BagOfMarbles, false),
        (K::Kunai, false),
        (K::Shuriken, false),
        (K::OrnamentalFan, false),
        (K::PenNib, false),
        (K::Vambrace, false),
        (K::TheBoot, false),
        (K::TungstenRod, false),
        (K::Lantern, false),
        (K::BloodVial, true),
        (K::Orichalcum, false),
    ],
    // Melted copies in front of live ones, a duplicated owned-channel relic,
    // and a melted GamblingChip (it must not suspend anything).
    &[
        (K::Kunai, true),
        (K::GamblingChip, true),
        (K::Kunai, false),
        (K::Kunai, false),
        (K::TwistedFunnel, false),
        (K::RedMask, true),
        (K::RedMask, false),
        (K::PaperKrane, false),
        (K::LetterOpener, false),
        (K::Nunchaku, false),
        (K::StrikeDummy, false),
        (K::ArtOfWar, false),
        (K::BronzeScales, false),
        (K::SturdyClamp, false),
        (K::CaptainsWheel, false),
        (K::HappyFlower, false),
        (K::Anchor, false),
        (K::HornCleat, false),
        (K::Vajra, false),
    ],
    // A short list: GamblingChip first, then a melted waiter and a live one.
    &[
        (K::GamblingChip, false),
        (K::MercuryHourglass, true),
        (K::Akabeko, false),
        (K::MercuryHourglass, false),
    ],
];

const DECK: &[CardId] = &[
    CardId::StrikeIronclad,
    CardId::StrikeIronclad,
    CardId::StrikeIronclad,
    CardId::StrikeIronclad,
    CardId::DefendIronclad,
    CardId::DefendIronclad,
    CardId::DefendIronclad,
    CardId::Bash,
    CardId::Whirlwind,
    CardId::Inflame,
    CardId::ShrugItOff,
    CardId::PommelStrike,
];

fn combat(layout: &[(K, bool)], seed: u64) -> CombatState {
    let seed_i32 = i32::try_from(seed).expect("small seed");
    let mut state = CombatState {
        rng: sts2_core::dotnet_rng::Rng::new(seed_i32),
        run_rng: sts2_core::run_rng_set::RunRngSet::new(seed_i32),
        ..Default::default()
    };
    state.player.character = Some(CharacterId::Ironclad);
    state.player.current_hp = 80;
    state.player.max_hp = 80;
    state.player.max_energy = 3;
    state.init_player_creature();
    for hp in [300, 250] {
        let enemy = state.spawn_enemy(MonsterKind::Nibbit);
        if let Some(c) = state.creatures.iter_mut().find(|c| c.id == enemy.0) {
            c.max_hp = hp;
            c.current_hp = hp;
        }
    }
    state.relics = layout
        .iter()
        .enumerate()
        .map(|(i, &(kind, melted))| Relic {
            id: std::num::NonZeroU32::new(u32::try_from(i + 1).expect("fits")).expect("nz"),
            kind,
            counter: kind.initial_counter(),
            is_wax: melted,
            is_melted: melted,
        })
        .collect();
    for &id in DECK {
        let mut card = sts2_core::lookup_card_factory()
            .and_then(|f| f(id, false))
            .expect("card factory has the card");
        let uid = CardUid::new(u32::try_from(state.cards.len() + 1).expect("fits")).expect("nz");
        card.uid = uid;
        state.cards.push(card);
        state.player.draw.push(uid);
    }
    state
}

/// Per-step hashes of one seeded uniform-random combat (the action, its
/// result, and the whole state's `Debug` form, which covers every relic
/// counter and RNG cursor), plus how many GamblingChip screens it opened.
fn random_combat(layout: &[(K, bool)], seed: u64) -> (Vec<u64>, usize) {
    let mut state = combat(layout, seed);
    let _ = start_combat(&mut state);
    let mut chip_screens = 0;
    if start_player_turn(&mut state) == StepResult::PendingChoice {
        chip_screens += usize::from(
            state
                .pending_choice
                .as_ref()
                .is_some_and(|pc| pc.kind == ChoiceKind::DiscardFromHandAndDraw),
        );
    }
    let mut agent = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut steps = Vec::new();
    while !state.is_over && steps.len() < 300 {
        let actions = legal_actions(&state);
        if actions.is_empty() {
            break;
        }
        agent ^= agent << 13;
        agent ^= agent >> 7;
        agent ^= agent << 17;
        let action = actions[usize::try_from(agent % actions.len() as u64).expect("fits")].clone();
        let had_choice = state.pending_choice.is_some();
        let result = apply(&mut state, action.clone());
        if !had_choice
            && state
                .pending_choice
                .as_ref()
                .is_some_and(|pc| pc.kind == ChoiceKind::DiscardFromHandAndDraw)
        {
            chip_screens += 1;
        }
        let mut h = DefaultHasher::new();
        format!("{action:?}|{result:?}|{state:?}").hash(&mut h);
        steps.push(h.finish());
    }
    (steps, chip_screens)
}

fn all_combats() -> (Vec<Vec<u64>>, usize) {
    let mut out = Vec::new();
    let mut chip_screens = 0;
    for layout in LAYOUTS {
        for seed in 1..=8 {
            let (steps, screens) = random_combat(layout, seed);
            out.push(steps);
            chip_screens += screens;
        }
    }
    (out, chip_screens)
}

/// Re-register every handler of the 14 planned channels with itself. The
/// listener sets do not change, but each channel leaves its frozen list, so
/// every planned dispatcher takes its generic walk from here on.
fn send_planned_channels_to_the_generic_walk() {
    re::register_many_after_card_played(&re::listeners_after_card_played());
    re::register_many_before_card_played(&re::listeners_before_card_played());
    re::register_many_before_turn_end(&re::listeners_before_turn_end());
    re::register_many_after_damage_given(&re::listeners_after_damage_given());
    re::register_many_after_block_gained(&re::listeners_after_block_gained());
    re::register_many_after_block_cleared(&re::listeners_after_block_cleared());
    re::register_many_modify_damage_additive(&re::listeners_modify_damage_additive());
    re::register_many_modify_block_multiplicative(&re::listeners_modify_block_multiplicative());
    re::register_many_modify_damage_multiplicative(&re::listeners_modify_damage_multiplicative());
    re::register_many_after_damage_received(&re::listeners_after_damage_received());
    re::register_many_modify_hp_lost(&re::listeners_modify_hp_lost());
    re::register_many_after_side_turn_start(&re::listeners_after_side_turn_start());
    re::register_many_after_player_turn_start(&re::listeners_after_player_turn_start());
    re::register_many_before_side_turn_start(&re::listeners_before_side_turn_start());
}

#[test]
fn planned_and_generic_relic_walks_agree_on_melted_and_duplicate_relics() {
    install();
    assert!(re::all_channels_frozen());
    let (planned, planned_screens) = all_combats();

    send_planned_channels_to_the_generic_walk();
    assert!(!re::all_channels_frozen());
    let (generic, generic_screens) = all_combats();

    assert_eq!(planned.len(), generic.len());
    for (i, (p, g)) in planned.iter().zip(&generic).enumerate() {
        assert_eq!(
            p, g,
            "combat {i} diverged between the planned and generic walks"
        );
    }
    assert!(
        planned.iter().map(Vec::len).sum::<usize>() > 1000,
        "the combats must run for many turns"
    );
    assert_eq!(planned_screens, generic_screens);
    // Layouts 1 and 3 open GamblingChip's screen on turn one of every combat;
    // layout 2's GamblingChip is melted and never does.
    assert_eq!(planned_screens, 16, "GamblingChip screens opened");
}
