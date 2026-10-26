//! Frozen adversarial-branching checkpoint observations, built from native state.
//! The Python simulator is used only to export schema templates/import fixtures.
//! No Python objects, JSON serialization or RNG fields occur in the actor loop.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use rayon::prelude::*;
use serde_json::{json, Value};
use sts2_core::cards::{effective_target_type, Card, PileKind, TargetType};
use sts2_core::event_log::EventLogEntry;
use sts2_core::{Action, CombatState, CreatureUid, Intent, PowerUid};

use crate::PyCombatState;

type Result<T> = std::result::Result<T, String>;
type Rows = Vec<(String, Value)>;
#[cfg(feature = "native-gpu-feeder")]
pub mod closed_loop;
mod fast_flat;
mod native_arrays;
mod numeric;
mod reward;
pub(crate) mod worker_queue;

#[derive(Debug, Default)]
struct Node {
    names: HashMap<String, usize>,
    indices: HashMap<usize, usize>,
    id: Option<u32>,
}

#[derive(Debug)]
struct Schema {
    numeric: numeric::Catalog,
    trie: Vec<Node>,
    templates: Value,
}

impl Schema {
    fn new(labels: &[Value], templates: Value) -> Result<Self> {
        let mut trie = vec![Node::default()];
        for (id, label) in labels.iter().enumerate() {
            let mut node = 0;
            for part in label.as_array().ok_or("registry labels must be arrays")? {
                let found = if let Some(s) = part.as_str() {
                    trie[node].names.get(s).copied()
                } else {
                    trie[node]
                        .indices
                        .get(&(part.as_u64().ok_or("bad path component")? as usize))
                        .copied()
                };
                let next = found.unwrap_or_else(|| {
                    let next = trie.len();
                    trie.push(Node::default());
                    if let Some(s) = part.as_str() {
                        trie[node].names.insert(s.to_owned(), next);
                    } else {
                        trie[node]
                            .indices
                            .insert(part.as_u64().unwrap() as usize, next);
                    }
                    next
                });
                node = next;
            }
            if trie[node].id.replace(id as u32).is_some() {
                return Err("duplicate registry label".into());
            }
        }
        Ok(Self {
            numeric: numeric::Catalog::new(&templates),
            trie,
            templates,
        })
    }

    fn name(&self, node: usize, name: &str) -> Result<usize> {
        self.trie[node].names.get(name).copied().ok_or_else(|| {
            format!("checkpoint vocabulary has no path component {name:?} below trie node {node}")
        })
    }
    fn id(&self, node: usize) -> Result<u32> {
        self.trie[node]
            .id
            .ok_or_else(|| format!("incomplete checkpoint field at node {node}"))
    }
}

// Used only when an observation encounters a field absent from the saved
// registry. The actor's normal encoder remains immutable and allocation-light.
fn missing_labels(schema: &Schema, rows: &Rows) -> Vec<Value> {
    fn atom(path: &[Value], kind: &str, out: &mut Vec<Vec<Value>>) {
        let mut label = path.to_vec();
        label.push(json!(kind));
        out.push(label);
    }
    fn visit(value: &Value, path: &mut Vec<Value>, out: &mut Vec<Vec<Value>>) {
        match value {
            Value::Null => atom(path, "null", out),
            Value::Bool(_) => atom(path, "bool", out),
            Value::Number(n) => {
                if let Some(n) = n.as_i64() {
                    if n.unsigned_abs() <= 1 << 24 {
                        atom(path, "number", out);
                    } else {
                        atom(path, "integer_sign", out);
                        let mut n = n.unsigned_abs();
                        let mut i = 0;
                        while n > 0 {
                            path.extend([json!("limb"), json!(i)]);
                            atom(path, "integer_limb", out);
                            path.truncate(path.len() - 2);
                            n /= 65536;
                            i += 1;
                        }
                    }
                } else {
                    atom(path, "float", out);
                }
            }
            Value::String(text) => {
                path.extend([json!("category"), json!(text)]);
                atom(path, "category", out);
                path.truncate(path.len() - 2);
            }
            Value::Array(values) => {
                atom(path, "list_length", out);
                for (i, v) in values.iter().enumerate() {
                    path.push(json!(i));
                    visit(v, path, out);
                    path.pop();
                }
            }
            Value::Object(values) => {
                if values.is_empty() {
                    atom(path, "empty_dict", out);
                }
                for (k, v) in values {
                    path.push(json!(k));
                    visit(v, path, out);
                    path.pop();
                }
            }
        }
    }
    fn record(section: &str, value: &Value, out: &mut Vec<Vec<Value>>) {
        out.push(vec![json!("entity"), json!(section)]);
        visit(value, &mut vec![json!("field"), json!(section)], out);
    }
    let mut labels = Vec::new();
    for (name, value) in rows {
        if name == "actions" {
            for action in value.as_array().into_iter().flatten() {
                record("candidate", action, &mut labels);
            }
            continue;
        }
        if let Some(items) = value.as_array().filter(|a| !a.is_empty()) {
            record(&format!("{name}_count"), &json!(items.len()), &mut labels);
            for item in items {
                record(name, item, &mut labels);
            }
        } else {
            record(name, value, &mut labels);
        }
    }
    let mut unique = std::collections::HashSet::new();
    let mut missing = Vec::new();
    for label in labels {
        let mut at = Some(0);
        for part in &label {
            at = at.and_then(|node| {
                if let Some(s) = part.as_str() {
                    schema.trie[node].names.get(s).copied()
                } else {
                    schema.trie[node]
                        .indices
                        .get(&(part.as_u64().unwrap_or(0) as usize))
                        .copied()
                }
            });
        }
        if at.and_then(|node| schema.trie[node].id).is_none() {
            let key = serde_json::to_string(&label).expect("finite schema label");
            if unique.insert(key) {
                missing.push(json!(label));
            }
        }
    }
    missing
}

#[derive(Debug, Default, Clone)]
struct Observation {
    kinds: Vec<u32>,
    ids: Vec<u32>,
    values: Vec<f32>,
    offsets: Vec<u32>,
    actions: Vec<u32>,
}

impl Observation {
    fn atom(&mut self, schema: &Schema, node: usize, kind: &str, value: f32) -> Result<()> {
        self.ids.push(schema.id(schema.name(node, kind)?)?);
        self.values.push(value);
        Ok(())
    }
    fn visit(&mut self, schema: &Schema, node: usize, value: &Value) -> Result<()> {
        match value {
            Value::Null => self.atom(schema, node, "null", 1.),
            Value::Bool(v) => self.atom(schema, node, "bool", f32::from(*v)),
            Value::Number(v) => {
                if let Some(n) = v.as_i64() {
                    if n.unsigned_abs() <= (1 << 24) {
                        self.atom(schema, node, "number", n as f32)
                    } else {
                        self.atom(schema, node, "integer_sign", if n >= 0 { 1. } else { -1. })?;
                        let mut n = n.unsigned_abs();
                        let mut i = 0;
                        let limbs = schema.name(node, "limb")?;
                        while n > 0 {
                            let at = *schema.trie[limbs]
                                .indices
                                .get(&i)
                                .ok_or("unseen integer limb")?;
                            self.atom(schema, at, "integer_limb", (n % 65536) as f32)?;
                            n /= 65536;
                            i += 1;
                        }
                        Ok(())
                    }
                } else {
                    let n = v.as_f64().ok_or("invalid numeric feature")?;
                    if !n.is_finite() || f64::from(n as f32).to_bits() != n.to_bits() {
                        return Err("non-exact float32 feature needs limb encoding".into());
                    }
                    self.atom(schema, node, "float", n as f32)
                }
            }
            Value::String(v) => {
                let category = schema.name(node, "category")?;
                self.atom(schema, schema.name(category, v)?, "category", 1.)
            }
            Value::Array(items) => {
                self.atom(schema, node, "list_length", items.len() as f32)?;
                for (i, item) in items.iter().enumerate() {
                    let child = *schema.trie[node]
                        .indices
                        .get(&i)
                        .ok_or_else(|| format!("unseen list index {i} at {node}"))?;
                    self.visit(schema, child, item)?;
                }
                Ok(())
            }
            Value::Object(items) => {
                if items.is_empty() {
                    self.atom(schema, node, "empty_dict", 1.)?;
                }
                // serde_json's default object map is key-sorted, like Writer.visit.
                for (k, v) in items {
                    self.visit(schema, schema.name(node, k)?, v)?;
                }
                Ok(())
            }
        }
    }
    fn record(&mut self, schema: &Schema, section: &str, value: &Value) -> Result<()> {
        let entity = schema.name(schema.name(0, "entity")?, section)?;
        self.kinds.push(schema.id(entity)?);
        let node = schema.name(schema.name(0, "field")?, section)?;
        self.visit(schema, node, value)
            .map_err(|e| format!("{section}: {e}"))?;
        self.offsets.push(self.ids.len() as u32);
        Ok(())
    }
    fn encode(schema: &Schema, rows: &Rows) -> Result<Self> {
        let out = Self::encode_state(schema, rows)?;
        if out.actions.is_empty() {
            return Err("empty legal action set".into());
        }
        Ok(out)
    }
    // Terminal value/transition targets legitimately have no candidate action.
    fn encode_state(schema: &Schema, rows: &Rows) -> Result<Self> {
        let mut out = Self {
            offsets: vec![0],
            ..Self::default()
        };
        let mut actions = None;
        for (name, value) in rows {
            if name == "actions" {
                actions = Some(value);
                continue;
            }
            if let Some(items) = value.as_array().filter(|a| !a.is_empty()) {
                out.record(schema, &format!("{name}_count"), &json!(items.len()))?;
                for item in items {
                    out.record(schema, name, item)?;
                }
            } else {
                out.record(schema, name, value)?;
            }
        }
        for action in actions.and_then(Value::as_array).ok_or("missing actions")? {
            out.actions.push(out.kinds.len() as u32);
            out.record(schema, "candidate", action)?;
        }
        Ok(out)
    }
}

#[derive(Debug, Clone)]
enum Command {
    Core(Action),
    Pick(u8),
    Confirm,
}

#[derive(Debug, Clone)]
struct Session {
    reward: reward::Context,
    numeric_cache: numeric::Cache,
    native_sidecars: Vec<[i32; 12]>,
    fast_cache: fast_flat::Cache,
    state: CombatState,
    context: Arc<Value>,
    decisions: usize,
    initial_hp: i32,
    initial_costs: Arc<HashMap<sts2_core::CardUid, i8>>,
    commands: Vec<Command>,
    selected: Vec<u8>,
    played_turn: Vec<Value>,
    played_counts: BTreeMap<String, u32>,
    drawn: u32,
    exhausted: u32,
    discarded: u32,
    damage_events: u32,
    history_round: u16,
    log_cursor: usize,
    last_potion: Option<String>,
    opening_prompt: Option<String>,
    max_actions: usize,
}

