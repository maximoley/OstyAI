//! Run logs (M4): the JSONL a finished run leaves behind and the replay
//! store that reads one back.
//!
//! * [`write_run_log`] is `BranchingLiveSession.quit` + `game_log.GameLogger`:
//!   the active path as `{step, ts, type, data}` records — a self-describing
//!   `meta` header (identity + `run_mode` / `act1` / `backend`), one `state`
//!   per node, each node's `action` (native args, display label, replayable
//!   verb), its `combat_log` rows, the recorded `wpa` value, and the
//!   terminal `{"summary": true}` row the MCTS Runs page keys on.
//! * [`ReplayStore`] is `workbench/timeline.py`'s `TimelineStore`: one
//!   [`Step`] per `state` record with the following `action` / `mcts` /
//!   `wpa` / `combat_log` records attached, `floor` / `act` carried forward,
//!   the per-floor map node collected in a pass of its own.
//! * [`parse_run`] / [`aggregate_runs`] are `workbench/aggregate.py`.
//!
//! The browser has no filesystem: the host (the worker) keeps each log's
//! text under its filename in IndexedDB and hands it back to
//! [`ReplayStore::parse`] on `/api/load`.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::advisors::native_action_recorded;
use crate::session::Session;

/// The fixed clock a host without one gets (the golden dump's pinned
/// `datetime.now()`), also the filename stamp of that instant.
pub const DEFAULT_NOW_ISO: &str = "2026-01-01T00:00:00+00:00";

/// `datetime.strftime("%Y%m%d_%H%M%S")` of an ISO-8601 timestamp
/// (`2026-09-03T04:22:57.123Z` → `20260903_042257`).
#[must_use]
pub fn file_stamp(now_iso: &str) -> String {
    let digits: String = now_iso
        .chars()
        .take_while(|c| *c != '.' && *c != '+' && *c != 'Z')
        .filter(char::is_ascii_digit)
        .collect();
    if digits.len() >= 14 {
        format!("{}_{}", &digits[..8], &digits[8..14])
    } else {
        "00000000_000000".to_string()
    }
}

/// The written log: its filename (`{stamp}_{character}_{seed}.jsonl`) and
/// the JSONL text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLog {
    pub filename: String,
    pub text: String,
}

fn record(step: u64, ts: &str, kind: &str, data: Value) -> String {
    let entry = json!({"step": step, "ts": ts, "type": kind, "data": data});
    // `json.dumps(ensure_ascii=False)` — serde_json emits raw UTF-8 too.
    serde_json::to_string(&entry).expect("serialize record")
}

