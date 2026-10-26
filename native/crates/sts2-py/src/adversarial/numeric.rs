//! Versioned direct numeric observations. No public-record projection in this path.
use super::{effective_target_type, json, Action, Command, Intent, Result, Schema, Session, Value};
use std::collections::{BTreeMap, BTreeSet};
use sts2_core::cards::{CardId, PileKind};

pub(super) const WIDTH: usize = 16;
pub(super) type Row = [i32; WIDTH];

#[derive(Clone, Debug, Default)]
pub(super) struct Cache {
    pub rows: Vec<Row>,
    pub action_start: usize,
    pub trace: u64,
    pub forced: u64,
}

#[derive(Debug)]
pub(super) struct Catalog {
    pub names: BTreeMap<String, i32>,
    pub public_vars: BTreeSet<String>,
    pub card_static: Vec<[f32; 8]>,
    pub card_names: Vec<String>,
}
impl Catalog {
    pub fn new(templates: &Value) -> Self {
        fn collect(v: &Value, names: &mut BTreeSet<String>) {
            match v {
                Value::String(s) => {
                    names.insert(s.clone());
                }
                Value::Array(a) => {
                    for v in a {
                        collect(v, names);
                    }
                }
                Value::Object(o) => {
                    for (k, v) in o {
                        names.insert(k.clone());
                        collect(v, names);
                    }
                }
                _ => {}
            }
        }
        let mut names = BTreeSet::new();
        collect(templates, &mut names);
        for c in CardId::all() {
            names.insert(c.to_python_name().into());
        }
        for m in sts2_core::MonsterKind::ALL_KNOWN {
            names.insert(m.python_name().into());
        }
        for r in sts2_core::RelicKind::ALL {
            names.insert(r.python_name().into());
        }
        for p in sts2_core::PotionKind::ALL {
            names.insert(p.python_name().into());
        }
        for n in ["OSTY", "PLAYER", "STUNNED"] {
            names.insert(n.into());
        }
        let public_vars = templates["public_combat_var_names"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let mut card_static = vec![[0.; 8]; CardId::all().len() * 2 + 2];
        let mut card_names = vec![String::new(); card_static.len()];
        for card in CardId::all() {
            for upgraded in [false, true] {
                let id = card_id(*card, upgraded) as usize;
                let key = format!("{}:{}", card.to_python_name(), u8::from(upgraded));
                let r = &templates["cards"][&key]["record"];
                let reference = &templates["cards"][&key]["reference"];
                card_names[id] = key;
                card_static[id] = [
                    reference["cost"].as_f64().unwrap_or(0.) as f32,
                    reference["base_damage"].as_f64().unwrap_or(0.) as f32,
                    reference["base_block"].as_f64().unwrap_or(0.) as f32,
                    f32::from(r["has_energy_cost_x"].as_bool().unwrap_or(false)),
                    f32::from(r["has_star_cost_x"].as_bool().unwrap_or(false)),
                    reference["base_replay_count"].as_f64().unwrap_or(0.) as f32,
                    f32::from(upgraded),
                    1.,
                ];
            }
        }
        Self {
            names: names
                .into_iter()
                .enumerate()
                .map(|(i, n)| (n, i as i32 + 1))
                .collect(),
            public_vars,
            card_static,
            card_names,
        }
    }
    pub fn id(&self, name: &str) -> Result<i32> {
        self.names
            .get(name)
            .copied()
            .ok_or_else(|| format!("numeric catalog missing {name}"))
    }
    pub fn json(&self) -> String {
        json!({"version":1,"width":WIDTH,"names":self.names,"card_names":self.card_names,"card_static":self.card_static}).to_string()
    }
}
fn card_id(id: CardId, upgraded: bool) -> i32 {
    1 + 2 * (id as i32) + i32::from(upgraded)
}
/// Append directly into either cached rows or a final framed observation buffer.
struct RowWriter<'a> {
    destination: &'a mut Vec<Row>,
    base: usize,
}
trait RowSink {
    fn len(&self) -> usize;
    fn push(&mut self, row: Row);
}
impl RowSink for RowWriter<'_> {
    fn len(&self) -> usize {
        self.destination.len() - self.base
    }
    fn push(&mut self, row: Row) {
        self.destination.push(row);
    }
}
#[cfg(feature = "native-gpu-feeder")]
struct SliceWriter<'a> {
    destination: &'a mut [Row],
    cursor: usize,
    overflow: bool,
}
#[cfg(feature = "native-gpu-feeder")]
impl RowSink for SliceWriter<'_> {
    fn len(&self) -> usize {
        self.cursor - 1
    }
    fn push(&mut self, row: Row) {
        if let Some(slot) = self.destination.get_mut(self.cursor) {
            *slot = row;
        } else {
            self.overflow = true;
        }
        self.cursor += 1;
    }
}

