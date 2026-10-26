//! The advisors (M2): ports of `workbench/reasoning.py`,
//! `choice_ranker.py`, `regret_review.py`, `card_advisor.py` and
//! `decision_advisor.py`, over the live [`Session`].
//!
//! Two families:
//!
//! * **Search-backed** — the combat hint, the search-tree inspector, the
//!   in-combat choice ranker, "play best" and the regret coach / review.
//!   All are one `mcts_recommend` / `mcts_search_tree` over a
//!   [`CombatState`], through the emitters in `sts2_wire::emit::mcts` that
//!   the `PyO3` module also uses. Deterministic for a config, so the hint /
//!   tree / ranker rows are golden-parity tested against the Python
//!   workbench. The regret routes score the parent node's **native**
//!   snapshot ([`Node::combat`]) — the Python session no longer holds that
//!   state and re-scores a lossy wire dict — so those are checked
//!   structurally, not by hash.
//! * **Rollout-backed** — the card advisor and the decision advisor: fork the
//!   run per option, take that option, play the rest of the act out
//!   `rollouts` times (a cheap MCTS in combat, the tier-1 [`HeuristicPolicy`]
//!   off combat), paired across options by seed. They report progress
//!   through a [`Progress`] sink — one `start` frame, one `tick` per playout —
//!   which the streaming routes forward as NDJSON. Python drives its rollouts
//!   with `random.Random` and its own `run_policy` heuristic, so these are
//!   structural twins, not bit-for-bit.
//!
//! Everything is single-threaded: `threads` in a config is honoured as 1
//! (wasm has no thread pool; the `PyO3` path keeps its rayon option).

use rand::{Rng, SeedableRng};
use serde_json::{json, Map, Value};
use sts2_core::full_run::RunPolicy;
use sts2_core::{CombatState, RunAction, RunManager, RunRngSet};
use sts2_mcts::{Mcts, MctsConfig};
use sts2_runai::policy::HeuristicPolicy;
use sts2_wire::emit::mcts::{recommend_value, search_tree_value, RecommendParams};
use sts2_wire::emit::run_action::combat_action_to_run_action_value;

use crate::display_maps::loc_id;
use crate::localization::resolve_state_names;
use crate::options::normalize_options;
use crate::rmx;
use crate::session::{Node, Session, SessionError};

/// Sentinel `stable_idx` values (`sts2_hint.recommender`): end turn, and
/// potion slot `s` → `-100 - s` (range `(-200, -100]`).
pub const END_TURN: i64 = -1;
pub const POTION_BASE: i64 = -100;
pub const POTION_MIN: i64 = -200;

/// Default blunder threshold (percentage points of win-probability lost
/// versus the MCTS line) — above MCTS sampling noise at the usual budget.
pub const DEFAULT_BLUNDER_DROP_PCT: f64 = 8.0;

/// A progress sink: receives each wire frame (`{"type": "start", …}`,
/// `{"type": "tick", …}`) as the rollout advisors go.
pub type Progress<'a> = &'a mut dyn FnMut(&Value);

fn config_obj(config: &Value) -> Value {
    // `dict(config or {})`: the echoed config is always an object.
    match config {
        Value::Object(_) => config.clone(),
        _ => json!({}),
    }
}

/// `RecommendConfig.from_dict` + the browser's single-thread clamp.
fn params_for(config: &Value, fn_name: &str) -> Result<RecommendParams, String> {
    let mut p = RecommendParams::from_config(config, fn_name)?;
    p.threads = p.threads.min(1);
    Ok(p)
}

// ── reasoning.py ────────────────────────────────────────────────────────

fn potion_name(state: &Value, slot: usize) -> Option<String> {
    let potions = state
        .get("player")
        .and_then(|p| p.get("potions"))
        .and_then(Value::as_array)
        .or_else(|| state.get("potions").and_then(Value::as_array))?;
    let p = potions.get(slot)?;
    let name = if p.is_object() {
        p.get("name")
    } else {
        Some(p)
    };
    match name {
        Some(Value::Object(m)) => Some(
            m.get("en")
                .or_else(|| m.get("zh"))
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_string(),
        ),
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => Some(format!("slot {slot}")),
    }
}

/// `_action_label`: the human label for a stable action index.
#[must_use]
pub fn action_label(stable_idx: i64, state: &Value) -> String {
    if stable_idx == END_TURN {
        return "end turn".to_string();
    }
    if POTION_MIN < stable_idx && stable_idx <= POTION_BASE {
        #[allow(clippy::cast_sign_loss)]
        let slot = (POTION_BASE - stable_idx) as usize;
        return match potion_name(state, slot) {
            Some(name) => format!("drink {name}"),
            None => format!("drink potion slot {slot}"),
        };
    }
    if let Some(hand) = state.get("hand").and_then(Value::as_array) {
        for card in hand {
            if card.get("index").and_then(Value::as_i64) == Some(stable_idx) {
                return card
                    .get("name")
                    .and_then(Value::as_str)
                    .map_or_else(|| format!("card {stable_idx}"), str::to_string);
            }
        }
    }
    format!("action {stable_idx}")
}

/// `action_stable_key`: a native action dict → `(stable_idx, target_index)`.
/// A `play` card's `card_index` is its stable index, `end_turn` is `-1`, a
/// potion slot `s` is `-100 - s`. `(None, target)` for anything else.
#[must_use]
pub fn action_stable_key(args: &Value) -> (Option<i64>, Option<i64>) {
    let Some(a) = args.as_object() else {
        return (None, None);
    };
    if a.is_empty() {
        return (None, None);
    }
    let target = a.get("target_index").and_then(Value::as_i64);
    match a.get("action").and_then(Value::as_str) {
        Some("play") => (a.get("card_index").and_then(Value::as_i64), target),
        Some("end_turn") => (Some(END_TURN), None),
        Some("potion" | "drink_potion") => (
            a.get("slot")
                .and_then(Value::as_i64)
                .map(|s| POTION_BASE - s),
            target,
        ),
        _ => (None, target),
    }
}