/// `quit()`'s log of the session's active path, or `None` when no run is
/// live. `now_iso` stamps every record and the filename.
#[must_use]
pub fn write_run_log(sess: &Session, now_iso: &str) -> Option<RunLog> {
    let path = sess.active_path().ok()?;
    if path.is_empty() {
        return None;
    }
    let ts = now_iso;
    let mut lines: Vec<String> = Vec::new();
    let mut step: u64 = 0;
    lines.push(record(
        step,
        ts,
        "meta",
        json!({
            "character": sess.character,
            "seed": sess.seed,
            "ascension": sess.ascension,
            "run_mode": sess.run_mode(),
            "act1": sess.act1,
            "backend": sess.backend(),
        }),
    ));
    let log_wpa = |lines: &mut Vec<String>, step: u64, i: usize| {
        if let Some(v) = path[i].value {
            let taken = path.get(i + 1).map_or(Value::Null, |n| json!(n.label));
            lines.push(record(
                step,
                ts,
                "wpa",
                json!({"value": v, "taken_label": taken}),
            ));
        }
    };
    step += 1;
    lines.push(record(step, ts, "state", path[0].state.clone()));
    log_wpa(&mut lines, step, 0);
    for (j, node) in path.iter().enumerate().skip(1) {
        lines.push(record(
            step,
            ts,
            "action",
            json!({
                "cmd": "action",
                "action": node.action,
                "args": native_action_recorded(node.action.as_deref(), &node.args),
                "label": node.label,
                "replay": {"action": node.action, "args": node.args},
            }),
        ));
        // The root distribution of the search that CHOSE this action, attached
        // to the decision it answered (an `mcts` record shares the step number
        // of the preceding `state`, which is the parent's). Without it a replay
        // of this run carries no recorded reasoning at all, so the regret
        // review re-searches every position with a different referee and
        // reports the agent blundering against itself.
        let agent_scores = crate::advisors::agent_scores(path[j - 1], node, sess.loc());
        if !agent_scores.is_empty() {
            let total_visits: i64 = agent_scores
                .iter()
                .map(|r| r.get("visits").and_then(Value::as_i64).unwrap_or(0))
                .sum();
            // `action_label` is the Markdown report's key and it indexes it
            // unguarded; `action` is the review's. Same string, written under
            // both so a report of a workbench-played run cannot break.
            let rows: Vec<Value> = agent_scores
                .iter()
                .map(|r| {
                    let mut r = r.clone();
                    let label = r.get("action").cloned().unwrap_or(Value::Null);
                    if let Some(m) = r.as_object_mut() {
                        m.insert("action_label".to_string(), label);
                    }
                    r
                })
                .collect();
            lines.push(record(
                step,
                ts,
                "mcts",
                json!({"source": "agent", "total_visits": total_visits, "root_outcomes": rows}),
            ));
        }
        if !node.combat_log.is_empty() {
            lines.push(record(step, ts, "combat_log", json!(node.combat_log)));
        }
        step += 1;
        lines.push(record(step, ts, "state", node.state.clone()));
        log_wpa(&mut lines, step, j);
    }
    let leaf = &path[path.len() - 1].state;
    let progress = sess.progress_of_path(&path);
    lines.push(record(
        step,
        ts,
        "meta",
        json!({
            "summary": true,
            "final_won": crate::render::py::truthy(leaf.get("victory").unwrap_or(&Value::Null)),
            "final_turn": leaf.get("round").cloned().unwrap_or(Value::Null),
            "final_floor": progress.floor,
            "final_act": progress.act,
            "final_player_hp": progress.hp,
            "final_player_max_hp": progress.max_hp,
        }),
    ));
    let safe_seed = sess.seed.replace('/', "_");
    Some(RunLog {
        filename: format!(
            "{}_{}_{}.jsonl",
            file_stamp(now_iso),
            sess.character,
            safe_seed
        ),
        text: lines.join("\n") + "\n",
    })
}

// ── the replay store ─────────────────────────────────────────────────────

/// One decision-point state plus the action that followed it (`Step`).
#[derive(Debug, Clone)]
pub struct Step {
    pub idx: usize,
    pub decision: String,
    pub state: Value,
    pub scores: Vec<Value>,
    pub pv: Value,
    pub chosen_action: Value,
    pub wpa: Value,
    pub combat_log: Vec<Value>,
    pub meta: Value,
}

impl Step {
    /// The step as the dict the routes serialize (`Step` TypedDict).
    #[must_use]
    pub fn to_value(&self) -> Value {
        json!({
            "idx": self.idx,
            "decision": self.decision,
            "state": self.state,
            "legal_actions": [],
            "scores": self.scores,
            "pv": self.pv,
            "chosen_action": self.chosen_action,
            "wpa": self.wpa,
            "combat_log": self.combat_log,
            "source": "log",
            "meta": self.meta,
        })
    }
}

