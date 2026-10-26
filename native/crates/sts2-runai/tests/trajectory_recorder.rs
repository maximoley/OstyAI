// The recorder lives in the CLI-sweep cluster (`sweeps` feature).
#![cfg(feature = "sweeps")]
//! Run-trajectory recorder (`sts2_runai::trajectory`) — projection +
//! ledger contract tests.
//!
//! Two flavours:
//!
//! * **Driven runs** — a deterministic [`RandomPolicy`] wrapped in the
//!   [`RecordingPolicy`] decorator, driven through a whole run, then
//!   projected with `into_run_file`. These pin the native-`RunHistory`
//!   shape and the offered payloads against a real trajectory.
//! * **Hand-driven ledgers** — a stub policy fed synthetic `RunManager`
//!   states so the floor-of-addition / room-visit bookkeeping can be
//!   asserted exactly (no RNG, no engine).
//!
//! Native-schema expectations come from the v0.107.1 decompilation
//! (`MegaCrit.Sts2.Core.Runs/RunHistory.cs`,
//! `MegaCrit.Sts2.Core.Saves.Runs/SerializableCard.cs`): a deck entry's
//! `id` is the **base** card ModelId with the upgrade level in its own
//! field, and `killed_by_encounter` defaults to `"NONE.NONE"`.

use std::collections::BTreeMap;

use sts2_content::run_callbacks::{
    initialize_run_setup, new_run_manager_for_character_with_ascension,
};
use sts2_core::cards::{Card, CardId};
use sts2_core::events::EventId;
use sts2_core::full_run::{
    drive_full_run, FullRunConfig, FullRunReport, RandomPolicy, RunOutcome, RunPolicy,
};
use sts2_core::map::MapCoord;
use sts2_core::potions::{CharacterId, PotionKind};
use sts2_core::run_manager::{RunAction, RunManager};
use sts2_core::runstate::{Room, RoomType};
use sts2_runai::policy::DecisionTelemetry;
use sts2_runai::trajectory::schema::{Offered, RunFile};
use sts2_runai::trajectory::{Detail, Meta, RecordingPolicy, RECORDER_VERSION};

/// Room-type debug strings that legitimately carry an encounter. The
/// recorder stamps `format!("{:?}", RoomType)`, so these are the exact
/// tokens a combat map point can show.
const COMBAT_ROOM_TYPES: [&str; 3] = ["Monster", "Elite", "Boss"];

/// Seed whose coverage-mode run wins and walks all three acts (shops,
/// rest sites, boss relics, upgraded card rewards).
const DEEP_SEED: u64 = 1;
/// Seed whose coverage-mode run dies to the act-2 boss.
const DEATH_SEED: u64 = 2;

/// The arena's run construction (Neow start + deck-mutation surfaces +
/// the shared C#-faithful setup), mirroring `arena::new_arena_run_manager`.
fn build_run(seed: u64) -> RunManager {
    sts2_runai::rollout::ensure_content_installed();
    let mut rm = new_run_manager_for_character_with_ascension(seed, CharacterId::Ironclad, 0)
        .with_neow_start();
    rm.run_state.interactive_card_removal = true;
    rm.run_state.interactive_card_upgrade = true;
    rm.run_state.interactive_card_transform = true;
    rm.run_state.direct_event_relic_grant = true;
    initialize_run_setup(&mut rm);
    rm
}

fn meta(seed: u64, detail: Detail) -> Meta {
    Meta {
        seed_int: seed,
        character: "ironclad".into(),
        ascension: 0,
        policy: "random".into(),
        policy_label: "random".into(),
        combat_budget: "none".into(),
        sims: 0,
        boss_sims: 0,
        model_path: None,
        model_sha256: None,
        engine_git_sha: None,
        env: BTreeMap::new(),
        detail: detail.token().to_string(),
        outcome: "unset".into(),
        // Deliberately bogus — `into_run_file` must overwrite these from
        // the report.
        final_hp: -1,
        final_gold: -1,
        steps: 0,
    }
}

/// The run-side facts a projection is checked against (the projection
/// consumes the recorder, so snapshot what the assertions need).
struct Recorded {
    file: RunFile,
    report: FullRunReport,
    /// `("CARD.X", upgraded)` per master-deck card, in deck order.
    deck: Vec<(String, bool)>,
    relic_count: usize,
    potion_count: usize,
    max_potion_slots: i32,
    total_floor: i32,
}