fn f64_of(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

fn win_prob(row: &Value) -> f64 {
    f64_of(&row["win_prob"]).clamp(0.0, 1.0)
}

/// `rows_from_mcts`: score rows from a raw `mcts_recommend` result, labelled
/// against `state` (the position the search ROOTED at, names resolved).
/// Channel / evoke / choose actions are not surfaced at this boundary.
#[must_use]
pub fn rows_from_mcts(result: &Value, state: &Value) -> Vec<Value> {
    let mut rows: Vec<Value> = Vec::new();
    for entry in result["actions"].as_array().into_iter().flatten() {
        let a = &entry["action"];
        let stable_idx = match a["action"].as_str() {
            Some("play") => a["card_index"].as_i64().unwrap_or(0),
            Some("end_turn") => END_TURN,
            Some("potion" | "drink_potion") => POTION_BASE - a["slot"].as_i64().unwrap_or(0),
            _ => continue,
        };
        rows.push(json!({
            "stable_idx": stable_idx,
            "action": action_label(stable_idx, state),
            "win_prob": f64_of(&entry["p_win"]),
            "q": entry.get("q").or_else(|| entry.get("value")).map_or(0.0, f64_of),
            "visits": entry["visits"].as_i64().unwrap_or(0),
            "is_best": entry["is_best"].as_bool().unwrap_or(false),
            "q_saturated": entry["q_saturated"].as_bool().unwrap_or(false),
            "low_confidence": entry["low_confidence"].as_bool().unwrap_or(false),
            "target_index": a.get("target_index").and_then(Value::as_i64),
            "wins": entry["wins"].as_i64().unwrap_or(0),
            "losses": entry["losses"].as_i64().unwrap_or(0),
            "ties": entry["ties"].as_i64().unwrap_or(0),
        }));
    }
    rows.sort_by(|a, b| {
        let ka = (
            !a["is_best"].as_bool().unwrap_or(false),
            -f64_of(&a["win_prob"]),
        );
        let kb = (
            !b["is_best"].as_bool().unwrap_or(false),
            -f64_of(&b["win_prob"]),
        );
        ka.0.cmp(&kb.0)
            .then(ka.1.partial_cmp(&kb.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    rows
}

/// `score_native`: per-action MCTS scores for a native combat state —
/// `{scores, pv: null, config}`, or `{error, scores: [], pv: null, config}`.
#[must_use]
pub fn score(cs: &CombatState, named_state: &Value, config: &Value) -> Value {
    let echo = config_obj(config);
    match params_for(config, "mcts_recommend") {
        Ok(p) => json!({
            "scores": rows_from_mcts(&recommend_value(cs, &p, true), named_state),
            "pv": Value::Null,
            "config": echo,
        }),
        Err(e) => json!({"error": e, "scores": [], "pv": Value::Null, "config": echo}),
    }
}

/// `_choice_key`: `choice:skip` / `choice:{opt}` for a `choose` action dict.
fn choice_key(action: &Value) -> String {
    match action.get("selection").and_then(Value::as_array) {
        Some(sel) if sel.is_empty() => "choice:skip".to_string(),
        Some(sel) => format!("choice:{}", sel[0].as_i64().unwrap_or(0)),
        None => format!("choice:{}", action["index"].as_i64().unwrap_or(0)),
    }
}

/// `rank_choice_native`: `{by_key: {choice_key: {win_prob, q, visits,
/// is_best}}, config}` from one search over an open pending choice.
#[must_use]
pub fn rank_choice_native(cs: &CombatState, config: &Value) -> Value {
    let echo = config_obj(config);
    let p = match params_for(config, "mcts_recommend") {
        Ok(p) => p,
        Err(e) => return json!({"error": e, "by_key": {}, "config": echo}),
    };
    let result = recommend_value(cs, &p, true);
    let mut by_key = Map::new();
    for entry in result["actions"].as_array().into_iter().flatten() {
        let a = &entry["action"];
        if a["action"].as_str() != Some("choose") {
            continue;
        }
        by_key.insert(
            choice_key(a),
            json!({
                "win_prob": f64_of(&entry["p_win"]),
                "q": entry.get("q").or_else(|| entry.get("value")).map_or(0.0, f64_of),
                "visits": entry["visits"].as_i64().unwrap_or(0),
                "is_best": entry["is_best"].as_bool().unwrap_or(false),
            }),
        );
    }
    json!({"by_key": by_key, "config": echo})
}

/// `search_tree_native`: the full tree plus the echoed config.
#[must_use]
pub fn search_tree(cs: &CombatState, config: &Value) -> Value {
    let echo = config_obj(config);
    match params_for(config, "mcts_search_tree") {
        Ok(p) => {
            let mut tree = search_tree_value(cs, &p);
            tree["config"] = echo;
            tree
        }
        Err(e) => json!({"error": e, "nodes": [], "root": Value::Null, "pv": [], "config": echo}),
    }
}

// ── decision_advisor.py candidates ──────────────────────────────────────

/// One evaluable option of the current decision: the join key the SPA
/// renders its option under, a log label, and the engine action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub key: String,
    pub label: String,
    pub action: RunAction,
}

fn pending_choice(rm: &RunManager) -> Option<Value> {
    rm.combat.as_ref().and_then(rmx::pending_choice)
}

/// A run-level *choose a card* screen (Lead Paperweight / Hefty Tablet):
/// offered through the `card_reward` phase but rendered as a `card_select`.
fn choose_screen(rm: &RunManager) -> bool {
    rmx::phase_str(rm) == "card_reward" && rmx::card_offer_choose_screen(rm)
}

fn choose_screen_candidates(rm: &RunManager) -> Vec<Candidate> {
    let ids = rmx::card_reward_option_ids(rm);
    let mut out = Vec::new();
    for action in rmx::available_actions(rm) {
        let RunAction::PickCardReward { idx } = action else {
            continue;
        };
        match idx {
            None => out.push(Candidate {
                key: "choice:skip".into(),
                label: "skip".into(),
                action,
            }),
            Some(i) => out.push(Candidate {
                key: format!("choice:{i}"),
                label: ids
                    .get(i)
                    .map_or_else(|| format!("card {i}"), |id| loc_id(id)),
                action,
            }),
        }
    }
    out
}

fn candidate_key_label(action: RunAction) -> Option<(String, String)> {
    Some(match action {
        RunAction::CombatResolveChoice { option: None } => ("choice:skip".into(), "skip".into()),
        RunAction::CombatResolveChoice { option: Some(o) } => {
            (format!("choice:{o}"), format!("option {o}"))
        }
        RunAction::ShopBuyCard { idx } => (format!("card:{idx}"), format!("buy card {idx}")),
        RunAction::ShopBuyRelic { idx } => (format!("relic:{idx}"), format!("buy relic {idx}")),
        RunAction::ShopBuyPotion { idx } => (format!("potion:{idx}"), format!("buy potion {idx}")),
        RunAction::LeaveShop => ("leave".into(), "leave".into()),
        RunAction::EventChoice { idx } => (format!("option:{idx}"), format!("option {idx}")),
        RunAction::PickBossRelic { idx } => (format!("boss_relic:{idx}"), format!("relic {idx}")),
        _ => return None,
    })
}

/// `_candidates`: `(key, label, action)` per evaluable option of the current
/// decision, straight off the engine's legal moves.
#[must_use]
pub fn candidates(rm: &RunManager) -> Vec<Candidate> {
    if choose_screen(rm) {
        return choose_screen_candidates(rm);
    }
    let pc = pending_choice(rm);
    let choice_cards: Vec<Value> = pc
        .as_ref()
        .and_then(|p| p.get("cards"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for action in rmx::available_actions(rm) {
        let Some((key, mut label)) = candidate_key_label(action) else {
            continue;
        };
        if let RunAction::CombatResolveChoice { option: Some(o) } = action {
            if let Some(cid) = choice_cards.get(o).and_then(|c| c["id"].as_str()) {
                if !cid.is_empty() {
                    label = loc_id(cid);
                }
            }
        }
        out.push(Candidate { key, label, action });
    }
    out
}

// ── choice_ranker.py ────────────────────────────────────────────────────

/// `is_rankable`: an in-combat pending choice with ≥2 legal options.
#[must_use]
pub fn is_rankable(rm: &RunManager) -> bool {
    rmx::phase_str(rm) == "combat" && pending_choice(rm).is_some() && candidates(rm).len() >= 2
}

/// `choice_ranker.rank`: `{candidates: [{key, label, win_prob, q, visits,
/// is_best}], decision: "card_select", act, config}` or `{error,
/// candidates: [], config}`.
#[must_use]
pub fn rank(rm: &RunManager, config: &Value) -> Value {
    let echo = config_obj(config);
    if !is_rankable(rm) {
        return json!({
            "error": "the choice ranker only runs on an in-combat card-select choice with at least two legal options",
            "candidates": [],
            "config": echo,
        });
    }
    let Some(cs) = rm.combat.as_ref() else {
        return json!({
            "error": "no native combat state (the choice ranker requires the Rust engine backend)",
            "candidates": [],
            "config": echo,
        });
    };
    let ranking = rank_choice_native(cs, config);
    if let Some(e) = ranking.get("error") {
        return json!({"error": e, "candidates": [], "config": echo});
    }
    let by_key = &ranking["by_key"];
    let mut rows: Vec<Value> = candidates(rm)
        .into_iter()
        .map(|c| {
            let row = by_key.get(&c.key);
            json!({
                "key": c.key,
                "label": c.label,
                "win_prob": row.map_or(Value::Null, |r| r["win_prob"].clone()),
                "q": row.map_or(Value::Null, |r| r["q"].clone()),
                "visits": row.map_or(0, |r| r["visits"].as_i64().unwrap_or(0)),
                "is_best": row.is_some_and(|r| r["is_best"].as_bool().unwrap_or(false)),
            })
        })
        .collect();
    // If the search marked no row best (e.g. a single visited option), fall
    // back to the highest win_prob among scored rows so the UI always has one.
    if !rows.is_empty() && !rows.iter().any(|r| r["is_best"].as_bool() == Some(true)) {
        let best = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| !r["win_prob"].is_null())
            .max_by(|(_, a), (_, b)| {
                (f64_of(&a["win_prob"]), a["visits"].as_i64().unwrap_or(0))
                    .partial_cmp(&(f64_of(&b["win_prob"]), b["visits"].as_i64().unwrap_or(0)))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i);
        if let Some(i) = best {
            rows[i]["is_best"] = json!(true);
        }
    }
    json!({
        "candidates": rows,
        "decision": "card_select",
        "act": rm.run_state.act_index,
        "config": ranking["config"].clone(),
    })
}

/// `choice_ranker.best_action`: the engine action behind the option
/// [`rank`] marks best (`None` when not rankable / the search failed).
#[must_use]
pub fn best_action(rm: &RunManager, config: &Value) -> Option<RunAction> {
    let out = rank(rm, config);
    if out.get("error").is_some() {
        return None;
    }
    let best_key = out["candidates"]
        .as_array()?
        .iter()
        .find(|c| c["is_best"].as_bool() == Some(true))?["key"]
        .as_str()?
        .to_string();
    candidates(rm)
        .into_iter()
        .find(|c| c.key == best_key)
        .map(|c| c.action)
}

// ── regret_review.py ────────────────────────────────────────────────────

fn match_row(scores: &[Value], stable_idx: Option<i64>, target: Option<i64>) -> Option<&Value> {
    let stable_idx = stable_idx?;
    let same_idx = |r: &&Value| r["stable_idx"].as_i64() == Some(stable_idx);
    scores
        .iter()
        .filter(same_idx)
        .find(|r| r["target_index"].as_i64() == target)
        .or_else(|| scores.iter().find(same_idx))
}

fn best_row(scores: &[Value]) -> &Value {
    scores
        .iter()
        .find(|r| r["is_best"].as_bool() == Some(true))
        .unwrap_or_else(|| {
            scores
                .iter()
                .max_by(|a, b| {
                    win_prob(a)
                        .partial_cmp(&win_prob(b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .expect("non-empty")
        })
}

fn enemy_name(enemies: &[Value], target: Option<i64>) -> Option<String> {
    let target = target?;
    enemies
        .iter()
        .find(|e| e["index"].as_i64() == Some(target))
        .and_then(|e| e["name"].as_str())
        .filter(|n| !n.is_empty())
        .map(str::to_string)
}

/// `_targeted_label`: the row's label, with `→ <enemy>` when the fight has
/// more than one enemy and the target resolves.
fn targeted_label(row: &Value, enemies: &[Value]) -> String {
    let base = row["action"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("?");
    if enemies.len() <= 1 {
        return base.to_string();
    }
    match enemy_name(enemies, row["target_index"].as_i64()) {
        Some(name) => format!("{base} → {name}"),
        None => base.to_string(),
    }
}

/// Python `round(p * 100)`: ties to even.
#[allow(clippy::cast_possible_truncation)]
fn pct(p: f64) -> i64 {
    (p * 100.0).round_ties_even() as i64
}

/// `_native_action`: a branch node's `(verb, args)` → the native action dict
/// `assess_decision` keys on. Both index spellings are accepted per verb.
/// The workbench's own `use_potion` verb carries `potion_index`, which the
/// Python normaliser did not read (so it never matched a potion row); it
/// is read here.
#[must_use]
pub fn native_action(verb: Option<&str>, args: &Value) -> Value {
    native_action_with(verb, args, true)
}

/// `_native_action` exactly as Python normalises it — WITHOUT reading a
/// `use_potion`'s `potion_index`, so a hand-played potion records
/// `slot: null`. The run log is written with this so the recorded line hashes
/// like the oracle's; the live coach and verdict keep [`native_action`].
#[must_use]
pub fn native_action_recorded(verb: Option<&str>, args: &Value) -> Value {
    native_action_with(verb, args, false)
}

fn native_action_with(verb: Option<&str>, args: &Value, read_potion_index: bool) -> Value {
    let a = args.as_object().cloned().unwrap_or_default();
    let first = |keys: &[&str]| -> Value {
        keys.iter()
            .find_map(|k| a.get(*k).filter(|v| !v.is_null()).cloned())
            .unwrap_or(Value::Null)
    };
    let kind = if verb == Some(crate::render::NATIVE_ACTION) {
        a.get("action").and_then(Value::as_str).map(str::to_string)
    } else {
        verb.map(str::to_string)
    };
    match kind.as_deref() {
        Some("play_card" | "play") => json!({
            "action": "play",
            "card_index": first(&["card_index", "hand_idx"]),
            "target_index": a.get("target_index").cloned().unwrap_or(Value::Null),
        }),
        Some("end_turn") => json!({"action": "end_turn"}),
        Some("drink_potion" | "potion" | "use_potion") => json!({
            "action": "drink_potion",
            "slot": if read_potion_index {
                first(&["slot", "potion_slot", "potion_index"])
            } else {
                first(&["slot", "potion_slot"])
            },
            "target_index": a.get("target_index").cloned().unwrap_or(Value::Null),
        }),
        _ => Value::Object(a),
    }
}

/// What `assess_decision` needs of one combat decision.
#[derive(Clone, Debug)]
pub struct DecisionStep {
    pub idx: i64,
    pub node_id: Option<i64>,
    /// The (name-resolved) combat state the move was chosen from.
    pub state: Value,
    /// Native action dict of the move taken (see [`native_action`]).
    pub chosen: Value,
    pub turn: Value,
    pub floor: Value,
    pub act: Value,
}

/// `assess_decision`: one combat step diffed against the MCTS line, or
/// `None` when it can't be assessed (fewer than two rows, no resolvable
/// recorded move).
#[must_use]
pub fn assess_decision(step: &DecisionStep, scores: &[Value], blunder_pct: f64) -> Option<Value> {
    if scores.len() < 2 {
        return None;
    }
    let (played_idx, played_target) = action_stable_key(&step.chosen);
    let played = match_row(scores, played_idx, played_target)?;
    let best = best_row(scores);
    let best_wp = win_prob(best);
    let played_wp = win_prob(played);
    let regret = (best_wp - played_wp).max(0.0);
    let matched_best = played["stable_idx"] == best["stable_idx"]
        && played["target_index"] == best["target_index"];
    let rank = scores.iter().filter(|r| win_prob(r) > played_wp).count();
    let drop_pct = pct(regret);
    let is_blunder = !matched_best && regret * 100.0 >= blunder_pct;
    let enemies: Vec<Value> = step.state["enemies"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let played_label = targeted_label(played, &enemies);
    let best_label = targeted_label(best, &enemies);
    let turn = &step.turn;
    let turn_s = match turn {
        Value::Null => "None".to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let message = if matched_best {
        format!(
            "Turn {turn_s}: {played_label} matched the MCTS line (P(win) {}%)",
            pct(played_wp)
        )
    } else {
        format!(
            "Turn {turn_s}: {played_label} instead of {best_label} dropped P(win) {}%→{}% (−{drop_pct} pts)",
            pct(best_wp),
            pct(played_wp)
        )
    };
    Some(json!({
        "idx": step.idx,
        "node_id": step.node_id,
        "turn": turn,
        "floor": step.floor,
        "act": step.act,
        "played_label": played_label,
        "played_win_prob": played_wp,
        "best_label": best_label,
        "best_win_prob": best_wp,
        "regret": regret,
        "drop_pct": drop_pct,
        "player_rank": rank,
        "n_actions": scores.len(),
        "visits": best["visits"].as_i64().unwrap_or(0),
        "matched_best": matched_best,
        "is_blunder": is_blunder,
        "message": message,
    }))
}

/// `summarize`: `{decisions, blunders (top_n costliest), summary}`.
#[must_use]
pub fn summarize(decisions: Vec<Value>, top_n: usize) -> Value {
    let mut flagged: Vec<&Value> = decisions
        .iter()
        .filter(|d| d["is_blunder"].as_bool() == Some(true))
        .collect();
    flagged.sort_by(|a, b| {
        let key = |d: &Value| {
            (
                -f64_of(&d["regret"]),
                d["turn"].as_i64().unwrap_or(1 << 30),
                d["idx"].as_i64().unwrap_or(0),
            )
        };
        let (ka, kb) = (key(a), key(b));
        ka.0.partial_cmp(&kb.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(ka.1.cmp(&kb.1))
            .then(ka.2.cmp(&kb.2))
    });
    let cumulative: f64 = decisions.iter().map(|d| f64_of(&d["regret"])).sum();
    let worst = flagged
        .first()
        .map_or(0, |d| d["drop_pct"].as_i64().unwrap_or(0));
    let blunders: Vec<Value> = flagged.iter().take(top_n).map(|d| (*d).clone()).collect();
    json!({
        "summary": {
            "total_decisions": decisions.len(),
            "blunder_count": flagged.len(),
            "cumulative_drop_pct": pct(cumulative),
            "worst_drop_pct": worst,
        },
        "decisions": decisions,
        "blunders": blunders,
    })
}

// ── BranchingLiveSession advisor methods ────────────────────────────────

fn combat_play(node: &Node) -> bool {
    node.decision == "combat_play"
}

/// `hint`: MCTS scores for the active combat state, `None` off combat.
pub fn hint(sess: &Session, config: &Value) -> Result<Option<Value>, SessionError> {
    let node = sess.active_node()?;
    if !combat_play(node) {
        return Ok(None);
    }
    let named = resolve_state_names(Some(&node.state), sess.loc());
    let cs = sess
        .game()
        .and_then(|g| g.combat_state())
        .ok_or_else(|| SessionError::new(400, "no native combat state"))?;
    Ok(Some(score(cs, &named, config)))
}

/// `search_tree`: the full tree for the active combat state, `None` off combat.
pub fn live_search_tree(sess: &Session, config: &Value) -> Result<Option<Value>, SessionError> {
    let node = sess.active_node()?;
    if !combat_play(node) {
        return Ok(None);
    }
    let cs = sess
        .game()
        .and_then(|g| g.combat_state())
        .ok_or_else(|| SessionError::new(400, "no native combat state"))?;
    Ok(Some(search_tree(cs, config)))
}

/// `rank_choice`: the ranker over the active in-combat choice, `None` when
/// the active node is not a `card_select`.
pub fn rank_choice(sess: &Session, config: &Value) -> Result<Option<Value>, SessionError> {
    let node = sess.active_node()?;
    if node.decision != "card_select" {
        return Ok(None);
    }
    let rm = sess
        .game()
        .and_then(|g| g.native_run())
        .ok_or_else(|| SessionError::new(400, "no live run"))?;
    Ok(Some(rank(rm, config)))
}

/// `recommended_action`: `{action, args, policy?}` for the active decision —
/// an open in-combat choice is answered by the combat search at the ranker's
/// default config (so "play best" plays the option the panel stars); every
/// other run-level decision by the tier-1 [`HeuristicPolicy`]. `None` in plain
/// combat (the hint owns card plays), at game over, or when the chosen action
/// has no workbench verb.
pub fn recommended_action(sess: &Session) -> Result<Option<Value>, SessionError> {
    let node = sess.active_node()?;
    let game = sess
        .game()
        .ok_or_else(|| SessionError::new(400, "no live run"))?;
    let rm = game
        .native_run()
        .ok_or_else(|| SessionError::new(400, "no live run"))?;
    let options = normalize_options(&node.state);
    if let Some(action) = best_action(rm, &json!({})) {
        return Ok(game
            .verb_for(action, &options)
            .map(|(verb, args)| json!({"action": verb, "args": args, "policy": "combat_mcts"})));
    }
    if rmx::phase_str(rm) == "combat" || rm.is_over() {
        return Ok(None);
    }
    let actions = rmx::available_actions(rm);
    if actions.is_empty() {
        return Ok(None);
    }
    let Some(i) = HeuristicPolicy::new().choose(rm, &actions) else {
        return Ok(None);
    };
    Ok(game
        .verb_for(actions[i], &options)
        .map(|(verb, args)| json!({"action": verb, "args": args})))
}

fn pop_f64(cfg: &mut Value, key: &str, default: f64) -> f64 {
    cfg.as_object_mut()
        .and_then(|m| m.remove(key))
        .and_then(|v| v.as_f64())
        .unwrap_or(default)
}

fn pop_usize(cfg: &mut Value, key: &str, default: usize) -> usize {
    cfg.as_object_mut()
        .and_then(|m| m.remove(key))
        .and_then(|v| v.as_u64())
        .map_or(default, |v| usize::try_from(v).unwrap_or(usize::MAX))
}

fn step_of(
    parent: &Node,
    node: &Node,
    loc: &'static crate::localization::Loc,
    floor: &Value,
    act: &Value,
) -> DecisionStep {
    let named = resolve_state_names(Some(&parent.state), loc);
    let turn = named.get("round").cloned().unwrap_or(Value::Null);
    DecisionStep {
        idx: i64::try_from(parent.idx).unwrap_or(i64::MAX),
        node_id: i64::try_from(parent.id).ok(),
        state: named,
        chosen: native_action(node.action.as_deref(), &node.args),
        turn,
        floor: floor.clone(),
        act: act.clone(),
    }
}

/// `last_move_regret`: regret of the move that produced the active node —
/// `None` when it has no parent or the parent was not a combat decision;
/// `{error, regret: null}` when the parent cannot be scored; else a
/// `DecisionRegret` dict (or `None` if the move is unresolvable).
pub fn last_move_regret(sess: &Session, config: &Value) -> Result<Option<Value>, SessionError> {
    let node = sess.active_node()?;
    let Some(parent) = node.parent_id.and_then(|p| sess.node(p)) else {
        return Ok(None);
    };
    if !combat_play(parent) {
        return Ok(None);
    }
    let mut cfg = config_obj(config);
    let blunder_pct = pop_f64(&mut cfg, "blunder_pct", DEFAULT_BLUNDER_DROP_PCT);
    let step = step_of(parent, node, sess.loc(), &Value::Null, &Value::Null);
    let scores: Vec<Value> = parent
        .combat
        .as_ref()
        .map(|cs| score(cs, &step.state, &cfg))
        .and_then(|r| r["scores"].as_array().cloned())
        .unwrap_or_default();
    if scores.is_empty() {
        return Ok(Some(
            json!({"error": "no MCTS scores available", "regret": Value::Null}),
        ));
    }
    let mut step = step;
    step.node_id = None; // the live coach's step carries no node id
    Ok(assess_decision(&step, &scores, blunder_pct))
}

/// The root distribution of the search that actually chose `node`.
///
/// `Session::run_mode_step` stashes the native agent's `combat_search` on the
/// node the move PRODUCED (`node.state["mcts"]`), so the search for the
/// decision at `parent` lives one node downstream. Rows are labelled and
/// targeted against `parent` — the position the search rooted at — since
/// [`rows_from_mcts`] resolves both out of the state's hand/encounter by index,
/// and by `node` the card is gone and the enemy slots have shifted.
///
/// Empty when the move was not an agent-searched combat play: a hand-played
/// move, an off-combat pick, or a run recorded before this was persisted.
#[must_use]
pub fn agent_scores(
    parent: &Node,
    node: &Node,
    loc: &'static crate::localization::Loc,
) -> Vec<Value> {
    if !combat_play(parent) {
        return Vec::new();
    }
    let Some(search) = node.state.get("mcts").filter(|v| v.is_object()) else {
        return Vec::new();
    };
    if !search
        .get("actions")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty())
    {
        return Vec::new();
    }
    let named = resolve_state_names(Some(&parent.state), loc);
    rows_from_mcts(search, &named)
}

/// `review_run`: the full-run regret review over the active line —
/// `{decisions, blunders, summary, config: {blunder_pct, top_n}}`. Every
/// combat decision on the path is re-scored from its node's snapshot;
/// `floor` / `act` are carried forward from the last screen that reported
/// them.
pub fn review_run(sess: &Session, config: &Value) -> Result<Value, SessionError> {
    let path = sess.active_path()?;
    let mut cfg = config_obj(config);
    let blunder_pct = pop_f64(&mut cfg, "blunder_pct", DEFAULT_BLUNDER_DROP_PCT);
    let top_n = pop_usize(&mut cfg, "top_n", 2);
    let mut decisions = Vec::new();
    let mut floor = Value::Null;
    let mut act = Value::Null;
    for pair in path.windows(2) {
        let (parent, node) = (pair[0], pair[1]);
        if let Some(f) = parent.state.get("floor").filter(|v| !v.is_null()) {
            floor = f.clone();
        }
        if let Some(a) = parent.state.get("act").filter(|v| !v.is_null()) {
            act = a.clone();
        }
        if !combat_play(parent) {
            continue;
        }
        let step = step_of(parent, node, sess.loc(), &floor, &act);
        // The AGENT'S OWN root distribution when the move came from a searched
        // autoplay step. This is what makes the review agree with the run: it
        // grades the move against the very numbers that picked it, so an
        // autoplayed line reports its real regret (zero, wherever the agent
        // took its own best) instead of a disagreement between two different
        // searches. A hand-played move has none, and falls back to a fresh
        // search below — a DIFFERENT referee, so those rows are advisory.
        let mut scores = agent_scores(parent, node, sess.loc());
        if scores.is_empty() {
            scores = parent
                .combat
                .as_ref()
                .map(|cs| score(cs, &step.state, &cfg))
                .and_then(|r| r["scores"].as_array().cloned())
                .unwrap_or_default();
        }
        if let Some(d) = assess_decision(&step, &scores, blunder_pct) {
            decisions.push(d);
        }
    }
    let mut out = summarize(decisions, top_n);
    out["config"] = json!({"blunder_pct": blunder_pct, "top_n": top_n});
    Ok(out)
}

// ── card_advisor.py / decision_advisor.py ───────────────────────────────

pub const DEFAULT_ROLLOUTS: u32 = 12;
pub const DEFAULT_SIMS: u32 = 128;
pub const DEFAULT_THREADS: u32 = 8;
pub const DEFAULT_MAX_STEPS: u32 = 600;
pub const MAX_ROLLOUTS: u32 = 200;
pub const MAX_SIMS: u32 = 8000;

/// The rollout advisors' knobs, resolved from a request config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RolloutConfig {
    pub rollouts: u32,
    /// `0` = a uniform-random legal move in combat (fast mode).
    pub sims: u32,
    /// Echoed only — the browser search is single-threaded.
    pub threads: u32,
    pub max_steps: u32,
    pub seed: u64,
}

impl RolloutConfig {
    fn from_config(config: &Value, rm: &RunManager) -> Self {
        let get = |k: &str| config.get(k).and_then(Value::as_i64);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let u = |v: Option<i64>, d: u32| v.map_or(d, |v| v.max(0) as u32);
        Self {
            rollouts: u(get("rollouts"), DEFAULT_ROLLOUTS).clamp(1, MAX_ROLLOUTS),
            sims: u(get("sims"), DEFAULT_SIMS).min(MAX_SIMS),
            threads: u(get("threads"), DEFAULT_THREADS),
            max_steps: u(get("max_steps"), DEFAULT_MAX_STEPS),
            seed: config
                .get("seed")
                .and_then(Value::as_u64)
                .unwrap_or(rm.seed ^ 0x5EED),
        }
    }

    fn to_value(self) -> Value {
        json!({
            "rollouts": self.rollouts,
            "sims": self.sims,
            "threads": self.threads,
            "max_steps": self.max_steps,
            "seed": self.seed,
        })
    }
}

/// `_playout_rest_of_act`'s result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Playout {
    pub won: bool,
    pub died: bool,
    pub final_hp: i32,
    pub hp_at_deepest: i32,
    pub floors_advanced: i32,
    pub steps: u32,
    pub reached_act: u8,
    pub inconclusive: bool,
}

/// `RunManager.mcts_pick_action(sims, seed, threads=1)`: the cheap combat
/// search (depth 10, `c_puct = √2`, otherwise the engine defaults) → the
/// engine action, or `None` when the search's pick has no run-action form.
fn mcts_pick_action(rm: &RunManager, sims: u32, seed: u64) -> Option<RunAction> {
    let combat = rm.combat.as_ref()?;
    let cfg = MctsConfig {
        c_puct: std::f32::consts::SQRT_2,
        max_simulations: sims,
        max_rollout_depth: 10,
        seed,
        ..MctsConfig::default()
    };
    let mut mcts = Mcts::new(combat.clone(), cfg);
    let chosen = mcts.try_search().ok()?;
    rmx::action_from_value(&combat_action_to_run_action_value(&chosen)).ok()
}

/// `_playout_rest_of_act`: drive an already-forked + reseeded `rm` to the
/// end of its current act (death, act boundary, a won run, or `max_steps`
/// → inconclusive). Combat: the cheap MCTS at `sims` (uniform-random legal
/// move when `sims == 0` or the search declines); off combat: the tier-1
/// heuristic run policy.
pub fn playout_rest_of_act(
    rm: &mut RunManager,
    sims: u32,
    max_steps: u32,
    mcts_seed: u64,
    rng: &mut rand_xoshiro::Xoshiro256PlusPlus,
) -> Playout {
    let start_act = rm.run_state.act_index;
    let start_floor = rmx::total_floor(rm);
    let mut deepest = start_floor;
    let mut hp_at_deepest = rm.run_state.current_hp;
    let mut steps = 0_u32;
    let mut brain = HeuristicPolicy::new();
    while steps < max_steps {
        if rm.is_over() || rm.run_state.act_index != start_act {
            break;
        }
        // Sample progress out of combat, where `current_hp` is the settled
        // between-rooms HP.
        let floor = rmx::total_floor(rm);
        if floor > deepest {
            deepest = floor;
            hp_at_deepest = rm.run_state.current_hp;
        }
        let action = if rmx::phase_str(rm) == "combat" {
            let picked = if sims > 0 {
                mcts_pick_action(rm, sims, mcts_seed.wrapping_add(u64::from(steps)))
            } else {
                None
            };
            if let Some(a) = picked {
                a
            } else {
                let legal = rmx::available_actions(rm);
                if legal.is_empty() {
                    break;
                }
                legal[rng.gen_range(0..legal.len())]
            }
        } else {
            let acts = rmx::available_actions(rm);
            if acts.is_empty() {
                break;
            }
            match brain.choose(rm, &acts) {
                Some(i) => acts[i],
                None => acts[rng.gen_range(0..acts.len())],
            }
        };
        let result = rmx::take_action(rm, action, false, false);
        if result != "advanced" && result != "run_over" {
            // Engine rejected a legal move — bail rather than spin.
            return Playout {
                won: false,
                died: false,
                final_hp: rm.run_state.current_hp,
                hp_at_deepest,
                floors_advanced: deepest - start_floor,
                steps,
                reached_act: rm.run_state.act_index,
                inconclusive: true,
            };
        }
        steps += 1;
    }
    let over = rm.is_over();
    let cleared_act = rm.run_state.act_index != start_act;
    let won_run = over && rm.run_state.player_won;
    let died = over && !rm.run_state.player_won;
    let inconclusive = steps >= max_steps && !over && !cleared_act;
    let final_floor = rmx::total_floor(rm);
    if final_floor > deepest {
        deepest = final_floor;
        hp_at_deepest = rm.run_state.current_hp;
    }
    Playout {
        won: cleared_act || won_run,
        died,
        final_hp: rm.run_state.current_hp,
        hp_at_deepest,
        floors_advanced: deepest - start_floor,
        steps,
        reached_act: rm.run_state.act_index,
        inconclusive,
    }
}

/// `rank_rows`: mark the single best row, attach each row's `delta` vs the
/// best OTHER row, sort best-first.
pub fn rank_rows(rows: &mut [Value]) {
    if rows.is_empty() {
        return;
    }
    let values: Vec<f64> = rows.iter().map(|r| f64_of(&r["value"])).collect();
    let best = values
        .iter()
        .enumerate()
        .fold(0, |b, (i, v)| if *v > values[b] { i } else { b });
    for (i, row) in rows.iter_mut().enumerate() {
        let best_other = values
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, v)| *v)
            .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))))
            .unwrap_or(0.0);
        row["delta"] = json!(values[i] - best_other);
        row["is_best"] = json!(i == best);
    }
    rows.sort_by(|a, b| {
        let ka = (
            !a["is_best"].as_bool().unwrap_or(false),
            -f64_of(&a["value"]),
        );
        let kb = (
            !b["is_best"].as_bool().unwrap_or(false),
            -f64_of(&b["value"]),
        );
        ka.0.cmp(&kb.0)
            .then(ka.1.partial_cmp(&kb.1).unwrap_or(std::cmp::Ordering::Equal))
    });
}

/// One candidate's `rollouts` paired playouts → its row (without
/// `delta` / `is_best`, which [`rank_rows`] adds).
#[allow(clippy::too_many_arguments)]
fn evaluate_candidate(
    rm: &RunManager,
    action: RunAction,
    cfg: RolloutConfig,
    max_hp: i32,
    progress: &mut Option<Progress<'_>>,
    done: &mut u32,
    total: u32,
    tick_fields: &Value,
) -> Value {
    let mut wins = 0_u32;
    let mut inconclusive = 0_u32;
    let mut hp_total = 0_i64;
    let mut floors_total = 0_i64;
    let mut hp_deep_total = 0_i64;
    let mut tick = |done: &mut u32| {
        *done += 1;
        if let Some(p) = progress.as_mut() {
            let mut frame = json!({"type": "tick", "done": *done, "total": total});
            if let (Some(f), Some(extra)) = (frame.as_object_mut(), tick_fields.as_object()) {
                for (k, v) in extra {
                    f.insert(k.clone(), v.clone());
                }
            }
            p(&frame);
        }
    };
    for r in 0..cfg.rollouts {
        let mut sim = rm.clone();
        let res = rmx::take_action(&mut sim, action, false, false);
        if res != "advanced" && res != "run_over" {
            inconclusive += 1;
            tick(done);
            continue;
        }
        // Paired across candidates: the same run seed + policy seed per
        // rollout `r`, so the only difference is the option taken.
        let seed = cfg.seed.wrapping_add(u64::from(r));
        #[allow(clippy::cast_possible_truncation)]
        let run_seed = seed;
        sim.run_state.rng = RunRngSet::new(run_seed);
        #[allow(clippy::cast_sign_loss)]
        let mut rng = rand_xoshiro::Xoshiro256PlusPlus::seed_from_u64(seed);
        #[allow(clippy::cast_sign_loss)]
        let mcts_seed = cfg.seed.wrapping_add(u64::from(r) * 1009);
        let out = playout_rest_of_act(&mut sim, cfg.sims, cfg.max_steps, mcts_seed, &mut rng);
        if out.inconclusive {
            inconclusive += 1;
        }
        if out.won {
            wins += 1;
        }
        hp_total += i64::from(out.final_hp);
        floors_total += i64::from(out.floors_advanced);
        hp_deep_total += i64::from(out.hp_at_deepest);
        tick(done);
    }
    let n = f64::from(cfg.rollouts);
    let win_rate = f64::from(wins) / n;
    #[allow(clippy::cast_precision_loss)]
    let avg_floors = floors_total as f64 / n;
    #[allow(clippy::cast_precision_loss)]
    let avg_hp_deep = hp_deep_total as f64 / n;
    #[allow(clippy::cast_precision_loss)]
    let avg_final_hp = hp_total as f64 / n;
    // Composite value: act-clear dominates, with depth reached and HP carried
    // to the wall as fine-grained tiebreakers.
    let value = win_rate + 0.01 * avg_floors + 0.02 * (avg_hp_deep / f64::from(max_hp));
    json!({
        "win_rate": win_rate,
        "wins": wins,
        "n": cfg.rollouts,
        "avg_final_hp": avg_final_hp,
        "avg_floors_advanced": avg_floors,
        "avg_hp_at_deepest": avg_hp_deep,
        "value": value,
        "inconclusive": inconclusive,
    })
}

/// `card_advisor.is_advisable`: a plain card-pick reward (no bundle, no
/// relic reward) with at least one card offered.
#[must_use]
pub fn card_advisable(rm: &RunManager) -> bool {
    if rmx::phase_str(rm) != "card_reward" || rmx::card_reward_bundles(rm).is_some() {
        return false;
    }
    let acts = rmx::available_actions(rm);
    if acts.iter().any(|a| matches!(a, RunAction::PickRelicReward)) {
        return false;
    }
    acts.iter()
        .any(|a| matches!(a, RunAction::PickCardReward { idx: Some(_) }))
}

/// `card_advisor.advise`: Monte-Carlo act win-rate per card-reward option —
/// `{candidates: [{idx, label, win_rate, wins, n, avg_final_hp,
/// avg_floors_advanced, avg_hp_at_deepest, value, delta, is_best,
/// inconclusive}], act, config}`, or `{error, candidates: [], config}`.
pub fn card_advise(rm: &RunManager, config: &Value, mut progress: Option<Progress<'_>>) -> Value {
    if !card_advisable(rm) {
        return json!({
            "error": "card advisor only runs on a plain card-reward decision",
            "candidates": [],
            "config": config_obj(config),
        });
    }
    let cfg = RolloutConfig::from_config(config, rm);
    let start_act = rm.run_state.act_index;
    let max_hp = rm.run_state.max_hp.max(1);
    let ids = rmx::card_reward_option_ids(rm);
    let acts = rmx::available_actions(rm);
    let mut cands: Vec<(Option<usize>, String)> = acts
        .iter()
        .filter_map(|a| match a {
            RunAction::PickCardReward { idx: Some(i) } => Some((
                Some(*i),
                ids.get(*i).map_or_else(
                    || format!("card {i}"),
                    |id| id.strip_prefix("CARD.").unwrap_or(id).to_string(),
                ),
            )),
            _ => None,
        })
        .collect();
    if acts
        .iter()
        .any(|a| matches!(a, RunAction::PickCardReward { idx: None }))
    {
        cands.push((None, "skip".to_string()));
    }
    let total = u32::try_from(cands.len()).unwrap_or(u32::MAX) * cfg.rollouts;
    if let Some(p) = progress.as_mut() {
        p(&json!({
            "type": "start",
            "total": total,
            "rollouts": cfg.rollouts,
            "act": start_act,
            "candidates": cands.iter().map(|(idx, label)| json!({"idx": idx, "label": label})).collect::<Vec<_>>(),
        }));
    }
    let mut done = 0_u32;
    let mut rows: Vec<Value> = Vec::with_capacity(cands.len());
    for (idx, label) in &cands {
        let mut row = evaluate_candidate(
            rm,
            RunAction::PickCardReward { idx: *idx },
            cfg,
            max_hp,
            &mut progress,
            &mut done,
            total,
            &json!({"candidate_idx": idx}),
        );
        row["idx"] = json!(idx);
        row["label"] = json!(label);
        rows.push(row);
    }
    rank_rows(&mut rows);
    json!({"candidates": rows, "act": start_act, "config": cfg.to_value()})
}

/// `decision_advisor.is_advisable`: a shop / event / treasure / boss-relic
/// pick, a run-level choose screen, or a single-pick in-combat choice — with
/// at least two evaluable options.
#[must_use]
pub fn decision_advisable(rm: &RunManager) -> bool {
    let phase_ok = match rmx::phase_str(rm) {
        "combat" => match pending_choice(rm) {
            Some(pc) => pc["max_select"].as_i64().unwrap_or(1) <= 1,
            None => false,
        },
        "card_reward" => choose_screen(rm),
        "shop" | "event" | "treasure" | "boss_relic" => true,
        _ => false,
    };
    phase_ok && candidates(rm).len() >= 2
}

/// `_decision_of`: the frontend `decision` string for the current phase.
fn decision_of(rm: &RunManager) -> &'static str {
    let phase = rmx::phase_str(rm);
    if phase == "combat" || choose_screen(rm) {
        return "card_select";
    }
    match phase {
        "shop" => "shop",
        "event" | "treasure" | "boss_relic" => "event_choice",
        other => other,
    }
}

/// `decision_advisor.advise`: Monte-Carlo act win-rate per option of the
/// current decision — `{candidates: [{key, label, …}], decision, act,
/// config}`, or `{error, candidates: [], config}`.
pub fn decision_advise(
    rm: &RunManager,
    config: &Value,
    mut progress: Option<Progress<'_>>,
) -> Value {
    if !decision_advisable(rm) {
        return json!({
            "error": "the decision advisor only runs on a shop, event, or card-select decision with at least two options",
            "candidates": [],
            "config": config_obj(config),
        });
    }
    let cfg = RolloutConfig::from_config(config, rm);
    let start_act = rm.run_state.act_index;
    let decision = decision_of(rm);
    let max_hp = rm.run_state.max_hp.max(1);
    let cands = candidates(rm);
    let total = u32::try_from(cands.len()).unwrap_or(u32::MAX) * cfg.rollouts;
    if let Some(p) = progress.as_mut() {
        p(&json!({
            "type": "start",
            "total": total,
            "rollouts": cfg.rollouts,
            "act": start_act,
            "decision": decision,
            "candidates": cands.iter().map(|c| json!({"key": c.key, "label": c.label})).collect::<Vec<_>>(),
        }));
    }
    let mut done = 0_u32;
    let mut rows: Vec<Value> = Vec::with_capacity(cands.len());
    for c in &cands {
        let mut row = evaluate_candidate(
            rm,
            c.action,
            cfg,
            max_hp,
            &mut progress,
            &mut done,
            total,
            &json!({"candidate_key": c.key}),
        );
        row["key"] = json!(c.key);
        row["label"] = json!(c.label);
        rows.push(row);
    }
    rank_rows(&mut rows);
    json!({
        "candidates": rows,
        "decision": decision,
        "act": start_act,
        "config": cfg.to_value(),
    })
}

/// `advise_card_reward`: the card advisor over the live run; `None` when the
/// active node is not a `card_reward`.
pub fn advise_card_reward(
    sess: &Session,
    config: &Value,
    progress: Option<Progress<'_>>,
) -> Result<Option<Value>, SessionError> {
    let node = sess.active_node()?;
    if node.decision != "card_reward" {
        return Ok(None);
    }
    let rm = sess
        .game()
        .and_then(|g| g.native_run())
        .ok_or_else(|| SessionError::new(400, "no live run"))?;
    Ok(Some(card_advise(rm, config, progress)))
}

/// `advise_decision`: the decision advisor over the live run; `None` when the
/// active node is not a `card_select` / `shop` / `event_choice`.
pub fn advise_decision(
    sess: &Session,
    config: &Value,
    progress: Option<Progress<'_>>,
) -> Result<Option<Value>, SessionError> {
    let node = sess.active_node()?;
    if !matches!(
        node.decision.as_str(),
        "card_select" | "shop" | "event_choice"
    ) {
        return Ok(None);
    }
    let rm = sess
        .game()
        .and_then(|g| g.native_run())
        .ok_or_else(|| SessionError::new(400, "no live run"))?;
    Ok(Some(decision_advise(rm, config, progress)))
}

/// `wpa_review` for a session with no recorded node values (PPO values
/// arrive with M3): the `available: false` payload, verbatim.
#[must_use]
pub fn wpa_review_unavailable() -> Value {
    json!({
        "available": false,
        "reason": "no run-level value data — value recording needs the rust backend and a loadable PPO checkpoint",
        "points": [],
        "turning_points": [],
        "summary": {},
        "caveats": [
            "The outlook score is the AI's shaped-return estimate of the run — a relative signal, not a win percentage.",
            "A swing mixes your choice with the luck revealed after it (what the shop stocked, what the reward offered, how the fight rolled) — use “verify” to check whether the choice itself mattered.",
            "Judgments are relative to the AI's own play style; a line that is weak for the AI may still be fine for a human plan.",
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_resolve_hand_potions_and_sentinels() {
        let state = json!({
            "hand": [{"index": 0, "name": "Strike"}, {"index": 3, "name": "Bash"}],
            "player": {"potions": [{"name": "Fire Potion"}, "Block Potion"]},
        });
        assert_eq!(action_label(END_TURN, &state), "end turn");
        assert_eq!(action_label(3, &state), "Bash");
        assert_eq!(action_label(7, &state), "action 7");
        assert_eq!(action_label(POTION_BASE, &state), "drink Fire Potion");
        assert_eq!(action_label(POTION_BASE - 1, &state), "drink Block Potion");
        assert_eq!(action_label(POTION_BASE - 2, &state), "drink potion slot 2");
    }

    #[test]
    fn stable_keys_mirror_the_recorder() {
        assert_eq!(
            action_stable_key(&json!({"action": "play", "card_index": 2, "target_index": 1})),
            (Some(2), Some(1))
        );
        assert_eq!(
            action_stable_key(&json!({"action": "end_turn"})),
            (Some(END_TURN), None)
        );
        assert_eq!(
            action_stable_key(&json!({"action": "drink_potion", "slot": 1})),
            (Some(POTION_BASE - 1), None)
        );
        assert_eq!(action_stable_key(&json!({})), (None, None));
        assert_eq!(
            action_stable_key(&json!({"action": "choose"})),
            (None, None)
        );
    }

    #[test]
    fn native_action_reads_the_workbench_verbs() {
        assert_eq!(
            native_action(
                Some("play_card"),
                &json!({"card_index": 1, "target_index": 0})
            ),
            json!({"action": "play", "card_index": 1, "target_index": 0})
        );
        assert_eq!(
            native_action(Some("use_potion"), &json!({"potion_index": 2})),
            json!({"action": "drink_potion", "slot": 2, "target_index": null})
        );
        assert_eq!(
            native_action(Some("__agent_native__"), &json!({"action": "end_turn"})),
            json!({"action": "end_turn"})
        );
        assert_eq!(
            native_action(Some("select_cards"), &json!({"indices": "1"})),
            json!({"indices": "1"})
        );
    }

    fn row(stable_idx: i64, target: Option<i64>, wp: f64, best: bool, label: &str) -> Value {
        json!({
            "stable_idx": stable_idx, "target_index": target, "win_prob": wp,
            "is_best": best, "visits": 40, "action": label,
        })
    }

    fn step(chosen: Value, enemies: Value) -> DecisionStep {
        DecisionStep {
            idx: 5,
            node_id: Some(9),
            state: json!({"enemies": enemies}),
            chosen,
            turn: json!(3),
            floor: json!(2),
            act: json!(1),
        }
    }

    #[test]
    fn assess_decision_flags_a_blunder_with_the_python_message() {
        let scores = vec![
            row(1, Some(0), 0.71, true, "Defend"),
            row(0, Some(0), 0.58, false, "Strike"),
        ];
        let d = assess_decision(
            &step(
                json!({"action": "play", "card_index": 0, "target_index": 0}),
                json!([]),
            ),
            &scores,
            DEFAULT_BLUNDER_DROP_PCT,
        )
        .unwrap();
        assert_eq!(d["is_blunder"], json!(true));
        assert_eq!(d["drop_pct"], json!(13));
        assert_eq!(d["player_rank"], json!(1));
        assert_eq!(d["n_actions"], json!(2));
        assert_eq!(
            d["message"],
            json!("Turn 3: Strike instead of Defend dropped P(win) 71%→58% (−13 pts)")
        );
        // Multi-enemy fights name the target.
        let d = assess_decision(
            &step(
                json!({"action": "play", "card_index": 0, "target_index": 0}),
                json!([{"index": 0, "name": "Cultist"}, {"index": 1, "name": "Jaw Worm"}]),
            ),
            &scores,
            DEFAULT_BLUNDER_DROP_PCT,
        )
        .unwrap();
        assert!(d["played_label"]
            .as_str()
            .unwrap()
            .contains("Strike → Cultist"));
    }

    #[test]
    fn assess_decision_matched_line_and_unresolvable() {
        let scores = vec![
            row(1, None, 0.6, true, "Defend"),
            row(0, None, 0.5, false, "Strike"),
        ];
        let d = assess_decision(
            &step(json!({"action": "play", "card_index": 1}), json!([])),
            &scores,
            8.0,
        )
        .unwrap();
        assert_eq!(d["matched_best"], json!(true));
        assert_eq!(d["regret"], json!(0.0));
        assert_eq!(
            d["message"],
            json!("Turn 3: Defend matched the MCTS line (P(win) 60%)")
        );
        assert!(assess_decision(
            &step(json!({"action": "play", "card_index": 4}), json!([])),
            &scores,
            8.0
        )
        .is_none());
        assert!(assess_decision(
            &step(json!({"action": "end_turn"}), json!([])),
            &scores[..1],
            8.0
        )
        .is_none());
    }

    #[test]
    fn summarize_ranks_blunders_by_regret_then_turn() {
        let d = |idx: i64, turn: i64, regret: f64, blunder: bool| json!({"idx": idx, "turn": turn, "regret": regret, "drop_pct": pct(regret), "is_blunder": blunder});
        let out = summarize(
            vec![
                d(1, 1, 0.10, true),
                d(2, 2, 0.30, true),
                d(3, 3, 0.30, true),
                d(4, 4, 0.02, false),
            ],
            2,
        );
        assert_eq!(out["summary"]["total_decisions"], json!(4));
        assert_eq!(out["summary"]["blunder_count"], json!(3));
        assert_eq!(out["summary"]["cumulative_drop_pct"], json!(72));
        assert_eq!(out["summary"]["worst_drop_pct"], json!(30));
        let b = out["blunders"].as_array().unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!(b[0]["idx"], json!(2));
        assert_eq!(b[1]["idx"], json!(3));
    }

    #[test]
    fn rank_rows_marks_best_and_deltas() {
        let mut rows = vec![
            json!({"key": "a", "value": 0.2}),
            json!({"key": "b", "value": 0.5}),
            json!({"key": "c", "value": 0.4}),
        ];
        rank_rows(&mut rows);
        assert_eq!(rows[0]["key"], json!("b"));
        assert_eq!(rows[0]["is_best"], json!(true));
        assert!((f64_of(&rows[0]["delta"]) - 0.1).abs() < 1e-9);
        assert_eq!(rows[1]["key"], json!("c"));
        assert!((f64_of(&rows[1]["delta"]) + 0.1).abs() < 1e-9);
        assert!((f64_of(&rows[2]["delta"]) + 0.3).abs() < 1e-9);
        let mut one = vec![json!({"value": 0.3})];
        rank_rows(&mut one);
        assert_eq!(one[0]["delta"], json!(0.3));
    }

    #[test]
    fn pct_rounds_ties_to_even_like_python() {
        assert_eq!(pct(0.125), 12);
        assert_eq!(pct(0.135), 14);
        assert_eq!(pct(0.71), 71);
    }
}
