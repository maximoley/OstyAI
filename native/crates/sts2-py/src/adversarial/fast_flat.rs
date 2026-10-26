//! Exact flat packets with direct scalar emission and per-card feature caches.
use super::{
    effective_target_type, json, structural_cmp, Card, Observation, Result, Schema, Session,
    TargetType, Value,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
struct CardKey {
    card: Card,
    cost: i64,
    star_cost: i64,
    target: u8,
    retained: bool,
    imported_round: bool,
}
#[derive(Clone, Debug)]
struct CachedCard {
    key: CardKey,
    record: Value,
    sections: BTreeMap<usize, (Vec<u32>, Vec<f32>)>,
}
#[derive(Clone, Debug, Default)]
pub(super) struct Cache {
    cards: BTreeMap<u32, CachedCard>,
    run: Option<Observation>,
    pub hits: u64,
    pub misses: u64,
}

fn begin(out: &mut Observation, schema: &Schema, section: &str) -> Result<usize> {
    out.kinds
        .push(schema.id(schema.name(schema.name(0, "entity")?, section)?)?);
    schema.name(schema.name(0, "field")?, section)
}
fn number(out: &mut Observation, schema: &Schema, node: usize, n: i64) -> Result<()> {
    if n.unsigned_abs() <= 1 << 24 {
        return out.atom(schema, node, "number", n as f32);
    }
    out.atom(schema, node, "integer_sign", if n >= 0 { 1. } else { -1. })?;
    let mut n = n.unsigned_abs();
    let mut i = 0;
    let limbs = schema.name(node, "limb")?;
    while n > 0 {
        let child = *schema.trie[limbs]
            .indices
            .get(&i)
            .ok_or("unseen integer limb")?;
        out.atom(schema, child, "integer_limb", (n % 65536) as f32)?;
        n /= 65536;
        i += 1;
    }
    Ok(())
}
fn field(out: &mut Observation, schema: &Schema, node: usize, key: &str, n: i64) -> Result<()> {
    number(out, schema, schema.name(node, key)?, n)
}
fn category(
    out: &mut Observation,
    schema: &Schema,
    node: usize,
    key: &str,
    value: &str,
) -> Result<()> {
    let node = schema.name(schema.name(schema.name(node, key)?, "category")?, value)?;
    out.atom(schema, node, "category", 1.)
}
fn finish(out: &mut Observation) {
    out.offsets.push(out.ids.len() as u32);
}
fn append(out: &mut Observation, other: &Observation) {
    let offset = out.ids.len() as u32;
    out.kinds.extend_from_slice(&other.kinds);
    out.ids.extend_from_slice(&other.ids);
    out.values.extend_from_slice(&other.values);
    out.offsets
        .extend(other.offsets.iter().skip(1).map(|v| v + offset));
}

impl Session {
    fn cache_card(&mut self, schema: &Schema, uid: sts2_core::CardUid) -> Result<()> {
        let card = *sts2_core::card_by_uid(&self.state.cards, uid).ok_or("missing card UID")?;
        let target = match effective_target_type(&self.state, &card) {
            TargetType::Enemy => 0,
            TargetType::AllEnemies => 1,
            TargetType::Self_ => 2,
            TargetType::None => 3,
            TargetType::Random => 4,
        };
        let key = CardKey {
            card,
            cost: i64::from(sts2_core::hooks::effective_card_cost(&self.state, uid)),
            star_cost: i64::from(sts2_core::effective_star_cost_in_combat(&self.state, &card)),
            target,
            retained: self.state.player.retain_this_turn.contains(&uid),
            imported_round: self.state.round
                == self.context["combat"]["round"].as_u64().unwrap_or(0) as u16,
        };
        if self
            .fast_cache
            .cards
            .get(&uid.get())
            .is_some_and(|entry| entry.key == key)
        {
            self.fast_cache.hits += 1;
            return Ok(());
        }
        let record = self.card(schema, &card)?;
        self.fast_cache.cards.insert(
            uid.get(),
            CachedCard {
                key,
                record,
                sections: BTreeMap::new(),
            },
        );
        self.fast_cache.misses += 1;
        Ok(())
    }
    fn fast_pile(&mut self, schema: &Schema, name: &str, out: &mut Observation) -> Result<()> {
        let pile = match name {
            "hand" => self.state.player.hand.as_slice(),
            "draw_pile" => self.state.player.draw.as_slice(),
            "discard_pile" => self.state.player.discard.as_slice(),
            "exhaust_pile" => self.state.player.exhaust.as_slice(),
            "play_pile" => self.state.player.play.as_slice(),
            _ => return Err("invalid pile".into()),
        };
        let mut uids = pile.to_vec();
        if uids.is_empty() {
            return out.record(schema, name, &Value::Array(Vec::new()));
        }
        for uid in &uids {
            self.cache_card(schema, *uid)?;
        }
        if name == "draw_pile" {
            uids.sort_by(|a, b| {
                structural_cmp(
                    &self.fast_cache.cards[&a.get()].record,
                    &self.fast_cache.cards[&b.get()].record,
                )
            });
        }
        out.record(schema, &format!("{name}_count"), &json!(uids.len()))?;
        let entity = schema.id(schema.name(schema.name(0, "entity")?, name)?)?;
        let node = schema.name(schema.name(0, "field")?, name)?;
        for uid in uids {
            let card = self
                .fast_cache
                .cards
                .get_mut(&uid.get())
                .expect("cached card");
            if let std::collections::btree_map::Entry::Vacant(slot) = card.sections.entry(node) {
                let mut features = Observation::default();
                features.visit(schema, node, &card.record)?;
                slot.insert((features.ids, features.values));
            }
            let (ids, values) = &card.sections[&node];
            out.kinds.push(entity);
            out.ids.extend_from_slice(ids);
            out.values.extend_from_slice(values);
            finish(out);
        }
        Ok(())
    }
    fn fast_combat(&self, schema: &Schema, out: &mut Observation) -> Result<()> {
        let s = &self.state;
        let p = &s.player;
        let node = begin(out, schema, "combat")?;
        for (k, v) in [
            ("ascension", i64::from(s.ascension)),
            ("decision", self.decisions as i64),
            ("energy", i64::from(p.energy)),
            ("gold", i64::from(s.gold)),
            ("max_energy", i64::from(p.max_energy)),
        ] {
            field(out, schema, node, k, v)?;
        }
        let phase = match s.phase {
            sts2_core::CombatPhase::PlayerTurnStart => "player_turn_start",
            sts2_core::CombatPhase::EnemyTurnStart
            | sts2_core::CombatPhase::EnemyTurnExec
            | sts2_core::CombatPhase::EnemyTurnEnd => "enemy_turn",
            sts2_core::CombatPhase::PlayerEndTurnPhaseOne => "player_before_turn_end",
            sts2_core::CombatPhase::PlayerEndTurnPhaseTwo => "player_turn_end_in_hand",
            _ => "player_turn",
        };
        category(out, schema, node, "phase", phase)?;
        for (k, v) in [
            ("round", i64::from(s.round)),
            ("stars", i64::from(p.stars)),
            ("turn", i64::from(s.round)),
        ] {
            field(out, schema, node, k, v)?;
        }
        finish(out);
        Ok(())
    }
    fn fast_player(&self, schema: &Schema, out: &mut Observation) -> Result<()> {
        let p = &self.state.player;
        let node = begin(out, schema, "player")?;
        out.atom(
            schema,
            schema.name(node, "alive")?,
            "bool",
            f32::from(p.current_hp > 0),
        )?;
        field(out, schema, node, "block", i64::from(p.block))?;
        field(out, schema, node, "hp", i64::from(p.current_hp))?;
        category(out, schema, node, "id", "NECROBINDER")?;
        field(out, schema, node, "max_hp", i64::from(p.max_hp))?;
        field(out, schema, node, "stars", i64::from(p.stars))?;
        out.visit(
            schema,
            schema.name(node, "statuses")?,
            &self.statuses(schema, &p.powers)?,
        )?;
        finish(out);
        Ok(())
    }
    fn fast_history(&self, schema: &Schema, out: &mut Observation) -> Result<()> {
        let node = begin(out, schema, "history")?;
        for (k, v) in [
            ("damage_events_this_turn", i64::from(self.damage_events)),
            ("discarded_this_turn", i64::from(self.discarded)),
            ("drawn_this_turn", i64::from(self.drawn)),
            (
                "energy_spent_this_turn",
                i64::from(self.state.combat_log.energy_spent_this_turn),
            ),
            ("exhausted_this_turn", i64::from(self.exhausted)),
        ] {
            field(out, schema, node, k, v)?;
        }
        let counts = schema.name(node, "played_combat_counts")?;
        if self.played_counts.is_empty() {
            out.atom(schema, counts, "empty_dict", 1.)?;
        } else {
            for (k, v) in &self.played_counts {
                field(out, schema, counts, k, i64::from(*v))?;
            }
        }
        let played = schema.name(node, "played_this_turn")?;
        field(out, schema, played, "length", self.played_turn.len() as i64)?;
        category(out, schema, played, "record_section", "played_turn_records")?;
        finish(out);
        Ok(())
    }
    pub(super) fn fast_observation(&mut self, schema: &Schema) -> Result<Observation> {
        let rows = self.project_mode(schema, true)?;
        let mut out = Observation {
            kinds: Vec::with_capacity(96),
            ids: Vec::with_capacity(768),
            values: Vec::with_capacity(768),
            offsets: vec![0],
            actions: Vec::with_capacity(32),
        };
        let mut actions = None;
        for (name, value) in &rows {
            match name.as_str() {
                "actions" => {
                    actions = Some(value);
                }
                "combat" => self.fast_combat(schema, &mut out)?,
                "player" => self.fast_player(schema, &mut out)?,
                "history" => self.fast_history(schema, &mut out)?,
                "hand" | "draw_pile" | "discard_pile" | "exhaust_pile" | "play_pile" => {
                    self.fast_pile(schema, name, &mut out)?;
                }
                "run" => {
                    if self.fast_cache.run.is_none() {
                        let mut run = Observation {
                            offsets: vec![0],
                            ..Observation::default()
                        };
                        run.record(schema, "run", &self.context["run"])?;
                        self.fast_cache.run = Some(run);
                    }
                    append(&mut out, self.fast_cache.run.as_ref().unwrap());
                }
                _ => {
                    if let Some(items) = value.as_array().filter(|v| !v.is_empty()) {
                        out.record(schema, &format!("{name}_count"), &json!(items.len()))?;
                        for item in items {
                            out.record(schema, name, item)?;
                        }
                    } else {
                        out.record(schema, name, value)?;
                    }
                }
            }
        }
        for action in actions.and_then(Value::as_array).ok_or("missing actions")? {
            out.actions.push(out.kinds.len() as u32);
            out.record(schema, "candidate", action)?;
        }
        Ok(out)
    }
}