fn deck_tokens(rm: &RunManager) -> Vec<(String, bool)> {
    rm.run_state
        .master_deck
        .iter()
        .map(|c| (format!("CARD.{}", c.card_id.to_python_name()), c.upgraded))
        .collect()
}

/// Drive one whole run through the recorder and project it.
///
/// `invincible` is the driver's coverage mode: random play otherwise dies
/// in act 1, and the shop / rest / boss-relic offered payloads only exist
/// deeper in a run. It perturbs combat state, never the recorder.
fn record_run(seed: u64, detail: Detail, invincible: bool) -> Recorded {
    let mut rm = build_run(seed);
    let mut policy = RecordingPolicy::new(RandomPolicy::from_run_seed(seed), detail);
    let cfg = FullRunConfig {
        invincible,
        ..Default::default()
    };
    let report = drive_full_run(&mut rm, &mut policy, &cfg);
    let deck = deck_tokens(&rm);
    let relic_count = rm.run_state.relics.len();
    let potion_count = rm.run_state.potions.len();
    let max_potion_slots = rm.run_state.max_potion_slots;
    let total_floor = rm.run_state.total_floor;
    let file = policy.into_run_file(&rm, &report, meta(seed, detail));
    Recorded {
        file,
        report,
        deck,
        relic_count,
        potion_count,
        max_potion_slots,
        total_floor,
    }
}

// ── native RunHistory projection ──────────────────────────────────────

#[test]
fn projection_fills_the_native_run_history_header() {
    let r = record_run(DEEP_SEED, Detail::Full, true);
    let f = &r.file;

    assert_eq!(f.schema_version, 9, "native RunHistory v9");
    assert_eq!(f.platform_type, "sts2sim");
    assert_eq!(f.game_mode, "standard");
    assert!(!f.was_abandoned);
    // Integer-seeded runs stamp the decimal form; `meta.seed_int` is the
    // round-trip source.
    assert_eq!(f.seed, DEEP_SEED.to_string());
    assert_eq!(f.sts2sim.meta.seed_int, DEEP_SEED);
    assert_eq!(f.ascension, 0);
    // Deterministic runs have no wall clock.
    assert_eq!(f.start_time, 0);
    assert_eq!(f.win, r.report.player_won);
    assert!(f.win, "seed {DEEP_SEED} in coverage mode is a winning run");
    assert_eq!(f.players.len(), 1);
    assert_eq!(f.players[0].id, 1);
    assert_eq!(f.players[0].character, "CHARACTER.IRONCLAD");
    assert_eq!(f.players[0].max_potion_slot_count, r.max_potion_slots);
    // The report is authoritative for the outcome fields, overwriting
    // whatever the caller put in `Meta`.
    assert_eq!(f.sts2sim.meta.steps, r.report.steps);
    assert_eq!(f.sts2sim.meta.final_hp, r.report.final_hp);
    assert_eq!(f.sts2sim.meta.final_gold, r.report.final_gold);
    assert_eq!(f.sts2sim.recorder_version, RECORDER_VERSION);
}

#[test]
fn deck_projection_is_the_final_master_deck_with_base_ids() {
    let r = record_run(DEEP_SEED, Detail::Full, true);
    let deck = &r.file.players[0].deck;

    assert_eq!(deck.len(), r.deck.len());
    for (entry, (token, upgraded)) in deck.iter().zip(&r.deck) {
        // Native `SerializableCard`: `id` is the plain card ModelId and the
        // upgrade level rides in its own field — never a `+` suffix.
        assert_eq!(&entry.id, token);
        assert!(!entry.id.ends_with('+'), "deck id must be the base token");
        assert_eq!(entry.upgrades, u32::from(*upgraded));
        assert!(
            entry.floor_added_to_deck >= 0 && entry.floor_added_to_deck <= r.total_floor,
            "floor {} outside the run (0..={})",
            entry.floor_added_to_deck,
            r.total_floor
        );
    }
    assert!(
        deck.iter().any(|d| d.floor_added_to_deck == 0),
        "starter cards are stamped floor 0"
    );
    assert!(
        deck.iter().any(|d| d.floor_added_to_deck > 0),
        "a 3-act run adds cards after floor 0"
    );
    assert!(
        deck.iter().any(|d| d.upgrades == 1),
        "a 3-act run upgrades something (smith / pre-upgraded reward)"
    );

    // Copies of the same base token take their floors earliest-first, so
    // per token the stamps ascend in deck order.
    let mut seen: BTreeMap<&str, i32> = BTreeMap::new();
    for e in deck {
        let prev = seen.entry(e.id.as_str()).or_insert(i32::MIN);
        assert!(
            e.floor_added_to_deck >= *prev,
            "{} floors must ascend across copies",
            e.id
        );
        *prev = e.floor_added_to_deck;
    }
}

