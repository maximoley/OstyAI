//! Actual v0.111.0 DLL vectors collected by beta_potion_power_vectors.py.
//! Synthetic injected powers and direct cost-model probes are explicitly scoped
//! separately from naturally obtained potion gameplay in the evidence ledger.
use serde_json::Value;
use sts2_content::cards::{dispatch, spec_index::card_pod_from_id};
use sts2_core::cards::{CardId, VarKey};
use sts2_core::hooks::{CreatureRef, Effect};
use sts2_core::{
    Action, CardUid, CharacterId, CombatPhase, CombatState, CreatureUid, PotionKind, PowerKind,
    Side,
};

fn vectors() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../docs/beta-migration/potion_power_csharp_vectors.json"
    )))
    .unwrap()
}
fn board() -> CombatState {
    dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();
    let mut state = CombatState {
        round: 1,
        phase: CombatPhase::PlayerPlayPhase,
        side: Side::Player,
        ..Default::default()
    };
    state.player.character = Some(CharacterId::Necrobinder);
    state.player.current_hp = 80;
    state.player.max_hp = 80;
    state.player.energy = 3;
    state.player.max_energy = 3;
    state.init_player_creature();
    state.spawn_enemy(sts2_core::MonsterKind::Nibbit);
    state
}
fn kind(name: &str) -> PowerKind {
    match name {
        "STRENGTH" => PowerKind::Strength,
        "DEXTERITY" => PowerKind::Dexterity,
        "FOCUS" => PowerKind::Focus,
        _ => panic!("unknown signed stat"),
    }
}
fn amount(state: &CombatState, target: CreatureUid, kind: PowerKind) -> i32 {
    state
        .creatures
        .iter()
        .find(|c| c.id == target.0)
        .unwrap()
        .powers
        .iter()
        .filter_map(|uid| {
            state
                .powers
                .iter()
                .find(|p| p.id == uid.0 && p.kind == kind)
        })
        .map(|p| p.amount)
        .sum()
}

#[test]
fn signed_stat_applications_match_all_six_actual_dll_vectors() {
    let data = vectors();
    let mut checked = 0;
    for row in data["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["case"] == "signed_stat")
    {
        assert_eq!(row["world_errors"]["count"], 0);
        let mut state = board();
        let player = row["target_index"].as_i64().unwrap() < 0;
        let target = if player {
            state.player.creature_uid.unwrap()
        } else {
            state.enemy_uids[0]
        };
        let reference = if player {
            CreatureRef::Player
        } else {
            CreatureRef::Enemy(target)
        };
        let stat = kind(row["power"].as_str().unwrap());
        let mut stats = Vec::new();
        let mut artifacts = Vec::new();
        for (power, delta) in [(stat, 5), (PowerKind::Artifact, 1), (stat, -3), (stat, -8)] {
            sts2_core::hooks::apply_effect(
                &mut state,
                Effect::ApplyPower {
                    target: reference,
                    kind: power,
                    amount: delta,
                    applier: None,
                },
            );
            stats.push(amount(&state, target, stat));
            artifacts.push(amount(&state, target, PowerKind::Artifact));
        }
        assert_eq!(serde_json::json!(stats), row["stat_amounts"], "{row}");
        assert_eq!(
            serde_json::json!(artifacts),
            row["artifact_amounts"],
            "{row}"
        );
        checked += 1;
    }
    assert_eq!(checked, 6);
}

#[test]
fn speed_and_flex_rollback_match_actual_dll_artifact_vectors() {
    let data = vectors();
    let mut checked = 0;
    for row in data["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["case"] == "potion_rollback")
    {
        assert_eq!(row["world_errors"]["count"], 0);
        let mut state = board();
        let target = state.player.creature_uid.unwrap();
        let stat = kind(row["power"].as_str().unwrap());
        sts2_core::card_effects::apply_power_to_creature(
            &mut state,
            target,
            PowerKind::Artifact,
            1,
        );
        state
            .player
            .potions
            .push(if row["potion"] == "SPEED_POTION" {
                PotionKind::SpeedPotion
            } else {
                PotionKind::FlexPotion
            });
        let _ = sts2_core::apply(
            &mut state,
            Action::DrinkPotion {
                potion_slot: 0,
                target: None,
            },
        );
        assert_eq!(amount(&state, target, stat), 5);
        assert_eq!(amount(&state, target, PowerKind::Artifact), 1);
        let _ = sts2_core::apply(&mut state, Action::EndTurn);
        assert_eq!(amount(&state, target, stat), 5);
        assert_eq!(amount(&state, target, PowerKind::Artifact), 0);
        checked += 1;
    }
    assert_eq!(checked, 2);
}

#[test]
fn potion_cost_expiry_matches_actual_dll_card_energy_cost_vectors() {
    let data = vectors();
    let mut checked = 0;
    for row in data["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["case"] == "cost_modifiers")
    {
        assert_eq!(row["world_errors"]["count"], 0);
        let mut state = board();
        let uid = CardUid::new(1).unwrap();
        let mut card = card_pod_from_id(CardId::RocketPunch, false).unwrap();
        card.uid = uid;
        state.cards.push(card);
        state.player.hand.push(uid);
        let mut costs = vec![sts2_core::hooks::effective_card_cost(&state, uid)];
        for operation in row["operations"].as_array().unwrap() {
            let value = operation["amount"].as_i64().unwrap_or(0) as i32;
            match operation["op"].as_str().unwrap() {
                "set_turn" => {
                    state.run_rng.combat_energy_costs = (0_u64..1000)
                        .map(sts2_core::dotnet_rng::Rng::new)
                        .find(|rng| {
                            let mut candidate = rng.clone();
                            candidate.next_int(0, 3) == value
                        })
                        .unwrap();
                    state.snecko_randomize_hand_costs();
                }
                "free_combat" => state.set_card_free_this_combat(uid),
                "discount_until_played" => {
                    for _ in 0..value {
                        let id =
                            CardUid::new(u32::try_from(state.cards.len() + 1).unwrap()).unwrap();
                        let mut status = card_pod_from_id(CardId::Wound, false).unwrap();
                        status.uid = id;
                        state.cards.push(status);
                        sts2_core::card_effects::route_generated_into_hand(&mut state, &[id]);
                    }
                }
                "played_cleanup" => state.clear_card_play_cost_modifiers(uid),
                "turn_cleanup" => state.restore_snecko_costs_for_turn_start(),
                _ => panic!("unknown DLL cost operation"),
            }
            costs.push(sts2_core::hooks::effective_card_cost(&state, uid));
        }
        assert_eq!(
            serde_json::json!(costs),
            row["result"]["costs"],
            "{}",
            row["name"]
        );
        if row["name"] == "equal_roll_keeps_earlier_absolute" {
            assert_eq!(state.cards[0].combat_var(VarKey::CostAbsolutelySet), 1);
        }
        checked += 1;
    }
    assert_eq!(checked, 4);
}
