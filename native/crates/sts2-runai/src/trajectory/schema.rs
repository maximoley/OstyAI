//! Serde output types for recorded run trajectories.
//!
//! The top level mirrors the game's native `.run` file (`RunHistory`,
//! `schema_version: 9`) as decompiled in
//! `sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Runs/RunHistory.cs`,
//! so a recorded file is parseable by the same tooling that reads the
//! scraped spire-codex / sts2replays corpus (`run_files.load_run`). All
//! of our extra, non-native data lives under the single [`RunFile::sts2sim`]
//! key, which native readers ignore.

use serde::Serialize;

/// Current native run-history schema version (v0.103.2 = v9).
pub const SCHEMA_VERSION: u32 = 9;

/// A recorded run file: native `RunHistory` fields at the top level plus
/// our [`Sts2Sim`] extension block.
#[derive(Clone, Debug, Serialize)]
pub struct RunFile {
    pub schema_version: u32,
    pub platform_type: String,
    pub game_mode: String,
    pub win: bool,
    /// Alphanumeric run seed (native uses the string form, e.g. `"UBZEKCH94L"`).
    pub seed: String,
    /// Epoch seconds. Deterministic runs have no wall clock, so this is
    /// `0` under record mode (repro metadata lives in [`Sts2Sim::meta`]).
    pub start_time: i64,
    pub run_time: f64,
    pub ascension: u8,
    pub build_id: String,
    pub was_abandoned: bool,
    /// `"<POOL>.<ID>"` ModelId string, or `"NONE.NONE"`.
    pub killed_by_encounter: String,
    pub killed_by_event: String,
    pub players: Vec<RunPlayer>,
    /// Act ModelIds in visit order (`"ACT.THE_SPIRE_HEART"` etc.).
    pub acts: Vec<String>,
    pub modifiers: Vec<String>,
    /// One entry per act; each act is a list of visited map points.
    pub map_point_history: Vec<Vec<MapPointEntry>>,
    /// Our non-native extension. Ignored by native `.run` readers.
    pub sts2sim: Sts2Sim,
}

/// Native `RunHistoryPlayer`.
#[derive(Clone, Debug, Serialize)]
pub struct RunPlayer {
    pub id: u32,
    /// `"CHARACTER.IRONCLAD"`.
    pub character: String,
    pub deck: Vec<DeckCardEntry>,
    pub relics: Vec<RelicEntry>,
    pub potions: Vec<PotionEntry>,
    pub badges: Vec<String>,
    pub max_potion_slot_count: i32,
}

/// Native `SerializableCard` — the card id and the floor it entered the
/// master deck. `id` is `"CARD.X"` (base) or carries the upgrade via
/// [`DeckCardEntry::upgrades`], matching the game's serialization.
#[derive(Clone, Debug, Serialize)]
pub struct DeckCardEntry {
    pub id: String,
    /// Best-effort: the floor at which this card was added to the deck,
    /// derived from the decision stream (0 = starter deck).
    pub floor_added_to_deck: i32,
    /// Upgrade count (0 or 1 in practice for Ironclad).
    pub upgrades: u32,
}

/// Native `SerializableRelic`.
#[derive(Clone, Debug, Serialize)]
pub struct RelicEntry {
    pub id: String,
    pub floor_added_to_deck: i32,
}

/// Native `SerializablePotion`.
#[derive(Clone, Debug, Serialize)]
pub struct PotionEntry {
    pub id: String,
}

/// Native `MapPointHistoryEntry` (flattened to the fields we can observe:
/// the room type, its encounter/event model id, and the monsters faced).
#[derive(Clone, Debug, Serialize)]
pub struct MapPointEntry {
    /// `MapPointType` debug string: `Combat`/`Elite`/`Boss`/`Shop`/`Event`/
    /// `RestSite`/`Treasure`/`Unknown`.
    pub map_point_type: String,
    /// Floor (act-local) this point sits on.
    pub floor: i32,
    /// Monster/encounter ids faced here (`"MONSTER.X"`), empty for
    /// non-combat rooms.
    pub monster_ids: Vec<String>,
    /// Event id resolved here (`"EVENT.X"`), when the room was an event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
}

