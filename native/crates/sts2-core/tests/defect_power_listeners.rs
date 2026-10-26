//! Defect power-listener integration tests.
//!
//! Pins the `CoolantPower` turn-start block body wired into
//! [`sts2_core::hooks::after_side_turn_start`], mirroring its C#
//! `CoolantPower.AfterSideTurnStart` in
//! `../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Powers/CoolantPower.cs`:
//! at the owner's side turn start, gain Block = `(distinct orb types in the
//! OrbQueue) × Amount`.

use smallvec::smallvec;
use sts2_core::cards::{CardTags, CardType};
use sts2_core::hooks::{self, CardCtx, CardPlayCountCtx, CreatureRef, TurnSideCtx};
use sts2_core::{orbs, CombatState, Orb, Power, PowerKind, PowerUid, Side};

/// Push a fresh `kind(amount)` power onto the player's power list.
fn push_player_power(state: &mut CombatState, kind: PowerKind, amount: i32) -> PowerUid {
    let pid = PowerUid::new(state.powers.len() as u32 + 1).expect("nonzero power uid");
    state.powers.push(Power {
        id: pid.0,
        kind,
        amount,
        counter: 0,
        applier: None,
        skip_next_tick: false,
    });
    state.player.powers.push(pid);
    pid
}

/// Coolant(2) with two Frost orbs (one distinct type) grants `1 × 2 = 2` block
/// at the player's turn start.
#[test]
fn coolant_grants_block_per_distinct_orb_type_at_turn_start() {
    let mut state = CombatState::default();
    state.player.orbs.slots = smallvec![Orb::Frost, Orb::Frost];
    push_player_power(&mut state, PowerKind::Coolant, 2);

    hooks::dispatch_after_side_turn_start(&mut state, &TurnSideCtx { side: Side::Player });
    assert_eq!(
        state.player.block, 2,
        "two Frost orbs = one distinct type → Coolant(2) grants 2 block"
    );
}

/// Distinct orb TYPES are counted, not slot count: Frost + Frost + Lightning =
/// 2 distinct types → Coolant(3) grants `2 × 3 = 6` block.
#[test]
fn coolant_counts_distinct_orb_types() {
    let mut state = CombatState::default();
    state.player.orbs.slots = smallvec![Orb::Frost, Orb::Frost, Orb::Lightning];
    push_player_power(&mut state, PowerKind::Coolant, 3);

    hooks::dispatch_after_side_turn_start(&mut state, &TurnSideCtx { side: Side::Player });
    assert_eq!(
        state.player.block, 6,
        "two distinct orb types (Frost, Lightning) → Coolant(3) grants 6 block"
    );
}

/// No orbs → no Coolant block (the `distinct == 0` early-out).
#[test]
fn coolant_grants_no_block_without_orbs() {
    let mut state = CombatState::default();
    push_player_power(&mut state, PowerKind::Coolant, 2);

    hooks::dispatch_after_side_turn_start(&mut state, &TurnSideCtx { side: Side::Player });
    assert_eq!(
        state.player.block, 0,
        "no orbs in queue → Coolant grants no block"
    );
}

/// Frost orb passive block is `Unpowered` (C# `FrostOrb.Passive` →
/// `CreatureCmd.GainBlock(Owner, PassiveVal, ValueProp.Unpowered, null)`), so
/// the player's Dexterity must NOT inflate it. Two Frost orbs grant exactly
/// `2 × 2 = 4` block with Dexterity(3) present — not `(2 + 3) × 2 = 10`.
/// Regression for the Defect `PLAYER_HP|end_turn` cluster (Rust used the
/// powered `gain_block_player` path, so Dexterity over-blocked the enemy hit).
#[test]
fn frost_orb_passive_block_is_dexterity_exempt() {
    let mut state = CombatState::default();
    state.player.orbs.slots = smallvec![Orb::Frost, Orb::Frost];
    state.player.orbs.capacity = 2;
    push_player_power(&mut state, PowerKind::Dexterity, 3);

    orbs::passive_tick(&mut state);
    assert_eq!(
        state.player.block, 4,
        "two Frost orbs grant 2 block each, Dexterity-exempt (Unpowered)"
    );
}

/// SignalBoost adds +1 play to a POWER card (it plays twice) and consumes a
/// stack, mirroring C# `SignalBoostPower.ModifyCardPlayCount` /
/// `AfterModifyingCardPlayCount`. It does NOT boost Attack/Skill cards.
/// Regression for the Defect `PLAYER_STATE|HAILSTORM` cluster (a Power card
/// under SignalBoost applied its power once instead of twice, and SignalBoost
/// lingered instead of being consumed).
#[test]
fn signal_boost_doubles_power_cards_and_consumes_a_stack() {
    let mut state = CombatState::default();
    let pid = push_player_power(&mut state, PowerKind::SignalBoost, 1);

    let power_ctx = CardPlayCountCtx {
        owner: CreatureRef::Player,
        card_id: None,
        card_uid: None,
        card_type: Some(CardType::Power),
        card_tags: CardTags::NONE,
        target: None,
    };
    assert_eq!(
        hooks::dispatch_modify_card_play_count(&state, &power_ctx),
        1,
        "SignalBoost grants +1 play to a Power card"
    );

    // An Attack card is NOT boosted by SignalBoost.
    let attack_ctx = CardPlayCountCtx {
        card_type: Some(CardType::Attack),
        ..power_ctx
    };
    assert_eq!(
        hooks::dispatch_modify_card_play_count(&state, &attack_ctx),
        0,
        "SignalBoost does not boost non-Power cards"
    );

    // Playing the Power card consumes the (only) SignalBoost stack.
    let after = CardCtx {
        card_id: None,
        card_uid: None,
        card_tags: CardTags::NONE,
        card_type: Some(CardType::Power),
    };
    hooks::dispatch_after_modifying_card_play_count(&mut state, &after);
    assert!(
        !state.powers.iter().any(|p| p.id == pid.0),
        "SignalBoost(1) is consumed after boosting one Power card"
    );
}

