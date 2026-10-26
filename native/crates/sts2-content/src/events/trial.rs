//! v0.111.0 Models.Events/Trial.cs: complete solo multi-page judgments.
use super::{always_allowed, default_option_info_from_spec, EventOptionSpec, EventSpec};
use sts2_core::cards::CardId;
use sts2_core::{CardReward, EventId, EventOptionInfo, EventOutcome, Reward, RunState};

fn ongoing(description: &str) -> EventOutcome {
    EventOutcome {
        finished: false,
        description: description.to_string(),
        effects: Vec::new(),
    }
}
fn curse(state: &mut RunState, id: CardId) {
    if let Some(card) = crate::cards::spec_index::card_pod_from_id(id, false) {
        state.add_card_to_deck(card, sts2_core::relic_effects::AddSource::EventGain);
    }
}
fn accept(state: &mut RunState) -> EventOutcome {
    let mut rng = state
        .event_state
        .rng
        .take()
        .unwrap_or_else(|| sts2_core::dotnet_rng::Rng::with_name(state.rng.seed, "TRIAL"));
    state.event_state.page = 2 + rng.next_int_exclusive(0, 3) as u8;
    state.event_state.rng = Some(rng);
    ongoing("Judge the defendant guilty or innocent.")
}
fn reject(state: &mut RunState) -> EventOutcome {
    state.event_state.page = 1;
    ongoing("Accept the trial or double down.")
}
fn double_down(state: &mut RunState) -> EventOutcome {
    state.event_state.page = 5;
    ongoing("Abandon this run?")
}
fn cancel_abandon(state: &mut RunState) -> EventOutcome {
    state.event_state.page = 1;
    ongoing("Accept the trial or double down.")
}
fn confirm_abandon(state: &mut RunState) -> EventOutcome {
    state.is_over = true;
    state.player_won = false;
    EventOutcome::finished("Run abandoned.")
}
fn merchant_guilty(state: &mut RunState) -> EventOutcome {
    curse(state, CardId::Regret);
    state.obtain_random_event_relics_sequentially(2);
    EventOutcome::finished("Gained Regret and two relics.")
}
fn merchant_innocent(state: &mut RunState) -> EventOutcome {
    curse(state, CardId::Shame);
    state.upgrade_random_cards(2);
    EventOutcome::finished("Gained Shame; upgrade two cards.")
}
fn noble_guilty(state: &mut RunState) -> EventOutcome {
    state.heal(10);
    EventOutcome::finished("Healed 10 HP.")
}
fn noble_innocent(state: &mut RunState) -> EventOutcome {
    curse(state, CardId::Regret);
    state.gain_gold(300);
    EventOutcome::finished("Gained Regret and 300 gold.")
}
fn nondescript_guilty(state: &mut RunState) -> EventOutcome {
    curse(state, CardId::Doubt);
    for _ in 0..2 {
        state
            .pending_rewards
            .push(Reward::Card(CardReward::empty()));
    }
    EventOutcome::finished("Gained Doubt and two card rewards.")
}
fn nondescript_innocent(state: &mut RunState) -> EventOutcome {
    curse(state, CardId::Doubt);
    super::transform_two_cards_from_event(state, "TRIAL", 1);
    EventOutcome::finished("Gained Doubt; transform two cards.")
}
fn generate_options(state: &RunState, spec: &EventSpec) -> Vec<EventOptionInfo> {
    let ids: &[&str] = match state.event_state.page {
        0 => &["accept", "reject"],
        1 => &["accept", "double_down"],
        2 => &["merchant_guilty", "merchant_innocent"],
        3 => &["noble_guilty", "noble_innocent"],
        4 => &["nondescript_guilty", "nondescript_innocent"],
        5 => &["confirm_abandon", "cancel_abandon"],
        _ => &[],
    };
    let all = default_option_info_from_spec(state, spec);
    ids.iter()
        .filter_map(|id| all.iter().find(|option| option.option_id == *id).cloned())
        .map(|mut option| {
            if state.event_state.page == 1 && option.option_id == "accept" {
                option.loc_key = "TRIAL.pages.REJECT.options.ACCEPT";
            }
            option
        })
        .collect()
}
const OPTIONS: &[EventOptionSpec] = &[
    EventOptionSpec {
        option_id: "accept",
        loc_key: "TRIAL.pages.INITIAL.options.ACCEPT",
        default_label: "Accept",
        default_description: "Judge the defendant.",
        default_vars: &[],
        vars_fn: None,
        title_vars_fn: None,
        handler: accept,
    },
    EventOptionSpec {
        option_id: "reject",
        loc_key: "TRIAL.pages.INITIAL.options.REJECT",
        default_label: "Reject",
        default_description: "Reconsider the trial.",
        default_vars: &[],
        vars_fn: None,
        title_vars_fn: None,
        handler: reject,
    },
    EventOptionSpec {
        option_id: "double_down",
        loc_key: "TRIAL.pages.REJECT.options.DOUBLE_DOWN",
        default_label: "Double Down",
        default_description: "Open abandonment confirmation.",
        default_vars: &[],
        vars_fn: None,
        title_vars_fn: None,
        handler: double_down,
    },
    EventOptionSpec {
        option_id: "confirm_abandon",
        loc_key: "",
        default_label: "Abandon Run",
        default_description: "End this run.",
        default_vars: &[],
        vars_fn: None,
        title_vars_fn: None,
        handler: confirm_abandon,
    },
    EventOptionSpec {
        option_id: "cancel_abandon",
        loc_key: "",
        default_label: "Cancel",
        default_description: "Return to the trial.",
        default_vars: &[],
        vars_fn: None,
        title_vars_fn: None,
        handler: cancel_abandon,
    },
    EventOptionSpec {
        option_id: "merchant_guilty",
        loc_key: "TRIAL.pages.MERCHANT.options.GUILTY",
        default_label: "Guilty",
        default_description: "Gain Regret and two relics.",
        default_vars: &[],
        vars_fn: None,
        title_vars_fn: None,
        handler: merchant_guilty,
    },
    EventOptionSpec {
        option_id: "merchant_innocent",
        loc_key: "TRIAL.pages.MERCHANT.options.INNOCENT",
        default_label: "Innocent",
        default_description: "Gain Shame; upgrade two cards.",
        default_vars: &[],
        vars_fn: None,
        title_vars_fn: None,
        handler: merchant_innocent,
    },
    EventOptionSpec {
        option_id: "noble_guilty",
        loc_key: "TRIAL.pages.NOBLE.options.GUILTY",
        default_label: "Guilty",
        default_description: "Heal 10 HP.",
        default_vars: &[],
        vars_fn: None,
        title_vars_fn: None,
        handler: noble_guilty,
    },
    EventOptionSpec {
        option_id: "noble_innocent",
        loc_key: "TRIAL.pages.NOBLE.options.INNOCENT",
        default_label: "Innocent",
        default_description: "Gain Regret and 300 gold.",
        default_vars: &[],
        vars_fn: None,
        title_vars_fn: None,
        handler: noble_innocent,
    },
    EventOptionSpec {
        option_id: "nondescript_guilty",
        loc_key: "TRIAL.pages.NONDESCRIPT.options.GUILTY",
        default_label: "Guilty",
        default_description: "Gain Doubt and two card rewards.",
        default_vars: &[],
        vars_fn: None,
        title_vars_fn: None,
        handler: nondescript_guilty,
    },
    EventOptionSpec {
        option_id: "nondescript_innocent",
        loc_key: "TRIAL.pages.NONDESCRIPT.options.INNOCENT",
        default_label: "Innocent",
        default_description: "Gain Doubt; transform two cards.",
        default_vars: &[],
        vars_fn: None,
        title_vars_fn: None,
        handler: nondescript_innocent,
    },
];
pub const TRIAL: EventSpec = EventSpec {
    id: EventId::Trial,
    python_name: "Trial",
    is_allowed: always_allowed,
    generate_options,
    options: OPTIONS,
};