fn push(
    rows: &mut impl RowSink,
    kind: i32,
    id: i32,
    owner: i32,
    index: i32,
    values: &[i32],
) -> usize {
    assert!(values.len() <= 12);
    let at = rows.len();
    let mut row = [0; WIDTH];
    row[..4].copy_from_slice(&[kind, id, owner, index]);
    row[4..4 + values.len()].copy_from_slice(values);
    rows.push(row);
    at
}
fn pile_id(pile: Option<PileKind>) -> i32 {
    match pile {
        Some(PileKind::Hand) => 1,
        Some(PileKind::Draw) => 2,
        Some(PileKind::Discard) => 3,
        Some(PileKind::Exhaust) => 4,
        Some(PileKind::Play) => 5,
        None => 0,
    }
}

impl Session {
    pub(super) fn numeric_observation(&mut self, schema: &Schema) -> Result<()> {
        self.numeric_observation_mode(schema, false)
    }
    pub(super) fn numeric_observation_mode(&mut self, schema: &Schema, native: bool) -> Result<()> {
        let mut destination = std::mem::take(&mut self.numeric_cache.rows);
        destination.clear();
        let result = self.numeric_write_rows(
            schema,
            native,
            &mut RowWriter {
                destination: &mut destination,
                base: 0,
            },
        );
        self.numeric_cache.rows = destination;
        result
    }

    pub(super) fn numeric_direct(
        &mut self,
        schema: &Schema,
        destination: &mut Vec<Row>,
        seed: u64,
    ) -> Result<()> {
        destination.clear();
        destination.push([0; WIDTH]); // NUM1 header + one metadata row, 16 i32.
        self.numeric_write_rows(
            schema,
            false,
            &mut RowWriter {
                destination,
                base: 1,
            },
        )?;
        let n = destination.len() - 1;
        destination[0] = self.numeric_frame(n, seed);
        Ok(())
    }

    #[cfg(feature = "native-gpu-feeder")]
    pub(super) fn numeric_direct_slice(
        &mut self,
        schema: &Schema,
        destination: &mut [Row],
        seed: u64,
    ) -> Result<usize> {
        if destination.is_empty() {
            return Err("empty observation slot".into());
        }
        let mut writer = SliceWriter {
            destination,
            cursor: 1,
            overflow: false,
        };
        self.numeric_write_rows(schema, false, &mut writer)?;
        if writer.overflow {
            return Err(format!(
                "observation slot too small: need {} rows",
                writer.cursor
            ));
        }
        let len = writer.cursor;
        writer.destination[0] = self.numeric_frame(len - 1, seed);
        Ok(len)
    }

    pub(super) fn numeric_frame(&self, n: usize, seed: u64) -> Row {
        let count = self.commands.len();
        let random = sts2_core::run_rng_set::splitmix64(
            seed.wrapping_add((self.decisions as u64 + 1).wrapping_mul(0x9e3779b97f4a7c15)),
        );
        [
            0x4e55_4d31,
            if self.reward.enabled { 2 } else { 1 },
            1,
            n as i32,
            16,
            8,
            0,
            0,
            0,
            0,
            n as i32,
            self.numeric_cache.action_start as i32,
            count as i32,
            if count == 0 {
                -1
            } else {
                (random % count as u64) as i32
            },
            i32::from(self.done()),
            self.decisions as i32,
        ]
    }