fn legacy_monster_id(name: &str) -> String {
    let mut out = String::new();
    let mut previous = ' ';
    for c in name.chars() {
        if c.is_uppercase() && (previous.is_lowercase() || previous.is_ascii_digit()) {
            out.push('_');
        }
        out.extend(c.to_uppercase());
        previous = c;
    }
    out
}

fn old_move(schema: &Schema, monster: &str, id: &str) -> Result<String> {
    if id == "STUNNED" {
        return Ok("STUNNED".into());
    }
    schema.templates["moves"][monster][id]
        .as_str()
        .map(str::to_owned)
        .map_or_else(|| Ok(format!("rust_move:{id}")), Ok)
}

fn template<'a>(schema: &'a Schema, group: &str, name: &str) -> Result<&'a Value> {
    schema.templates[group]
        .get(name)
        .ok_or_else(|| format!("missing {group} encoding template: {name}"))
}

fn one_counter(state: &Value) -> Option<String> {
    let mut fields = state
        .as_object()?
        .iter()
        .filter(|(k, v)| k.as_str() != "amount" && v.is_i64());
    let key = fields.next()?.0.clone();
    if fields.next().is_none() {
        Some(key)
    } else {
        None
    }
}

fn structural_cmp(a: &Value, b: &Value) -> Ordering {
    let kind = |v: &Value| match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    };
    let k = kind(a).cmp(kind(b));
    if k != Ordering::Equal {
        return k;
    }
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            let mut a = a.iter();
            let mut b = b.iter();
            loop {
                match (a.next(), b.next()) {
                    (Some((ka, va)), Some((kb, vb))) => {
                        let c = ka.cmp(kb).then_with(|| structural_cmp(va, vb));
                        if c != Ordering::Equal {
                            return c;
                        }
                    }
                    (None, None) => return Ordering::Equal,
                    (None, Some(_)) => return Ordering::Less,
                    _ => return Ordering::Greater,
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            for (a, b) in a.iter().zip(b) {
                let c = structural_cmp(a, b);
                if c != Ordering::Equal {
                    return c;
                }
            }
            a.len().cmp(&b.len())
        }
        (Value::String(a), Value::String(b)) => a.cmp(b),
        (Value::Number(a), Value::Number(b)) => a
            .as_f64()
            .partial_cmp(&b.as_f64())
            .unwrap_or(Ordering::Equal),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        _ => Ordering::Equal,
    }
}

fn indexed_records(items: &[Value], context: &Value) -> Value {
    let mut groups: Vec<Value> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if let Some(row) = groups.iter_mut().find(|r| r["item"] == *item) {
            row["positions"].as_array_mut().unwrap().push(json!(i));
        } else {
            let mut row = context.clone();
            row["item"] = item.clone();
            row["positions"] = json!([i]);
            groups.push(row);
        }
    }
    json!(groups)
}

fn restore_opening_choice(state: &mut CombatState, context: &Value) -> Result<()> {
    let choice = &context["_opening_choice"];
    if choice.is_null() {
        return Ok(());
    }
    let name = choice["kind"]
        .as_str()
        .ok_or("missing opening choice kind")?;
    let kind = match name {
        "discard_from_hand_and_draw" => sts2_core::ChoiceKind::DiscardFromHandAndDraw,
        "add_generated_to_hand" => sts2_core::ChoiceKind::AddGeneratedToHand,
        "add_generated_retained_to_hand" => sts2_core::ChoiceKind::AddGeneratedRetainedToHand,
        "add_generated_free_to_hand" => sts2_core::ChoiceKind::AddGeneratedFreeToHand,
        "transform_draw_card_to_soul" => sts2_core::ChoiceKind::TransformDrawCardToSoul,
        "transfigure_card_in_hand" => sts2_core::ChoiceKind::TransfigureCardInHand,
        "exhaust_from_hand" => sts2_core::ChoiceKind::ExhaustFromHand,
        _ => return Err(format!("unsupported opening choice: {name}")),
    };
    let mut options = Vec::new();
    for row in choice["options"]
        .as_array()
        .ok_or("missing opening options")?
    {
        let id = row["card_id"].as_str().ok_or("missing choice card id")?;
        let card_id = sts2_wire::state_dict::card_id_from_python_name(id)
            .ok_or_else(|| format!("unknown choice card {id}"))?;
        let card_uid = row["uid"]
            .as_u64()
            .map(|u| sts2_core::CardUid::new(u as u32).ok_or("zero choice uid"))
            .transpose()?;
        if card_uid.is_some_and(|u| sts2_core::card_by_uid(&state.cards, u).is_none()) {
            return Err("missing imported choice instance".into());
        }
        options.push(sts2_core::ChoiceOption {
            card_id,
            card_uid,
            upgraded: row["upgraded"].as_bool().unwrap_or(false),
            source_pile: match row["source"].as_str() {
                Some("hand") => Some(PileKind::Hand),
                Some("draw") => Some(PileKind::Draw),
                Some("discard") => Some(PileKind::Discard),
                Some("exhaust") => Some(PileKind::Exhaust),
                _ => None,
            },
        });
    }
    state.pending_choice = Some(sts2_core::PendingChoice {
        kind,
        options,
        min_select: choice["min"].as_u64().ok_or("missing min")? as u8,
        max_select: choice["max"].as_u64().ok_or("missing max")? as u8,
        can_skip: choice["allow_skip"].as_bool().unwrap_or(false),
        post_effect: choice["strength"]
            .as_i64()
            .map(|v| sts2_core::PostChoiceEffect::GainStrength(v as i32)),
        ..Default::default()
    });
    if let Some(post) = choice.get("post_effect").filter(|p| !p.is_null()) {
        let amount = post["amount"]
            .as_i64()
            .ok_or("missing post-choice amount")? as i32;
        let effect = match post["kind"].as_str() {
            Some("draw") => sts2_core::PostChoiceEffect::DrawCards(amount),
            Some("strength") => sts2_core::PostChoiceEffect::GainStrength(amount),
            Some("power") => sts2_core::PostChoiceEffect::ApplyPlayerPower(
                sts2_wire::state_dict::power_kind_from_python_name(
                    post["power"].as_str().ok_or("missing post-choice power")?,
                )
                .ok_or("unknown post-choice power")?,
                amount,
            ),
            _ => return Err("unknown post-choice effect".into()),
        };
        state
            .pending_choice
            .as_mut()
            .ok_or("opening choice missing")?
            .post_effect = Some(effect);
    }
    if choice["source_card"].is_string() {
        state.phase = sts2_core::CombatPhase::PlayerPlayPhase;
        return Ok(());
    }
    state.phase = sts2_core::CombatPhase::PlayerTurnStart;
    if kind == sts2_core::ChoiceKind::AddGeneratedToHand {
        state.pending_relic_hand_draw_resume = Some(
            choice["draw_count"]
                .as_i64()
                .ok_or("missing deferred opening draw")? as i32,
        );
    } else {
        // The archived Python hook bus already visited all player-turn-start
        // relics before surfacing its choice. Resume after that bus, rather
        // than firing those relics a second time.
        state.pending_cluster_resume = Some(sts2_core::PendingClusterResume {
            relic_start: Some(state.relics.len() as u8),
            ..Default::default()
        });
    }
    Ok(())
}

fn combat_score(s: &Session) -> f32 {
    let state = &s.state;
    let hp = if !state.is_over && s.done() {
        0.0
    } else {
        (state.player.current_hp as f32 / state.player.max_hp.max(1) as f32).clamp(0.0, 1.0)
    };
    let mut maximum = 0i64;
    let mut remaining = 0i64;
    for uid in &state.enemy_uids {
        if let Some(enemy) = state.creatures.iter().find(|c| c.id == uid.0) {
            maximum += i64::from(enemy.max_hp.max(0));
            let doom = enemy
                .powers
                .iter()
                .filter_map(|u| state.powers.get_by_id(u.0))
                .find(|p| p.kind == sts2_core::powers::PowerKind::Doom)
                .map_or(0, |p| p.amount.max(0));
            remaining += i64::from((enemy.current_hp.min(enemy.max_hp) - doom).max(0));
        }
    }
    let damage = if state.is_over && state.player_won && state.player.current_hp > 0 {
        1.0
    } else {
        1.0 - (remaining as f32 / maximum.max(1) as f32).clamp(0.0, 1.0)
    };
    hp + damage
}

fn restore_snapshot_metadata(state: &mut CombatState, context: &Value) -> Result<()> {
    use sts2_core::cards::KeywordOverride as K;
    for card in &mut state.cards {
        let data = &context["_card_import"][card.uid.get().to_string()];
        if let Some(names) = data["keywords"].as_array() {
            let mut bits = 0;
            for name in names {
                bits |= match name.as_str() {
                    Some("ethereal") => K::ETHEREAL.0,
                    Some("unplayable") => K::UNPLAYABLE.0,
                    Some("retain") => K::RETAIN.0,
                    Some("exhaust") => K::EXHAUST.0,
                    Some("innate") => K::INNATE.0,
                    Some("eternal") => K::ETERNAL.0,
                    Some("sly") => 1 << 5,
                    _ => 0,
                };
            }
            card.keyword_override = Some(K(bits));
        }
        if let Some(count) = data["base_replay_count"].as_i64() {
            card.base_replay_count = count as i32;
        }
    }
    for relic in &mut state.relics {
        if let Some(row) = context["relics"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["id"] == relic.kind.python_name()))
        {
            relic.is_melted = !row["state"]["enabled"].as_bool().unwrap_or(true);
            if let Some(used) = row["state"]["_used_this_combat"].as_bool() {
                relic.counter = i32::from(used);
            }
        }
    }
    if let Some(potions) = context["potions"].as_array() {
        state.player.potion_slots = potions.len() as u8;
        state.player.potion_slot_indices = potions
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.is_null())
            .map(|(i, _)| i as u8)
            .collect();
        if state.player.potions.len() != state.player.potion_slot_indices.len() {
            return Err("potion slot restoration mismatch".into());
        }
    }
    let mut groups = vec![(
        state.player.creature_uid,
        context["player"]["statuses"].clone(),
    )];
    groups.push((state.player.pet_uid, context["osty"]["statuses"].clone()));
    for (i, uid) in state.enemy_uids.iter().enumerate() {
        groups.push((Some(*uid), context["enemies"][i]["statuses"].clone()));
    }
    for (owner, rows) in groups {
        let Some(owner) = owner else { continue };
        let Some(rows) = rows.as_array() else {
            continue;
        };
        for row in rows {
            let kind = sts2_wire::state_dict::power_kind_from_python_name(
                row["id"].as_str().ok_or("missing imported power id")?,
            )
            .ok_or("unknown imported power")?;
            let Some(creature) = state.creatures.iter().find(|c| c.id == owner.0) else {
                continue;
            };
            let uid = creature
                .powers
                .iter()
                .copied()
                .find(|u| state.powers.get_by_id(u.0).is_some_and(|p| p.kind == kind));
            let Some(uid) = uid else {
                return Err(format!("missing imported power {}", row["id"]));
            };
            let applier = row["state"]["applier"]["creature_ref"]
                .as_str()
                .and_then(|name| match name {
                    "PLAYER" => state.player.creature_uid,
                    "OSTY" => state.player.pet_uid,
                    _ => state
                        .creatures
                        .iter()
                        .find(|c| legacy_monster_id(c.monster_id.0) == name)
                        .map(|c| CreatureUid(c.id)),
                });
            let power = state
                .powers
                .get_mut_by_id(uid.0)
                .ok_or("missing power arena entry")?;
            power.applier = applier;
            power.skip_next_tick = row["state"]["skip_next_tick"].as_bool().unwrap_or(false);
        }
    }
    Ok(())
}

