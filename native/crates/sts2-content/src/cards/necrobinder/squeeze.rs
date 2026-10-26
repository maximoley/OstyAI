//! Squeeze (Necrobinder). Hand-authored C#-first port via the character-port
//! skill.
//! C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Squeeze.cs
use super::prelude::*;

/// Squeeze (Rare, 3 energy, Attack, AnyEnemy). The Osty attacks the target for
/// a calculated amount: `25 + 5 * (number of OTHER OstyAttack cards in your
/// deck)` (`30 + 6 * count` upgraded). Does nothing if the Osty is missing.
//
// C# CanonicalTags: { OstyAttack } — this card is itself an OstyAttack card
//   (so it is NOT counted in its own multiplier; the C# filter excludes
//   `c != card2`).
//
// C# CanonicalVars (3):
//   CalculationBaseVar(25m)              → base addend 25. OnUpgrade +5 → 30.
//   ExtraDamageVar(5m).FromOsty()        → per-stack addend 5. OnUpgrade +1 → 6.
//   CalculatedDamageVar(Move).FromOsty()
//       .WithMultiplier(card => card.Owner.PlayerCombatState.AllCards
//           .Count(c => c.Tags.Contains(OstyAttack) && c != card))
//   CalculatedVar.Calculate (CalculatedVar.cs):
//       value = CalculationBase.BaseValue + ExtraDamage.BaseValue * multiplier
//   ⇒ damage = 25 + 5 * (count of OTHER OstyAttack cards in AllCards)
//             (30 + 6 * count when upgraded).
//   `AllCards` = the full combat deck (draw + hand + discard + exhaust + play),
//   i.e. every card the player owns this combat, not just the current pile.
//
// C# OnPlay:
//   ArgumentNullException.ThrowIfNull(cardPlay.Target);
//   if (!Osty.CheckMissingWithAnim(Owner))
//       DamageCmd.Attack(DynamicVars.CalculatedDamage)
//           .FromOsty(Owner.Osty, this)          // attacker is the OSTY
//           .Targeting(cardPlay.Target).Execute(...);
//
// ShouldGlowRedInternal => Owner.IsOstyMissing — pure UI, ignored.
//
// `deal_damage_from_osty_to_enemy` is itself a no-op when the Osty is absent
// (`osty_dealer_ref` → None), so calling it unconditionally reproduces
// `!Osty.CheckMissingWithAnim` (minus the cosmetic summon-prompt animation).
//
// The `* (count of OTHER OstyAttack cards)` multiplier: the C# count walks
// `AllCards` and tests `Tags.Contains(CardTag.OstyAttack)`. The OstyAttack tag
// is a cosmetic glow flag not carried on the runtime `CardSpec`, so it is
// modeled here by the static `card_is_osty_attack(CardId)` predicate (option
// (b) of the original blocker), derived from the 13 C# CanonicalTags. See
// `osty_attack_card_count`. Bead `sts2-rs-osty-attack-count`.
/// `CalculatedDamage`: base + ExtraDamage for every OTHER Osty Attack you own.
/// Shared with the card face — see `crate::cards::preview`.
pub(crate) fn preview_squeeze(
    state: &CombatState,
    card: &Card,
    _target: Option<CreatureUid>,
) -> i32 {
    let (base_addend, per_stack) = if card.upgraded { (30, 6) } else { (25, 5) };
    base_addend + per_stack * osty_attack_card_count(state)
}

pub(crate) fn play_squeeze(state: &mut CombatState, target: Option<CreatureUid>) -> EffectResult {
    let t = target.expect("Squeeze: enemy target required");

    let dmg = resolving_preview(state, preview_squeeze, target);

    // C# `.FromOsty(Owner.Osty, this)` — the Osty is the dealer, so the hit
    // reads the Osty's dealer-side modifiers (Strength, Calcify's
    // `dealer is Osty` gate). No-op when the Osty is missing/dead.
    let _ = deal_damage_from_osty_to_enemy(state, t, dmg);

    EffectResult::Resolved
}

