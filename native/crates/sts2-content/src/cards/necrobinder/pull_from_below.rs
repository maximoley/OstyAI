//! Pull From Below (Necrobinder). Hand-authored C#-first port via character-port skill.
//! C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/PullFromBelow.cs
use super::prelude::*;

/// Pull From Below (Uncommon, 1 energy, Attack). Deal 5 damage to a
/// single enemy, once per Ethereal card you have played this combat.
//
// C# `PullFromBelow.cs`:
//   ctor base(1, Attack, Uncommon, AnyEnemy)    → cost 1 / Attack / Uncommon / Enemy
//   ExtraHoverTips: Ethereal keyword             → tooltip ONLY (explains the
//     "ethereal cards played" scaling); the card itself is NOT Ethereal — there
//     is no `CanonicalKeywords` override → keywords: NONE
//   CanonicalVars:
//     DamageVar(5m, Move)                        → base_damage: 5
//     CalculationBaseVar(0m)                     → Base = 0
//     CalculationExtraVar(1m)                    → Extra = 1
//     CalculatedVar("CalculatedHits").WithMultiplier(count of CardPlayFinishedEntry
//        where e.CardPlay.Card.Owner == this.Owner && e.WasEthereal)
//        → CalculatedVar = Base + Extra * multiplier = 0 + 1 * (#ethereal cards
//          the owner has played this combat). This is the `.WithHitCount(...)`.
//   OnPlay:
//     DamageCmd.Attack(Damage.BaseValue)         → deal_damage_player_to_enemy(5)
//       .WithHitCount(CalculatedHits.Calculate(target))  → loop `hits` times
//       .Targeting(cardPlay.Target)
//       .WithHitFx(...)                          → cosmetic, ignored
//   OnUpgrade: Damage.UpgradeValueBy(2m) (5 → 7) — a damage STAT, so the delta
//     lives in the central `cards/card_upgrades_generated.rs` table
//     (CardId::PullFromBelow => base_damage: Some(7)); `current_damage(state)`
//     already returns 5/7 and `upgrade()` returns None here.
//
// BLOCKER (needs-infra): the hit count is the number of ETHEREAL cards the owner
// has played this combat (C# `History.Entries.OfType<CardPlayFinishedEntry>()
// .Count(e => e.CardPlay.Card.Owner == Owner && e.WasEthereal)`). The engine has
// no combat-long "ethereal cards played" counter — only turn-scoped
// `cards_played_this_turn` (by type, not keyword) and combat-scoped
// `player_card_plays_this_combat` (all plays, not ethereal-filtered). A faithful
// port needs core infra: a `PlayerCombatState.ethereal_cards_played_this_combat`
// counter incremented in `combat_loop::play_card` whenever an ETHEREAL card
// resolves (mirroring the `WasEthereal` flag on `CardPlayFinishedEntry`), plus a
// reader helper. Until that lands the handler computes 0 hits (deals no damage),
// which is the only non-fabricated behavior available with current state.
/// `CalculatedHits`: one hit per Ethereal card played this combat. Shared with
/// the card face — see `crate::cards::preview`.
pub(crate) fn preview_pull_from_below(
    state: &CombatState,
    _card: &Card,
    _target: Option<CreatureUid>,
) -> i32 {
    ethereal_cards_played_this_combat(state)
}

pub(crate) fn play_pull_from_below(
    state: &mut CombatState,
    target: Option<CreatureUid>,
) -> EffectResult {
    let t = target.expect("PullFromBelow: Enemy-targeted");
    let dmg = current_damage(state);
    // C# `.WithHitCount(CalculatedHits)` = Base(0) + Extra(1) * (# ethereal cards
    // the owner played this combat). No core counter exists yet — see BLOCKER
    // above. `hits` is 0 until the infra lands; do NOT fabricate a count.
    let hits = resolving_preview(state, preview_pull_from_below, target);
    for _ in 0..hits {
        let _ = deal_damage_player_to_enemy(state, t, dmg);
    }
    EffectResult::Resolved
}

/// Number of ETHEREAL cards the player has played this combat — the hit count
/// for Pull From Below (C# `CardPlayFinishedEntry` count where `WasEthereal`).
///
/// Reads the combat-long counter `combat_log.ethereal_cards_played_this_combat`,
/// incremented in `combat_loop::play_card` after each ETHEREAL card resolves
/// (so this card does not count itself, matching C#'s post-`OnPlay` history
/// entry).
fn ethereal_cards_played_this_combat(state: &CombatState) -> i32 {
    i32::try_from(state.combat_log.ethereal_cards_played_this_combat).unwrap_or(i32::MAX)
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::PullFromBelow,
        name: "PULL_FROM_BELOW",
        cost: 1,
        card_type: CardType::Attack,
        target_type: TargetType::Enemy, // C# AnyEnemy collapses to single-player Enemy
        rarity: CardRarity::Uncommon,
        base_damage: 5,
        base_block: 0,
        // NOT Ethereal: C# `PullFromBelow.cs` has no `CanonicalKeywords`
        // override — Ethereal is only an `ExtraHoverTip` explaining that the
        // hit count scales on OTHER ethereal cards played this combat.
        keywords: CardKeywords::NONE,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade `Damage.UpgradeValueBy(2m)` (5 → 7) lives in the central
    // `card_upgrades_generated.rs` table (CardId::PullFromBelow), so no per-card
    // delta here.
    None
}