impl Session {
    fn refresh_commands(&mut self) {
        self.commands.clear();
        if let Some(pc) = &self.state.pending_choice {
            if self.selected.len() >= usize::from(pc.min_select)
                || (pc.can_skip && self.selected.is_empty())
            {
                self.commands.push(Command::Confirm);
            }
            if self.selected.len() < usize::from(pc.max_select) {
                for i in 0..pc.options.len() {
                    if !self.selected.contains(&(i as u8)) {
                        self.commands.push(Command::Pick(i as u8));
                    }
                }
            }
        } else {
            let mut legal = sts2_core::legal_actions(&self.state);
            legal.sort_by_key(|a| u8::from(!matches!(a, Action::EndTurn)));
            self.commands.extend(legal.into_iter().map(Command::Core));
        }
    }
    fn card(&self, schema: &Schema, card: &Card) -> Result<Value> {
        let key = format!(
            "{}:{}",
            card.card_id.to_python_name(),
            u8::from(card.upgraded)
        );
        let base = template(schema, "cards", &key)?;
        let reference = &base["reference"];
        let mut row = base["record"].clone();
        let mut modifiers = json!({"upgraded":card.upgraded,"enchantments":{}});
        let mut enchantments = json!({});
        for (id, amount) in &card.enchantments {
            enchantments[&legacy_potion_id(id.python_name())] = json!(amount);
        }
        modifiers["enchantments"] = enchantments;
        let damage = if card.base_damage == 0 && reference["base_damage"].is_null() {
            Value::Null
        } else {
            json!(card.base_damage)
        };
        let block = if card.base_block == 0 && reference["base_block"].is_null() {
            Value::Null
        } else {
            json!(card.base_block)
        };
        let mut effects = reference["effect_vars"].clone();
        if effects.get("damage").is_some() {
            effects["damage"] = damage.clone();
        }
        if effects.get("block").is_some() {
            effects["block"] = block.clone();
        }
        let instance = &self.context["_card_instances"][card.uid.get().to_string()];
        let mut vars = instance
            .get("combat_vars")
            .cloned()
            .unwrap_or_else(|| json!({}));
        for (key, value) in &card.combat_vars {
            let name = sts2_wire::state_dict::var_key_wire_name(*key);
            if schema.templates["public_combat_var_names"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(name)))
            {
                vars[name] = json!(value);
            }
        }
        use sts2_core::cards::VarKey;
        if card.combat_var(VarKey::FreeThisTurn) != 0 {
            vars["_turn_cost_override"] = json!(0);
            vars["_turn_star_cost_override"] = json!(0);
        } else if card.combat_var(VarKey::WireTransientCostThisTurn) != 0 {
            vars["_turn_cost_override"] = json!(card.cost);
        } else if !(self.state.round
            == self.context["combat"]["round"].as_u64().unwrap_or(0) as u16
            && vars.get("_turn_cost_override").is_some())
        {
            vars.as_object_mut().unwrap().remove("_turn_cost_override");
            vars.as_object_mut()
                .unwrap()
                .remove("_turn_star_cost_override");
        }
        if card.combat_var(VarKey::StarCostFreeThisCombat) != 0 {
            vars["_combat_star_cost_override"] = json!(0);
        }
        if card.combat_var(VarKey::ReturnIfPlayedRound) != 0
            || vars.get("_return_before_hand_draw_round").is_some()
        {
            vars["_return_before_hand_draw_round"] =
                json!(card.combat_var(VarKey::ReturnIfPlayedRound) + 1);
        }
        if let Some(affliction) = card.affliction {
            vars["_affliction"] = json!(sts2_wire::state_dict::card_affliction_kind_wire_name(
                affliction.kind
            )
            .to_lowercase());
        } else {
            vars.as_object_mut().unwrap().remove("_affliction");
            vars.as_object_mut().unwrap().remove("_smoggy");
        }
        let mut keywords = instance
            .get("keywords")
            .cloned()
            .unwrap_or_else(|| reference["keywords"].clone());
        if card.keyword_override.is_some() {
            let mut names = Vec::new();
            for (enabled, name) in [
                (
                    sts2_core::card_effects::card_ethereal_inst(card),
                    "ethereal",
                ),
                (
                    sts2_core::card_effects::card_unplayable_inst(card),
                    "unplayable",
                ),
                (sts2_core::card_effects::card_innate_inst(card), "innate"),
                (sts2_core::card_effects::card_retain_inst(card), "retain"),
                (sts2_core::card_effects::card_exhausts_inst(card), "exhaust"),
                (
                    card.keyword_override
                        .is_some_and(|k| k.contains(sts2_core::cards::KeywordOverride::ETERNAL)),
                    "eternal",
                ),
                (
                    card.keyword_override.is_some_and(|k| k.0 & (1 << 5) != 0),
                    "sly",
                ),
            ] {
                if enabled {
                    names.push(name);
                }
            }
            names.sort_unstable();
            keywords = json!(names);
        }
        let fields = [
            ("cost", json!(card.cost)),
            (
                "original_cost",
                json!(self
                    .initial_costs
                    .get(&card.uid)
                    .copied()
                    .unwrap_or(card.cost)),
            ),
            ("base_damage", damage.clone()),
            ("base_block", block.clone()),
            ("effect_vars", effects.clone()),
            ("combat_vars", vars.clone()),
            ("keywords", keywords.clone()),
            ("base_replay_count", json!(card.base_replay_count)),
            (
                "bound",
                json!(card.affliction.is_some_and(
                    |a| a.kind == sts2_core::card_afflictions::CardAfflictionKind::Bound
                )),
            ),
            (
                "single_turn_retain",
                json!(self.state.player.retain_this_turn.contains(&card.uid)),
            ),
        ];
        for (key, value) in fields {
            if reference[key] != value {
                modifiers[key] = value;
            }
        }
        row["$mod"] = modifiers;
        row["cost"] = json!(sts2_core::hooks::effective_card_cost(&self.state, card.uid));
        row["star_cost"] = json!(sts2_core::effective_star_cost_in_combat(&self.state, card));
        row["damage"] = damage;
        row["block"] = block;
        if let Some(value) = instance.get("has_energy_cost_x") {
            row["has_energy_cost_x"] = value.clone();
        }
        if let Some(value) = instance.get("has_star_cost_x") {
            row["has_star_cost_x"] = value.clone();
        }
        row["type"] = json!(format!("{:?}", card.card_type).to_uppercase());
        row["target"] = json!(match effective_target_type(&self.state, card) {
            TargetType::Enemy => "ANY_ENEMY",
            TargetType::AllEnemies => "ALL_ENEMIES",
            TargetType::Self_ => "SELF",
            TargetType::None => "NONE",
            TargetType::Random => "RANDOM_ENEMY",
        });
        for (key, value) in [
            ("effect_vars", effects),
            ("combat_vars", vars),
            ("keywords", keywords),
        ] {
            let nonempty = match &value {
                Value::Object(v) => !v.is_empty(),
                Value::Array(v) => !v.is_empty(),
                _ => false,
            };
            if nonempty {
                row[key] = value;
            } else {
                row.as_object_mut().unwrap().remove(key);
            }
        }
        Ok(row)
    }

    fn statuses(&self, schema: &Schema, uids: &[PowerUid]) -> Result<Value> {
        let mut rows = Vec::new();
        for uid in uids {
            let p = self
                .state
                .powers
                .iter()
                .find(|p| p.id == uid.0)
                .ok_or("missing power UID")?;
            let name = p.kind.python_name();
            let mut state = template(schema, "powers", name)?.clone();
            if state.get("_first_round_block_given").is_some() {
                state["_first_round_block_given"] = json!(p.counter != 0);
            }
            state["amount"] = json!(p.amount);
            state["skip_next_tick"] = json!(p.skip_next_tick);
            state["applier"] = if let Some(uid) = p.applier {
                let id = if self.state.player.creature_uid == Some(uid) {
                    "PLAYER"
                } else if self.state.player.pet_uid == Some(uid) {
                    "OSTY"
                } else {
                    self.state
                        .creatures
                        .iter()
                        .find(|c| c.id == uid.0)
                        .map_or("UNKNOWN", |c| c.monster_id.0)
                };
                json!({"creature_ref":legacy_monster_id(id)})
            } else {
                Value::Null
            };
            if let Some(key) = one_counter(&state) {
                state[&key] = json!(p.counter);
            }
            rows.push(json!({"id":name,"state":state}));
        }
        rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        Ok(json!(rows))
    }
    fn creature(&self, schema: &Schema, uid: CreatureUid, id: &str) -> Result<Value> {
        let c = self
            .state
            .creatures
            .iter()
            .find(|c| c.id == uid.0)
            .ok_or("missing creature")?;
        Ok(
            json!({"id":id,"hp":c.current_hp,"max_hp":c.max_hp,"block":c.block,"alive":c.current_hp>0,"stars":0,"statuses":self.statuses(schema,&c.powers)?}),
        )
    }
    fn prompt(&self, schema: &Schema) -> String {
        if self.state.pending_choice.is_none() {
            return String::new();
        }
        if let Some(prompt) = &self.opening_prompt {
            return prompt.clone();
        }
        if let Some(id) = &self.last_potion {
            if let Some(prompt) = schema.templates["potion_prompts"][id].as_str() {
                return prompt.to_owned();
            }
        }
        if let Some(card) = self
            .state
            .player
            .play
            .last()
            .and_then(|uid| sts2_core::card_by_uid(&self.state.cards, *uid))
        {
            if let Some(prompt) =
                schema.templates["choice_prompts"][card.card_id.to_python_name()].as_str()
            {
                return prompt.to_owned();
            }
        }
        for event in self.state.combat_log.event_log.rows().iter().rev() {
            let card = match event.source {
                Some(sts2_core::event_log::LogSource::Card { card, .. }) => Some(card),
                _ => match event.entry {
                    EventLogEntry::CardPlayStarted { card, .. } => Some(card),
                    _ => None,
                },
            };
            if let Some(card) = card {
                if let Some(prompt) =
                    schema.templates["choice_prompts"][card.to_python_name()].as_str()
                {
                    return prompt.to_owned();
                }
            }
        }
        let name = self.state.pending_choice.as_ref().map_or("", |p| {
            sts2_wire::emit::names::choice_kind_python_name(p.kind)
        });
        let prompt = match name {
            "exhaust_from_hand" => "Choose a hand card to exhaust",
            "discard_from_hand" | "discard_from_hand_and_draw" => "Choose a hand card to discard",
            "add_from_discard_free" => "Choose a discard card to return to hand",
            "add_from_draw_to_hand" => "Choose a card from draw pile",
            "upgrade_in_hand" => "Choose a hand card to upgrade",
            "monster_curse_select" => "Curse of Knowledge",
            _ => return format!("rust_choice:{name}"),
        };
        prompt.to_owned()
    }

    fn project(&mut self, schema: &Schema) -> Result<Rows> {
        self.project_mode(schema, false)
    }
    fn project_mode(&mut self, schema: &Schema, fast: bool) -> Result<Rows> {
        let s = &self.state;
        let p = &s.player;
        let mut rows = Vec::new();
        if fast {
            rows.push(("combat".into(), Value::Null));
        } else {
            rows.push(("combat".into(),json!({"turn":s.round,"round":s.round,"decision":self.decisions,"phase":match s.phase {sts2_core::CombatPhase::PlayerTurnStart=>"player_turn_start",sts2_core::CombatPhase::EnemyTurnStart|sts2_core::CombatPhase::EnemyTurnExec|sts2_core::CombatPhase::EnemyTurnEnd=>"enemy_turn",sts2_core::CombatPhase::PlayerEndTurnPhaseOne=>"player_before_turn_end",sts2_core::CombatPhase::PlayerEndTurnPhaseTwo=>"player_turn_end_in_hand",_=>"player_turn"},"energy":p.energy,"max_energy":p.max_energy,"stars":p.stars,"gold":s.gold,"ascension":s.ascension})));
        }
        if fast {
            rows.push(("player".into(), Value::Null));
        } else {
            rows.push(("player".into(),json!({"id":"NECROBINDER","hp":p.current_hp,"max_hp":p.max_hp,"block":p.block,"alive":p.current_hp>0,"stars":p.stars,"statuses":self.statuses(schema,&p.powers)?})));
        }
        rows.push((
            "osty".into(),
            match p.pet_uid {
                Some(uid) => self.creature(schema, uid, "OSTY")?,
                None => Value::Null,
            },
        ));
        let mut enemies = Vec::new();
        let mut moves = Vec::new();
        for (i, uid) in s.enemy_uids.iter().enumerate() {
            let c = s
                .creatures
                .iter()
                .find(|c| c.id == uid.0)
                .ok_or("missing enemy")?;
            let mut row = self.creature(schema, *uid, &legacy_monster_id(c.monster_id.0))?;
            if let Some(ai) = &c.ai {
                let current = ai.current_move();
                let monster = legacy_monster_id(c.monster_id.0);
                let initial = self.context["_initial_moves"][i].as_str();
                let initial_name = self.context["enemies"][i]["move"].as_str();
                let move_name = if ai.move_history().is_empty() && initial == Some(current.id) {
                    initial_name
                        .map(str::to_owned)
                        .unwrap_or(old_move(schema, &monster, current.id)?)
                } else {
                    old_move(schema, &monster, current.id)?
                };
                row["move"] = json!(move_name);
                let history = ai
                    .move_history()
                    .iter()
                    .enumerate()
                    .map(|(at, id)| {
                        if at == 0 && initial == Some(*id) {
                            if let Some(name) = initial_name {
                                return Ok(json!(name));
                            }
                        }
                        old_move(schema, &monster, id).map(|v| json!(v))
                    })
                    .collect::<Result<Vec<_>>>()?;
                row["move_history"] =
                    json!({"record_section":"move_records","length":history.len()});
                moves.extend(
                    indexed_records(&history, &json!({"owner_index":i}))
                        .as_array()
                        .unwrap()
                        .iter()
                        .cloned(),
                );
                let mut intents = Vec::new();
                let mut telegraphs = Vec::new();
                if let Some(head) = c.intent {
                    telegraphs.push(head);
                }
                if c.intent.map(Intent::kind) == current.intents.first().copied().map(Intent::kind)
                {
                    telegraphs.extend(current.intents.iter().skip(1).copied());
                }
                for intent in telegraphs {
                    let (kind, damage, hits) = match intent {
                        Intent::Attack { damage } | Intent::DeathBlow { damage } => {
                            (intent.python_name(), damage, 1)
                        }
                        Intent::MultiAttack { damage, hits } => {
                            ("MULTI_ATTACK", damage, i32::from(hits))
                        }
                        _ => (intent.python_name(), 0, 1),
                    };
                    intents.push(json!({"intent_type":kind,"damage":damage,"hits":hits}));
                }
                if let Some(order) =
                    schema.templates["intent_order"][&monster][&move_name].as_array()
                {
                    let mut ordered = Vec::new();
                    for kind in order {
                        if let Some(at) = intents.iter().position(|row| row["intent_type"] == *kind)
                        {
                            ordered.push(intents.remove(at));
                        } else if kind == "CARD_DEBUFF" {
                            // The engine models this icon-only wire intent as
                            // Unknown. Keep the trained public icon vocabulary.
                            if let Some(at) = intents.iter().position(|row| {
                                row["intent_type"] == "UNKNOWN" || row["intent_type"] == "DEBUFF"
                            }) {
                                let mut intent = intents.remove(at);
                                intent["intent_type"] = kind.clone();
                                ordered.push(intent);
                            }
                        }
                    }
                    ordered.extend(intents);
                    intents = ordered;
                }
                row["intents"] = json!(intents);
            } else if let Some(original) = self.context["enemies"]
                .as_array()
                .and_then(|a| a.get(i))
                .filter(|r| r["id"] == row["id"])
            {
                for name in ["move", "intents", "move_history"] {
                    if let Some(value) = original.get(name) {
                        row[name] = value.clone();
                    }
                }
            }
            enemies.push(row);
        }
        rows.push(("enemies".into(), json!(enemies)));
        for (name, pile) in [
            ("draw_pile", p.draw.as_slice()),
            ("hand", p.hand.as_slice()),
            ("discard_pile", p.discard.as_slice()),
            ("exhaust_pile", p.exhaust.as_slice()),
            ("play_pile", p.play.as_slice()),
        ] {
            if fast {
                rows.push((name.into(), Value::Null));
                continue;
            }
            let mut cards = Vec::with_capacity(pile.len());
            for uid in pile {
                cards.push(self.card(
                    schema,
                    sts2_core::card_by_uid(&s.cards, *uid).ok_or("missing card UID")?,
                )?);
            }
            if name == "draw_pile" {
                cards.sort_by(structural_cmp);
            }
            rows.push((name.into(), json!(cards)));
        }
        let mut relics = Vec::new();
        for r in &s.relics {
            let name = r.kind.python_name();
            let original = self.context["relics"]
                .as_array()
                .and_then(|a| a.iter().find(|r| r["id"] == name));
            let mut state = if let Some(r) = original {
                r["state"].clone()
            } else {
                template(schema, "relics", name)?.clone()
            };
            state["enabled"] = json!(!r.is_melted);
            if let Some(key) = one_counter(&state) {
                state[&key] = json!(r.counter);
            }
            relics.push(json!({"id":name,"state":state}));
        }
        rows.push(("relics".into(), json!(relics)));
        let slots = self.context["potions"]
            .as_array()
            .map_or(usize::from(p.potion_slots), Vec::len);
        let mut potions = vec![Value::Null; slots];
        for (i, potion) in p.potions.iter().enumerate() {
            let slot = usize::from(*p.potion_slot_indices.get(i).unwrap_or(&(i as u8)));
            if slot >= potions.len() {
                return Err("potion slot outside inventory".into());
            }
            let native_name = potion.python_name();
            let name = legacy_potion_id(native_name);
            let original = self.context["potions"]
                .as_array()
                .and_then(|a| a.get(slot))
                .filter(|r| r["id"] == name);
            potions[slot] = if let Some(original) = original {
                original.clone()
            } else {
                template(schema, "potions", &name)?.clone()
            };
            potions[slot]["state"]["slot_index"] = json!(slot);
        }
        rows.push(("potions".into(), json!(potions)));
        rows.push(("orbs".into(), Value::Null));
        if fast {
            rows.push(("history".into(), Value::Null));
        } else {
            rows.push(("history".into(),json!({"played_this_turn":{"record_section":"played_turn_records","length":self.played_turn.len()},"played_combat_counts":self.played_counts,"drawn_this_turn":self.drawn,"exhausted_this_turn":self.exhausted,"discarded_this_turn":self.discarded,"damage_events_this_turn":self.damage_events,"energy_spent_this_turn":s.combat_log.energy_spent_this_turn})));
        }
        let mut choice_options = Vec::new();
        let prompt = self.prompt(schema);
        let choice = if let Some(pc) = &s.pending_choice {
            for (i, opt) in pc.options.iter().enumerate() {
                let card = if let Some(uid) = opt.card_uid {
                    self.card(
                        schema,
                        sts2_core::card_by_uid(&s.cards, uid).ok_or("missing choice card")?,
                    )?
                } else {
                    template(
                        schema,
                        "cards",
                        &format!(
                            "{}:{}",
                            opt.card_id.to_python_name(),
                            u8::from(opt.upgraded)
                        ),
                    )?["record"]
                        .clone()
                };
                choice_options
                    .push(json!({"index":i,"source":pile_name(opt.source_pile),"card":card}));
            }
            json!({"prompt":prompt,"min_choices":pc.min_select,"max_choices":pc.max_select,"allow_skip":pc.can_skip,"selected":self.selected,"options":{"record_section":"choice_options","length":pc.options.len()}})
        } else {
            Value::Null
        };
        rows.push(("pending_choice".into(), choice));
        rows.push(("phase".into(), json!("COMBAT")));
        let mut actions = Vec::new();
        let mut commands = Vec::new();
        if let Some(pc) = &s.pending_choice {
            if self.selected.len() >= usize::from(pc.min_select)
                || (pc.can_skip && self.selected.is_empty())
            {
                actions.push(json!({"action":"confirm_choice","prompt":prompt,"selected_count":self.selected.len()}));
                commands.push(Command::Confirm);
            }
            if self.selected.len() < usize::from(pc.max_select) {
                for (i, opt) in pc
                    .options
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !self.selected.contains(&(*i as u8)))
                {
                    let cost = opt
                        .card_uid
                        .and_then(|u| sts2_core::card_by_uid(&s.cards, u))
                        .map_or_else(
                            || {
                                template(
                                    schema,
                                    "cards",
                                    &format!(
                                        "{}:{}",
                                        opt.card_id.to_python_name(),
                                        u8::from(opt.upgraded)
                                    ),
                                )
                                .ok()
                                .and_then(|v| v["reference"]["cost"].as_i64())
                                .unwrap_or(0)
                            },
                            |c| i64::from(c.cost),
                        );
                    actions.push(json!({"action":"choose","index":i,"card_id":opt.card_id.to_python_name(),"cost":cost,"source_pile":pile_name(opt.source_pile),"prompt":prompt,"selected":false}));
                    commands.push(Command::Pick(i as u8));
                }
            }
        } else {
            let mut legal = sts2_core::legal_actions(s);
            legal.sort_by_key(|a| u8::from(!matches!(a, Action::EndTurn)));
            for action in legal {
                let row = match &action {
                    Action::EndTurn => json!({"action":"end_turn"}),
                    Action::PlayCard { hand_index, target } => {
                        let uid = p.hand[usize::from(*hand_index)];
                        let card =
                            sts2_core::card_by_uid(&s.cards, uid).ok_or("missing hand card")?;
                        let mut row = json!({"action":"play_card","player_id":1,"hand_index":hand_index,"card_id":card.card_id.to_python_name(),"cost":card.cost,"target_index":target});
                        if let Some(t) = target {
                            row["target_name"] = json!(format!("Target_{t}"));
                        }
                        row
                    }
                    Action::DrinkPotion {
                        potion_slot,
                        target,
                    }
                    | Action::UsePotion {
                        slot: potion_slot,
                        target,
                    } => {
                        let at = usize::from(*potion_slot);
                        let physical = p
                            .potion_slot_indices
                            .get(at)
                            .copied()
                            .unwrap_or(*potion_slot);
                        json!({"action":"use_potion","slot_index":physical,"target_index":target,"potion_id":legacy_potion_id(p.potions[at].python_name())})
                    }
                    _ => return Err("unexpected internal action at policy boundary".into()),
                };
                actions.push(row);
                commands.push(Command::Core(action));
            }
        }
        rows.push(("actions".into(), json!(actions)));
        self.commands = commands;
        rows.push((
            "run".into(),
            if fast {
                Value::Null
            } else {
                self.context["run"].clone()
            },
        ));
        rows.push(("power_records".into(), json!([])));
        rows.push(("move_records".into(), json!(moves)));
        rows.push((
            "played_turn_records".into(),
            indexed_records(&self.played_turn, &json!({})),
        ));
        if s.pending_choice.is_some() {
            rows.push(("choice_options".into(), json!(choice_options)));
        }
        Ok(rows)
    }

    fn done(&self) -> bool {
        self.state.is_over || self.decisions >= self.max_actions
    }
    fn step(&mut self, index: usize) -> Result<()> {
        self.numeric_cache.trace = sts2_core::run_rng_set::splitmix64(
            self.numeric_cache.trace ^ ((self.commands.len() as u64) << 32) ^ index as u64,
        );
        self.numeric_cache.forced += u64::from(self.commands.len() == 1);
        let command = self
            .commands
            .get(index)
            .cloned()
            .ok_or("invalid or stale action index")?;
        self.commands.clear();
        let used_potion = match &command {
            Command::Core(
                Action::DrinkPotion { potion_slot, .. }
                | Action::UsePotion {
                    slot: potion_slot, ..
                },
            ) => Some(usize::from(*potion_slot)),
            _ => None,
        };
        match command {
            Command::Pick(i) => {
                let pc = self
                    .state
                    .pending_choice
                    .as_ref()
                    .ok_or("choice closed before pick")?;
                if pc.min_select == 1 && pc.max_select == 1 {
                    self.opening_prompt = None;
                    if matches!(
                        sts2_core::resolve_choice_atomic_apply(&mut self.state, &[i]),
                        sts2_core::StepResult::InvalidTarget
                    ) {
                        return Err("Rust rejected a legal choice".into());
                    }
                } else {
                    self.selected.push(i);
                    self.selected.sort_unstable();
                }
            }
            Command::Confirm => {
                self.opening_prompt = None;
                sts2_core::resolve_choice_atomic_apply(&mut self.state, &self.selected);
                self.selected.clear();
            }
            Command::Core(action) => {
                // legal_actions historically exposes EndTurn in transient
                // phases, where core apply is a no-op. Reject that inconsistency
                // rather than spin forever inside forced-chain search.
                if matches!(action, Action::EndTurn)
                    && self.state.phase != sts2_core::CombatPhase::PlayerPlayPhase
                    && !self.state.is_over {
                    return Err(format!("non-progressing EndTurn outside player play phase: phase={:?} round={}", self.state.phase, self.state.round));
                }
                self.last_potion = match &action {
                    Action::DrinkPotion { potion_slot, .. }
                    | Action::UsePotion {
                        slot: potion_slot, ..
                    } => {
                        let at = usize::from(*potion_slot);
                        Some(legacy_potion_id(
                            self.state.player.potions[at].python_name(),
                        ))
                    }
                    _ => None,
                };
                if matches!(
                    sts2_core::apply(&mut self.state, action),
                    sts2_core::StepResult::InvalidTarget
                ) {
                    return Err("Rust rejected a legal action".into());
                }
            }
        }
        self.reward.update(&self.state, used_potion);
        self.decisions += 1;
        if self.state.combat_log.event_log.dropped() != 0 {
            return Err("combat history recording overflow".into());
        }
        for event in &self.state.combat_log.event_log.rows()[self.log_cursor..] {
            if event.round != self.history_round {
                self.history_round = event.round;
                self.played_turn.clear();
                self.drawn = 0;
                self.exhausted = 0;
                self.discarded = 0;
                self.damage_events = 0;
            }
            match event.entry {
                EventLogEntry::CardPlayStarted { card, .. } => {
                    let id = card.to_python_name();
                    self.played_turn.push(json!(id));
                    *self.played_counts.entry(id.into()).or_default() += 1;
                }
                EventLogEntry::CardDrawn { .. } => self.drawn += 1,
                EventLogEntry::CardExhausted { .. } => self.exhausted += 1,
                EventLogEntry::CardDiscarded { .. } => self.discarded += 1,
                EventLogEntry::DamageReceived { .. } => self.damage_events += 1,
                _ => {}
            }
        }
        // Keep the source stack while a choice suspends a card. clear() would
        // discard that stack, so only clear at an ordinary decision boundary.
        self.log_cursor = self.state.combat_log.event_log.rows().len();
        if self.state.pending_choice.is_none() {
            self.state.combat_log.event_log.clear();
            self.log_cursor = 0;
        }

        Ok(())
    }
}