/// Count of OstyAttack cards in the player's full combat deck **other than**
/// the card currently resolving (C# `AllCards.Count(c => c.Tags.Contains(
/// OstyAttack) && c != card)`). `AllCards` = hand + draw + discard + exhaust +
/// play. The resolving Squeeze sits on top of the Play pile during its OnPlay,
/// so it is excluded by uid. Bead `sts2-rs-osty-attack-count`.
fn osty_attack_card_count(state: &CombatState) -> i32 {
    let resolving = sts2_core::card_effects::resolving_card_uid(state);
    let mut count = 0i32;
    for pile in [
        state.player.hand.as_slice(),
        state.player.draw.as_slice(),
        state.player.discard.as_slice(),
        state.player.exhaust.as_slice(),
        state.player.play.as_slice(),
    ] {
        for &uid in pile {
            if Some(uid) == resolving {
                continue;
            }
            let idx = uid.0.get() as usize - 1;
            if state
                .cards
                .get(idx)
                .is_some_and(|c| card_is_osty_attack(c.card_id))
            {
                count += 1;
            }
        }
    }
    count
}

/// C# `CardTag.OstyAttack` membership — the 13 Necrobinder cards whose
/// CanonicalTags include OstyAttack. Delegates to the core
/// [`CardId::is_osty_attack`] single-source-of-truth (shared with the
/// `after_card_played` Vigor consume gate in `hooks.rs`).
pub(crate) fn card_is_osty_attack(id: CardId) -> bool {
    id.is_osty_attack()
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Squeeze,
        name: "SQUEEZE",
        cost: 3,
        card_type: CardType::Attack,
        target_type: TargetType::Enemy, // C# TargetType.AnyEnemy → single-player collapse
        rarity: CardRarity::Rare,
        // CalculationBaseVar(25m). The damage is computed (CalculatedDamage,
        // dealt by the Osty), not a plain player-attack stat, so this is the
        // cosmetic base addend; the real amount is computed in the handler.
        base_damage: 25,
        base_block: 0,
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: CalculationBase.UpgradeValueBy(5m) (25 → 30) and
    // ExtraDamage.UpgradeValueBy(1m) (5 → 6). Neither is a plain player-attack
    // stat (the amount is the Osty's CalculatedDamage), so both bumps are
    // applied in the handler via current_card_upgraded rather than a static
    // spec delta. Returns None to keep one source of truth.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mint(state: &mut CombatState, id: CardId) -> sts2_core::CardUid {
        let mut card = crate::cards::spec_index::card_pod_from_id(id, false).expect("spec exists");
        let raw = u32::try_from(state.cards.len() + 1).expect("fits");
        let uid = sts2_core::CardUid::new(raw).expect("nz");
        card.uid = uid;
        state.cards.push(card);
        uid
    }

    /// `card_is_osty_attack` matches the 13 C# OstyAttack-tagged cards and
    /// nothing else.
    #[test]
    fn osty_attack_predicate_membership() {
        for id in [
            CardId::Squeeze,
            CardId::Poke,
            CardId::Flatten,
            CardId::Unleash,
            CardId::Snap,
            CardId::BoneShards,
            CardId::SweepingGaze,
        ] {
            assert!(card_is_osty_attack(id), "{id:?} is OstyAttack");
        }
        assert!(!card_is_osty_attack(CardId::DefendNecrobinder));
        assert!(!card_is_osty_attack(CardId::StrikeNecrobinder));
    }

    /// The Squeeze multiplier counts every OstyAttack card across ALL piles
    /// (hand/draw/discard/exhaust/play) EXCEPT the resolving Squeeze on top of
    /// Play, and ignores non-OstyAttack cards. Bead `sts2-rs-osty-attack-count`.
    #[test]
    fn osty_attack_count_spans_all_piles_and_excludes_resolving() {
        let mut state = CombatState::default();
        // OstyAttack cards spread across piles.
        let poke = mint(&mut state, CardId::Poke);
        state.player.hand.push(poke);
        let flatten = mint(&mut state, CardId::Flatten);
        state.player.draw.push(flatten);
        let snap = mint(&mut state, CardId::Snap);
        state.player.discard.push(snap);
        let bone = mint(&mut state, CardId::BoneShards);
        state.player.exhaust.push(bone);
        // Non-OstyAttack filler that must NOT be counted.
        let defend = mint(&mut state, CardId::DefendNecrobinder);
        state.player.hand.push(defend);
        // Play pile: another OstyAttack (Unleash, counted) + the resolving
        // Squeeze on TOP (excluded).
        let unleash = mint(&mut state, CardId::Unleash);
        state.player.play.push(unleash);
        let squeeze = mint(&mut state, CardId::Squeeze);
        state.player.play.push(squeeze);

        // Poke, Flatten, Snap, BoneShards, Unleash = 5 (Squeeze excluded; Defend ignored).
        assert_eq!(osty_attack_card_count(&state), 5);
    }
}