    fn numeric_write_rows<W: RowSink>(
        &mut self,
        schema: &Schema,
        native: bool,
        rows: &mut W,
    ) -> Result<()> {
        self.refresh_commands();
        let catalog = &schema.numeric;
        let s = &self.state;
        let p = &s.player;
        push(
            rows,
            1,
            0,
            -1,
            0,
            &[
                s.round.into(),
                match s.phase {
                    sts2_core::CombatPhase::Setup => 0,
                    sts2_core::CombatPhase::PlayerTurnStart => 1,
                    sts2_core::CombatPhase::PlayerPlayPhase => 2,
                    sts2_core::CombatPhase::PlayerEndTurnPhaseOne => 3,
                    sts2_core::CombatPhase::PlayerEndTurnPhaseTwo => 4,
                    sts2_core::CombatPhase::EnemyTurnStart => 5,
                    sts2_core::CombatPhase::EnemyTurnExec => 6,
                    sts2_core::CombatPhase::EnemyTurnEnd => 7,
                    sts2_core::CombatPhase::Resolved { .. } => 8,
                },
                s.ascension.into(),
                s.gold,
                p.energy.into(),
                p.max_energy.into(),
                p.stars.into(),
                self.decisions as i32,
                i32::from(s.is_over),
                i32::from(s.player_won),
                s.act_index.into(),
                (i32::from(s.is_boss) | (i32::from(s.is_elite) << 1)),
            ],
        );
        let player = push(
            rows,
            2,
            0,
            -1,
            0,
            &[
                p.current_hp,
                p.max_hp,
                p.block,
                p.energy.into(),
                p.max_energy.into(),
                p.stars.into(),
                i32::from(p.current_hp > 0),
                p.potion_slots.into(),
                p.hand.len() as i32,
                p.draw.len() as i32,
                p.discard.len() as i32,
                p.exhaust.len() as i32,
            ],
        ) as i32;
        let mut card_rows = Vec::with_capacity(if native { 0 } else { s.cards.len() });
        if !native {
            let mut locations = Vec::with_capacity(s.cards.len());
            for (kind, pile) in [
                (1, p.hand.as_slice()),
                (2, p.draw.as_slice()),
                (3, p.discard.as_slice()),
                (4, p.exhaust.as_slice()),
                (5, p.play.as_slice()),
            ] {
                for (index, uid) in pile.iter().enumerate() {
                    locations.push((*uid, kind, if kind == 2 { -1 } else { index as i32 }));
                }
            }

            for card in &s.cards {
                let Some((_, pile, position)) =
                    locations.iter().find(|(uid, _, _)| *uid == card.uid)
                else {
                    continue;
                };
                let flags = i32::from(sts2_core::card_effects::card_ethereal_inst(card))
                    | (i32::from(sts2_core::card_effects::card_unplayable_inst(card)) << 1)
                    | (i32::from(sts2_core::card_effects::card_innate_inst(card)) << 2)
                    | (i32::from(sts2_core::card_effects::card_retain_inst(card)) << 3)
                    | (i32::from(sts2_core::card_effects::card_exhausts_inst(card)) << 4)
                    | (i32::from(p.retain_this_turn.contains(&card.uid)) << 5);
                let at = push(
                    rows,
                    3,
                    card_id(card.card_id, card.upgraded),
                    player,
                    *position,
                    &[
                        *pile,
                        sts2_core::hooks::effective_card_cost(s, card.uid),
                        sts2_core::effective_star_cost_in_combat(s, card),
                        card.base_damage,
                        card.base_block,
                        card.cost.into(),
                        self.initial_costs
                            .get(&card.uid)
                            .copied()
                            .unwrap_or(card.cost)
                            .into(),
                        card.base_replay_count,
                        flags,
                        card.card_type as i32,
                        effective_target_type(s, card) as i32,
                        card.keyword_override.map_or(-1, |v| i32::from(v.0)),
                    ],
                );
                card_rows.push((card.uid, at as i32));
                for (key, value) in &card.combat_vars {
                    let name = sts2_wire::state_dict::var_key_wire_name(*key);
                    if catalog.public_vars.contains(name) {
                        push(rows, 8, catalog.id(name)?, at as i32, 0, &[*value]);
                    }
                }
                for (key, value) in &card.enchantments {
                    push(rows, 8, *key as i32 + 1, at as i32, 1, &[*value]);
                }
                if let Some(a) = card.affliction {
                    push(
                        rows,
                        8,
                        a.kind as i32 + 1,
                        at as i32,
                        2,
                        &[a.stacks, i32::from(a.applied_exhaust)],
                    );
                }
            }
        }
        let card_row = |uid: sts2_core::CardUid| {
            if native {
                -2 - uid.get() as i32
            } else {
                card_rows
                    .iter()
                    .find(|(u, _)| *u == uid)
                    .map_or(-1, |(_, r)| *r)
            }
        };
        let mut creature_rows = Vec::with_capacity(s.enemy_uids.len() + 1);
        if let Some(uid) = p.creature_uid {
            creature_rows.push((uid, player));
        }
        let mut export_creature =
            |uid: sts2_core::CreatureUid, index: i32, rows: &mut W| -> Result<()> {
                let c = s
                    .creatures
                    .iter()
                    .find(|c| c.id == uid.0)
                    .ok_or("numeric missing creature")?;
                let name = if index < 0 { "OSTY" } else { c.monster_id.0 };
                let current =
                    c.ai.as_ref()
                        .map(|ai| catalog.id(ai.current_move().id))
                        .transpose()?
                        .unwrap_or(0);
                let at = push(
                    rows,
                    4,
                    catalog.id(name)?,
                    -1,
                    index,
                    &[
                        c.current_hp,
                        c.max_hp,
                        c.block,
                        i32::from(c.current_hp > 0),
                        current,
                    ],
                );
                creature_rows.push((uid, at as i32));
                let mut intents = Vec::with_capacity(4);
                if let Some(intent) = c.intent {
                    intents.push(intent);
                }
                if let Some(ai) = &c.ai {
                    let current = ai.current_move();
                    if c.intent.map(Intent::kind)
                        == current.intents.first().copied().map(Intent::kind)
                    {
                        intents.extend(current.intents.iter().skip(1).copied());
                    }
                    for (j, id) in ai.move_history().iter().enumerate() {
                        push(rows, 16, catalog.id(id)?, at as i32, j as i32, &[]);
                    }
                }
                for (j, intent) in intents.iter().enumerate() {
                    let (damage, hits) = match intent {
                        Intent::Attack { damage } | Intent::DeathBlow { damage } => (*damage, 1),
                        Intent::MultiAttack { damage, hits } => (*damage, i32::from(*hits)),
                        _ => (0, 1),
                    };
                    push(
                        rows,
                        9,
                        intent.kind() as i32 + 1,
                        at as i32,
                        j as i32,
                        &[damage, hits],
                    );
                }
                Ok(())
            };
        if let Some(uid) = p.pet_uid {
            export_creature(uid, -1, rows)?;
        }
        for (i, uid) in s.enemy_uids.iter().enumerate() {
            export_creature(*uid, i as i32, rows)?;
        }
        let mut power_owners = vec![(player, p.powers.as_slice())];
        for (uid, owner) in &creature_rows {
            if *owner != player {
                power_owners.push((
                    *owner,
                    s.creatures
                        .iter()
                        .find(|c| c.id == uid.0)
                        .ok_or("numeric owner missing")?
                        .powers
                        .as_slice(),
                ));
            }
        }
        for (owner, powers) in power_owners {
            for power_uid in powers {
                let power = s
                    .powers
                    .iter()
                    .find(|v| v.id == power_uid.0)
                    .ok_or("numeric power missing")?;
                let applier = power
                    .applier
                    .and_then(|uid| creature_rows.iter().find(|(u, _)| *u == uid))
                    .map_or(-1, |(_, r)| *r);
                push(
                    rows,
                    5,
                    catalog.id(power.kind.python_name())?,
                    owner,
                    0,
                    &[
                        power.amount,
                        power.counter,
                        i32::from(power.skip_next_tick),
                        applier,
                    ],
                );
            }
        }
        for (i, relic) in s.relics.iter().enumerate() {
            push(
                rows,
                6,
                catalog.id(relic.kind.python_name())?,
                player,
                i as i32,
                &[
                    relic.counter,
                    i32::from(relic.is_melted),
                    i32::from(relic.is_wax),
                ],
            );
        }
        for (i, potion) in p.potions.iter().enumerate() {
            let potion_row = push(
                rows,
                7,
                catalog.id(potion.python_name())?,
                player,
                i32::from(*p.potion_slot_indices.get(i).unwrap_or(&(i as u8))),
                &[],
            );
            if self.reward.enabled {
                let instance = &self.reward.ledger[self.reward.active[i]];
                push(
                    rows,
                    18,
                    0,
                    potion_row as i32,
                    instance.id as i32,
                    &[instance.cost.to_bits() as i32],
                );
            }
        }
        push(
            rows,
            13,
            0,
            player,
            0,
            &[
                self.drawn as i32,
                self.discarded as i32,
                self.exhausted as i32,
                self.damage_events as i32,
                s.combat_log.energy_spent_this_turn,
                p.card_plays_started_this_turn as i32,
                p.cards_played_this_turn[0] as i32,
                self.played_turn.len() as i32,
            ],
        );
        for (name, count) in &self.played_counts {
            push(rows, 14, catalog.id(name)?, player, 0, &[*count as i32]);
        }
        for (i, value) in self.played_turn.iter().enumerate() {
            if let Some(name) = value.as_str() {
                push(rows, 15, catalog.id(name)?, player, i as i32, &[]);
            }
        }
        let mut option_rows = Vec::new();
        if let Some(pc) = &s.pending_choice {
            let at = push(
                rows,
                10,
                pc.kind as i32 + 1,
                player,
                0,
                &[
                    pc.min_select.into(),
                    pc.max_select.into(),
                    i32::from(pc.can_skip),
                    self.selected.len() as i32,
                    pc.options.len() as i32,
                ],
            );
            for (i, opt) in pc.options.iter().enumerate() {
                let reference = opt.card_uid.map_or(-1, card_row);
                let cost = opt
                    .card_uid
                    .and_then(|uid| sts2_core::card_by_uid(&s.cards, uid))
                    .map_or(0, |c| i32::from(c.cost));
                option_rows.push(push(
                    rows,
                    11,
                    card_id(opt.card_id, opt.upgraded),
                    at as i32,
                    i as i32,
                    &[
                        reference,
                        pile_id(opt.source_pile),
                        i32::from(self.selected.contains(&(i as u8))),
                        cost,
                    ],
                ) as i32);
            }
        }
        self.numeric_cache.action_start = rows.len();
        for (i, command) in self.commands.iter().enumerate() {
            let target_row = |target: Option<u8>| {
                target
                    .and_then(|t| s.enemy_uids.get(usize::from(t)))
                    .and_then(|uid| creature_rows.iter().find(|(u, _)| u == uid))
                    .map_or(-1, |(_, r)| *r)
            };
            match command {
                Command::Core(Action::EndTurn) => {
                    push(rows, 12, 1, player, i as i32, &[]);
                }
                Command::Core(Action::PlayCard { hand_index, target }) => {
                    push(
                        rows,
                        12,
                        2,
                        card_row(p.hand[usize::from(*hand_index)]),
                        i as i32,
                        &[
                            target_row(*target),
                            i32::from(*hand_index),
                            target.map_or(-1, i32::from),
                        ],
                    );
                }
                Command::Core(
                    Action::DrinkPotion {
                        potion_slot,
                        target,
                    }
                    | Action::UsePotion {
                        slot: potion_slot,
                        target,
                    },
                ) => {
                    let slot = *p
                        .potion_slot_indices
                        .get(usize::from(*potion_slot))
                        .unwrap_or(potion_slot);
                    push(
                        rows,
                        12,
                        3,
                        player,
                        i as i32,
                        &[
                            target_row(*target),
                            i32::from(slot),
                            target.map_or(-1, i32::from),
                            catalog.id(p.potions[usize::from(*potion_slot)].python_name())?,
                        ],
                    );
                }
                Command::Pick(index) => {
                    push(
                        rows,
                        12,
                        4,
                        option_rows[usize::from(*index)],
                        i as i32,
                        &[i32::from(*index)],
                    );
                }
                Command::Confirm => {
                    push(rows, 12, 5, player, i as i32, &[self.selected.len() as i32]);
                }
                Command::Core(_) => return Err("unsupported numeric action".into()),
            }
        }
        if self.reward.enabled {
            let r = &self.reward;
            push(
                rows,
                17,
                0,
                player,
                0,
                &[
                    r.spent.to_bits() as i32,
                    r.initial_hp.to_bits() as i32,
                    r.progress().to_bits() as i32,
                    (r.initial_enemy_hp() as f32).to_bits() as i32,
                ],
            );
            for (uid, hp, min) in &r.damage {
                if let Some((_, owner)) = creature_rows.iter().find(|(u, _)| u.0.get() == *uid) {
                    push(
                        rows,
                        19,
                        0,
                        *owner,
                        0,
                        &[
                            (*hp as f32).to_bits() as i32,
                            (*min as f32).to_bits() as i32,
                        ],
                    );
                }
            }
        }
        Ok(())
    }
}