#[test]
fn relic_and_potion_entries_carry_native_tokens() {
    let r = record_run(DEEP_SEED, Detail::Full, true);
    let player = &r.file.players[0];

    assert_eq!(player.relics.len(), r.relic_count);
    assert!(
        !player.relics.is_empty(),
        "the starter relic is always held"
    );
    for relic in &player.relics {
        assert!(relic.id.starts_with("RELIC."), "{}", relic.id);
        assert!(
            relic.floor_added_to_deck >= 0 && relic.floor_added_to_deck <= r.total_floor,
            "{} floor {} outside the run",
            relic.id,
            relic.floor_added_to_deck
        );
    }
    assert_eq!(player.potions.len(), r.potion_count);
    for potion in &player.potions {
        assert!(potion.id.starts_with("POTION."), "{}", potion.id);
    }
}

#[test]
fn map_point_history_is_act_grouped_and_floor_ordered() {
    let r = record_run(DEEP_SEED, Detail::Full, true);
    let history = &r.file.map_point_history;

    assert_eq!(history.len(), 3, "a winning run walks all three acts");
    let mut last_floor = -1;
    for act in history {
        assert!(!act.is_empty(), "every visited act has map points");
        for entry in act {
            // One map point per floor, in visit order, across the run.
            assert!(
                entry.floor > last_floor,
                "floors must strictly ascend: {} after {last_floor}",
                entry.floor
            );
            last_floor = entry.floor;
            for id in &entry.monster_ids {
                assert!(id.starts_with("MONSTER."), "{id}");
            }
            if let Some(ev) = &entry.event_id {
                assert!(ev.starts_with("EVENT."), "{ev}");
            }
            // Every fight is attributed: no combat map point is blank.
            if COMBAT_ROOM_TYPES.contains(&entry.map_point_type.as_str()) {
                assert!(
                    !entry.monster_ids.is_empty(),
                    "combat room {} @{} lost its roster",
                    entry.map_point_type,
                    entry.floor
                );
            }
        }
    }
}

/// Regression: the enrich block once read `rm.combat` without gating on
/// `Phase::Combat`, but `exit_combat_and_transition` hands the
/// `CombatState` straight back — so the next shop / campfire / treasure
/// inherited the last fight's roster. (The same gate existed on the
/// never-merged `worktree-recreate-ppo-ic-a0-corpus` branch, `11f78f43b`.)
#[test]
fn non_combat_rooms_carry_no_monster_ids() {
    let r = record_run(DEEP_SEED, Detail::Full, true);
    for act in &r.file.map_point_history {
        for entry in act {
            if !COMBAT_ROOM_TYPES.contains(&entry.map_point_type.as_str()) {
                assert!(
                    entry.monster_ids.is_empty(),
                    "{} @{} carries stale {:?}",
                    entry.map_point_type,
                    entry.floor,
                    entry.monster_ids
                );
            }
        }
    }
}

#[test]
fn killed_by_encounter_is_none_on_a_win_and_the_last_lead_on_a_loss() {
    let won = record_run(DEEP_SEED, Detail::Full, true);
    assert!(won.file.win);
    // Native default ModelId for "not killed by anything".
    assert_eq!(won.file.killed_by_encounter, "NONE.NONE");
    assert_eq!(won.file.killed_by_event, "NONE.NONE");

    let lost = record_run(DEATH_SEED, Detail::Full, false);
    assert!(!lost.file.win);
    assert!(
        lost.file.killed_by_encounter.starts_with("MONSTER."),
        "a death in combat names its killer, got {}",
        lost.file.killed_by_encounter
    );
    // The killer is the lead of the last encounter actually fought.
    let last_roster = lost
        .file
        .map_point_history
        .iter()
        .flatten()
        .rfind(|e| !e.monster_ids.is_empty())
        .expect("the run fought something");
    assert_eq!(lost.file.killed_by_encounter, last_roster.monster_ids[0]);
}

// ── decision stream ───────────────────────────────────────────────────

