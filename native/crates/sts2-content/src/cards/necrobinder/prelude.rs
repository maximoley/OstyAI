//! Per-card prelude (Necrobinder pool). Rebuilt from scratch (C#-first port).
#![allow(clippy::wildcard_imports, unused_imports)]

pub(crate) use crate::cards::preview::resolving_preview;
pub(crate) use crate::cards::status::card_from_spec;
pub(crate) use crate::cards::status_specs_generated as status_specs;
pub(crate) use std::num::NonZeroU32;
pub(crate) use sts2_core::card_effects::{
    self, apply_power_to_enemy, deal_damage_from_osty_to_all_alive_enemies,
    deal_damage_from_osty_to_enemy, deal_damage_player_to_enemy, gain_block_player,
    osty_dealer_ref, spawn_card_into_draw_pile_bottom, spawn_card_into_draw_pile_random_position,
    spawn_card_into_pile, EffectResult, PileKind, PlayCardFn,
};
pub(crate) use sts2_core::cards::{Card, CardId, CardType, TargetType};
pub(crate) use sts2_core::{CombatState, CreatureUid, Power, PowerKind, PowerUid};

pub(crate) use super::helpers::*;
pub(crate) use super::specs::*;
pub(crate) use crate::cards::types::{CardKeywords, CardRarity, CardSpec, CardUpgradeDelta};