#[cfg(test)]
mod tests {
    use super::*;
    fn deck(state: &mut RunState) {
        state.master_deck = [
            CardId::StrikeNecrobinder,
            CardId::DefendNecrobinder,
            CardId::Bash,
        ]
        .into_iter()
        .map(|id| crate::cards::spec_index::card_pod_from_id(id, false).unwrap())
        .collect();
    }
    #[test]
    fn trial_reject_and_abandon_require_explicit_confirmation_and_accept_rolls_once() {
        let mut state = RunState::new();
        assert!(!reject(&mut state).finished);
        assert!(!double_down(&mut state).finished);
        assert!(!state.is_over);
        assert!(!cancel_abandon(&mut state).finished);
        assert_eq!(state.event_state.page, 1);
        assert!(!accept(&mut state).finished);
        assert!((2..=4).contains(&state.event_state.page));
        assert_eq!(state.event_state.rng.as_ref().unwrap().counter(), 1);
        assert_eq!(generate_options(&state, &TRIAL).len(), 2);
        double_down(&mut state);
        assert!(confirm_abandon(&mut state).finished);
        assert!(state.is_over);
        assert!(!state.player_won);
    }
    #[test]
    fn trial_merchant_and_noble_rewards_apply_source_curses_and_values() {
        crate::relics::dispatch::ensure_installed();
        let mut state = RunState::new();
        state.relic_grab_bag_by_rarity.insert(
            sts2_core::RelicRarity::Rare,
            vec![sts2_core::RelicId::Anchor, sts2_core::RelicId::ArtOfWar],
        );
        merchant_guilty(&mut state);
        assert_eq!(state.master_deck[0].card_id, CardId::Regret);
        assert_eq!(state.relics.len(), 2);
        assert!(
            state.relic_rewards.is_empty(),
            "direct Obtain gives no skip screen"
        );
        state.interactive_card_upgrade = true;
        deck(&mut state);
        merchant_innocent(&mut state);
        assert_eq!(state.master_deck.last().unwrap().card_id, CardId::Shame);
        assert_eq!(state.pending_deck_upgrades, 2);
        state.current_hp = 1;
        noble_guilty(&mut state);
        assert_eq!(state.current_hp, 11);
        let before = state.gold;
        noble_innocent(&mut state);
        assert_eq!(state.gold - before, 300);
        assert_eq!(state.master_deck.last().unwrap().card_id, CardId::Regret);
    }
    #[test]
    fn trial_nondescript_rewards_or_atomic_pair_use_event_stream_after_type_roll() {
        let mut state = RunState::new();
        state.character_id = sts2_core::CharacterId::Necrobinder;
        deck(&mut state);
        state.interactive_card_transform = true;
        let niche = state.rng.niche.counter();
        nondescript_innocent(&mut state);
        assert_eq!(state.master_deck.last().unwrap().card_id, CardId::Doubt);
        assert_eq!(state.pending_deck_transforms.len(), 1);
        assert_eq!(state.pending_deck_transforms[0].batch_count, 2);
        assert_eq!(
            state.pending_deck_transforms[0].event_stream_roll,
            Some(("TRIAL", 1))
        );
        assert_eq!(state.rng.niche.counter(), niche);
        let mut state = RunState::new();
        nondescript_guilty(&mut state);
        assert_eq!(state.master_deck[0].card_id, CardId::Doubt);
        assert_eq!(state.pending_rewards.len(), 2);
    }
    #[test]
    fn trial_second_random_relic_is_not_drawn_or_obtained_before_first_relic_choice() {
        crate::cards::dispatch::ensure_installed();
        crate::relics::dispatch::ensure_installed();
        let mut rm =
            sts2_core::run_manager::RunManager::new(1, sts2_core::CharacterId::Necrobinder)
                .with_content_callbacks(crate::run_callbacks::ironclad_content_callbacks())
                .with_neow_start();
        rm.run_state.enter_event_room(EventId::Trial);
        rm.run_state.event_state.page = 2;
        rm.run_state.relic_grab_bag_by_rarity.insert(
            sts2_core::RelicRarity::Rare,
            vec![sts2_core::RelicId::LostCoffer, sts2_core::RelicId::Anchor],
        );
        assert_eq!(
            rm.take_action(sts2_core::run_manager::RunAction::EventChoice { idx: 0 }),
            sts2_core::run_manager::ActionResult::Advanced
        );
        assert!(rm
            .run_state
            .relics
            .contains(&sts2_core::RelicId::LostCoffer));
        assert!(!rm.run_state.relics.contains(&sts2_core::RelicId::Anchor));
        assert_eq!(rm.run_state.pending_post_pick_random_relic_grants, 1);
        assert_eq!(rm.phase(), sts2_core::run_manager::Phase::CardReward);
        assert_eq!(
            rm.take_action(sts2_core::run_manager::RunAction::PickCardReward { idx: None }),
            sts2_core::run_manager::ActionResult::Advanced
        );
        assert!(rm.run_state.relics.contains(&sts2_core::RelicId::Anchor));
        assert_eq!(rm.run_state.pending_post_pick_random_relic_grants, 0);
    }
}