#[test]
fn detail_level_gates_the_decision_stream_only() {
    let full = record_run(DEEP_SEED, Detail::Full, false);
    let decisions = record_run(DEEP_SEED, Detail::Decisions, false);
    let summary = record_run(DEEP_SEED, Detail::Summary, false);

    // Ledgers + room history feed the native projection and are tracked at
    // every detail level.
    let native =
        |r: &Recorded| serde_json::to_value((&r.file.players, &r.file.map_point_history)).unwrap();
    assert_eq!(native(&full), native(&decisions));
    assert_eq!(native(&full), native(&summary));

    assert!(summary.file.sts2sim.decisions.is_empty());
    assert!(!full.file.sts2sim.decisions.is_empty());
    assert!(full
        .file
        .sts2sim
        .decisions
        .iter()
        .any(|d| d.phase == "Combat"));
    assert!(
        decisions
            .file
            .sts2sim
            .decisions
            .iter()
            .all(|d| d.phase != "Combat"),
        "`decisions` detail drops combat plies"
    );

    // The `step` cursor counts every decision the policy made, not just the
    // recorded ones — so the `decisions` stream is exactly the non-combat
    // subsequence of the `full` one, step numbers included.
    let non_combat: Vec<_> = full
        .file
        .sts2sim
        .decisions
        .iter()
        .filter(|d| d.phase != "Combat")
        .map(|d| serde_json::to_value(d).unwrap())
        .collect();
    let recorded: Vec<_> = decisions
        .file
        .sts2sim
        .decisions
        .iter()
        .map(|d| serde_json::to_value(d).unwrap())
        .collect();
    assert_eq!(non_combat, recorded);
}

#[test]
fn offered_payloads_are_phase_typed_and_well_formed() {
    let r = record_run(DEEP_SEED, Detail::Full, true);
    let mut saw = BTreeMap::<&str, usize>::new();

    let mut last_step = None;
    for d in &r.file.sts2sim.decisions {
        if let Some(prev) = last_step {
            assert!(d.step > prev, "decision steps must ascend");
        }
        last_step = Some(d.step);
        assert!(d.chosen_idx < d.n_options);

        match &d.offered {
            Offered::CardReward {
                options,
                relic_offer,
                picked,
            } => {
                *saw.entry("card_reward").or_default() += 1;
                assert!(!options.is_empty());
                for o in options {
                    assert!(o.starts_with("CARD."), "{o}");
                }
                if let Some(p) = picked {
                    assert!(options.contains(p), "picked {p} was not offered");
                }
                if let Some(rel) = relic_offer {
                    assert!(rel.starts_with("RELIC."), "{rel}");
                }
            }
            Offered::Shop {
                cards,
                relics,
                potions,
                action,
            } => {
                *saw.entry("shop").or_default() += 1;
                if !cards.is_empty() {
                    *saw.entry("shop_stocked").or_default() += 1;
                }
                assert!(!action.is_empty());
                for (items, prefix) in [(cards, "CARD."), (relics, "RELIC."), (potions, "POTION.")]
                {
                    for item in items {
                        assert!(item.id.starts_with(prefix), "{}", item.id);
                        assert!(item.price > 0, "{} priced {}", item.id, item.price);
                    }
                }
            }
            Offered::BossRelic {
                options,
                picked_idx,
            } => {
                *saw.entry("boss_relic").or_default() += 1;
                // `generate_boss_relic_pool` offers up to 3 (the next act's
                // Ancient event rolls 3 relics).
                assert!(!options.is_empty() && options.len() <= 3, "{options:?}");
                assert!(*picked_idx < options.len());
                for o in options {
                    assert!(o.starts_with("RELIC."), "{o}");
                }
            }
            Offered::Rest {
                option_idx,
                smith_card,
            } => {
                *saw.entry("rest").or_default() += 1;
                assert_ne!(
                    option_idx.is_some(),
                    smith_card.is_some(),
                    "a rest decision is either the option pick or the smith target"
                );
                if let Some(card) = smith_card {
                    assert!(card.starts_with("CARD."), "{card}");
                }
            }
            Offered::Event {
                event_id,
                choice_idx,
                options,
            } => {
                *saw.entry("event").or_default() += 1;
                if let Some(ev) = event_id {
                    assert!(ev.starts_with("EVENT."), "{ev}");
                }
                // The offered set is what makes the pick explainable: when
                // it is recorded at all, the chosen index must address it.
                if !options.is_empty() {
                    assert!(
                        *choice_idx < options.len(),
                        "choice_idx {choice_idx} out of range for {} options",
                        options.len()
                    );
                    for opt in options {
                        assert!(!opt.option_id.is_empty(), "event option needs an id");
                    }
                }
            }
            Offered::Combat(ply) => {
                *saw.entry("combat").or_default() += 1;
                assert_eq!(d.phase, "Combat");
                assert!(
                    ["play", "end_turn", "potion", "resolve"].contains(&ply.action.as_str()),
                    "unexpected combat label {}",
                    ply.action
                );
                if let Some(card) = &ply.card {
                    assert_eq!(ply.action, "play", "only a play resolves a hand card");
                    assert!(card.starts_with("CARD."), "{card}");
                }
                // The board is the parity-bridge snapshot: a plain ply
                // carries the live round, an open choice carries the
                // card-select shape instead.
                let kind = ply.board["type"].as_str().expect("board type tag");
                if kind == "combat_action" {
                    assert_eq!(ply.board["round"], serde_json::json!(ply.round));
                    assert!(ply.board["enemies"].is_array());
                }
            }
            Offered::Map {
                col,
                room: _,
                reachable,
            } => {
                *saw.entry("map").or_default() += 1;
                // `reachable` is the road not taken; the column actually
                // moved to has to be one of the options on offer.
                if !reachable.is_empty() {
                    assert!(
                        reachable.iter().any(|(c, _)| c == col),
                        "chosen col {col} absent from reachable {reachable:?}"
                    );
                }
            }
            Offered::Other { action } => assert!(!action.is_empty()),
        }
    }

    for kind in [
        "card_reward",
        "shop",
        "shop_stocked",
        "boss_relic",
        "rest",
        "event",
        "combat",
        "map",
    ] {
        assert!(saw.contains_key(kind), "no {kind} decision recorded");
    }
}