/// Bulk copy is valid because Row is a fully initialized array of sixteen i32s.
/// Its byte representation is the wire format on little-endian hosts.
pub(super) fn copy_rows(rows: &[Row], destination: &mut [u8]) {
    assert_eq!(destination.len(), std::mem::size_of_val(rows));
    #[cfg(target_endian = "little")]
    {
        // SAFETY: arrays have no padding, every i32 is initialized, and this
        // immutable view lives only for the copy. The destination is disjoint.
        let bytes = unsafe {
            std::slice::from_raw_parts(rows.as_ptr().cast::<u8>(), std::mem::size_of_val(rows))
        };
        destination.copy_from_slice(bytes);
    }
    #[cfg(target_endian = "big")]
    for (chunk, word) in destination.chunks_exact_mut(4).zip(rows.iter().flatten()) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::{push, RowSink, RowWriter, WIDTH};
    #[cfg(feature = "native-gpu-feeder")]
    #[test]
    fn slice_writer_detects_overflow_without_overwriting_guard() {
        let mut target = [[7; WIDTH]; 3];
        let mut w = super::SliceWriter {
            destination: &mut target[..2],
            cursor: 1,
            overflow: false,
        };
        w.push([1; WIDTH]);
        w.push([2; WIDTH]);
        assert!(w.overflow);
        assert_eq!(w.cursor, 3);
        assert_eq!(target[0], [7; WIDTH]);
        assert_eq!(target[1], [1; WIDTH]);
        assert_eq!(target[2], [7; WIDTH]);
    }

    #[test]
    fn direct_writer_preserves_prefix_and_local_row_indices() {
        let mut destination = Vec::with_capacity(8);
        destination.push([77; WIDTH]);
        let ptr = destination.as_ptr();
        let mut writer = RowWriter {
            destination: &mut destination,
            base: 1,
        };
        assert_eq!(push(&mut writer, 3, 9, -1, 0, &[42]), 0);
        assert_eq!(push(&mut writer, 8, 4, 0, 0, &[-7]), 1);
        assert_eq!(writer.len(), 2);
        assert_eq!(destination[0], [77; WIDTH]);
        assert_eq!(destination[2][2], 0);
        assert_eq!(destination[2][4], -7);
        assert_eq!(destination.as_ptr(), ptr);
    }

    #[test]
    fn bulk_copy_preserves_every_signed_integer_bit() {
        let mut rows = [[0i32; 16]; 2];
        for (i, value) in rows.iter_mut().flatten().enumerate() {
            *value = match i % 4 {
                0 => i32::MIN,
                1 => i32::MAX,
                2 => -(i as i32),
                _ => i as i32,
            };
        }
        let expected = rows
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        let mut actual = vec![0; expected.len()];
        super::copy_rows(&rows, &mut actual);
        assert_eq!(actual, expected);
    }
}
