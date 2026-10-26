//! LastingCandy has no combat-end mutation in v0.111.0.
//! Its CombatRewardsSeen count advances in RunManager::prepare_combat_rewards,
//! after population and only for sets containing a CardReward. The source
//! BeforeCombatRewardOffered hook is in Models.Relics/LastingCandy.cs.
pub(crate) fn install() {}
