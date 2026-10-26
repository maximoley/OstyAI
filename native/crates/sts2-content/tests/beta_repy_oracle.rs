//! Independent state-by-state comparison against actual v0.111.0 event methods.
use serde_json::{json, Value};
use sts2_content::events::dispatch::present;
use sts2_content::run_callbacks::{
    initialize_run_setup, new_run_manager_for_character_with_ascension,
};
use sts2_core::cards::CardId;
use sts2_core::dotnet_rng::Rng;
use sts2_core::run_manager::{ActionResult, RunAction};
use sts2_core::runstate::RunState;
use sts2_core::{CharacterId, EventId, PotionKind};

fn state(rs: &RunState, event_rng: &Rng) -> Value {
    json!({
        "gold": rs.gold, "hp": rs.current_hp, "max_hp": rs.max_hp,
        "freed_repy": rs.freed_repy,
        "deck": rs.master_deck.iter().map(|card| json!({
            "id": card.card_id.to_python_name(), "upgraded": card.upgraded,
            "original_index": card.uid.get() - 1
        })).collect::<Vec<_>>(),
        "relics": rs.relics.iter().map(|id| id.to_python_name()).collect::<Vec<_>>(),
        "potions": rs.potions.iter().map(|kind| kind.python_name()).collect::<Vec<_>>(),
        "event_rng": event_rng.state(), "rewards": rs.rng.rewards.state(),
        "niche": rs.rng.niche.state(), "finished": rs.current_event.is_none(),
        "options": present(rs).unwrap_or_default().iter().map(|option| option.loc_key).collect::<Vec<_>>()
    })
}

fn compare(actual: Value, expected: &Value, label: &str, failures: &mut Vec<String>) {
    for (key, value) in expected.as_object().unwrap() {
        if actual.get(key) != Some(value) {
            failures.push(format!(
                "{label} {key}: actual={} expected={value}",
                actual[key]
            ));
        }
    }
}

#[test]
fn beta_repy_matches_twenty_four_actual_csharp_lifecycles() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../docs/beta-migration/repy_csharp_vectors.json"
    ))
    .unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 24);
    let mut failures = Vec::new();
    for case in cases {
        let label = format!(
            "{} keys={} option={} full={}",
            case["seed"], case["keys"], case["option"], case["full_belt"]
        );
        let seed = case["run_seed"].as_u64().unwrap();
        let mut rm =
            new_run_manager_for_character_with_ascension(seed, CharacterId::Necrobinder, 0)
                .with_neow_start();
        initialize_run_setup(&mut rm);
        let rs = &mut rm.run_state;
        rs.relics.clear();
        rs.relic_counters.clear();
        rs.master_deck = case["deck_input"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, raw)| {
                let id = match raw.as_str().unwrap() {
                    "STRIKE_NECROBINDER" => CardId::StrikeNecrobinder,
                    "DEFEND_NECROBINDER" => CardId::DefendNecrobinder,
                    "LANTERN_KEY" => CardId::LanternKey,
                    other => panic!("unhandled input {other}"),
                };
                let mut card =
                    sts2_content::cards::spec_index::card_pod_from_id(id, false).unwrap();
                card.uid = sts2_core::CardUid::new(u32::try_from(index + 1).unwrap()).unwrap();
                card
            })
            .collect();
        rs.current_hp = 50;
        rs.max_hp = 66;
        rs.gold = 100;
        rs.potions = case["initial"]["potions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|raw| match raw.as_str().unwrap() {
                "BLOCK_POTION" => PotionKind::BlockPotion,
                "STRENGTH_POTION" => PotionKind::StrengthPotion,
                "ENERGY_POTION" => PotionKind::EnergyPotion,
                other => panic!("unhandled input {other}"),
            })
            .collect();
        rs.interactive_potion_swap = false;
        rs.enter_event_room(EventId::WarHistorianRepy);
        let event_rng = Rng::with_name(seed, "WAR_HISTORIAN_REPY");
        compare(
            state(rs, &event_rng),
            &case["initial"],
            &format!("initial {label}"),
            &mut failures,
        );
        for (index, expected) in case["stages"].as_array().unwrap().iter().enumerate() {
            let idx = if index == 0 {
                case["option"].as_u64().unwrap().try_into().unwrap()
            } else {
                0
            };
            assert_eq!(
                rm.take_action(RunAction::EventChoice { idx }),
                ActionResult::Advanced,
                "action {index} {label}"
            );
            compare(
                state(&rm.run_state, &event_rng),
                expected,
                &format!("stage {index} {label}"),
                &mut failures,
            );
        }
        compare(
            state(&rm.run_state, &event_rng),
            &case["final"],
            &format!("final {label}"),
            &mut failures,
        );
        // Every offered relic is passive in these vectors and must be claimed in order.
        let expected_relics: Vec<_> = case["final"]["relics"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let offered: Vec<_> = case["offers"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|offer| offer["rewards"].as_array().unwrap())
            .filter(|reward| reward["kind"] == "RelicReward")
            .map(|reward| reward["id"].as_str().unwrap())
            .collect();
        for id in offered {
            assert!(expected_relics.contains(&id), "offered relic {id} {label}");
        }
    }
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
