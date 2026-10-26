//! Actual beta DLL vectors, exercised through the deployed Rust combat exit.
use serde_json::{json, Value};
use sts2_content::run_callbacks::{
    initialize_run_setup, new_run_manager_for_character_with_ascension,
};
use sts2_core::run_manager::{Phase, RunAction};
use sts2_core::runstate::{Reward, Room, RoomType};
use sts2_core::{CharacterId, RelicId};

fn relic(raw: &str) -> RelicId {
    match raw {
        "WING_CHARM" => RelicId::WingCharm,
        "GLITTER" => RelicId::Glitter,
        "FRESNEL_LENS" => RelicId::FresnelLens,
        "LAVA_LAMP" => RelicId::LavaLamp,
        "LASTING_CANDY" => RelicId::LastingCandy,
        "PRAYER_WHEEL" => RelicId::PrayerWheel,
        "WHITE_STAR" => RelicId::WhiteStar,
        "BLACK_STAR" => RelicId::BlackStar,
        "LAVA_ROCK" => RelicId::LavaRock,
        "WONGOS_MYSTERY_TICKET" => RelicId::WongosMysteryTicket,
        "AMETHYST_AUBERGINE" => RelicId::AmethystAubergine,
        _ => panic!("unexpected input relic {raw}"),
    }
}

#[test]
fn beta_combat_reward_sets_match_thirty_eight_actual_csharp_population_vectors() {
    sts2_content::relics::dispatch::ensure_installed();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../docs/beta-migration/reward_set_csharp_vectors.json"
    ))
    .unwrap();
    for case in fixture["vectors"].as_array().unwrap() {
        let label = format!("{} {}", case["seed"], case["case"]);
        let seed = sts2_core::dotnet_rng::run_seed_hash(&sts2_core::canonicalize_seed(
            case["seed"].as_str().unwrap(),
        ));
        let act = u8::try_from(case["act"].as_u64().unwrap()).unwrap();
        let mut rm = new_run_manager_for_character_with_ascension(
            seed,
            CharacterId::Necrobinder,
            if act == 3 { 10 } else { 0 },
        );
        initialize_run_setup(&mut rm);
        rm.run_state.relics = case["relics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| relic(id.as_str().unwrap()))
            .collect();
        let original_relics = rm.run_state.relics.len();
        rm.run_state.relic_counters.clear();
        if rm.run_state.relics.contains(&RelicId::LastingCandy) {
            rm.run_state.relic_counters.insert(
                RelicId::LastingCandy,
                i32::try_from(case["candy_seen"].as_i64().unwrap()).unwrap(),
            );
        }
        if rm.run_state.relics.contains(&RelicId::WongosMysteryTicket) {
            rm.run_state.relic_counters.insert(
                RelicId::WongosMysteryTicket,
                i32::try_from(case["wongo_combats"].as_i64().unwrap()).unwrap() - 1,
            );
        }
        rm.run_state.act_index = act;
        let encounter = case["encounter"].as_str().unwrap();
        let room_type = if encounter.ends_with("ELITE") {
            RoomType::Elite
        } else if encounter.ends_with("BOSS") {
            RoomType::Boss
        } else {
            RoomType::Monster
        };
        let mut room = Room::new(room_type);
        if act == 3 {
            rm.run_state.generate_map();
            let map = rm.run_state.current_act_map.as_ref().unwrap();
            room.map_coord = Some(map.point(map.boss.unwrap()).coord);
        }
        rm.run_state.current_room = Some(room);
        rm.run_state.gold = 0;
        rm.run_state.potions.clear();
        let combat = sts2_content::run_bridge::enter_combat(
            &rm.run_state,
            &sts2_core::encounters::act1_normal_pool()[0],
        );
        rm.debug_enter_combat(combat, room_type);
        for amount in case["extra_gold"].as_array().unwrap() {
            rm.run_state.pending_rewards.push(Reward::UnpopulatedGold {
                amount: i32::try_from(amount.as_i64().unwrap()).unwrap(),
            });
        }
        assert_eq!(
            serde_json::to_value(rm.run_state.rng.rewards.state()).unwrap(),
            case["result"]["initial_rng"],
            "initial {label}"
        );
        rm.resolve_combat_fast(0.0, true);
        let rewards = case["result"]["rewards"].as_array().unwrap();
        let gold: i64 = rewards
            .iter()
            .filter_map(|reward| reward.get("gold").and_then(Value::as_i64))
            .sum();
        assert_eq!(i64::from(rm.run_state.gold), gold, "gold {label}");
        let expected_relics: Vec<_> = rewards
            .iter()
            .filter_map(|reward| reward.get("relic").and_then(Value::as_str))
            .collect();
        let actual_relics: Vec<_> = rm.run_state.relics[original_relics..]
            .iter()
            .map(|id| id.to_python_name())
            .collect();
        assert_eq!(actual_relics, expected_relics, "relics {label}");
        let expected_potions: Vec<_> = rewards
            .iter()
            .filter_map(|reward| reward.get("potion").and_then(Value::as_str))
            .collect();
        let actual_potions: Vec<_> = rm
            .run_state
            .potions
            .iter()
            .map(|kind| kind.python_name())
            .collect();
        assert_eq!(actual_potions, expected_potions, "potions {label}");
        let expected_slates: Vec<_> = rewards
            .iter()
            .filter_map(|reward| reward.get("cards"))
            .cloned()
            .collect();
        let mut actual_slates = Vec::new();
        for _ in 0..8 {
            if rm.card_reward_options().is_empty() {
                break;
            }
            actual_slates.push(Value::Array(
                rm.card_reward_prepared_cards()
                    .iter()
                    .map(|card| {
                        let id = match card.card_id {
                            sts2_core::cards::CardId::NecroMasteryCard => "NECRO_MASTERY",
                            sts2_core::cards::CardId::CountdownCard => "COUNTDOWN",
                            id => id.to_python_name(),
                        };
                        let mut value = json!({"id": id, "upgraded": card.upgraded});
                        if !card.enchantments.is_empty() {
                            value.as_object_mut().unwrap().insert("enchantments".into(),
                        Value::Array(card.enchantments.iter().map(|(id, amount)| {
                            json!({"id": format!("{id:?}").to_uppercase(), "amount": amount})
                        }).collect()));
                        }
                        value
                    })
                    .collect(),
            ));
            rm.take_action(RunAction::PickCardReward { idx: None });
        }
        assert_eq!(actual_slates, expected_slates, "card slates {label}");
        assert_eq!(
            serde_json::to_value(rm.run_state.rng.rewards.state()).unwrap(),
            case["result"]["final_rng"],
            "final RNG {label}"
        );
        assert_eq!(
            serde_json::to_value(rm.run_state.rng.niche.state()).unwrap(),
            case["result"]["final_niche_rng"],
            "Niche RNG {label}"
        );
        if let Some(seen) = case["result"]["lasting_candy_seen"].as_i64() {
            assert_eq!(
                i64::from(rm.run_state.relic_counters[&RelicId::LastingCandy]),
                seen,
                "Candy count {label}"
            );
        }
        if act == 3 {
            assert_eq!(rm.phase(), Phase::MapChoice, "A10 continues {label}");
        }
    }
}