// ── Our extension block ───────────────────────────────────────────────

/// Non-native trajectory extension: reproducibility metadata, the full
/// per-decision stream, and (at `full` detail) per-combat detail.
#[derive(Clone, Debug, Serialize)]
pub struct Sts2Sim {
    /// Recorder schema version for the extension block itself.
    pub recorder_version: u32,
    pub meta: Meta,
    /// One entry per run-level/combat decision, in order.
    pub decisions: Vec<Decision>,
}

/// Everything needed to reproduce the run byte-for-byte given the
/// deterministic engine.
#[derive(Clone, Debug, Serialize)]
pub struct Meta {
    pub seed_int: u64,
    pub character: String,
    pub ascension: u8,
    /// `ranker` / `ppo` / …
    pub policy: String,
    /// The full policy label (budget-tainted where relevant).
    pub policy_label: String,
    pub combat_budget: String,
    pub sims: u32,
    pub boss_sims: u32,
    /// PPO/ranker model file path, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_path: Option<String>,
    /// SHA-256 of the model file, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_sha256: Option<String>,
    /// Engine git sha (compile-time), if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_git_sha: Option<String>,
    /// Relevant `STS2_RUNAI_*` / `STS2_PPO_*` env at run time.
    pub env: std::collections::BTreeMap<String, String>,
    /// Detail level this file was recorded at (`summary`/`decisions`/`full`).
    pub detail: String,
    /// Final run outcome token (`won`/`lost`/`max_steps`/…).
    pub outcome: String,
    pub final_hp: i32,
    pub final_gold: i32,
    pub steps: usize,
}

/// One decision the policy made. `offered` is phase-typed so downstream
/// stats can see exactly what was on the table and what was taken.
#[derive(Clone, Debug, Serialize)]
pub struct Decision {
    pub step: u64,
    /// `Phase` debug string.
    pub phase: String,
    /// 1-based engine act index.
    pub act: u8,
    pub floor: i32,
    pub hp: i32,
    /// Max HP at decision time — `hp` alone can't say whether 40 HP
    /// walking into a boss was healthy or nearly dead.
    #[serde(default)]
    pub max_hp: i32,
    pub gold: i32,
    pub chosen_idx: usize,
    pub n_options: usize,
    /// Potions held at decision time (`"POTION.X"`), so "held a Fire
    /// Potion into the act boss and never drank it" is answerable from
    /// the stream alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub potions: Vec<String>,
    /// Phase-specific offered/taken payload.
    pub offered: Offered,
    /// Combat MCTS root distribution behind this pick (combat plies the
    /// search actually decided; absent on meta-policy fallbacks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchTelemetry>,
    /// What the meta policy scored for this decision (non-combat).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta_eval: Option<MetaEvalTelemetry>,
}

/// The combat MCTS root distribution for one ply: every root action the
/// search expanded, with its visit count and mean value.
///
/// This is what separates "the agent judged this best" from "the search
/// never distinguished the options": `top_gap` near 0 over a wide
/// candidate list means the pick was effectively arbitrary, and a low
/// `visits` on the chosen action means it was barely explored.
#[derive(Clone, Debug, Serialize)]
pub struct SearchTelemetry {
    /// Root children, sorted by mean value then visits (the pick order
    /// `mcts_pick` itself uses).
    pub root: Vec<RootAction>,
    /// Total root visits across all children.
    pub visits: u32,
    /// The action the search returned (`Action` debug string).
    pub chosen: String,
    /// `mean_value` gap between the best and second-best root action.
    /// `None` when the search expanded a single action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_gap: Option<f32>,
}

/// One root child of a combat search.
#[derive(Clone, Debug, Serialize)]
pub struct RootAction {
    /// `Action` debug string.
    pub action: String,
    pub visits: u32,
    pub mean_value: f32,
    pub wins: u32,
    pub losses: u32,
}