/// Subroutine grants `Amount` energy when a POWER card is played (C#
/// `SubroutinePower.Before/AfterCardPlayed` → `PlayerCmd.GainEnergy`), armed in
/// `BeforeCardPlayed` (here `arm_storm_snapshot`) and fired in `AfterCardPlayed`.
/// The card that APPLIED Subroutine doesn't self-trigger (no prior arm), and a
/// non-Power card grants nothing. Regression for `PLAYER_STATE|BUFFER`.
#[test]
fn subroutine_grants_energy_per_power_card() {
    let mut state = CombatState::default();
    state.player.energy = 3;
    push_player_power(&mut state, PowerKind::Subroutine, 1);

    let power_card = CardCtx {
        card_id: None,
        card_uid: None,
        card_tags: CardTags::NONE,
        card_type: Some(CardType::Power),
    };

    // Applying card: AfterCardPlayed WITHOUT a preceding arm grants no energy.
    hooks::dispatch_after_card_played(&mut state, &power_card);
    assert_eq!(
        state.player.energy, 3,
        "the applying Power card does not self-grant"
    );

    // A real subsequent Power play: arm in BeforeCardPlayed, then fire.
    hooks::arm_storm_snapshot(&mut state);
    hooks::dispatch_after_card_played(&mut state, &power_card);
    assert_eq!(
        state.player.energy, 4,
        "Subroutine(1) grants 1 energy on an armed Power card"
    );

    // A non-Power card does NOT trigger Subroutine.
    let skill_card = CardCtx {
        card_type: Some(CardType::Skill),
        ..power_card
    };
    hooks::arm_storm_snapshot(&mut state);
    hooks::dispatch_after_card_played(&mut state, &skill_card);
    assert_eq!(
        state.player.energy, 4,
        "Subroutine does not fire on non-Power cards"
    );
}

/// `focus_bonus` (the Focus folded into orb passive/evoke values) counts only
/// the real `FocusPower`, NOT the transient `Synchronize`/`TemporaryFocus`
/// markers — those already inject an equal `FocusPower` on apply (C#
/// `TemporaryFocusPower.InternallyAppliedPower`; Rust's Synchronize card
/// applies Focus too). Counting them again double-scaled orbs (orb-parity
/// `ORB_STATE|SYNCHRONIZE`: Glass passive 6 vs C#'s 4 under Focus(2)+Sync(2)).
#[test]
fn focus_bonus_counts_focus_only_not_synchronize_marker() {
    let mut state = CombatState::default();
    // Synchronize's card applies both: the marker AND the injected Focus.
    push_player_power(&mut state, PowerKind::Focus, 2);
    push_player_power(&mut state, PowerKind::Synchronize, 2);

    assert_eq!(
        orbs::focus_bonus(&state),
        2,
        "orb Focus = the injected FocusPower only; the Synchronize marker is not re-counted"
    );

    // A Frost orb passive therefore grants base(2) + focus(2) = 4 block (not 6).
    state.player.orbs.slots = smallvec![Orb::Frost];
    state.player.orbs.capacity = 1;
    orbs::passive_tick(&mut state);
    assert_eq!(
        state.player.block, 4,
        "Frost passive = 2 + focus(2), not 2 + 4"
    );
}

/// A Focus-scaled orb value clamps at 0 (C# `FocusPower.ModifyOrbValue` →
/// `Math.Max(value + focus, 0)`). Under negative Focus a Frost orb grants 0
/// block, not negative; a Lightning orb would deal 0, not heal the enemy.
/// Regression for orb-parity `ORB_STATE|{HYPERBEAM,ICE_LANCE,COLD_SNAP}`.
#[test]
fn orb_focus_scaled_value_clamps_at_zero() {
    assert_eq!(
        orbs::focus_scaled(2, -3),
        0,
        "Frost passive 2 with Focus(-3) → 0, not -1"
    );
    assert_eq!(
        orbs::focus_scaled(3, -5),
        0,
        "Lightning 3 with Focus(-5) → 0, not -2"
    );
    assert_eq!(orbs::focus_scaled(8, 2), 10, "positive Focus still scales");

    // End to end: a Frost orb passive grants 0 block (not negative) at Focus(-3).
    let mut state = CombatState::default();
    state.player.orbs.slots = smallvec![Orb::Frost, Orb::Frost];
    state.player.orbs.capacity = 2;
    push_player_power(&mut state, PowerKind::Focus, -3);
    orbs::passive_tick(&mut state);
    assert_eq!(
        state.player.block, 0,
        "Frost passive clamps to 0 block under Focus(-3)"
    );
}

/// Coolant does NOT fire on the ENEMY's turn start (owner-side gate).
#[test]
fn coolant_does_not_fire_on_enemy_turn_start() {
    let mut state = CombatState::default();
    state.player.orbs.slots = smallvec![Orb::Frost];
    push_player_power(&mut state, PowerKind::Coolant, 2);

    hooks::dispatch_after_side_turn_start(&mut state, &TurnSideCtx { side: Side::Enemy });
    assert_eq!(
        state.player.block, 0,
        "Coolant only grants block on the owner's (player's) turn start"
    );
}