fn legacy_potion_id(name: &str) -> String {
    name.split('_')
        .map(|s| {
            let mut chars = s.chars();
            match chars.next() {
                Some(c) => format!("{}{}", c, chars.as_str().to_lowercase()),
                None => String::new(),
            }
        })
        .collect()
}

fn pile_name(pile: Option<PileKind>) -> &'static str {
    match pile {
        Some(PileKind::Hand) => "hand",
        Some(PileKind::Draw) => "draw",
        Some(PileKind::Discard) => "discard",
        Some(PileKind::Exhaust) => "exhaust",
        Some(PileKind::Play) => "play",
        None => "generated",
    }
}

#[pyclass(name = "AdversarialEncoder")]
#[derive(Debug)]
pub struct PyAdversarialEncoder {
    schema: Arc<Schema>,
}

#[pymethods]
impl PyAdversarialEncoder {
    #[new]
    fn new(registry_json: &str, templates_json: &str) -> PyResult<Self> {
        let labels: Vec<Value> = serde_json::from_str(registry_json)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let templates = serde_json::from_str(templates_json)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self {
            schema: Arc::new(Schema::new(&labels, templates).map_err(PyValueError::new_err)?),
        })
    }
}

#[pyclass(name = "AdversarialCombatBatch")]
#[derive(Debug)]
pub struct PyAdversarialCombatBatch {
    schema: Arc<Schema>,
    sessions: Vec<Session>,
    active: Vec<usize>,
    pool: Option<Arc<rayon::ThreadPool>>,
}