/// `_compact_advice`: a recorded advice object as the small `meta.advice`
/// display row.
fn compact_advice(advice: &Value) -> Value {
    let Some(a) = advice.as_object().filter(|m| !m.is_empty()) else {
        return Value::Null;
    };
    let raw: Vec<&Map<String, Value>> = a
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .collect();
    let total_visits: f64 = raw
        .iter()
        .map(|o| o.get("visits").and_then(Value::as_f64).unwrap_or(0.0))
        .sum();
    let opts: Vec<Value> = raw
        .iter()
        .map(|o| {
            let share = match o.get("visit_share") {
                Some(s) if !s.is_null() => s.clone(),
                _ if total_visits != 0.0 => {
                    json!(o.get("visits").and_then(Value::as_f64).unwrap_or(0.0) / total_visits)
                }
                _ => Value::Null,
            };
            json!({
                "label": o.get("label").filter(|v| crate::render::py::truthy(v))
                    .or_else(|| o.get("action").filter(|v| crate::render::py::truthy(v)))
                    .cloned().unwrap_or_else(|| json!("?")),
                "share": share,
                "is_best": o.get("is_best").is_some_and(crate::render::py::truthy),
            })
        })
        .collect();
    let best = a.get("best").and_then(Value::as_object);
    let out = json!({
        "best": best.and_then(|b| b.get("label")).cloned().unwrap_or(Value::Null),
        "note": a.get("note").cloned().unwrap_or(Value::Null),
        "options": opts.iter().take(12).cloned().collect::<Vec<_>>(),
    });
    let any = crate::render::py::truthy(&out["best"])
        || crate::render::py::truthy(&out["note"])
        || !opts.is_empty();
    if any {
        out
    } else {
        Value::Null
    }
}

/// `_graft_path_plan`: draw a recorded advisor's route on the map.
fn graft_path_plan(state: &mut Value, advice: &Value) {
    let Some(a) = advice.as_object() else {
        return;
    };
    let Some(path) = a.get("path").and_then(Value::as_array) else {
        return;
    };
    let Some(fm) = state.get_mut("full_map").and_then(Value::as_object_mut) else {
        return;
    };
    if fm.get("path_plan").is_some_and(|p| !p.is_null()) {
        return;
    }
    let route: Vec<Value> = path
        .iter()
        .filter_map(|h| {
            let h = h.as_object()?;
            let col = h.get("col").filter(|v| !v.is_null())?;
            let row = h.get("row").filter(|v| !v.is_null())?;
            Some(json!({"col": col, "row": row}))
        })
        .collect();
    if route.is_empty() {
        return;
    }
    let first = route[0]["col"].clone();
    fm.insert(
        "path_plan".into(),
        json!({
            "best_col": first,
            "baseline_col": first,
            "diverged": false,
            "route": route,
            "columns": [],
        }),
    );
}

/// `TimelineStore`: the parsed run.
#[derive(Debug, Clone)]
pub struct ReplayStore {
    steps: Vec<Step>,
    head: Map<String, Value>,
    /// The log's identity for the host (`/api/load`'s `path`).
    pub path: String,
}

