//! Forbidden Grimoire (Necrobinder pool). C#-first port.
use super::prelude::*;

/// Forbidden Grimoire (Ancient, 2 energy, Power, Eternal). Apply 1 stack of
/// ForbiddenGrimoire to self.
// C# source: ../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Models.Cards/ForbiddenGrimoire.cs
// OnPlay: PowerCmd.Apply<ForbiddenGrimoirePower>(Owner.Creature, 1, Owner.Creature).
// OnUpgrade: EnergyCost.UpgradeBy(-1) → cost 2 → 1 (central table:
// card_upgrades_generated.rs, so upgrade() returns None here).
// CanonicalKeywords = [Eternal]. Self-target, so `target` is unused.
//
// ForbiddenGrimoirePower.AfterCombatEnd (each stack adds a CardRemovalReward to
// the post-combat rewards) is wired via the run_bridge::exit_combat seam → a
// Reward::RemoveCard staged on RunState::pending_rewards (bead sts2-rs-nfoh0).
pub(crate) fn play_forbidden_grimoire(
    state: &mut CombatState,
    _target: Option<CreatureUid>,
) -> EffectResult {
    apply_power_to_player(state, PowerKind::ForbiddenGrimoire, 1);
    EffectResult::Resolved
}

#[must_use]
pub(crate) const fn spec() -> CardSpec {
    CardSpec {
        id: CardId::ForbiddenGrimoire,
        name: "FORBIDDEN_GRIMOIRE",
        cost: 2,
        card_type: CardType::Power,
        target_type: TargetType::Self_,
        rarity: CardRarity::Ancient,
        base_damage: 0,
        base_block: 0,
        keywords: CardKeywords::ETERNAL,
        ..CardSpec::base()
    }
}

#[allow(clippy::unnecessary_wraps, dead_code)]
pub(crate) fn upgrade() -> Option<CardUpgradeDelta> {
    // OnUpgrade `EnergyCost.UpgradeBy(-1)` (cost 2 → 1) lives in the central
    // card_upgrades_generated.rs table (CardId::ForbiddenGrimoire row).
    None
}
