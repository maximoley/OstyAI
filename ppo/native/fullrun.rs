//! Public full-run NUM1 encoder and native batched worker prototype.
//! No JSON/Python objects in encode/step; JSON exports are debug/reference only.
use super::*;
use self::ppo::{PpoState,ResetCache,StartBank};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{c_char, c_void, CStr, CString};
use sts2_core::map::{MapPointIdx, MapPointType};
use sts2_core::run_manager::{Phase, RunAction, RunManager};
type Row = [i32; 16];
fn row(out: &mut Vec<Row>, k: i32, d: i32, owner: i32, index: i32, v: &[i32]) -> i32 {
    let mut r = [0; 16];
    r[..4].copy_from_slice(&[k, d, owner, index]);
    r[4..4 + v.len().min(12)].copy_from_slice(&v[..v.len().min(12)]);
    let at = out.len() as i32 - 1;
    out.push(r);
    at
}
fn card(out: &mut Vec<Row>, c: &Card, index: usize, pile: i32) -> i32 {
    let flags = i32::from(sts2_core::card_effects::card_ethereal_inst(c))
        | (i32::from(sts2_core::card_effects::card_unplayable_inst(c)) << 1)
        | (i32::from(sts2_core::card_effects::card_innate_inst(c)) << 2)
        | (i32::from(sts2_core::card_effects::card_retain_inst(c)) << 3)
        | (i32::from(sts2_core::card_effects::card_exhausts_inst(c)) << 4);
    let at = row(
        out,
        3,
        1 + 2 * c.card_id as i32 + i32::from(c.upgraded),
        1,
        index as i32,
        &[
            pile,
            c.cost.into(),
            sts2_core::effective_star_cost(c),
            c.base_damage,
            c.base_block,
            c.cost.into(),
            c.cost.into(),
            c.base_replay_count,
            flags,
            c.card_type as i32,
            c.target_type as i32,
            c.keyword_override.map_or(-1, |v| v.0.into()),
        ],
    );
    for (k, v) in &c.enchantments {
        row(out, 8, *k as i32 + 1, at, 1, &[*v]);
    }
    if let Some(a) = c.affliction {
        row(
            out,
            8,
            a.kind as i32 + 1,
            at,
            2,
            &[a.stacks, i32::from(a.applied_exhaust)],
        );
    }
    at
}
fn phase(p: Phase) -> i32 {
    match p {
        Phase::MapChoice => 1,
        Phase::Combat => 2,
        Phase::CardReward => 3,
        Phase::BossRelic => 4,
        Phase::Shop => 5,
        Phase::RestSite => 6,
        Phase::Event => 7,
        Phase::Treasure => 8,
        Phase::RunOver => 9,
    }
}
// All graph summaries describe visible room TYPES only. Unknown-room outcomes,
// generated encounters, seeds/RNG, and future reward offers are never read.
fn map_rows(rm: &RunManager, out: &mut Vec<Row>) -> Result<BTreeMap<i32, i32>> {
    let mut next = BTreeMap::new();
    let Some(m) = &rm.run_state.current_act_map else {
        return Ok(next);
    };
    let mut live: BTreeSet<MapPointIdx> = m.grid.values().copied().collect();
    if let Some(x) = m.boss {
        live.insert(x);
    }
    if let Some(x) = m.second_boss {
        live.insert(x);
    }
    let cur = rm.run_state.current_room.as_ref().and_then(|r| r.map_coord);
    let origin = cur.or_else(|| m.start.map(|i| m.point(i).coord));
    let mut reachable = BTreeSet::new();
    let mut stack = Vec::new();
    if let Some(c) = origin {
        if let Some(i) = m.grid.get(&c) {
            stack.push(*i);
        }
    } else {
        stack.extend(live.iter().copied().filter(|i| m.point(*i).coord.row == 0));
    }
    while let Some(i) = stack.pop() {
        if reachable.insert(i) {
            stack.extend(
                m.point(i)
                    .children
                    .iter()
                    .copied()
                    .filter(|x| live.contains(x)),
            );
        }
    }
    let mut rows = BTreeMap::new();
    for &i in &live {
        let p = m.point(i);
        let at = row(
            out,
            21,
            p.point_type as i32,
            -1,
            i as i32,
            &[
                p.coord.row,
                p.coord.col,
                i32::from(rm.run_state.visited_map_coords.contains(&p.coord)),
                i32::from(cur == Some(p.coord)),
                i32::from(reachable.contains(&i)),
                p.children.iter().filter(|x| live.contains(x)).count() as i32,
            ],
        );
        rows.insert(i, at);
    }
    // Public boss identities are shown on the current map, not future act content.
    let generated = rm
        .run_state
        .act_index
        .checked_sub(1)
        .and_then(|i| rm.run_state.generated_rooms.get(usize::from(i)));
    let boss_id = generated
        .map(|g| g.boss_id)
        .or_else(|| {
            rm.run_state
                .pinned_act_boss
                .as_ref()
                .map(|b| b.display_name)
        })
        .or_else(|| {
            let pool = match rm.run_state.act_index {
                2 => sts2_core::encounters::act2_boss_pool(),
                3 => sts2_core::encounters::act3_boss_pool(),
                _ => return None,
            };
            if pool.len() == 1 {
                Some(pool[0].name)
            } else {
                None
            }
        });
    for (node, id) in [
        (m.boss, boss_id),
        (m.second_boss, generated.and_then(|g| g.second_boss_id)),
    ] {
        if let (Some(node), Some(id)) = (node, id) {
            if let Some(&owner) = rows.get(&node) {
                for (j, b) in id.as_bytes().chunks(12).enumerate() {
                    row(
                        out,
                        29,
                        5,
                        owner,
                        j as i32,
                        &b.iter().map(|x| i32::from(*x)).collect::<Vec<_>>(),
                    );
                }
            }
        }
    }
    for &i in &live {
        for &ch in &m.point(i).children {
            if let Some(&target) = rows.get(&ch) {
                row(out, 22, 0, rows[&i], 0, &[target]);
            }
        }
    }
    // Reverse topological DP. A route starts AT the candidate: distance 0 for a
    // candidate rest/shop; elites/fights include the candidate itself. -1=no path.
    let mut order: Vec<_> = live.iter().copied().collect();
    order.sort_by_key(|i| std::cmp::Reverse(m.point(*i).coord.row));
    let mut dp: BTreeMap<MapPointIdx, [i32; 5]> = BTreeMap::new();
    for i in order {
        let p = m.point(i);
        let children: Vec<_> = p.children.iter().filter_map(|c| dp.get(c)).collect();
        let best = |col: usize| {
            children
                .iter()
                .filter_map(|a| if a[col] >= 0 { Some(a[col]) } else { None })
                .min()
        };
        let rest = if p.point_type == MapPointType::RestSite {
            0
        } else {
            best(0).map_or(-1, |x| x + 1)
        };
        let shop = if p.point_type == MapPointType::Shop {
            0
        } else {
            best(1).map_or(-1, |x| x + 1)
        };
        let boss = if p.point_type == MapPointType::Boss {
            0
        } else {
            best(2).map_or(-1, |x| x + 1)
        };
        let elite = if p.point_type == MapPointType::RestSite {
            0
        } else {
            best(3).map_or(-1, |x| x + i32::from(p.point_type == MapPointType::Elite))
        };
        let fights = if p.point_type == MapPointType::Boss {
            1
        } else {
            best(4).map_or(-1, |x| x + i32::from(p.point_type.is_combat()))
        };
        dp.insert(i, [rest, shop, boss, elite, fights]);
    }
    for a in rm.get_available_actions() {
        if let RunAction::Move { col } = a {
            let target = live
                .iter()
                .filter(|i| {
                    m.point(**i).coord.col == col
                        && m.point(**i).coord.row == origin.map_or(0, |c| c.row + 1)
                })
                .next()
                .copied()
                .or_else(|| m.boss.filter(|i| m.point(*i).coord.col == col));
            if let Some(i) = target {
                let s = dp[&i];
                let at = row(
                    out,
                    23,
                    m.point(i).point_type as i32,
                    rows[&i],
                    col,
                    &[s[2], s[0], s[1], s[3], s[4]],
                );
                next.insert(col, at);
            }
        }
    }
    Ok(next)
}
fn dummy_session() -> Session {
    Session {
        reward: reward::Context::default(),
        numeric_cache: numeric::Cache::default(),
        native_sidecars: Vec::new(),
        fast_cache: fast_flat::Cache::default(),
        state: CombatState::default(),
        context: Arc::new(Value::Null),
        decisions: 0,
        initial_hp: 0,
        initial_costs: Arc::new(HashMap::new()),
        commands: Vec::new(),
        selected: Vec::new(),
        played_turn: Vec::new(),
        played_counts: BTreeMap::new(),
        drawn: 0,
        exhausted: 0,
        discarded: 0,
        damage_events: 0,
        history_round: 0,
        log_cursor: 0,
        last_potion: None,
        opening_prompt: None,
        max_actions: usize::MAX,
    }
}
struct Slot {
    rm: RunManager,
    view: Session,
    rows: Vec<Row>,
    actions: Vec<RunAction>,
    decisions: usize,
    generation: u64,
    ppo: PpoState,
}
// This isolated run library is character-specific. Export the identity so a
// mismatched Python configuration fails before creating any environments.
const RUN_CHARACTER: sts2_core::CharacterId = sts2_core::CharacterId::Necrobinder;
#[no_mangle]
pub extern "C" fn sm_fr_character() -> *const c_char {
    match RUN_CHARACTER {
        sts2_core::CharacterId::Necrobinder => b"Necrobinder\0".as_ptr() as *const c_char,
        _ => b"unsupported\0".as_ptr() as *const c_char,
    }
}
impl Slot {
    fn prepare_decision(&mut self) -> Result<()> {
        if self.rm.phase() == Phase::Event && self.rm.run_state.current_event.is_none() {
            let id = sts2_content::events::dispatch::pick_event(&mut self.rm.run_state, None)
                .ok_or("event pool has no materializable event")?;
            self.rm.run_state.enter_event_room(id);
        }
        Ok(())
    }
    fn new(seed: u64) -> Self {
        let mut rm = sts2_content::run_callbacks::new_run_manager_for_character_with_ascension(
            seed,
            RUN_CHARACTER,
            10,
        )
        .with_neow_start();
        rm.run_state.interactive_card_removal = true;
        rm.run_state.interactive_card_upgrade = true;
        rm.run_state.interactive_card_transform = true;
        rm.run_state.interactive_potion_swap = true;
        Self {
            rm,
            view: dummy_session(),
            rows: Vec::with_capacity(1024),
            actions: Vec::new(),
            decisions: 0,
            generation: 0,
            ppo: PpoState {seed,..Default::default()},
        }
    }
    fn encode(&mut self, schema: &Schema) -> Result<()> {
        let rm = &mut self.rm;
        let actions = rm.get_available_actions();
        let out = &mut self.rows;
        out.clear();
        out.push([0; 16]);
        let mut combat_cards = HashMap::new();
        let mut creatures = HashMap::new();
        if let Some(c) = rm.combat.as_mut().filter(|_| {
            actions.iter().any(|a| {
                matches!(
                    a,
                    RunAction::EndTurn
                        | RunAction::PlayCard { .. }
                        | RunAction::CombatResolveChoice { .. }
                        | RunAction::CombatResolveChoiceTakeFirst { .. }
                        | RunAction::DrinkPotion { .. }
                )
            })
        }) {
            // Move ownership temporarily: no CombatState clone and no mutation of the
            // simulator. Existing combat numeric entity encoding is reused verbatim.
            self.view.selected = c
                .pending_choice
                .as_ref()
                .map_or_else(Vec::new, |p| p.picked.to_vec());
            std::mem::swap(c, &mut self.view.state);
            self.view.decisions = self.decisions;
            let result = self.view.numeric_direct(schema, out, 0);
            std::mem::swap(c, &mut self.view.state);
            result?;
            out.truncate(out[0][11] as usize + 1);
            for (idx, r) in out.iter().enumerate().skip(1) {
                if r[0] == 3 && r[4] == 1 {
                    combat_cards.insert(r[3] as usize, idx as i32 - 1);
                }
                if r[0] == 4 {
                    creatures.insert(r[3], idx as i32 - 1);
                }
            }
        } else {
            row(
                out,
                1,
                0,
                -1,
                0,
                &[
                    0,
                    phase(rm.phase()),
                    rm.ascension_level.into(),
                    rm.run_state.gold,
                    0,
                    0,
                    0,
                    self.decisions as i32,
                    i32::from(rm.run_state.is_over),
                    i32::from(rm.run_state.player_won),
                    rm.run_state.act_index.into(),
                    0,
                ],
            );
            row(
                out,
                2,
                0,
                -1,
                0,
                &[
                    rm.run_state.current_hp,
                    rm.run_state.max_hp,
                    0,
                    0,
                    0,
                    0,
                    i32::from(rm.run_state.current_hp > 0),
                    rm.run_state.max_potion_slots as i32,
                    0,
                    0,
                    0,
                    0,
                ],
            );
        }
        ppo::public_combat_context(rm,out,schema)?;
        if let Some(marker)=out.iter_mut().find(|r|r[0]==31&&r[1]==6){marker[6]=if rm.phase()==Phase::Combat&&self.ppo.combat_limit>0{self.ppo.combat_limit.saturating_sub(self.ppo.combat_actions) as i32}else{0};}
        let rs = &rm.run_state;
        row(
            out,
            20,
            phase(rm.phase()),
            1,
            0,
            &[
                rs.act_index.into(),
                rs.total_floor,
                rs.act_floor,
                rs.gold,
                rs.current_hp,
                rs.max_hp,
                rs.card_shop_removals_used as i32,
                rs.max_potion_slots as i32,
                rs.winged_boots_off_path_used as i32,
                rm.ascension_level.into(),
                rs.master_deck.len() as i32,
                rs.current_event.map_or(0, |x| x as i32 + 1),
            ],
        );
        let odds=&rs.unknown_map_point_odds.current;
        use sts2_core::runstate::RoomType;
        row(out,31,0,1,0,&[
            (odds.get(&RoomType::Monster).copied().unwrap_or(0.0)*10000.0).round() as i32,
            (odds.get(&RoomType::Elite).copied().unwrap_or(-1.0)*10000.0).round() as i32,
            (odds.get(&RoomType::Treasure).copied().unwrap_or(0.0)*10000.0).round() as i32,
            (odds.get(&RoomType::Shop).copied().unwrap_or(0.0)*10000.0).round() as i32,
            (rs.card_rarity_odds.current_value*10000.0).round() as i32,
            (rs.potion_reward_odds.current_value*10000.0).round() as i32,
            rs.monster_combats_this_act as i32,rs.elite_combats_this_act as i32,
            rs.current_room.map_or(0,|x|x.room_type as i32),rs.act1 as i32,
            i32::from(rs.has_double_boss),i32::from(rs.potion_use_locked_by_event.is_some())]);
        // Known top/bottom cards are public knowledge; unknown order stays private.
        if rm.phase()==Phase::Combat {if let Some(c)=rm.combat.as_ref(){for (kind,pins) in [(0,&c.player.pinned_top),(1,&c.player.pinned_bottom)]{for (i,uid) in pins.iter().enumerate(){if let Some(x)=sts2_core::card_by_uid(&c.cards,*uid){row(out,35,1+2*x.card_id as i32+i32::from(x.upgraded),1,i as i32,&[kind]);}}}}}
        let mut deck = BTreeMap::new();
        for i in sts2_wire::emit::run_manager::effective_master_deck_indices(rm) {
            deck.insert(i, card(out, &rs.master_deck[i], i, 6));
        }
        if rm.phase() != Phase::Combat {
            for (i, r) in rs.relics.iter().enumerate() {
                row(
                    out,
                    6,
                    schema.numeric.id(r.to_python_name())?,
                    1,
                    i as i32,
                    &[*rs.relic_counters.get(r).unwrap_or(&0)],
                );
            }
            for (i, p) in rs.potions.iter().enumerate() {
                row(
                    out,
                    7,
                    schema.numeric.id(p.python_name())?,
                    1,
                    i as i32,
                    &[],
                );
            }
        }
        for (j,id) in rs.relics.iter().enumerate(){let parent=out.iter().enumerate().skip(1).find(|(_,x)|x[0]==6&&x[1]==schema.numeric.id(id.to_python_name()).unwrap_or(-2)&&x[3]==j as i32).map(|(i,_)|i as i32-1);if let Some(parent)=parent{row(out,31,1,parent,j as i32,&[i32::from(rs.wax_relics.contains(id)),i32::from(rs.melted_relics.contains(id)),i32::from(rs.owns_active_relic(*id))]);}}
        let routes = if rm.phase() != Phase::Combat {
            map_rows(rm, out)?
        } else {
            BTreeMap::new()
        };
        let mut shop = BTreeMap::new();
        if let Some(inv) = rm.shop_inventory() {
            let relics = rs.active_relics();
            let price = |p| sts2_core::shop::merchant_price_with_modifiers(&relics, p);
            for (i, e) in inv
                .cards
                .iter()
                .chain(inv.colorless_cards.iter())
                .enumerate()
            {
                let obj = e.card.as_ref().map_or(-1, |c| card(out, c, i, 7));
                let live = e.card.is_some() && sts2_core::shop::is_shop_entry_available(e.price);
                let at = row(
                    out,
                    24,
                    0,
                    obj,
                    i as i32,
                    &[
                        price(e.price),
                        i32::from(live),
                        i32::from(live && rs.gold >= price(e.price)),
                        i32::from(e.on_sale),
                        i32::from(e.is_colorless),
                    ],
                );
                shop.insert((0, i), at);
            }
            for (i, e) in inv.relics.iter().enumerate() {
                let obj = if let Some(x) = e.relic_id {
                    row(
                        out,
                        6,
                        schema.numeric.id(x.to_python_name())?,
                        1,
                        i as i32,
                        &[],
                    )
                } else {
                    -1
                };
                let live =
                    e.relic_id.is_some() && sts2_core::shop::is_shop_entry_available(e.price);
                let at = row(
                    out,
                    24,
                    1,
                    obj,
                    i as i32,
                    &[
                        price(e.price),
                        i32::from(live),
                        i32::from(live && rs.gold >= price(e.price)),
                    ],
                );
                shop.insert((1, i), at);
            }
            for (i, e) in inv.potions.iter().enumerate() {
                let obj = if let Some(x) = e.potion_kind {
                    row(
                        out,
                        7,
                        schema.numeric.id(x.python_name())?,
                        1,
                        i as i32,
                        &[],
                    )
                } else {
                    -1
                };
                let live =
                    e.potion_kind.is_some() && sts2_core::shop::is_shop_entry_available(e.price);
                let at = row(
                    out,
                    24,
                    2,
                    obj,
                    i as i32,
                    &[
                        price(e.price),
                        i32::from(live),
                        i32::from(live && rs.gold >= price(e.price)),
                        i32::from(rs.potions.len() >= rs.max_potion_slots as usize),
                    ],
                );
                shop.insert((2, i), at);
            }
            let at = row(
                out,
                24,
                3,
                1,
                0,
                &[
                    price(inv.removal_cost),
                    i32::from(!inv.removal_used),
                    i32::from(!inv.removal_used && rs.gold >= price(inv.removal_cost)),
                    rs.card_shop_removals_used as i32,
                ],
            );
            shop.insert((3, 0), at);
        }
        let incoming_potion = if let Some(p) = rs.pending_potion_swaps.first() {
            let obj = row(out, 7, schema.numeric.id(p.python_name())?, 1, -1, &[]);
            row(
                out,
                28,
                3,
                obj,
                0,
                &[
                    rs.pending_potion_swaps.len() as i32,
                    i32::from(rs.potion_use_locked_by_event.is_some()),
                ],
            );
            obj
        } else {
            -1
        };
        let mut rewards = BTreeMap::new();
        for (i, c) in rm.card_reward_prepared_cards().iter().enumerate() {
            rewards.insert(i, card(out, c, i, 7));
        }
        if rewards.is_empty() {
            for (i, id) in rm.card_reward_options().iter().enumerate() {
                let at = row(
                    out,
                    27,
                    1 + 2 * (*id as i32)
                        + i32::from(*rm.card_reward_upgraded_flags().get(i).unwrap_or(&false)),
                    1,
                    i as i32,
                    &[],
                );
                rewards.insert(i, at);
            }
        }
        if let Some(bundles) = rm.card_reward_bundles() {
            for (i, pack) in bundles.iter().enumerate() {
                let parent = *rewards.get(&i).ok_or("bundle representative missing")?;
                for (j, c) in pack.iter().enumerate() {
                    row(out, 27, 1 + 2 * (*c as i32), parent, j as i32, &[1]);
                }
            }
        }
        let mut event = BTreeMap::new();
        if let Some(opts) = if rm.phase() == Phase::Event {
            sts2_content::events::dispatch::present(rs)
        } else {
            None
        } {
            for (i, o) in opts.iter().enumerate() {
                let delta = sts2_content::events::dispatch::preview_option_effect(rs, i);
                let mut vals = [0; 12];
                vals[0] = i32::from(o.enabled);
                vals[1] = i32::from(delta.is_some());
                if let Some(x) = delta {
                    vals[2] = x.gold_delta;
                    vals[3] = x.hp_delta;
                    vals[4] = x.max_hp_delta;
                    vals[5] = x.relics_gained;
                    vals[6] = x.relics_lost;
                    vals[7] = x.potions_gained;
                    vals[8] = x.potions_lost;
                    vals[9] = x.cards_gained;
                    vals[10] = x.cards_lost;
                }
                vals[11] = rs.event_state.page.into();
                let at = row(
                    out,
                    25,
                    rs.current_event.map_or(0, |x| x as i32 + 1),
                    1,
                    i as i32,
                    &vals,
                );
                for (j, chunk) in o.option_id.as_bytes().chunks(12).enumerate() {
                    let bytes: Vec<_> = chunk.iter().map(|b| i32::from(*b)).collect();
                    row(out, 29, 0, at, j as i32, &bytes);
                }
                for (field, text) in [(1, o.label.as_str()), (2, o.description.as_str())] {
                    for (j, chunk) in text.as_bytes().chunks(12).enumerate() {
                        let bytes: Vec<_> = chunk.iter().map(|b| i32::from(*b)).collect();
                        row(out, 29, field, at, j as i32, &bytes);
                    }
                }
                for (j, (name, value)) in o.vars.iter().enumerate() {
                    let (low, high, is_int) = match value {
                        sts2_core::EventVar::Int(x) => (*x as i32, (*x >> 32) as i32, 1),
                        _ => (0, 0, 0),
                    };
                    let var = row(out, 30, 0, at, j as i32, &[low, high, is_int]);
                    for (chunk, bytes) in name.as_bytes().chunks(12).enumerate() {
                        row(
                            out,
                            29,
                            3,
                            var,
                            chunk as i32,
                            &bytes.iter().map(|b| i32::from(*b)).collect::<Vec<_>>(),
                        );
                    }
                    if let sts2_core::EventVar::Text(text) = value {
                        for (chunk, bytes) in text.as_bytes().chunks(12).enumerate() {
                            row(
                                out,
                                29,
                                4,
                                var,
                                chunk as i32,
                                &bytes.iter().map(|b| i32::from(*b)).collect::<Vec<_>>(),
                            );
                        }
                    }
                }
                event.insert(i, at);
            }
        }
        let rest: Vec<_> = (if rm.phase()==Phase::RestSite {rs.rest_site_options()}else{Vec::new()})
            .into_iter()
            .enumerate()
            .map(|(i, x)| {
                use sts2_core::runstate::RestSiteOption::*;
                let (kind, enabled, value) = match x {
                    Rest => (
                        0,
                        true,
                        rs.compute_rest_site_heal_amount(rs.max_hp * 3 / 10),
                    ),
                    Smith { enabled } => (1, enabled, 0),
                    Dig => (2, true, 0),
                    Lift { lifts_done } => (3, lifts_done < 3, lifts_done as i32),
                    Cook { enabled } => (4, enabled, 0),
                    Clone => (5, true, 0),
                    Hatch => (6, true, 0),
                    Kindle => (7, true, 0),
                    Mend => (8, true, 0),
                };
                (
                    i,
                    row(out, 26, kind, 1, i as i32, &[i32::from(enabled), value]),
                )
            })
            .collect();
        let mut choice_context = -1;
        if actions.iter().any(|a| {
            matches!(
                a,
                RunAction::TransformDeckCard { .. } | RunAction::TransformDeckCards { .. }
            )
        }) {
            if let Some(c) = rs.pending_deck_transforms.first() {
                let target = c.fixed_target.as_ref().map_or(-1, |x| card(out, x, 0, 7));
                choice_context = row(
                    out,
                    28,
                    1,
                    target,
                    0,
                    &[
                        c.batch_count as i32,
                        i32::from(c.upgrade_result),
                        i32::from(c.basic_only),
                        c.post_hp_loss,
                        i32::from(c.fixed_target.is_some()),
                    ],
                );
            }
        }
        if actions
            .iter()
            .any(|a| matches!(a, RunAction::EnchantDeckCard { .. }))
        {
            if let Some(c) = rs.pending_deck_enchants.first() {
                choice_context = row(
                    out,
                    28,
                    2,
                    1,
                    0,
                    &[
                        c.enchantment as i32,
                        c.amount,
                        c.type_restriction.map_or(-1, |x| x as i32),
                        c.secondary_type.map_or(-1, |x| x as i32),
                    ],
                );
            }
        }
        let start = out.len() - 1;
        for (i, a) in actions.iter().enumerate() {
            let code;
            let (mut owner, mut target, mut v) = (-1, -1, [0; 8]);
            match *a {
                RunAction::EndTurn => code = 1,
                RunAction::PlayCard {
                    hand_idx,
                    target: t,
                } => {
                    code = 2;
                    owner = *combat_cards.get(&hand_idx).ok_or("combat hand reference")?;
                    if let Some(uid) = t {
                        target = *creatures.get(&(uid as i32)).ok_or("play target index")?;
                    }
                }
                RunAction::DrinkPotion {
                    potion_slot,
                    target: t,
                } => {
                    code = 3;
                    owner = out
                        .iter()
                        .enumerate()
                        .skip(1)
                        .find(|(_, r)| r[0] == 7 && r[3] == potion_slot as i32)
                        .map_or(-1, |(j, _)| j as i32 - 1);
                    v[1] = potion_slot as i32;
                    if owner >= 0 {
                        v[3] = out[owner as usize + 1][1];
                    }
                    if let Some(uid) = t {
                        target = *creatures.get(&(uid as i32)).ok_or("potion target index")?;
                    }
                }
                RunAction::CombatResolveChoice { option } => {
                    code = if option.is_some() { 4 } else { 5 };
                    v[1] = option.map_or(-1, |x| x as i32);
                    owner = out
                        .iter()
                        .enumerate()
                        .skip(1)
                        .find(|(_, r)| r[0] == 11 && r[3] == v[1])
                        .map_or(-1, |(j, _)| j as i32 - 1);
                }
                RunAction::CombatResolveChoiceTakeFirst { count } => {
                    code = 26;
                    v[1] = count.into();
                }
                RunAction::Move { col } => {
                    code = 6;
                    owner = *routes.get(&col).ok_or_else(|| {
                        format!(
                            "map route col={col}, origin={:?}, rows={:?}, phase={:?}, moves={:?}",
                            rm.run_state.current_room.and_then(|x| x.map_coord),
                            rm.run_state.current_act_map.as_ref().map(|m| m
                                .grid
                                .keys()
                                .map(|x| (x.row, x.col))
                                .collect::<Vec<_>>()),
                            rm.phase(),
                            actions
                        )
                    })?;
                    v[1] = col;
                }
                RunAction::ShopBuyCard { idx } => {
                    code = 7;
                    owner = shop[&(0, idx)];
                }
                RunAction::ShopBuyRelic { idx } => {
                    code = 7;
                    owner = shop[&(1, idx)];
                }
                RunAction::ShopBuyPotion { idx } => {
                    code = 7;
                    owner = shop[&(2, idx)];
                }
                RunAction::LeaveShop => code = 8,
                RunAction::PickCardReward { idx: Some(idx) } => {
                    code = 9;
                    owner = rewards[&idx];
                }
                RunAction::PickCardReward { idx: None } => code = 10,
                RunAction::PickRelicReward => {
                    code = 11;
                    if let Some(r) = rs.relic_rewards.first() {
                        owner = row(
                            out,
                            6,
                            schema.numeric.id(r.relic_id.to_python_name())?,
                            1,
                            0,
                            &[i32::from(r.is_wax)],
                        );
                    }
                }
                RunAction::SkipRelic => code = 10,
                RunAction::PickBossRelic { idx } => {
                    code = 12;
                    owner = row(
                        out,
                        6,
                        schema
                            .numeric
                            .id(rm.boss_relic_options()[idx].to_python_name())?,
                        1,
                        idx as i32,
                        &[],
                    );
                }
                RunAction::CollectTreasure => code = 13,
                RunAction::EventChoice { idx } => {
                    code = 14;
                    owner = *event.get(&idx).ok_or("event reference")?;
                }
                RunAction::ShopRemoveCard { deck_idx } => {
                    code = 15;
                    owner = deck[&deck_idx];
                    target = shop[&(3, 0)];
                }
                RunAction::RemoveDeckCard { idx } => {
                    code = 15;
                    let eligible = rs.removal_offer_order();
                    owner = *deck.get(&eligible[idx]).ok_or("remove reference")?;
                }
                RunAction::RestOption { idx } => {
                    code = 16;
                    owner = rest[idx].1;
                }
                RunAction::SmithCard { idx } | RunAction::UpgradeDeckCard { idx } => {
                    code = 17;
                    let ids: Vec<_> =
                        sts2_wire::emit::run_manager::effective_master_deck_indices(rm)
                            .into_iter()
                            .filter(|i| rs.master_deck[*i].is_upgradable())
                            .collect();
                    owner = deck[&ids[idx]];
                }
                RunAction::PotionSwap { discard_slot } => {
                    code = 18;
                    target = incoming_potion;
                    v[1] = discard_slot.map_or(-1, |x| x as i32);
                    owner = discard_slot.map_or(incoming_potion, |slot| {
                        out.iter()
                            .enumerate()
                            .skip(1)
                            .find(|(_, r)| r[0] == 7 && r[3] == slot as i32)
                            .map_or(-1, |(j, _)| j as i32 - 1)
                    });
                }
                RunAction::TransformDeckCard { idx } => {
                    code = 20;
                    owner = deck[&rm.transform_eligible_deck_indices()[idx]];
                    target = choice_context;
                }
                RunAction::EnchantDeckCard { idx } => {
                    code = 21;
                    owner = deck[&rm.enchant_eligible_deck_indices()[idx]];
                    target = choice_context;
                }
                RunAction::GrantDeckCard { idx } => {
                    code = 22;
                    owner = card(out, &rs.pending_card_grants[0].options[idx], idx, 7);
                }
                RunAction::CookCards { selection } => {
                    code = 23;
                    if let Some(pair) = selection {
                        let ids = rs.removal_offer_order();
                        owner = deck[&ids[pair[0]]];
                        target = deck[&ids[pair[1]]];
                    }
                }
                RunAction::TransformDeckCards { selection } => {
                    code = 24;
                    let ids = rm.transform_eligible_deck_indices();
                    owner = deck[&ids[selection[0]]];
                    target = deck[&ids[selection[1]]];
                }
                RunAction::SacrificeCardReward => code = 25,
                _ => return Err(format!("unencoded legal action {a:?}")),
            }
            v[0] = target;
            row(out, 12, code, owner, i as i32, &v);
        }
        // Entities referenced by late choices must precede the contiguous action block.
        // Reorder all action rows last and remap references, including those choices.
        let mut indices: Vec<_> = (1..out.len()).filter(|i| out[*i][0] != 12).collect();
        indices.extend((1..out.len()).filter(|i| out[*i][0] == 12));
        let mut map = vec![0; out.len()];
        for (j, &old) in indices.iter().enumerate() {
            map[old] = j as i32;
        }
        let mut packed = Vec::with_capacity(out.len());
        packed.push([0; 16]);
        for old in indices {
            let mut x = out[old];
            if x[2] >= 0 {
                x[2] = map[x[2] as usize + 1];
            }
            if (x[0] == 12 || x[0] == 22) && x[4] >= 0 {
                x[4] = map[x[4] as usize + 1];
            }
            packed.push(x);
        }
        let n = packed.len() - 1;
        let a = actions.len();
        packed[0] = [
            0x4e554d31,
            3,
            1,
            n as i32,
            16,
            8,
            0,
            0,
            0,
            0,
            n as i32,
            (n - a) as i32,
            a as i32,
            -1,
            i32::from(rm.phase() == Phase::RunOver),
            self.decisions as i32,
        ];
        packed[0][6] = self.generation as i32;
        packed[0][7] = (self.generation >> 32) as i32;
        packed[0][8]=i32::from(actions.iter().any(|a|matches!(a,RunAction::EndTurn|RunAction::PlayCard{..}|RunAction::DrinkPotion{..}|RunAction::CombatResolveChoice{..}|RunAction::CombatResolveChoiceTakeFirst{..})));
        packed[0][9] = phase(rm.phase());
        *out = packed;
        self.actions = actions;
        let _ = start;
        Ok(())
    }
}
struct Pool {
    slots: Vec<Slot>,
    threads: Arc<rayon::ThreadPool>,
    reset_cache: Option<Arc<ResetCache>>,
    reset_stride: u64,
    combat_limit:usize,
    start_bank: Option<StartBank>,
    schema: Arc<Schema>,
}
thread_local! {static ERROR:RefCell<CString>=RefCell::new(CString::new("").unwrap());static OUTPUT:RefCell<CString>=RefCell::new(CString::new("").unwrap());}
fn failure(e: String) -> i32 {
    ERROR.with(|x| *x.borrow_mut() = CString::new(e.replace('\0', " ")).unwrap());
    -1
}
fn guarded(f: impl FnOnce() -> Result<()>) -> i32 {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(Ok(())) => 0,
        Ok(Err(e)) => failure(e),
        Err(_) => failure("native encoder panic (inspect references/state)".into()),
    }
}
#[no_mangle]
pub extern "C" fn sm_fr_error() -> *const c_char {
    ERROR.with(|x| x.borrow().as_ptr())
}
#[no_mangle]
pub unsafe extern "C" fn sm_fr_create(
    schema: *const c_char,
    slots: usize,
    workers: usize,
    seed: u64,
) -> *mut c_void {
    let mut out = std::ptr::null_mut();
    let code = guarded(|| {
        if slots == 0 || slots > 8192 || workers == 0 || workers > 128 {
            return Err("invalid pool dimensions".into());
        }
        let path = CStr::from_ptr(schema).to_str().map_err(|e| e.to_string())?;
        let labels: Vec<Value> = serde_json::from_str(
            &std::fs::read_to_string(format!("{path}/registry.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let meta = serde_json::from_str(
            &std::fs::read_to_string(format!("{path}/metadata.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let schema = Arc::new(Schema::new(&labels, meta)?);
        let threads = Arc::new(rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .map_err(|e| e.to_string())?);
        let runs = (0..slots).map(|i| Slot::new(seed + i as u64)).collect();
        out = Box::into_raw(Box::new(Pool {
            slots: runs,
            threads,
            reset_cache:None,
            reset_stride:slots as u64,
            combat_limit:0,
            start_bank:None,
            schema,
        })) as *mut c_void;
        Ok(())
    });
    if code < 0 {
        std::ptr::null_mut()
    } else {
        out
    }
}
#[no_mangle]
pub unsafe extern "C" fn sm_fr_destroy(p: *mut c_void) {
    if !p.is_null() {
        drop(Box::from_raw(p as *mut Pool));
    }
}
#[no_mangle]
pub unsafe extern "C" fn sm_fr_encode(
    p: *mut c_void,
    dst: *mut i32,
    rows: usize,
    lengths: *mut u32,
    actions: *mut u32,
) -> i32 {
    guarded(|| {
        let p = &mut *(p as *mut Pool);
        if dst.is_null() || rows == 0 {
            return Err("null/empty destination".into());
        }
        let schema = &p.schema;
        for s in &mut p.slots{s.enable_combat_limit(p.combat_limit)?;}
        p.threads
            .install(|| p.slots.par_iter_mut().try_for_each(|s| s.encode(schema)))?;
        let n = p.slots.len();
        let d = std::slice::from_raw_parts_mut(dst, n * rows * 16);
        let l = std::slice::from_raw_parts_mut(lengths, n);
        let a = std::slice::from_raw_parts_mut(actions, n);
        for (i, s) in p.slots.iter().enumerate() {
            if s.rows.len() > rows {
                return Err(format!("row capacity {rows} < required {}", s.rows.len()));
            }
            let slot = &mut d[i * rows * 16..(i + 1) * rows * 16];
            // Padding is sanitized by the decoder before all lookups.
            // Write only real records, avoiding a full-capacity host clear.
            for (j, r) in s.rows.iter().enumerate() {
                slot[j * 16..j * 16 + 16].copy_from_slice(r);
            }
            l[i] = s.rows.len() as u32;
            a[i] = s.actions.len() as u32;
        }
        Ok(())
    })
}
#[no_mangle]
pub unsafe extern "C" fn sm_fr_step(p: *mut c_void, indices: *const i32) -> i32 {
    guarded(|| {
        let p = &mut *(p as *mut Pool);
        let choices = std::slice::from_raw_parts(indices, p.slots.len());
        p.threads.install(|| {
            p.slots
                .par_iter_mut()
                .zip(choices.par_iter())
                .try_for_each(|(s, &i)| {
                    if i < 0 {
                        return Ok(());
                    }
                    let a = *s
                        .actions
                        .get(i as usize)
                        .ok_or("action index out of range")?;
                    let result = s.rm.take_action(a);
                    if !matches!(
                        result,
                        sts2_core::run_manager::ActionResult::Advanced
                            | sts2_core::run_manager::ActionResult::RunOver
                    ) {
                        return Err(format!("rejected legal action {a:?}: {result:?}"));
                    }
                    s.decisions += 1;
                    s.generation += 1;
                    s.prepare_decision()?;
                    Ok(())
                })
        })
    })
}
// Explicit coverage-only utility. Never called by fr_step or policy collection.
#[no_mangle]
pub unsafe extern "C" fn sm_fr_coverage_combat(p: *mut c_void) -> i32 {
    guarded(|| {
        let p = &mut *(p as *mut Pool);
        for s in &mut p.slots {
            if s.rm.phase() == Phase::Combat {
                s.rm.resolve_combat_fast(0.05, true);
                s.generation += 1;
                s.prepare_decision()?;
            }
        }
        Ok(())
    })
}
#[no_mangle]
pub unsafe extern "C" fn sm_fr_debug(p: *mut c_void, index: usize) -> *const c_char {
    let mut result = std::ptr::null();
    guarded(|| {
        let p = &mut *(p as *mut Pool);
        let s = p.slots.get_mut(index).ok_or("slot index")?;
        let before = sts2_core::hashing::run_hash(&s.rm);
        s.encode(&p.schema)?;
        let after = sts2_core::hashing::run_hash(&s.rm);
        if before != after {
            return Err("encoding mutated run/RNG".into());
        }
        let rs = &s.rm.run_state;
        let value = json!({"character":format!("{:?}",s.rm.character_id),"seed":s.ppo.seed,"player_won":rs.player_won,"run_decisions":s.ppo.actions.len(),"combat_actions":s.ppo.combat_actions,"combat_action_limit":s.ppo.combat_limit,"combat_timeout":s.ppo.timed_out,"phase":phase(s.rm.phase()),"generation":s.generation,"hash":after,"rows":s.rows,"actions":s.actions.iter().map(sts2_wire::emit::run_action_to_value).collect::<Vec<_>>(),"map":sts2_wire::emit::run_manager::full_map(&s.rm),"shop":sts2_wire::emit::run_manager::shop_inventory(&s.rm),"events":sts2_wire::emit::run_manager::event_option_infos(&s.rm),"removal_options":sts2_wire::emit::run_manager::card_removal_options(&s.rm),"choice":s.rm.combat.as_ref().and_then(|c|c.pending_choice.as_ref()).map(|p|json!({"min":p.min_select,"max":p.max_select,"picked":p.picked.to_vec()})),"gold":rs.gold,"hp":rs.current_hp,"max_hp":rs.max_hp,"act":rs.act_index,"floor":rs.total_floor,"deck":sts2_wire::emit::run_manager::master_deck_detailed(&s.rm)});
        OUTPUT.with(|x| {
            *x.borrow_mut() = CString::new(value.to_string()).unwrap();
            result = x.borrow().as_ptr();
        });
        Ok(())
    });
    result
}

#[no_mangle]
pub unsafe extern "C" fn sm_fr_reset_terminal_ppo(p: *mut c_void, seed: u64) -> i32 {
    let mut reset = 0;
    let code = guarded(|| {
        let p = &mut *(p as *mut Pool);
        if let Some(bank)=p.start_bank.as_mut(){
            let stride=p.reset_stride;
            if let Some(queues)=bank.rolling.as_mut(){
                for(i,s)in p.slots.iter().enumerate(){if s.rm.phase()==Phase::RunOver{let expected=s.ppo.seed.checked_add(stride).ok_or("seed overflow")?;if expected>=1u64<<63||queues[i].front().map(|x|x.0)!=Some(expected){return Err(format!("rolling start queue missing/out of training namespace in slot {i}"))}}}
                for(i,s)in p.slots.iter_mut().enumerate(){if s.rm.phase()==Phase::RunOver{let(seed,rm)=queues[i].pop_front().unwrap();let generation=s.generation+1;*s=Slot{rm,view:dummy_session(),rows:Vec::with_capacity(1024),actions:Vec::new(),decisions:0,generation,ppo:PpoState{seed,..Default::default()}};bank.consumed+=1;reset+=1;}}
                return Ok(());
            }
            // Validate every needed seed before mutating any live slot.
            for s in &p.slots{if s.rm.phase()==Phase::RunOver{let seed=s.ppo.seed+stride;let ix=seed.checked_sub(bank.base).and_then(|v|usize::try_from(v).ok()).ok_or("start-bank seed below range")?;if bank.starts.get(ix).is_none_or(|v|v.is_none()){return Err(format!("start bank exhausted/missing seed {seed}; regenerate a larger bank before resuming"))}}}
            for s in &mut p.slots{if s.rm.phase()==Phase::RunOver{let seed=s.ppo.seed+stride;let generation=s.generation+1;let rm=bank.starts[(seed-bank.base) as usize].take().unwrap();*s=Slot{rm,view:dummy_session(),rows:Vec::with_capacity(1024),actions:Vec::new(),decisions:0,generation,ppo:PpoState{seed,..Default::default()}};bank.consumed+=1;reset+=1;}}
        }else if let Some(cache)=p.reset_cache.clone(){
            for (i,s) in p.slots.iter_mut().enumerate(){if s.rm.phase()==Phase::RunOver{
                let generation=s.generation+1;let expected=s.ppo.seed+cache.stride;
                let mut next=cache.take(i)?;if next.ppo.seed!=expected{return Err("prefetched reset seed mismatch".into())}
                next.generation=generation;*s=next;cache.schedule(&p.threads,i,expected+cache.stride);reset+=1;
            }}
        }else{
            let stride=p.reset_stride;
            reset=p.threads.install(||p.slots.par_iter_mut().map(|s|{if s.rm.phase()==Phase::RunOver{let generation=s.generation+1;let next_seed=s.ppo.seed+stride;*s=Slot::new(next_seed);s.generation=generation;1}else{0}}).sum());
        }
        let _=seed;
        Ok(())
    });
    if code < 0 {
        code
    } else {
        reset
    }
}

#[no_mangle]
pub unsafe extern "C" fn sm_fr_step_checked(
    p: *mut c_void,
    indices: *const i32,
    generations: *const u64,
) -> i32 {
    let pool = &*(p as *mut Pool);
    let a = std::slice::from_raw_parts(indices, pool.slots.len());
    let g = std::slice::from_raw_parts(generations, pool.slots.len());
    for (i, s) in pool.slots.iter().enumerate() {
        if a[i] >= 0 && (g[i] != s.generation || a[i] as usize >= s.actions.len()) {
            return failure(format!("stale generation or invalid action in slot {i}"));
        }
    }
    sm_fr_step(p, indices)
}
// Isolated synthetic witnesses for action-reference tests; never used in collection.
#[no_mangle]
pub unsafe extern "C" fn sm_fr_test_case(p: *mut c_void, index: usize, kind: u32) -> i32 {
    guarded(|| {
        let p = &mut *(p as *mut Pool);
        let s = p.slots.get_mut(index).ok_or("fixture slot")?;
        let generation = s.generation;
        *s = Slot::new(7342 + index as u64);
        s.generation = generation;
        let make = |id, upgraded| {
            sts2_content::cards::spec_index::card_pod_from_id(id, upgraded)
                .ok_or("missing card spec")
        };
        match kind {
            1 => {
                for _ in 0..30 {
                    if s.rm.phase() == Phase::Combat {
                        break;
                    }
                    let a = *s
                        .rm
                        .get_available_actions()
                        .first()
                        .ok_or("no fixture path")?;
                    s.rm.take_action(a);
                    s.prepare_decision()?;
                }
                let c = s.rm.combat.as_mut().ok_or("no combat fixture")?;
                let mut opts = Vec::new();
                for uid in c.player.hand.iter().take(4) {
                    let x = sts2_core::card_by_uid(&c.cards, *uid).ok_or("hand card")?;
                    opts.push(sts2_core::ChoiceOption {
                        card_id: x.card_id,
                        upgraded: x.upgraded,
                        card_uid: Some(*uid),
                        source_pile: Some(PileKind::Hand),
                    });
                }
                if opts.len() < 4 {
                    return Err("fixture needs four cards".into());
                }
                c.pending_choice = Some(sts2_core::PendingChoice {
                    kind: sts2_core::ChoiceKind::ExhaustFromHand,
                    options: opts,
                    min_select: 2,
                    max_select: 2,
                    ..Default::default()
                });
            }
            2 => {
                s.rm.run_state
                    .master_deck
                    .push(make(sts2_core::cards::CardId::Injury, false)?);
                s.rm.run_state.pending_deck_removals = 1;
            }
            3 => {
                let id = s.rm.run_state.master_deck[0].card_id;
                s.rm.run_state.master_deck[0] = make(id, true)?;
                s.rm.run_state.pending_deck_upgrades = 1;
            }
            4 => {
                s.rm.run_state.pending_deck_enchants.push(
                    sts2_core::runstate::PendingDeckEnchant {
                        enchantment: sts2_core::cards::EnchantmentId::Slither,
                        amount: 1,
                        type_restriction: None,
                        secondary_type: None,
                        post_selection_rng_read: None,
                    },
                );
            }
            5 => {
                s.rm.run_state.pending_deck_transforms.push(
                    sts2_core::runstate::PendingDeckTransform {
                        batch_count: 1,
                        selected_deck_indices: Vec::new(),
                        upgrade_result: true,
                        basic_only: false,
                        fixed_target: Some(make(
                            sts2_core::cards::CardId::DefendNecrobinder,
                            false,
                        )?),
                        event_stream_roll: None,
                        post_hp_loss: 3,
                    },
                );
            }
            6 | 7 => {
                let m =
                    s.rm.run_state
                        .current_act_map
                        .as_ref()
                        .ok_or("fixture map")?;
                let shop = m
                    .grid
                    .values()
                    .copied()
                    .find(|i| {
                        m.point(*i).point_type == MapPointType::Shop
                            && m.point(*i)
                                .parents
                                .iter()
                                .any(|j| m.grid.contains_key(&m.point(*j).coord))
                    })
                    .ok_or("no shop witness")?;
                let parent = *m
                    .point(shop)
                    .parents
                    .iter()
                    .find(|j| m.grid.contains_key(&m.point(**j).coord))
                    .ok_or("shop parent")?;
                let pc = m.point(parent).coord;
                let col = m.point(shop).coord.col;
                s.rm.debug_place_on_map(1, pc, vec![pc], pc.row + 1, pc.row + 1)?;
                s.rm.take_action(RunAction::Move { col });
                if s.rm.phase() != Phase::Shop {
                    return Err("shop transition".into());
                }
                s.rm.run_state.gold = if kind == 6 { 10000 } else { 0 };
                if kind == 6 {
                    s.rm.run_state
                        .obtain_relic(sts2_core::RelicId::MembershipCard);
                }
            }
            8 => {
                s.rm.enter_rest_site();
                s.rm.take_action(RunAction::RestOption { idx: 1 });
            }
            9 => {
                s.rm.run_state
                    .pending_card_grants
                    .push(sts2_core::runstate::PendingCardGrant {
                        options: vec![
                            make(sts2_core::cards::CardId::StrikeNecrobinder, false)?,
                            make(sts2_core::cards::CardId::DefendNecrobinder, true)?,
                            make(sts2_core::cards::CardId::Soul, false)?,
                        ],
                        selected: Vec::new(),
                        pick: 2,
                    });
            }
            10 => {
                s.rm.run_state.potions = vec![
                    sts2_core::potions::PotionKind::BlockPotion,
                    sts2_core::potions::PotionKind::FirePotion,
                ];
                s.rm.run_state.max_potion_slots = 2;
                s.rm.run_state
                    .pending_potion_swaps
                    .push(sts2_core::potions::PotionKind::EnergyPotion);
            }
            _ => return Err("unknown test witness".into()),
        }
        s.generation += 1;
        Ok(())
    })
}

// Counterfactual privacy check: changing private seeds and draw order cannot
// change public tokens, and the original simulator is restored exactly.
#[no_mangle]
pub unsafe extern "C" fn sm_fr_test_private_invariance(p: *mut c_void, index: usize) -> i32 {
    guarded(|| {
        let p = &mut *(p as *mut Pool);
        let s = p.slots.get_mut(index).ok_or("privacy slot")?;
        let original = s.rm.clone();
        let hash = sts2_core::hashing::run_hash(&original);
        s.encode(&p.schema)?;
        let baseline = s.rows.clone();
        s.rm.seed ^= 0x12345678;
        s.rm.run_state.rewards_rng_seed ^= 0x98765432;
        if let Some(c) = s.rm.combat.as_mut() {
            c.player.draw.reverse();
        }
        let result = s.encode(&p.schema).map(|_| s.rows == baseline);
        s.rm = original;
        s.encode(&p.schema)?;
        if sts2_core::hashing::run_hash(&s.rm) != hash {
            return Err("privacy check failed to restore simulator".into());
        }
        if !result? {
            return Err("private seed or draw order leaked into public encoding".into());
        }
        Ok(())
    })
}

mod ppo;

mod curriculum;

mod runvalue;