/// The meta policy's scoring of one non-combat decision.
#[derive(Clone, Debug, Serialize)]
pub struct MetaEvalTelemetry {
    /// `ppo` | `ppo_auto_take` | `heuristic_fallback`. (`heuristic_potion_swap`
    /// retired in GH #379 — the net owns that surface now.)
    pub source: String,
    /// Every candidate the scorer saw, with its probability — `[]` when
    /// nothing was scored (auto-take / heuristic delegate).
    pub options: Vec<ScoredOption>,
    /// The net's value estimate for the state at this decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f32>,
    /// Probability mass on the action taken.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chosen_prob: Option<f32>,
    /// Shannon entropy (nats) of the scored distribution — the
    /// "deliberate pick vs coin flip" signal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entropy: Option<f32>,
}

/// One scored candidate action.
#[derive(Clone, Debug, Serialize)]
pub struct ScoredOption {
    /// `RunAction` debug string.
    pub action: String,
    pub prob: f32,
}

/// Phase-typed offered-choice payload. Serialized as an internally-tagged
/// enum (`{"kind": "card_reward", ...}`).
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Offered {
    /// CardReward: the offered cards (`"CARD.X"`/`"CARD.X+"`) + relic on
    /// offer; `picked` is the chosen card id, `None` = skip/sacrifice.
    CardReward {
        options: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        relic_offer: Option<String>,
        picked: Option<String>,
    },
    /// Event: the event id, which choice index was taken, and the full
    /// presented option list with previewed costs.
    Event {
        #[serde(skip_serializing_if = "Option::is_none")]
        event_id: Option<String>,
        choice_idx: usize,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        options: Vec<EventOption>,
    },
    /// Shop: inventory (ids+prices) and what was bought/removed.
    Shop {
        cards: Vec<ShopItem>,
        relics: Vec<ShopItem>,
        potions: Vec<ShopItem>,
        action: String,
    },
    /// RestSite: the option index taken (rest/smith/…) + smith target.
    Rest {
        option_idx: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        smith_card: Option<String>,
    },
    /// BossRelic: the 3 offered relics + the index taken.
    BossRelic {
        options: Vec<String>,
        picked_idx: usize,
    },
    /// Map: the destination column, the room type moved into, and every
    /// `(column, room type)` that was reachable instead.
    Map {
        col: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        room: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        reachable: Vec<(i32, String)>,
    },
    /// Combat: the play made this ply + a snapshot of the board.
    Combat(CombatPly),
    /// Any other run action (deck ops, treasure, relic pick): the action's
    /// debug string.
    Other { action: String },
}

/// One presented event option: what the player was shown, and what
/// taking it would have cost (previewed on a state clone, so the numbers
/// are the engine's own, not a scrape of the description text).
#[derive(Clone, Debug, Serialize)]
pub struct EventOption {
    pub option_id: String,
    pub label: String,
    pub description: String,
    pub enabled: bool,
    /// Signed current-HP change taking this option would apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hp_delta: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gold_delta: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_hp_delta: Option<i32>,
}

/// A shop line item.
#[derive(Clone, Debug, Serialize)]
pub struct ShopItem {
    pub id: String,
    pub price: i32,
}

/// One combat decision + board snapshot (recorded only at `full` detail).
/// The board is the proven parity bridge shape
/// ([`sts2_core::parity::combat_state_to_bridge_state`]): player
/// hp/block/energy/powers, hand (card ids), enemies (hp/block/intent),
/// pile counts, round.
#[derive(Clone, Debug, Serialize)]
pub struct CombatPly {
    /// `play`/`end_turn`/`potion`/`resolve` etc.
    pub action: String,
    /// Card played this ply, resolved from the hand index (`"CARD.X"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
    /// Potion drunk this ply, resolved from the belt slot (`"POTION.X"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub potion: Option<String>,
    pub round: u16,
    /// Full board snapshot at decision time.
    pub board: serde_json::Value,
}