#[test]
fn recording_does_not_perturb_the_run() {
    for seed in [1, 2, 3] {
        let mut plain_rm = build_run(seed);
        let mut plain = RandomPolicy::from_run_seed(seed);
        let plain_report = drive_full_run(&mut plain_rm, &mut plain, &FullRunConfig::default());

        let recorded = record_run(seed, Detail::Full, false);

        assert_eq!(
            format!("{:?}", plain_report.outcome),
            format!("{:?}", recorded.report.outcome),
            "seed {seed}"
        );
        assert_eq!(plain_report.steps, recorded.report.steps, "seed {seed}");
        assert_eq!(
            plain_report.final_hp, recorded.report.final_hp,
            "seed {seed}"
        );
        assert_eq!(
            plain_report.final_gold, recorded.report.final_gold,
            "seed {seed}"
        );
        assert_eq!(
            deck_tokens(&plain_rm),
            recorded.deck,
            "seed {seed}: the recorded run must end on the same deck"
        );
    }
}

// ── hand-driven ledger bookkeeping ────────────────────────────────────

/// Always takes the first action — the recorder only needs *a* pick to
/// fire its observation pass.
#[derive(Debug)]
struct FirstPolicy;
impl RunPolicy for FirstPolicy {
    fn choose(&mut self, _rm: &RunManager, _actions: &[RunAction]) -> Option<usize> {
        Some(0)
    }
}
// No search and no meta scoring, so the default `None` bodies are exactly
// right — the recorder just needs *a* pick to fire its observation pass.
impl DecisionTelemetry for FirstPolicy {}

fn stub_report(rm: &RunManager, won: bool) -> FullRunReport {
    FullRunReport {
        outcome: if won {
            RunOutcome::Won
        } else {
            RunOutcome::Lost
        },
        steps: 0,
        final_phase: rm.phase(),
        final_hp: rm.run_state.current_hp,
        final_gold: rm.run_state.gold,
        act_index: rm.run_state.act_index,
        player_won: won,
    }
}

/// One observation pass at the manager's current state.
fn observe(policy: &mut RecordingPolicy<FirstPolicy>, rm: &RunManager) {
    let _ = policy.choose(rm, &[RunAction::Move { col: 0 }]);
}

