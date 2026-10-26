//! Dirge (Necrobinder). Hand-authored C#-first port via character-port skill.
//! C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/Dirge.cs
use super::prelude::*;

/// Dirge (Uncommon, X energy, Skill — Exhaust). For each energy spent (X):
/// summon Osty (grow it by 3 max HP, or revive it at 3), then generate X Souls
/// into the draw pile at random positions. Upgraded: each generated Soul is
/// upgraded, and the per-summon HP gain rises from 3 → 4
/// (`SummonVar.UpgradeValueBy(1)`).
pub(crate) fn play_dirge(state: &mut CombatState, _target: Option<CreatureUid>) -> EffectResult {
    // C# Dirge.OnPlay:
    //   int x = ResolveEnergyXValue();
    //   for (i in 0..x) OstyCmd.Summon(.., Summon.BaseValue, this);
    //   var souls = Soul.Create(owner, x);
    //   if (IsUpgraded) foreach soul CardCmd.Upgrade(soul);
    //   CardPileCmd.AddGeneratedCardsToCombat(souls, Draw, addedByPlayer, Random);
    let x = current_card_energy_spent(state);
    let upgraded = current_card_upgraded(state);

    // Osty summon: for each of X, summon/grow the pet by the Summon var (3, or 4
    // upgraded). `summon_osty` spawns a fresh Osty, revives a dead one, or grows
    // a living one's max HP — matching `OstyCmd.Summon`. C# loops the summon X
    // times, so the first call spawns (or revives) and the rest grow.
    let summon = if upgraded { 4 } else { 3 };
    for _ in 0..x {
        state.summon_osty(summon);
    }

    let soul_spec = status_specs::soul();
    for _ in 0..x {
        let mut soul = card_from_spec(&soul_spec);
        soul.upgraded = upgraded;
        spawn_card_into_draw_pile_random_position(state, soul);
    }
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::Dirge,
        name: "DIRGE",
        cost: -1, // C# HasEnergyCostX => true
        card_type: CardType::Skill,
        target_type: TargetType::Self_,
        rarity: CardRarity::Uncommon,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::EXHAUST,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // C# OnUpgrade: DynamicVars.Summon.UpgradeValueBy(1m). The Summon var is the
    // Osty-growth / Soul-upgrade magnitude, not a damage/block/cost stat, so it
    // is handled in the play handler via the `current_card_upgraded` branch
    // (upgraded Souls). No stat-field delta.
    None
}