#[pymethods]
impl PyAdversarialCombatBatch {
    #[new]
    #[pyo3(signature=(encoder,states,contexts,max_actions=500,workers=1))]
    fn new(
        encoder: PyRef<'_, PyAdversarialEncoder>,
        states: Vec<PyRef<'_, PyCombatState>>,
        contexts: Vec<String>,
        max_actions: usize,
        workers: usize,
    ) -> PyResult<Self> {
        if workers == 0 || workers > 128 {
            return Err(PyValueError::new_err("workers must be in 1..=128"));
        }
        if states.len() != contexts.len() || max_actions == 0 {
            return Err(PyValueError::new_err(
                "state/context count mismatch or zero action cap",
            ));
        }
        let mut sessions = Vec::new();
        for (state, context) in states.into_iter().zip(contexts) {
            let context: Value =
                serde_json::from_str(&context).map_err(|e| PyValueError::new_err(e.to_string()))?;
            sessions.push(
                Session::from_parts(&encoder.schema, state.inner.clone(), context, max_actions)
                    .map_err(PyValueError::new_err)?,
            );
        }
        let pool = if workers > 1 {
            Some(Arc::new(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(workers)
                    .build()
                    .map_err(|e| PyValueError::new_err(e.to_string()))?,
            ))
        } else {
            None
        };
        Ok(Self {
            schema: Arc::clone(&encoder.schema),
            sessions,
            active: Vec::new(),
            pool,
        })
    }