#[test]
fn floor_of_addition_is_stamped_once_and_survives_an_in_place_upgrade() {
    let mut rm = build_run(11);
    rm.run_state.total_floor = 0;
    let starters = rm.run_state.master_deck.len();
    let first_id = rm.run_state.master_deck[0].card_id;

    let mut policy = RecordingPolicy::new(FirstPolicy, Detail::Summary);
    observe(&mut policy, &rm);

    // Floor 4: a genuinely new card.
    rm.run_state.total_floor = 4;
    let mut added = Card::pod_defaults();
    added.card_id = CardId::Thunderclap;
    rm.run_state.master_deck.push(added);
    observe(&mut policy, &rm);

    // Floor 7: upgrade a starter in place (must NOT re-stamp its floor)
    // and add a second copy of that same base token (must stamp floor 7).
    rm.run_state.total_floor = 7;
    rm.run_state.master_deck[0].upgraded = true;
    let mut copy = Card::pod_defaults();
    copy.card_id = first_id;
    rm.run_state.master_deck.push(copy);
    // A potion in the belt at projection time.
    rm.run_state.potions.push(PotionKind::FruitJuice);
    observe(&mut policy, &rm);

    let report = stub_report(&rm, false);
    let file = policy.into_run_file(&rm, &report, meta(11, Detail::Summary));
    let deck = &file.players[0].deck;
    assert_eq!(deck.len(), starters + 2);

    assert_eq!(deck[0].upgrades, 1);
    assert_eq!(
        deck[0].floor_added_to_deck, 0,
        "an in-place upgrade must not re-stamp the card's floor"
    );
    for entry in deck.iter().take(starters) {
        assert_eq!(entry.floor_added_to_deck, 0, "starter {} moved", entry.id);
    }
    let thunderclap = &deck[starters];
    assert_eq!(
        thunderclap.id,
        format!("CARD.{}", CardId::Thunderclap.to_python_name())
    );
    assert_eq!(thunderclap.floor_added_to_deck, 4);
    let extra_copy = &deck[starters + 1];
    assert_eq!(extra_copy.id, deck[0].id, "same base token as the starter");
    assert_eq!(
        extra_copy.floor_added_to_deck, 7,
        "a genuinely new copy stamps the floor it arrived on"
    );

    assert_eq!(file.players[0].potions.len(), 1);
    assert_eq!(
        file.players[0].potions[0].id,
        format!("POTION.{}", PotionKind::FruitJuice.python_name())
    );
}

#[test]
fn room_visits_push_on_coord_change_and_book_their_own_floor_and_event() {
    let mut rm = build_run(12);
    let mut policy = RecordingPolicy::new(FirstPolicy, Detail::Summary);

    let enter =
        |rm: &mut RunManager, act: u8, floor: i32, room_type: RoomType, coord: (i32, i32)| {
            rm.run_state.act_index = act;
            rm.run_state.total_floor = floor;
            rm.run_state.current_room = Some(Room {
                room_type,
                map_coord: Some(MapCoord {
                    col: coord.0,
                    row: coord.1,
                }),
                map_point_type: None,
            });
        };

    // Act 1, floor 1: a fight. Two observations at the same coord must
    // not duplicate the map point. (`with_neow_start` leaves the Neow
    // event bound; a resolved event clears via `exit_event_room`.)
    rm.run_state.current_event = None;
    enter(&mut rm, 1, 1, RoomType::Monster, (0, 0));
    observe(&mut policy, &rm);
    observe(&mut policy, &rm);

    // Act 1, floor 2: an event room resolving LostWisp.
    enter(&mut rm, 1, 2, RoomType::Event, (1, 1));
    rm.run_state.current_event = Some(EventId::LostWisp);
    observe(&mut policy, &rm);

    // Act 2, floor 18: a campfire, after `exit_event_room` cleared the
    // event.
    rm.run_state.current_event = None;
    enter(&mut rm, 2, 18, RoomType::RestSite, (2, 2));
    observe(&mut policy, &rm);

    let report = stub_report(&rm, false);
    let history = policy
        .into_run_file(&rm, &report, meta(12, Detail::Summary))
        .map_point_history;

    assert_eq!(history.len(), 2, "acts are 1-based, grouped into two lists");
    assert_eq!(history[0].len(), 2, "same-coord observations collapse");
    assert_eq!(history[0][0].map_point_type, "Monster");
    assert_eq!(history[0][0].floor, 1);
    assert_eq!(
        history[0][0].event_id, None,
        "the fight room predates the event — no retro-attachment"
    );
    assert_eq!(history[0][1].map_point_type, "Event");
    assert_eq!(
        history[0][1].floor, 2,
        "the map point books its entry floor"
    );
    assert_eq!(
        history[0][1].event_id,
        Some(format!("EVENT.{}", EventId::LostWisp.python_name()))
    );
    assert_eq!(history[1].len(), 1);
    assert_eq!(history[1][0].map_point_type, "RestSite");
    assert_eq!(history[1][0].floor, 18);
    assert_eq!(
        history[1][0].event_id, None,
        "the campfire is not the event's room"
    );
}