impl ReplayStore {
    /// `TimelineStore.load_jsonl`. Errors name the offending line like
    /// Python's `json.JSONDecodeError` text does.
    pub fn parse(path: &str, text: &str) -> Result<Self, String> {
        let mut states: Vec<(u64, Value)> = Vec::new();
        let mut actions: BTreeMap<usize, Value> = BTreeMap::new();
        let mut mcts: BTreeMap<usize, Value> = BTreeMap::new();
        let mut wpa: BTreeMap<usize, Value> = BTreeMap::new();
        let mut clog: BTreeMap<usize, Value> = BTreeMap::new();
        let mut head = Map::new();
        for (lineno, raw) in text.lines().enumerate() {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            let entry: Value =
                serde_json::from_str(raw).map_err(|e| format!("{e} (line {})", lineno + 1))?;
            let kind = entry.get("type").and_then(Value::as_str).unwrap_or("");
            let data = entry.get("data").cloned().unwrap_or(Value::Null);
            match kind {
                "state" => {
                    let step = entry.get("step").and_then(Value::as_u64).unwrap_or(0);
                    states.push((step, data));
                }
                "action" => {
                    if !states.is_empty() {
                        actions.insert(states.len() - 1, data);
                    }
                }
                "mcts" => {
                    if !states.is_empty() {
                        mcts.insert(
                            states.len() - 1,
                            if data.is_null() { json!({}) } else { data },
                        );
                    }
                }
                "wpa" => {
                    if !states.is_empty() {
                        wpa.insert(
                            states.len() - 1,
                            if data.is_null() { json!({}) } else { data },
                        );
                    }
                }
                "combat_log" => {
                    if !states.is_empty() {
                        clog.insert(
                            states.len() - 1,
                            if data.is_null() { json!([]) } else { data },
                        );
                    }
                }
                "meta" => {
                    if let Some(d) = data.as_object() {
                        if !d.get("summary").is_some_and(crate::render::py::truthy) {
                            for (k, v) in d {
                                head.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // WHICH MAP NODE each floor's room sits on: collected in a pass of its
        // own because the coord arrives after the room it belongs to.
        let mut coord_by_floor: BTreeMap<String, Value> = BTreeMap::new();
        for (_, data) in &states {
            let coord = data
                .get("full_map")
                .and_then(|m| m.get("current_coord"))
                .filter(|c| c.is_object());
            let floor = data.get("floor").filter(|f| !f.is_null());
            if let (Some(c), Some(f)) = (coord, floor) {
                coord_by_floor.insert(f.to_string(), c.clone());
            }
        }
        let (mut floor, mut act, mut act_name) = (Value::Null, Value::Null, Value::Null);
        let mut run_seed = Value::Null;
        let mut steps = Vec::with_capacity(states.len());
        for (i, (step_no, mut data)) in states.into_iter().enumerate() {
            if let Some(f) = data.get("floor").filter(|v| !v.is_null()) {
                floor = f.clone();
            }
            if let Some(a) = data.get("act").filter(|v| !v.is_null()) {
                act = a.clone();
            }
            if let Some(a) = data.get("act_name").filter(|v| !v.is_null()) {
                act_name = a.clone();
            }
            if let Some(s) = data
                .get("rng")
                .and_then(|r| r.get("run_seed"))
                .filter(|v| v.is_i64() || v.is_u64())
            {
                run_seed = s.clone();
            }
            let node = if floor.is_null() {
                None
            } else {
                coord_by_floor.get(&floor.to_string())
            };
            let m = mcts.get(&i).cloned().unwrap_or_else(|| json!({}));
            let advice = m.get("advice").cloned().unwrap_or(Value::Null);
            graft_path_plan(&mut data, &advice);
            let scores: Vec<Value> = m
                .get("root_outcomes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|r| {
                    let visits = r.get("visits").and_then(Value::as_f64).unwrap_or(0.0);
                    let wins = r.get("wins").and_then(Value::as_f64).unwrap_or(0.0);
                    // Two writers produce `root_outcomes` and they disagree on
                    // two keys, so read both shapes. The offline MCTS recorder
                    // writes `action_label` and no win-prob, leaving it to be
                    // derived; `write_run_log` writes the live agent's OWN root
                    // rows, which carry `action` and the search's `win_prob`
                    // verbatim. Prefer a recorded `win_prob`: `wins / visits`
                    // is only a proxy for it, and re-deriving would quietly
                    // regrade the agent's own numbers.
                    let recorded_wp = r.get("win_prob").and_then(Value::as_f64);
                    json!({
                        "stable_idx": r.get("stable_idx").cloned().unwrap_or(Value::Null),
                        "action": r
                            .get("action")
                            .filter(|v| !v.is_null())
                            .or_else(|| r.get("action_label"))
                            .cloned()
                            .unwrap_or(Value::Null),
                        "target_index": r.get("target_index").cloned().unwrap_or(Value::Null),
                        "win_prob": recorded_wp
                            .unwrap_or(if visits == 0.0 { 0.0 } else { wins / visits }),
                        "q": r.get("q").cloned().unwrap_or(json!(0.0)),
                        "visits": r.get("visits").cloned().unwrap_or(json!(0)),
                        "is_best": r.get("is_best").cloned().unwrap_or(json!(false)),
                        "wins": r.get("wins").cloned().unwrap_or(json!(0)),
                        "losses": r.get("losses").cloned().unwrap_or(json!(0)),
                        "ties": r.get("ties").cloned().unwrap_or(json!(0)),
                    })
                })
                .collect();
            let pv = match m.get("pv") {
                Some(p) if crate::render::py::truthy(p) => p.clone(),
                _ => Value::Null,
            };
            steps.push(Step {
                idx: i,
                decision: data
                    .get("decision")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                meta: json!({
                    "turn": data.get("round").cloned().unwrap_or(Value::Null),
                    "step_no": step_no,
                    "floor": floor,
                    "act": act,
                    "act_name": act_name,
                    "bg_seed": run_seed,
                    "bg_row": node.and_then(|n| n.get("row")).cloned().unwrap_or(Value::Null),
                    "bg_col": node.and_then(|n| n.get("col")).cloned().unwrap_or(Value::Null),
                    "advice": compact_advice(&advice),
                }),
                state: data,
                scores,
                pv,
                chosen_action: actions.get(&i).cloned().unwrap_or(Value::Null),
                wpa: wpa.get(&i).cloned().unwrap_or(Value::Null),
                combat_log: clog
                    .get(&i)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            });
        }
        Ok(Self {
            steps,
            head,
            path: path.to_string(),
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// `get(idx)`; the `IndexError` text on a bad index.
    pub fn get(&self, idx: i64) -> Result<&Step, String> {
        usize::try_from(idx)
            .ok()
            .and_then(|i| self.steps.get(i))
            .ok_or_else(|| {
                format!(
                    "step {idx} out of range (0..{})",
                    self.steps.len() as i64 - 1
                )
            })
    }

    pub fn get_mut(&mut self, idx: i64) -> Result<&mut Step, String> {
        let n = self.steps.len() as i64;
        usize::try_from(idx)
            .ok()
            .and_then(|i| self.steps.get_mut(i))
            .ok_or_else(|| format!("step {idx} out of range (0..{})", n - 1))
    }

    #[must_use]
    pub fn all(&self) -> &[Step] {
        &self.steps
    }

    pub fn all_mut(&mut self) -> &mut [Step] {
        &mut self.steps
    }

    /// `character()`: the recorded character, `Ironclad` by default.
    #[must_use]
    pub fn character(&self) -> String {
        self.head
            .get("character")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("Ironclad")
            .to_string()
    }

    /// `fixture_path()`: a fixture-sourced recording's source (never set by
    /// the browser's own logs).
    #[must_use]
    pub fn fixture_path(&self) -> Option<&str> {
        self.head.get("fixture_path").and_then(Value::as_str)
    }

    /// `run_identity()`: what rebuilds this run from its seed, or `None`.
    #[must_use]
    pub fn run_identity(&self) -> Option<RunIdentity> {
        let run_mode = self.head.get("run_mode").filter(|v| !v.is_null())?;
        let character = self
            .head
            .get("character")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())?;
        Some(RunIdentity {
            character: character.to_string(),
            seed: match self.head.get("seed") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Null) | None => String::new(),
                Some(other) => other.to_string(),
            },
            ascension: self
                .head
                .get("ascension")
                .and_then(Value::as_i64)
                .and_then(|a| u8::try_from(a).ok())
                .unwrap_or(0),
            run_mode: crate::render::py::truthy(run_mode),
            act1: self
                .head
                .get("act1")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("overgrowth")
                .to_string(),
            backend: self
                .head
                .get("backend")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("rust")
                .to_string(),
        })
    }

    /// `replay_line(idx)`: the `[{action, args}]` line that reaches step
    /// `idx`, or `None` when any earlier step lacks its replayable form.
    #[must_use]
    pub fn replay_line(&self, idx: usize) -> Option<Vec<Value>> {
        let mut out = Vec::with_capacity(idx);
        for j in 0..idx {
            let rec = self.steps.get(j)?.chosen_action.get("replay")?;
            let action = rec
                .get("action")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())?;
            let args = rec
                .get("args")
                .cloned()
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({}));
            out.push(json!({"action": action, "args": args}));
        }
        Some(out)
    }
}

/// `run_identity()`'s fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunIdentity {
    pub character: String,
    pub seed: String,
    pub ascension: u8,
    pub run_mode: bool,
    pub act1: String,
    pub backend: String,
}

/// `resume_capability`: whether "play from here" can take this recording
/// over. The browser has no fixture files, so only the `run` route exists.
#[must_use]
pub fn resume_capability(store: &ReplayStore) -> Value {
    let last = store.len().saturating_sub(1);
    if store.run_identity().is_some() && store.replay_line(last).is_some() {
        return json!({"kind": "run", "reason": Value::Null});
    }
    json!({
        "kind": Value::Null,
        "reason": "this recording cannot be resumed: it has no readable source fixture and \
                   no replayable action line (only runs played in the workbench after \
                   2026-08-26 carry one)",
    })
}

// ── MCTS Runs (workbench/aggregate.py) ───────────────────────────────────

/// `_parse_run`'s row, or `None` for a log without the terminal summary.
#[must_use]
pub fn parse_run(name: &str, text: &str) -> Option<Value> {
    let mut head = Map::new();
    let mut summary: Option<Map<String, Value>> = None;
    let mut states: Vec<Value> = Vec::new();
    let mut mcts: Vec<Value> = Vec::new();
    for raw in text.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let e: Value = serde_json::from_str(raw).ok()?;
        let data = e.get("data").cloned().unwrap_or_else(|| json!({}));
        match e.get("type").and_then(Value::as_str) {
            Some("meta") => {
                let d = data.as_object().cloned().unwrap_or_default();
                if d.get("summary").is_some_and(crate::render::py::truthy) {
                    summary = Some(d);
                } else {
                    head = d;
                }
            }
            Some("state") => states.push(data),
            Some("mcts") => mcts.push(data),
            _ => {}
        }
    }
    let summary = summary?;
    let mut enemy_pcts: Vec<f64> = Vec::new();
    for s in &states {
        for en in s
            .get("enemies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let mh = en.get("max_hp").and_then(Value::as_f64).unwrap_or(0.0);
            if mh != 0.0 {
                let hp = en.get("hp").and_then(Value::as_f64).unwrap_or(mh);
                enemy_pcts.push(100.0 * hp / mh);
            }
        }
    }
    let max_q = |m: &Value| -> Option<f64> {
        m.get("root_outcomes")
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty())
            .map(|a| {
                a.iter()
                    .map(|r| r.get("q").and_then(Value::as_f64).unwrap_or(0.0))
                    .fold(f64::NEG_INFINITY, f64::max)
            })
    };
    let turn1_q = mcts.first().and_then(max_q);
    let all_q: Vec<f64> = mcts.iter().filter_map(max_q).collect();
    let stem = head
        .get("fixture_stem")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map_or_else(
            || name.trim_end_matches(".jsonl").to_string(),
            str::to_string,
        );
    Some(json!({
        "path": name,
        "stem": stem,
        "won": summary.get("final_won").is_some_and(crate::render::py::truthy),
        "final_turn": summary.get("final_turn").cloned().unwrap_or(Value::Null),
        "final_floor": summary.get("final_floor").cloned().unwrap_or(Value::Null),
        "turn1_q": turn1_q,
        "enemy_min_pct": enemy_pcts.iter().copied().fold(None, |m: Option<f64>, p| Some(m.map_or(p, |x| x.min(p)))),
        "all_q": all_q,
    }))
}