    /// One bulk observation buffer; CPU work releases the GIL.
    fn observe<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = py
            .allow_threads(|| self.pack(0, self.sessions.len()))
            .map_err(PyValueError::new_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    fn native_layout(&self) -> PyResult<String> {
        self.native_layout_impl()
    }
    fn observe_native_into(
        &mut self,
        py: Python<'_>,
        buffer: &Bound<'_, PyAny>,
        policy_seeds: Vec<u64>,
    ) -> PyResult<usize> {
        self.observe_native_into_impl(py, buffer, policy_seeds)
    }

    /// Numeric schema and static GPU lookup data, exported once at startup.
    fn numeric_catalog(&self) -> String {
        self.schema.numeric.json()
    }

    /// Direct records including newly terminal post-states. Metadata is separate from model features.
    #[pyo3(signature=(policy_seeds,verify_actions=false))]
    fn observe_numeric<'py>(
        &mut self,
        py: Python<'py>,
        policy_seeds: Vec<u64>,
        verify_actions: bool,
    ) -> PyResult<Bound<'py, PyBytes>> {
        if policy_seeds.len() != self.sessions.len() {
            return Err(PyValueError::new_err(
                "one policy seed per session required",
            ));
        }
        let build_started = std::time::Instant::now();
        let previous = &self.active;
        let schema = &self.schema;
        let indices = py
            .allow_threads(|| -> Result<Vec<usize>> {
                let build = |(i, s): (usize, &mut Session)| -> Result<Option<usize>> {
                    if s.done() && previous.binary_search(&i).is_err() {
                        return Ok(None);
                    }
                    s.numeric_observation(schema)?;
                    if verify_actions {
                        let commands = format!("{:?}", s.commands);
                        s.project(schema)?;
                        if commands != format!("{:?}", s.commands) {
                            return Err(format!("numeric command ordering mismatch slot {i}"));
                        }
                    }
                    Ok(Some(i))
                };
                let results = if let Some(pool) = &self.pool {
                    pool.install(|| {
                        self.sessions
                            .par_iter_mut()
                            .enumerate()
                            .map(build)
                            .collect::<Result<Vec<_>>>()
                    })?
                } else {
                    self.sessions
                        .iter_mut()
                        .enumerate()
                        .map(build)
                        .collect::<Result<Vec<_>>>()?
                };
                Ok(results.into_iter().flatten().collect())
            })
            .map_err(PyValueError::new_err)?;
        self.active = indices
            .iter()
            .copied()
            .filter(|i| !self.sessions[*i].done())
            .collect();
        let n = indices
            .iter()
            .map(|i| self.sessions[*i].numeric_cache.rows.len())
            .sum::<usize>();
        let b = indices.len();
        let len = (8 + b * 8 + n * numeric::WIDTH) * 4;
        let build_us = build_started.elapsed().as_micros() as u32;
        let pack_started = std::time::Instant::now();
        PyBytes::new_with(py, len, |bytes| {
            let header = [
                0x4e55_4d31u32,
                1,
                b as u32,
                n as u32,
                numeric::WIDTH as u32,
                8,
                build_us,
                0,
            ];
            for (chunk, word) in bytes[..32].chunks_exact_mut(4).zip(header) {
                chunk.copy_from_slice(&word.to_le_bytes());
            }
            let mut offset = 0;
            for (j, i) in indices.iter().copied().enumerate() {
                let s = &self.sessions[i];
                let count = s.commands.len();
                let random = sts2_core::run_rng_set::splitmix64(
                    policy_seeds[i]
                        .wrapping_add((s.decisions as u64 + 1).wrapping_mul(0x9e3779b97f4a7c15)),
                );
                let choice = if count == 0 {
                    -1i32
                } else {
                    (random % count as u64) as i32
                };
                let meta = [
                    i as i32,
                    offset as i32,
                    s.numeric_cache.rows.len() as i32,
                    s.numeric_cache.action_start as i32,
                    count as i32,
                    choice,
                    i32::from(s.done()),
                    s.decisions as i32,
                ];
                for (chunk, word) in bytes[32 + j * 32..32 + (j + 1) * 32]
                    .chunks_exact_mut(4)
                    .zip(meta)
                {
                    chunk.copy_from_slice(&word.to_le_bytes());
                }
                let start = 32 + b * 32 + offset * 64;
                let end = start + s.numeric_cache.rows.len() * 64;
                numeric::copy_rows(&s.numeric_cache.rows, &mut bytes[start..end]);
                offset += s.numeric_cache.rows.len();
            }
            bytes[28..32]
                .copy_from_slice(&(pack_started.elapsed().as_micros() as u32).to_le_bytes());
            Ok(())
        })
    }

    /// Direct records including newly terminal post-states. Metadata is separate from model features.
    #[pyo3(signature=(buffer,policy_seeds,verify_actions=false))]
    fn observe_numeric_into<'py>(
        &mut self,
        py: Python<'py>,
        buffer: &Bound<'py, PyAny>,
        policy_seeds: Vec<u64>,
        verify_actions: bool,
    ) -> PyResult<usize> {
        if policy_seeds.len() != self.sessions.len() {
            return Err(PyValueError::new_err(
                "one policy seed per session required",
            ));
        }
        let output = pyo3::buffer::PyBuffer::<i32>::get(buffer)?;
        if output.readonly() || !output.is_c_contiguous() {
            return Err(PyValueError::new_err(
                "numeric output must be a writable contiguous int32 buffer",
            ));
        }
        let build_started = std::time::Instant::now();
        let previous = &self.active;
        let schema = &self.schema;
        let indices = py
            .allow_threads(|| -> Result<Vec<usize>> {
                let build = |(i, s): (usize, &mut Session)| -> Result<Option<usize>> {
                    if s.done() && previous.binary_search(&i).is_err() {
                        return Ok(None);
                    }
                    s.numeric_observation(schema)?;
                    if verify_actions {
                        let commands = format!("{:?}", s.commands);
                        s.project(schema)?;
                        if commands != format!("{:?}", s.commands) {
                            return Err(format!("numeric command ordering mismatch slot {i}"));
                        }
                    }
                    Ok(Some(i))
                };
                let results = if let Some(pool) = &self.pool {
                    pool.install(|| {
                        self.sessions
                            .par_iter_mut()
                            .enumerate()
                            .map(build)
                            .collect::<Result<Vec<_>>>()
                    })?
                } else {
                    self.sessions
                        .iter_mut()
                        .enumerate()
                        .map(build)
                        .collect::<Result<Vec<_>>>()?
                };
                Ok(results.into_iter().flatten().collect())
            })
            .map_err(PyValueError::new_err)?;
        let n = indices
            .iter()
            .map(|i| self.sessions[*i].numeric_cache.rows.len())
            .sum::<usize>();
        let b = indices.len();
        let len = (8 + b * 8 + n * numeric::WIDTH) * 4;
        if output.item_count() < len / 4 {
            return Err(PyValueError::new_err(format!(
                "numeric buffer too small: {}",
                len / 4
            )));
        }
        self.active = indices
            .iter()
            .copied()
            .filter(|i| !self.sessions[*i].done())
            .collect();
        let build_us = build_started.elapsed().as_micros() as u32;
        let pack_started = std::time::Instant::now();
        {
            // SAFETY: PyBuffer validates int32 format/contiguity and holds the
            // exporter alive. Writability and capacity were checked above.
            // The caller must not access this buffer concurrently.
            let bytes =
                unsafe { std::slice::from_raw_parts_mut(output.buf_ptr().cast::<u8>(), len) };
            let header = [
                0x4e55_4d31u32,
                1,
                b as u32,
                n as u32,
                numeric::WIDTH as u32,
                8,
                build_us,
                0,
            ];
            for (chunk, word) in bytes[..32].chunks_exact_mut(4).zip(header) {
                chunk.copy_from_slice(&word.to_le_bytes());
            }
            let mut offset = 0;
            for (j, i) in indices.iter().copied().enumerate() {
                let s = &self.sessions[i];
                let count = s.commands.len();
                let random = sts2_core::run_rng_set::splitmix64(
                    policy_seeds[i]
                        .wrapping_add((s.decisions as u64 + 1).wrapping_mul(0x9e3779b97f4a7c15)),
                );
                let choice = if count == 0 {
                    -1i32
                } else {
                    (random % count as u64) as i32
                };
                let meta = [
                    i as i32,
                    offset as i32,
                    s.numeric_cache.rows.len() as i32,
                    s.numeric_cache.action_start as i32,
                    count as i32,
                    choice,
                    i32::from(s.done()),
                    s.decisions as i32,
                ];
                for (chunk, word) in bytes[32 + j * 32..32 + (j + 1) * 32]
                    .chunks_exact_mut(4)
                    .zip(meta)
                {
                    chunk.copy_from_slice(&word.to_le_bytes());
                }
                let start = 32 + b * 32 + offset * 64;
                let end = start + s.numeric_cache.rows.len() * 64;
                numeric::copy_rows(&s.numeric_cache.rows, &mut bytes[start..end]);
                offset += s.numeric_cache.rows.len();
            }
            bytes[28..32]
                .copy_from_slice(&(pack_started.elapsed().as_micros() as u32).to_le_bytes());
            Ok(len / 4)
        }
    }

    /// Diagnostic clone: alter private state after import, preserving card/context identity.
    #[pyo3(signature=(slot,scramble=false))]
    fn numeric_visibility_probe(&self, slot: usize, scramble: bool) -> PyResult<String> {
        let mut session = self
            .sessions
            .get(slot)
            .ok_or_else(|| PyValueError::new_err("invalid slot"))?
            .clone();
        if scramble {
            session.state.player.draw.reverse();
            let _ = session.state.rng.next_unsigned_long();
        }
        session
            .numeric_observation(&self.schema)
            .map_err(PyValueError::new_err)?;
        Ok(json!({"rows":session.numeric_cache.rows,"state_hash":sts2_core::state_hash(&session.state)}).to_string())
    }

    fn numeric_proof(&self) -> String {
        let rows=self.sessions.iter().enumerate().map(|(i,s)|json!({"slot":i,"decisions":s.decisions,"forced":s.numeric_cache.forced,"terminal":s.state.is_over,"win":s.state.is_over&&s.state.player_won&&s.state.player.current_hp>0,"truncated":!s.state.is_over,"hp":s.state.player.current_hp,"state_hash":sts2_core::state_hash(&s.state),"trace":s.numeric_cache.trace})).collect::<Vec<_>>();
        serde_json::to_string(&rows).expect("finite numeric proof")
    }

    /// Compact flat-format observation; exact old IDs and ordered candidate tokens.
    fn observe_fast<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = py
            .allow_threads(|| self.pack_mode(0, self.sessions.len(), true))
            .map_err(PyValueError::new_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Encode a bounded range of search children without copying states.
    fn observe_range<'py>(
        &mut self,
        py: Python<'py>,
        first: usize,
        end: usize,
    ) -> PyResult<Bound<'py, PyBytes>> {
        if first > end || end > self.sessions.len() {
            return Err(PyValueError::new_err("invalid observation range"));
        }
        let bytes = py
            .allow_threads(|| self.pack(first, end))
            .map_err(PyValueError::new_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Strict one-ply search: one sampled successor per root command.
    /// Belief conversion uses a public caller seed and never the saved future
    /// draw order/RNG. The real root is left untouched; commit via step().
    #[pyo3(signature=(seed=0))]
    fn expand_depth_one(&mut self, py: Python<'_>, seed: u64) -> PyResult<(Self, Vec<usize>)> {
        let (sessions, active, offsets) = py
            .allow_threads(|| {
                let expand =
                    |(slot, root): (usize, &mut Session)| -> Result<(usize, Vec<Session>)> {
                        root.refresh_commands();
                        let mut belief = root.clone();
                        sts2_core::blindfold::belief_root(
                            &mut belief.state,
                            sts2_core::run_rng_set::splitmix64(
                                seed ^ (slot as u64).wrapping_mul(0x9e3779b97f4a7c15)
                                    ^ root.decisions as u64,
                            ),
                        );
                        let mut children = Vec::with_capacity(root.commands.len());
                        for index in 0..root.commands.len() {
                            let mut child = belief.clone();
                            child
                                .step(index)
                                .map_err(|e| format!("slot {slot}, branch {index}: {e}"))?;
                            children.push(child);
                        }
                        Ok((slot, children))
                    };
                let roots = if let Some(pool) = &self.pool {
                    pool.install(|| {
                        self.sessions
                            .par_iter_mut()
                            .enumerate()
                            .filter(|(_, s)| !s.done())
                            .map(expand)
                            .collect::<Result<Vec<_>>>()
                    })
                } else {
                    self.sessions
                        .iter_mut()
                        .enumerate()
                        .filter(|(_, s)| !s.done())
                        .map(expand)
                        .collect::<Result<Vec<_>>>()
                }?;
                let mut sessions = Vec::new();
                let mut active = Vec::new();
                let mut offsets = vec![0];
                for (slot, children) in roots {
                    if children.is_empty() {
                        return Err(format!("no legal search actions at slot {slot}"));
                    }
                    active.push(slot);
                    sessions.extend(children);
                    offsets.push(sessions.len());
                }
                Ok((sessions, active, offsets))
            })
            .map_err(PyValueError::new_err)?;
        self.active = active;
        Ok((
            Self {
                schema: Arc::clone(&self.schema),
                sessions,
                active: Vec::new(),
                pool: self.pool.clone(),
            },
            offsets,
        ))
    }

    /// Exact terminal/cap score on the checkpoint's [0,2] reward scale;
    /// nonterminal children must be evaluated by max_a Q(s,a).
    fn terminal_scores(&self) -> Vec<Option<f32>> {
        self.sessions
            .iter()
            .map(|s| s.done().then(|| combat_score(s)))
            .collect()
    }

    /// Run one imported combat with the deployed net-free eval-cheap search.
    /// The GIL is released for the entire combat, so callers can schedule
    /// independent fights on CPU workers without nested search parallelism.
    #[pyo3(signature=(slot,seed=0,max_rollout_depth=200,max_simulations=None,transition_metrics="off",simulations_per_decision=None))]
    #[allow(clippy::too_many_arguments)] // Python API keeps optional benchmark controls explicit.
    fn play_default_mcts(
        &self,
        py: Python<'_>,
        slot: usize,
        seed: u64,
        max_rollout_depth: u32,
        max_simulations: Option<u32>,
        transition_metrics: &str,
        simulations_per_decision: Option<u32>,
    ) -> PyResult<String> {
        use sts2_core::transition_metrics::{Mode, Phase, Recorder};
        let transition_mode = match transition_metrics {
            "off" => Mode::Off,
            "count" => Mode::Count,
            "time" => Mode::Time,
            _ => {
                return Err(PyValueError::new_err(
                    "transition_metrics must be off, count or time",
                ))
            }
        };
        if max_rollout_depth == 0 {
            return Err(PyValueError::new_err("max_rollout_depth must be positive"));
        }
        if max_simulations == Some(0) {
            return Err(PyValueError::new_err("max_simulations must be positive"));
        }
        if simulations_per_decision == Some(0) {
            return Err(PyValueError::new_err(
                "simulations_per_decision must be positive",
            ));
        }
        if max_simulations.is_some() && simulations_per_decision.is_some() {
            return Err(PyValueError::new_err(
                "choose either a simulation cap or an exact budget",
            ));
        }
        let session = self
            .sessions
            .get(slot)
            .ok_or_else(|| PyValueError::new_err("invalid combat slot"))?;
        let mut state = session.state.clone();
        state.combat_log.event_log.clear();
        state.combat_log.event_log.set_recording(false);
        let max_actions = session.max_actions;
        py.allow_threads(move || {
            use sts2_mcts::{HiddenInfoMode,LeafEval,Mcts,MctsConfig,SharedPolicy};
            use sts2_runai::policy::{mcts_config_for,CombatBudget};
            let budget=CombatBudget::eval_cheap();
            let tier=if state.is_boss {sts2_runai::policy::BudgetTier::Boss}
                else if state.is_elite {sts2_runai::policy::BudgetTier::Elite}
                else {sts2_runai::policy::BudgetTier::Common};
            let recorder=Recorder::start(transition_mode);
            let start=std::time::Instant::now();
            let starting_hp=state.player.current_hp;
            let mut actions=0;
            let mut searches=0;
            let mut requested_sims=0u64;
            let mut rollout_calls=0u64;
            let mut rollout_cap_hits=0u64;
            let mut rollout_skips=0u64;
            let mut nodes_allocated=0u64;
            let mut tt_hits=0u64;
            let prior=SharedPolicy::uniform();
            while !state.is_over && actions<max_actions {
                let legal=sts2_core::legal_actions(&state);
                let action=match legal.as_slice(){
                    []=>return Err(format!("no legal action after {actions} decisions")),
                    [only]=>only.clone(),
                    _=>{
                        let _phase=sts2_core::transition_metrics::phase(Phase::Tree);
                        let default_sims=budget.sims_for(tier,&state);
                        let sims=simulations_per_decision.unwrap_or_else(||max_simulations.map_or(default_sims,|cap|default_sims.min(cap)));
                        let decision_seed=sts2_core::run_rng_set::splitmix64(seed ^ (actions as u64+1).wrapping_mul(0x9e3779b97f4a7c15));
                        let config=mcts_config_for(HiddenInfoMode::BeliefDraw,sims,decision_seed,max_rollout_depth,MctsConfig::default().terminal_eval_mode);
                        let mut search=Mcts::with_policy(state.clone(),config,prior.clone()).with_leaf_eval(LeafEval::TexelWide);
                        // The constructor can raise the horizon for particular
                        // enemies. This benchmark parameter is a strict rollout cap.
                        search.config.max_rollout_depth=max_rollout_depth;
                        searches+=1;
                        requested_sims+=u64::from(sims);
                        let action=search.search();
                        let (calls,caps)=search.rollout_stats();
                        rollout_calls+=calls;
                        rollout_cap_hits+=caps;
                        rollout_skips+=search.rollouts_skipped();
                        nodes_allocated+=search.nodes_allocated();
                        tt_hits+=search.tt_hits();
                        action
                    }
                };
                if matches!(sts2_core::apply(&mut state,action),sts2_core::StepResult::InvalidTarget){return Err(format!("search returned an invalid action at decision {actions}"));}
                actions+=1;
            }
            let metrics=recorder.finish();
            let measured_seconds=start.elapsed().as_secs_f64();
            state.assert_invariants().map_err(|e|format!("final state invariant: {e}"))?;
            Ok(json!({"slot":slot,"win":state.is_over&&state.player_won&&state.player.current_hp>0,"terminal":state.is_over,"truncated":!state.is_over,"hp":state.player.current_hp,"initial_hp":starting_hp,"turns":state.round,"actions":actions,"searches":searches,"requested_simulations":requested_sims,"rollout_depth":max_rollout_depth,"simulation_cap":max_simulations,"simulations_per_decision":simulations_per_decision,"seconds":start.elapsed().as_secs_f64(),"seed":seed,"transition_metrics":metrics,"transition_metrics_mode":transition_metrics,"measured_seconds":measured_seconds,"rollout_calls":rollout_calls,"rollout_cap_hits":rollout_cap_hits,"rollout_skips":rollout_skips,"nodes_allocated":nodes_allocated,"tt_hits":tt_hits}).to_string())
        }).map_err(PyValueError::new_err)
    }

    /// Benchmark two complete compact encodings per random transition.
    #[pyo3(signature=(slot,seed=0,mode="encoded"))]
    fn play_random_encoded(
        &self,
        py: Python<'_>,
        slot: usize,
        seed: u64,
        mode: &str,
    ) -> PyResult<String> {
        if !matches!(
            mode,
            "encoded"
                | "discover"
                | "project"
                | "validate"
                | "raw"
                | "fast"
                | "fast_verify"
                | "fast_reuse"
                | "fast_verify_reuse"
        ) {
            return Err(PyValueError::new_err(
                "mode must be encoded, discover, project, validate or raw",
            ));
        }
        let mut session = self
            .sessions
            .get(slot)
            .ok_or_else(|| PyValueError::new_err("invalid slot"))?
            .clone();
        let schema = Arc::clone(&self.schema);
        py.allow_threads(move || -> Result<String> {
            let start=std::time::Instant::now();
            let mut random=seed;
            let mut encoded=0u64;
            let mut entities=0u64;
            let mut fields=0u64;
            let mut bytes=0u64;
            let mut forced=0u64;
            let mut trace=0u64;
            let mut missing=BTreeMap::<String,Value>::new();
            let mut sample_pre=Vec::new();
            let mut sample_post=Vec::new();
            let mut first_choice=None;
            let mut terminal_encodings=0u64;
            let fast=mode.starts_with("fast");
            let verify=mode.contains("verify");
            let reuse=mode.ends_with("reuse");
            let mut previous_post=None;
            let mut capture=|session:&mut Session,terminal:bool| -> Result<Vec<u8>> {
                let observation=if fast {session.fast_observation(&schema)?} else {
                    let rows=session.project(&schema)?;
                    if mode=="discover" {for label in missing_labels(&schema,&rows){missing.insert(label.to_string(),label);}return Ok(Vec::new());}
                    if mode=="project" {return Ok(Vec::new());}
                    Observation::encode_state(&schema,&rows).map_err(|e|format!("{e}; NEW_FEATURES:{}",json!(missing_labels(&schema,&rows))))?
                };
                if !terminal && observation.actions.is_empty(){return Err("nonterminal observation has no legal actions".into());}
                entities+=observation.kinds.len() as u64;fields+=observation.ids.len() as u64;
                let blob=pack_observations(&[slot],vec![observation]);
                if verify {
                    let commands=format!("{:?}",session.commands);
                    let rows=session.project(&schema)?;
                    let expected=pack_observations(&[slot],vec![Observation::encode_state(&schema,&rows)?]);
                    if expected!=blob{return Err(format!("packet mismatch at decision {}",session.decisions));}
                    if commands!=format!("{:?}",session.commands){return Err("ordered command mismatch".into());}
                }
                encoded+=1;bytes+=blob.len() as u64;terminal_encodings+=u64::from(terminal);Ok(blob)
            };
            while !session.done() {
                let before=if mode=="raw" {
                    session.refresh_commands();
                    Vec::new()
                } else if let Some(prior)=previous_post.take() {
                    if verify {
                        let rows=session.project(&schema)?;
                        let expected=pack_observations(&[slot],vec![Observation::encode_state(&schema,&rows)?]);
                        if prior!=expected{return Err("reused pre-state mismatch".into());}
                    }
                    prior
                } else {capture(&mut session,false)?};
                if session.commands.is_empty() {return Err("no legal random action".into());}
                let count=session.commands.len();
                forced+=u64::from(count==1);
                // Action sampling has a separate deterministic stream from game RNG.
                random=random.wrapping_add(0x9e3779b97f4a7c15);
                let choice=(sts2_core::run_rng_set::splitmix64(random) % count as u64) as usize;
                first_choice.get_or_insert(choice);
                trace=sts2_core::run_rng_set::splitmix64(trace ^ ((count as u64)<<32) ^ choice as u64);
                session.step(choice)?;
                let terminal=session.state.is_over;
                let after=if mode=="raw" {Vec::new()} else {capture(&mut session,terminal)?};
                // Keep both packed observations alive until the pair is complete.
                std::hint::black_box((&before,&after));
                if mode=="validate" && sample_pre.is_empty() {sample_pre=before;sample_post=after;} else if reuse {previous_post=Some(after);}
            }
            let seconds=start.elapsed().as_secs_f64();
            session.state.assert_invariants().map_err(|e|format!("random final invariant: {e}"))?;
            Ok(json!({"slot":slot,"seed":seed,"mode":mode,"decisions":session.decisions,"forced":forced,"terminal":session.state.is_over,"win":session.state.is_over&&session.state.player_won&&session.state.player.current_hp>0,"truncated":!session.state.is_over,"hp":session.state.player.current_hp,"state_hash":sts2_core::state_hash(&session.state),"trace":trace,"encodings":encoded,"entities":entities,"fields":fields,"bytes":bytes,"terminal_encodings":terminal_encodings,"cache_hits":session.fast_cache.hits,"cache_misses":session.fast_cache.misses,"missing":missing.into_values().collect::<Vec<_>>(),"seconds":seconds,"first_choice":first_choice,"sample_pre":sample_pre,"sample_post":sample_post}).to_string())
        }).map_err(PyValueError::new_err)
    }

    fn step(&mut self, py: Python<'_>, indices: Vec<usize>) -> PyResult<()> {
        if indices.len() != self.active.len() {
            return Err(PyValueError::new_err("expected one action per active slot"));
        }
        let mut assignments = vec![None; self.sessions.len()];
        for (slot, index) in self.active.iter().zip(indices) {
            if index >= self.sessions[*slot].commands.len() {
                return Err(PyValueError::new_err("invalid or stale action index"));
            }
            assignments[*slot] = Some(index);
        }
        py.allow_threads(|| {
            let apply = |(slot, s): (usize, &mut Session)| -> Result<()> {
                if let Some(index) = assignments[slot] {
                    s.step(index)?;
                }
                Ok(())
            };
            if let Some(pool) = &self.pool {
                pool.install(|| self.sessions.par_iter_mut().enumerate().try_for_each(apply))
            } else {
                self.sessions.iter_mut().enumerate().try_for_each(apply)
            }
        })
        .map_err(PyValueError::new_err)
    }

    /// Debug-only JSON view; never used during model-driven rollouts.
    fn public_state(&mut self, slot: usize) -> PyResult<String> {
        let s = self
            .sessions
            .get_mut(slot)
            .ok_or_else(|| PyValueError::new_err("invalid slot"))?;
        let rows = s.project(&self.schema).map_err(PyValueError::new_err)?;
        serde_json::to_string(&rows).map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Swap in an append-only vocabulary snapshot after the inference service
    /// has initialized matching embedding rows. No simulation state changes.
    fn use_encoder(&mut self, encoder: PyRef<'_, PyAdversarialEncoder>) {
        self.schema = Arc::clone(&encoder.schema);
        for session in &mut self.sessions {
            session.fast_cache = fast_flat::Cache::default();
        }
        self.active.clear();
    }

    fn check_invariants(&self) -> PyResult<()> {
        for (slot, session) in self.sessions.iter().enumerate() {
            session
                .state
                .assert_invariants()
                .map_err(|e| PyValueError::new_err(format!("slot {slot}: {e}")))?;
        }
        Ok(())
    }

    fn state_hashes(&self) -> Vec<u64> {
        self.sessions
            .iter()
            .map(|s| sts2_core::state_hash(&s.state))
            .collect()
    }

    fn results(&self) -> String {
        let rows=self.sessions.iter().enumerate().map(|(i,s)|json!({"slot":i,"done":s.done(),"win":s.state.is_over&&s.state.player_won&&s.state.player.current_hp>0,"truncated":!s.state.is_over&&s.done(),"hp":s.state.player.current_hp,"initial_hp":s.initial_hp,"hp_lost":(s.initial_hp-s.state.player.current_hp).max(0),"turns":s.state.round,"actions":s.decisions})).collect::<Vec<_>>();
        serde_json::to_string(&rows).expect("finite integer results")
    }
}

impl PyAdversarialCombatBatch {
    fn pack(&mut self, first: usize, end: usize) -> Result<Vec<u8>> {
        self.pack_mode(first, end, false)
    }
    fn pack_mode(&mut self, first: usize, end: usize, fast: bool) -> Result<Vec<u8>> {
        let schema = &self.schema;
        let build = |(i, session): (usize, &mut Session)| -> Result<(usize, Observation)> {
            if fast {
                return Ok((i, session.fast_observation(schema)?));
            }
            let rows = session
                .project(schema)
                .map_err(|e| format!("slot {i}: {e}"))?;
            let observation = Observation::encode(schema, &rows).map_err(|e| {
                let labels = missing_labels(schema, &rows);
                if labels.is_empty() {
                    format!("slot {i}: {e}")
                } else {
                    format!("slot {i}: NEW_FEATURES:{}", json!(labels))
                }
            })?;
            Ok((i, observation))
        };
        let rows = if let Some(pool) = &self.pool {
            pool.install(|| {
                self.sessions[first..end]
                    .par_iter_mut()
                    .enumerate()
                    .filter(|(_, s)| !s.done())
                    .map(|(i, s)| build((i + first, s)))
                    .collect::<Result<Vec<_>>>()
            })
        } else {
            self.sessions[first..end]
                .iter_mut()
                .enumerate()
                .filter(|(_, s)| !s.done())
                .map(|(i, s)| build((i + first, s)))
                .collect::<Result<Vec<_>>>()
        }?;
        let (active, observations): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
        self.active = active;
        Ok(pack_observations(&self.active, observations))
    }
}

fn pack_observations(active: &[usize], observations: Vec<Observation>) -> Vec<u8> {
    let b = observations.len();
    let n = observations
        .iter()
        .map(|o| o.kinds.len())
        .max()
        .unwrap_or(0);
    let f = observations.iter().map(|o| o.ids.len()).sum::<usize>();
    let a = observations.iter().map(|o| o.actions.len()).sum::<usize>();
    let mut words = Vec::<u32>::with_capacity(5 + b * n + 2 * f + b * n + 1 + 3 * a + 3 * b);
    words.extend([1, b as u32, n as u32, f as u32, a as u32]);
    for o in &observations {
        words.extend(&o.kinds);
        words.resize(words.len() + n - o.kinds.len(), 0);
    }
    for o in &observations {
        words.extend(&o.ids);
    }
    for o in &observations {
        words.extend(o.values.iter().map(|v| v.to_bits()));
    }
    let mut offset = 0;
    words.push(0);
    for o in &observations {
        words.extend(o.offsets[1..].iter().map(|x| x + offset));
        offset += o.ids.len() as u32;
        words.resize(words.len() + n - o.kinds.len(), offset);
    }
    for (i, o) in observations.iter().enumerate() {
        for pos in &o.actions {
            words.extend([i as u32, *pos]);
        }
    }
    words.resize(words.len() + a, 0); // phase=COMBAT
    words.extend(observations.iter().map(|o| o.kinds.len() as u32));
    words.extend(observations.iter().map(|o| o.actions.len() as u32));
    words.extend(active.iter().map(|s| *s as u32));
    let mut bytes = Vec::with_capacity(words.len() * 4);
    for word in words {
        bytes.extend(word.to_le_bytes());
    }
    bytes
}

impl Session {
    fn from_parts(
        schema: &Schema,
        mut state: CombatState,
        context: Value,
        max_actions: usize,
    ) -> Result<Self> {
        state.act_index = context["run"]["act"]
            .as_u64()
            .map_or(state.act_index, |v| v as u8);
        state.is_boss = context["_is_boss"].as_bool().unwrap_or(state.is_boss);
        restore_snapshot_metadata(&mut state, &context)?;
        restore_opening_choice(&mut state, &context)?;
        if state.player.character != Some(sts2_core::CharacterId::Necrobinder) {
            return Err("this checkpoint adapter requires solo Necrobinder".into());
        }
        state.combat_log.event_log.clear();
        state.combat_log.event_log.set_recording(true);
        let counts = context["history"]["played_combat_counts"]
            .as_object()
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0) as u32))
                    .collect()
            })
            .unwrap_or_default();
        let drawn = context["history"]["drawn_this_turn"].as_u64().unwrap_or(0) as u32;
        let initial_hp = state.player.current_hp;
        let round = state.round;
        let initial_costs = state
            .cards
            .iter()
            .map(|c| {
                let key = format!("{}:{}", c.card_id.to_python_name(), u8::from(c.upgraded));
                let original = context["_card_import"][c.uid.get().to_string()]["original_cost"]
                    .as_i64()
                    .or_else(|| {
                        context["_card_instances"][c.uid.get().to_string()]["$mod"]["original_cost"]
                            .as_i64()
                    })
                    .or_else(|| {
                        schema.templates["cards"][key]["reference"]["original_cost"].as_i64()
                    })
                    .map_or(c.cost, |v| v as i8);
                (c.uid, original)
            })
            .collect();
        Ok(Self {
            reward: reward::Context::default(),
            numeric_cache: numeric::Cache::default(),
            native_sidecars: Vec::new(),
            fast_cache: fast_flat::Cache::default(),
            state,
            opening_prompt: context["_opening_choice"]["prompt"]
                .as_str()
                .map(str::to_owned),
            selected: context["_opening_choice"]["selected"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_u64().map(|v| v as u8))
                        .collect()
                })
                .unwrap_or_default(),
            damage_events: context["history"]["damage_events_this_turn"]
                .as_u64()
                .unwrap_or(0) as u32,
            exhausted: context["history"]["exhausted_this_turn"]
                .as_u64()
                .unwrap_or(0) as u32,
            discarded: context["history"]["discarded_this_turn"]
                .as_u64()
                .unwrap_or(0) as u32,
            played_turn: context["played_turn_records"]
                .as_array()
                .map(|rows| {
                    let mut played = Vec::new();
                    for row in rows {
                        if let Some(positions) = row["positions"].as_array() {
                            for position in positions {
                                if let Some(i) = position.as_u64() {
                                    if played.len() <= i as usize {
                                        played.resize(i as usize + 1, Value::Null);
                                    }
                                    played[i as usize] = row["item"].clone();
                                }
                            }
                        }
                    }
                    played
                })
                .unwrap_or_default(),
            context: Arc::new(context),
            decisions: 0,
            initial_hp,
            initial_costs: Arc::new(initial_costs),
            commands: Vec::new(),
            played_counts: counts,
            drawn,
            history_round: round,
            log_cursor: 0,
            last_potion: None,
            max_actions,
        })
    }
}

#[cfg(test)]
mod random_pair_tests {
    use super::*;
    #[test]
    fn terminal_state_encodes_without_inventing_a_legal_action() {
        let labels = vec![
            json!(["entity", "player"]),
            json!(["field", "player", "hp", "number"]),
        ];
        let schema = Schema::new(&labels, json!({})).unwrap();
        let rows = vec![
            ("player".into(), json!({"hp":0})),
            ("actions".into(), json!([])),
        ];
        assert!(Observation::encode(&schema, &rows).is_err());
        let obs = Observation::encode_state(&schema, &rows).unwrap();
        assert_eq!(obs.kinds, vec![0]);
        assert_eq!(obs.ids, vec![1]);
        assert_eq!(obs.values, vec![0.]);
        assert!(obs.actions.is_empty());
        let packed = pack_observations(&[17], vec![obs]);
        let words = packed
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(&words[..5], &[1, 1, 1, 1, 0]);
        assert_eq!(&words[words.len() - 3..], &[1, 0, 17]);
    }
}
