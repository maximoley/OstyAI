//! The Scythe (Necrobinder rare attack). Hand-authored C#-first port.
//! C# source: ../sts2-wiki/decompiled/v0.111.0/MegaCrit.Sts2.Core.Models.Cards/TheScythe.cs
use super::prelude::*;

/// Deal the current damage (initially 13), then permanently increase it by
/// 5 (7 upgraded). The run bridge propagates this instance's growth to the
/// master deck at combat exit.
pub(crate) fn play_the_scythe(
    state: &mut CombatState,
    target: Option<CreatureUid>,
) -> EffectResult {
    let t = target.expect("TheScythe: Enemy-targeted");
    // C# `OnPlay`: deal `Damage.BaseValue` (== `CurrentDamage` == our
    // run-persistent `base_damage`).
    let dmg = current_damage(state);
    let _ = deal_damage_player_to_enemy(state, t, dmg);
    let increase = if current_card_upgraded(state) { 7 } else { 5 };
    if let Some(idx) = current_card_idx(state) {
        if let Some(card) = state.cards.get_mut(idx) {
            card.base_damage = card.base_damage.saturating_add(increase);
        }
    }
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::TheScythe,
        name: "THE_SCYTHE",
        cost: 2,
        card_type: CardType::Attack,
        target_type: TargetType::Enemy,
        rarity: CardRarity::Rare,
        base_damage: 13,
        base_block: 0,
        keywords: CardKeywords::EXHAUST,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# `OnUpgrade` bumps only the `Increase` IntVar (5 → 7); it does not
    // touch base damage, cost, block, or keywords. The per-play increment is
    // gated on `current_card_upgraded(state)` in the handler, so there is no
    // stat delta to apply here.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mint_into_play(state: &mut CombatState, upgraded: bool) -> sts2_core::CardUid {
        let mut card =
            crate::cards::spec_index::card_pod_from_id(CardId::TheScythe, upgraded).expect("spec");
        let raw = u32::try_from(state.cards.len() + 1).expect("fits");
        let uid = sts2_core::CardUid::new(raw).expect("nz");
        card.uid = uid;
        state.cards.push(card);
        state.player.play.push(uid);
        uid
    }

    fn enemy_hp(state: &CombatState, uid: CreatureUid) -> i32 {
        state
            .creatures
            .iter()
            .find(|c| c.id == uid.0)
            .expect("enemy present")
            .current_hp
    }

    /// Consecutive plays use the persisted damage and grow by five each time.
    #[test]
    fn the_scythe_grows_base_damage_per_play() {
        let mut state = CombatState::default();
        let enemy = state.spawn_enemy(sts2_core::MonsterKind::Nibbit);
        let uid = mint_into_play(&mut state, false);
        let idx = uid.0.get() as usize - 1;
        assert_eq!(state.cards[idx].base_damage, 13);

        let hp0 = enemy_hp(&state, enemy);
        play_the_scythe(&mut state, Some(enemy));
        assert_eq!(
            enemy_hp(&state, enemy),
            hp0 - 13,
            "first play deals base 13"
        );
        assert_eq!(state.cards[idx].base_damage, 18, "base grows +5 to 18");

        let hp1 = enemy_hp(&state, enemy);
        play_the_scythe(&mut state, Some(enemy));
        assert_eq!(enemy_hp(&state, enemy), hp1 - 18, "second play deals 18");
        assert_eq!(state.cards[idx].base_damage, 23, "base grows +5 to 23");
    }

    /// Upgraded Scythe grows by +7 per play (`Increase` IntVar 5 → 7).
    #[test]
    fn the_scythe_upgraded_grows_by_seven() {
        let mut state = CombatState::default();
        let enemy = state.spawn_enemy(sts2_core::MonsterKind::Nibbit);
        let uid = mint_into_play(&mut state, true);
        let idx = uid.0.get() as usize - 1;
        assert_eq!(state.cards[idx].base_damage, 13);
        play_the_scythe(&mut state, Some(enemy));
        assert_eq!(state.cards[idx].base_damage, 20, "upgraded base grows +7");
    }
}