fn tags(run: &Value) -> Vec<&'static str> {
    let mut out = Vec::new();
    let qs: Vec<f64> = run["all_q"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .collect();
    if !qs.is_empty() && qs.iter().copied().fold(f64::NEG_INFINITY, f64::max) < -0.5 {
        out.push("hopeless_position");
    }
    if run["enemy_min_pct"].as_f64().is_some_and(|p| p > 50.0) {
        out.push("no_damage");
    }
    if run["final_turn"]
        .as_f64()
        .is_some_and(|t| t != 0.0 && t <= 3.0)
    {
        out.push("fast_death");
    }
    out
}

/// `GET /api/mcts-runs`: the summary rows of every parsable log, in name
/// order.
#[must_use]
pub fn mcts_runs(logs: &[(String, String)]) -> Value {
    let mut sorted: Vec<&(String, String)> = logs.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let rows: Vec<Value> = sorted
        .iter()
        .filter_map(|(name, text)| parse_run(name, text))
        .map(|r| {
            json!({
                "stem": r["stem"],
                "won": r["won"],
                "final_turn": r["final_turn"],
                "final_floor": r["final_floor"],
                "turn1_q": r["turn1_q"],
                "enemy_min_pct": r["enemy_min_pct"],
                "path": r["path"],
            })
        })
        .collect();
    json!({"runs": rows})
}

/// `aggregate_runs`: loss patterns across the logs.
#[must_use]
pub fn aggregate_runs(logs: &[(String, String)]) -> Value {
    let mut sorted: Vec<&(String, String)> = logs.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let runs: Vec<Value> = sorted
        .iter()
        .filter_map(|(name, text)| parse_run(name, text))
        .collect();
    let losses: Vec<&Value> = runs.iter().filter(|r| r["won"] != json!(true)).collect();
    // `Counter(str(final_turn))` in first-seen order.
    let mut hist: Vec<(String, i64)> = Vec::new();
    for r in &losses {
        if r["final_turn"].is_null() {
            continue;
        }
        let key = crate::render::py::to_str(&r["final_turn"]);
        if let Some(e) = hist.iter_mut().find(|(k, _)| *k == key) {
            e.1 += 1;
        } else {
            hist.push((key, 1));
        }
    }
    let q_vals: Vec<f64> = losses
        .iter()
        .filter_map(|r| r["turn1_q"].as_f64())
        .collect();
    let mut tag_counts: Vec<(&str, i64)> = Vec::new();
    for r in &losses {
        for t in tags(r) {
            if let Some(e) = tag_counts.iter_mut().find(|(k, _)| *k == t) {
                e.1 += 1;
            } else {
                tag_counts.push((t, 1));
            }
        }
    }
    let mut ranked: Vec<&Value> = losses.clone();
    ranked.sort_by(|a, b| {
        let ka = a["turn1_q"].as_f64().unwrap_or(0.0);
        let kb = b["turn1_q"].as_f64().unwrap_or(0.0);
        ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
    });
    let to_obj = |v: Vec<(String, i64)>| {
        let mut m = Map::new();
        for (k, n) in v {
            m.insert(k, json!(n));
        }
        Value::Object(m)
    };
    json!({
        "n_runs": runs.len(),
        "n_losses": losses.len(),
        "final_turn_histogram": to_obj(hist),
        "turn1_q": {
            "min": q_vals.iter().copied().fold(None, |m: Option<f64>, q| Some(m.map_or(q, |x| x.min(q)))),
            "max": q_vals.iter().copied().fold(None, |m: Option<f64>, q| Some(m.map_or(q, |x| x.max(q)))),
            "mean": if q_vals.is_empty() { None } else { Some(q_vals.iter().sum::<f64>() / q_vals.len() as f64) },
        },
        "pattern_tags": to_obj(tag_counts.into_iter().map(|(k, n)| (k.to_string(), n)).collect()),
        "representative_losses": ranked.iter().take(5).map(|r| json!({
            "stem": r["stem"],
            "final_turn": r["final_turn"],
            "turn1_q": r["turn1_q"],
            "enemy_min_pct": r["enemy_min_pct"],
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_stamps() {
        assert_eq!(file_stamp("2026-09-03T04:22:57.123Z"), "20260903_042257");
        assert_eq!(file_stamp(DEFAULT_NOW_ISO), "20260101_000000");
        assert_eq!(file_stamp("garbage"), "00000000_000000");
    }

    #[test]
    fn parses_a_minimal_log_and_aggregates_it() {
        let text = [
            r#"{"step":0,"ts":"t","type":"meta","data":{"character":"Silent","seed":"S","ascension":0,"run_mode":false,"act1":"overgrowth","backend":"rust"}}"#,
            r#"{"step":1,"ts":"t","type":"state","data":{"decision":"map_select","floor":0,"act":1,"act_name":"A","rng":{"run_seed":7},"full_map":{"current_coord":{"col":1,"row":0}}}}"#,
            r#"{"step":1,"ts":"t","type":"wpa","data":{"value":0.5,"taken_label":"go"}}"#,
            r#"{"step":1,"ts":"t","type":"action","data":{"cmd":"action","action":"select_map_node","args":{"col":1,"row":1},"label":"go","replay":{"action":"select_map_node","args":{"col":1,"row":1}}}}"#,
            r#"{"step":2,"ts":"t","type":"state","data":{"decision":"combat_play","round":1,"enemies":[{"hp":5,"max_hp":10}]}}"#,
            r#"{"step":2,"ts":"t","type":"mcts","data":{"root_outcomes":[{"stable_idx":0,"action_label":"Strike","wins":3,"visits":4,"q":0.2,"is_best":true}],"pv":["a"]}}"#,
            r#"{"step":2,"ts":"t","type":"meta","data":{"summary":true,"final_won":false,"final_turn":3,"final_floor":1}}"#,
            "",
        ]
        .join("\n");
        let store = ReplayStore::parse("x.jsonl", &text).unwrap();
        assert_eq!(store.len(), 2);
        let s0 = store.get(0).unwrap();
        assert_eq!(s0.meta["bg_seed"], json!(7));
        assert_eq!(s0.meta["bg_col"], json!(1));
        assert_eq!(s0.wpa["value"], json!(0.5));
        assert_eq!(s0.chosen_action["label"], json!("go"));
        let s1 = store.get(1).unwrap();
        assert_eq!(s1.meta["floor"], json!(0), "floor carried forward");
        assert_eq!(s1.scores[0]["win_prob"], json!(0.75));
        assert_eq!(s1.pv, json!(["a"]));
        assert!(store
            .get(2)
            .unwrap_err()
            .contains("step 2 out of range (0..1)"));
        assert_eq!(store.run_identity().unwrap().character, "Silent");
        assert_eq!(store.replay_line(1).unwrap().len(), 1);
        assert_eq!(resume_capability(&store)["kind"], json!("run"));
        let logs = vec![("x.jsonl".to_string(), text.clone())];
        let runs = mcts_runs(&logs);
        assert_eq!(runs["runs"][0]["stem"], json!("x"));
        assert_eq!(runs["runs"][0]["turn1_q"], json!(0.2));
        let agg = aggregate_runs(&logs);
        assert_eq!(agg["n_losses"], json!(1));
        assert_eq!(agg["final_turn_histogram"], json!({"3": 1}));
        assert_eq!(agg["pattern_tags"], json!({"fast_death": 1}));
        assert!(ReplayStore::parse("y", "{not json").is_err());
    }

    /// The live agent's own rows carry `action` and the search's `win_prob`.
    /// Re-deriving `wins / visits` here would quietly regrade the agent's own
    /// numbers, which is the whole point of persisting them.
    #[test]
    fn agent_rows_keep_their_own_label_and_win_prob() {
        let text = [
            r#"{"step":0,"ts":"t","type":"meta","data":{"character":"Ironclad","seed":"S","ascension":0,"run_mode":true,"act1":"overgrowth","backend":"rust"}}"#,
            r#"{"step":1,"ts":"t","type":"state","data":{"decision":"combat_play","round":1}}"#,
            r#"{"step":1,"ts":"t","type":"mcts","data":{"source":"agent","root_outcomes":[{"stable_idx":0,"action":"Strike","action_label":"Strike","win_prob":0.62,"wins":3,"visits":4,"q":0.2,"is_best":true}]}}"#,
            r#"{"step":1,"ts":"t","type":"meta","data":{"summary":true,"final_won":true,"final_turn":2}}"#,
            "",
        ]
        .join("\n");
        let store = ReplayStore::parse("a.jsonl", &text).unwrap();
        let s0 = store.get(0).unwrap();
        // 0.62 as recorded, NOT wins/visits = 0.75.
        assert_eq!(s0.scores[0]["win_prob"], json!(0.62));
        assert_eq!(s0.scores[0]["action"], json!("Strike"));
    }
}
