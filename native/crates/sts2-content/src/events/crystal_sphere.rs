//! Beta CrystalSphere event and hidden-cell minigame. Source:
//! Models.Events/CrystalSphere.cs and Events.Custom.CrystalSphereEvent/*.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
use super::{crystal_sphere_allowed, EventOptionSpec, EventSpec};
use sts2_core::card_rewards::RewardRarity;
use sts2_core::cards::CardId;
use sts2_core::dotnet_rng::Rng;
use sts2_core::potions::PotionRarity;
use sts2_core::relic_effects::AddSource;
use sts2_core::EventVarSpec;
use sts2_core::{CardReward, EventId, EventOptionInfo, EventOutcome, Reward, RunState};

const WIDTH: usize = 11;
const CELL_COUNT: usize = WIDTH * WIDTH;
// Scratch encoding: cost, divinations, tool, placed, item count, 121 cells,
// then (kind,x,y,subscription_count) per item and ordered revealed indices.
// A hidden empty cell is 0, hidden item i is i+1, and clearing v maps to -v-1.
#[derive(Clone, Debug, PartialEq)]
struct Board {
    cost: i16,
    divinations: i16,
    small: bool,
    placed: bool,
    cells: Vec<i16>,
    items: Vec<[i16; 4]>,
    revealed: Vec<usize>,
}
impl Board {
    fn load(state: &RunState) -> Self {
        let s = &state.event_state.scratch;
        let count = s[4] as usize;
        let end = 5 + CELL_COUNT + count * 4;
        Self {
            cost: s[0],
            divinations: s[1],
            small: s[2] != 0,
            placed: s[3] != 0,
            cells: s[5..5 + CELL_COUNT].to_vec(),
            items: s[5 + CELL_COUNT..end]
                .chunks_exact(4)
                .map(|v| [v[0], v[1], v[2], v[3]])
                .collect(),
            revealed: s[end..].iter().map(|v| *v as usize).collect(),
        }
    }
    fn save(&self, state: &mut RunState) {
        let mut s = vec![
            self.cost,
            self.divinations,
            i16::from(self.small),
            i16::from(self.placed),
            self.items.len() as i16,
        ];
        s.extend(&self.cells);
        for item in &self.items {
            s.extend(item);
        }
        s.extend(self.revealed.iter().map(|v| *v as i16));
        state.event_state.scratch = s;
    }
    fn new(cost: i16, divinations: i16, rng: &mut Rng) -> Self {
        let mut board = Self {
            cost,
            divinations,
            small: false,
            placed: false,
            cells: vec![0; CELL_COUNT],
            items: Vec::new(),
            revealed: Vec::new(),
        };
        for x in 0..WIDTH {
            for y in 0..WIDTH {
                if x.min(10 - x) + y.min(10 - y) <= 2 {
                    board.cells[x * WIDTH + y] = -1;
                }
            }
        }
        // Source does not clear a failed partial attempt and re-subscribes
        // every accumulated item at the end of each PopulateItems call.
        for _ in 0..10 {
            let mut success = true;
            for kind in [0, 1, 1, 2, 3, 4, 5, 6, 7, 7, 7, 7, 7, 8, 8] {
                let mut item = [kind, 0, 0, 0];
                if success {
                    let (width, height) = size(kind);
                    let mut spots = Vec::new();
                    for x in 0..WIDTH {
                        for y in 0..WIDTH {
                            if x + width <= WIDTH
                                && y + height <= WIDTH
                                && (x..x + width).all(|i| {
                                    (y..y + height).all(|j| board.cells[i * WIDTH + j] == 0)
                                })
                            {
                                spots.push((x, y));
                            }
                        }
                    }
                    if spots.is_empty() {
                        success = false;
                    } else {
                        let (x, y) = spots[rng.next_int_exclusive(0, spots.len() as i32) as usize];
                        item[1] = x as i16;
                        item[2] = y as i16;
                        for i in x..x + width {
                            for j in y..y + height {
                                board.cells[i * WIDTH + j] = (board.items.len() + 1) as i16;
                            }
                        }
                    }
                }
                board.items.push(item);
            }
            for item in &mut board.items {
                item[3] += 1;
            }
            if success {
                board.placed = true;
                break;
            }
        }
        board
    }
    fn clear(&mut self, state: &mut RunState, x: usize, y: usize) {
        let index = x * WIDTH + y;
        let value = self.cells[index];
        if value < 0 {
            return;
        }
        self.cells[index] = -value - 1;
        if value == 0 {
            return;
        }
        let item_index = (value - 1) as usize;
        let [kind, ix, iy, subscriptions] = self.items[item_index];
        let (w, h) = size(kind);
        if (ix as usize..ix as usize + w)
            .all(|i| (iy as usize..iy as usize + h).all(|j| self.cells[i * WIDTH + j] < 0))
        {
            for _ in 0..subscriptions {
                self.revealed.push(item_index);
            }
            if kind == 6 {
                add_curse(state, CardId::Doubt);
            }
        }
    }
    fn click(&mut self, state: &mut RunState, x: usize, y: usize) {
        self.divinations -= 1;
        if self.small {
            self.clear(state, x, y);
            return;
        }
        // Horizontal, vertical, diagonals, center is observable reveal order.
        for (dx, dy) in [
            (-1, 0),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (-1, 1),
            (1, -1),
            (1, 1),
            (0, 0),
        ] {
            let xx = x as i32 + dx;
            let yy = y as i32 + dy;
            if (0..11).contains(&xx) && (0..11).contains(&yy) {
                self.clear(state, xx as usize, yy as usize);
            }
        }
    }
    fn visible_grid(&self) -> String {
        let mut out=String::from("Visible grid (# hidden, . empty, R relic, p common potion, P rare potion, c/u/r card rarity, ! curse, g/G gold):\n");
        for y in 0..WIDTH {
            for x in 0..WIDTH {
                let v = self.cells[x * WIDTH + y];
                out.push(if v >= 0 {
                    '#'
                } else if v == -1 {
                    '.'
                } else {
                    ['R', 'p', 'P', 'c', 'u', 'r', '!', 'g', 'G']
                        [self.items[(-v - 2) as usize][0] as usize]
                });
            }
            out.push('\n');
        }
        out
    }
}
fn size(kind: i16) -> (usize, usize) {
    match kind {
        0 => (4, 4),
        1 => (1, 3),
        7 => (1, 1),
        8 => (2, 1),
        _ => (2, 2),
    }
}
fn add_curse(state: &mut RunState, id: CardId) {
    if let Some(card) = crate::cards::spec_index::card_pod_from_id(id, false) {
        state.add_card_to_deck(card, AddSource::EventGain);
    }
}
pub(super) fn prepare(state: &mut RunState) {
    if state.event_state.page != 0 || !state.event_state.scratch.is_empty() {
        return;
    }
    let mut rng = state
        .event_state
        .rng
        .take()
        .unwrap_or_else(|| Rng::with_name(state.rng.seed, "CRYSTAL_SPHERE"));
    let cost = 50 + rng.next_int_exclusive(1, 50);
    state.event_state.scratch = vec![cost as i16];
    state.event_state.rng = Some(rng);
}
fn begin(state: &mut RunState, debt: bool) -> EventOutcome {
    prepare(state);
    let cost = state.event_state.scratch[0];
    if debt {
        add_curse(state, CardId::Debt);
    } else {
        state.gain_gold(-i32::from(cost));
    }
    let mut rng = state.event_state.rng.take().expect("prepared event RNG");
    let board = Board::new(cost, if debt { 6 } else { 3 }, &mut rng);
    board.save(state);
    state.event_state.rng = Some(rng);
    state.event_state.page = 1;
    ongoing("Choose a tool and a hidden cell to uncover the future.")
}
fn pay(state: &mut RunState) -> EventOutcome {
    begin(state, false)
}
fn debt(state: &mut RunState) -> EventOutcome {
    begin(state, true)
}
fn ongoing(text: &str) -> EventOutcome {
    EventOutcome {
        finished: false,
        description: text.to_string(),
        effects: Vec::new(),
    }
}
fn generate_options(state: &RunState, _spec: &EventSpec) -> Vec<EventOptionInfo> {
    if state.event_state.page == 0 {
        let cost = state
            .event_state
            .scratch
            .first()
            .copied()
            .unwrap_or_else(|| {
                let mut r = Rng::with_name(state.rng.seed, "CRYSTAL_SPHERE");
                (50 + r.next_int_exclusive(1, 50)) as i16
            });
        return vec![
            option(
                "pay",
                "CRYSTAL_SPHERE.pages.INITIAL.options.UNCOVER_FUTURE",
                format!("Uncover Future ({cost}g)"),
                "3 divinations.".to_string(),
                state.gold >= i32::from(cost),
            ),
            option(
                "debt",
                "CRYSTAL_SPHERE.pages.INITIAL.options.PAYMENT_PLAN",
                "Payment Plan".to_string(),
                "Gain Debt and 6 divinations.".to_string(),
                true,
            ),
        ];
    }
    let b = Board::load(state);
    let mut out = vec![option(
        if b.small { "tool_big" } else { "tool_small" },
        "",
        if b.small {
            "Use Big Tool"
        } else {
            "Use Small Tool"
        }
        .to_string(),
        format!(
            "{} divinations remain.\n{}",
            b.divinations,
            b.visible_grid()
        ),
        true,
    )];
    for (i, id) in CELL_IDS.iter().enumerate() {
        if b.cells[i] >= 0 {
            out.push(option(
                id,
                "",
                format!("Reveal ({},{})", i / WIDTH, i % WIDTH),
                format!("{} tool", if b.small { "Small" } else { "Big" }),
                true,
            ));
        }
    }
    out
}
fn option(
    id: &'static str,
    key: &'static str,
    label: String,
    description: String,
    enabled: bool,
) -> EventOptionInfo {
    EventOptionInfo {
        option_id: id,
        loc_key: key,
        label,
        description,
        enabled,
        vars: Vec::new(),
        title_vars: Vec::new(),
    }
}
pub(super) fn resolve_board(state: &mut RunState, id: &str) -> Option<EventOutcome> {
    let mut board = Board::load(state);
    match id {
        "tool_small" => board.small = true,
        "tool_big" => board.small = false,
        _ => {
            let index = CELL_IDS.iter().position(|v| *v == id)?;
            if board.divinations <= 0 || board.cells[index] < 0 {
                return None;
            }
            board.click(state, index / WIDTH, index % WIDTH);
        }
    }
    let finished = board.divinations == 0;
    board.save(state);
    if finished {
        finish_rewards(state, &board);
        Some(EventOutcome::finished("The future is revealed."))
    } else {
        Some(ongoing("Continue uncovering the future."))
    }
}
fn finish_rewards(state: &mut RunState, board: &Board) {
    let mut rng = state.event_state.rng.take().expect("minigame RNG");
    // ToReward constructs ALL rewards first; potions alone draw here.
    let mut rewards = Vec::new();
    for &index in &board.revealed {
        let kind = board.items[index][0];
        let reward = match kind {
            0 => Reward::Relic(sts2_core::RelicReward::new(sts2_core::RelicId::Circlet)),
            1 | 2 => {
                let rarity = if kind == 1 {
                    PotionRarity::Common
                } else {
                    PotionRarity::Rare
                };
                let pool: Vec<_> = crate::potions::all::full_potion_options(state.character_id)
                    .into_iter()
                    .filter(|p| crate::potions::all::spec_for(*p).rarity == rarity)
                    .collect();
                let potion = pool[rng.next_int_exclusive(0, pool.len() as i32) as usize];
                Reward::Potion(sts2_core::PotionReward::with_kind(potion))
            }
            3..=5 => {
                let rarity = match kind {
                    3 => RewardRarity::Common,
                    4 => RewardRarity::Uncommon,
                    _ => RewardRarity::Rare,
                };
                let mut reward = CardReward::with_forced_rarity(rarity);
                reward.roll_upgrade = true;
                Reward::Card(reward)
            }
            7 | 8 => Reward::Gold {
                amount: if kind == 7 { 10 } else { 30 },
            },
            _ => continue,
        };
        rewards.push(reward);
    }
    // Then RewardsSet.GenerateWithoutOffering populates in reveal order.
    for reward in &mut rewards {
        match reward {
            Reward::Gold { amount } => {
                let _ = rng.next_int_exclusive(*amount, *amount + 1);
            }
            Reward::Relic(reward) => {
                let roll = rng.next_float(1.0) as f32;
                let rarity = if roll < 0.5 {
                    sts2_core::RelicRarity::Common
                } else if roll < 0.83 {
                    sts2_core::RelicRarity::Uncommon
                } else {
                    sts2_core::RelicRarity::Rare
                };
                reward.relic_id = state
                    .pop_relic_from_grab_bag(rarity)
                    .unwrap_or(sts2_core::RelicId::Circlet);
            }
            Reward::Card(reward) => {
                let cards = crate::run_callbacks::generate_fixed_rarity_cards_with_rng(
                    state, reward, &mut rng,
                );
                reward.prepared_cards = state.offer_card_reward(cards);
                reward.options = reward.prepared_cards.iter().map(|c| c.card_id).collect();
                reward.forced_rarity = None;
                reward.populated = true;
            }
            _ => {}
        }
    }
    state.event_state.rng = Some(rng);
    let rewards = state.offer_rewards(sts2_core::RoomType::Event, rewards);
    state.pending_rewards.extend(rewards);
}
fn cost_vars(state: &RunState) -> Vec<(&'static str, sts2_core::EventVar)> {
    let cost = state.event_state.scratch.first().copied().unwrap_or(50);
    vec![
        (
            "UncoverFutureCost",
            sts2_core::EventVar::Int(i64::from(cost)),
        ),
        ("UncoverFutureProphesizeCount", sts2_core::EventVar::Int(3)),
    ]
}
/// Policy fallback uses only the visible grid. Never score hidden item IDs.
pub(super) fn policy_option(state: &RunState) -> Option<usize> {
    if state.current_event != Some(EventId::CrystalSphere) || state.event_state.page == 0 {
        return None;
    }
    let board = Board::load(state);
    if board.small {
        return Some(0);
    } // switch to Big, then the next call clicks
    let options = generate_options(state, &CRYSTAL_SPHERE);
    options
        .iter()
        .enumerate()
        .filter_map(|(option_index, option)| {
            let index = CELL_IDS.iter().position(|id| *id == option.option_id)?;
            let x = index / WIDTH;
            let y = index % WIDTH;
            let mut score = 0;
            for xx in x.saturating_sub(1)..=(x + 1).min(10) {
                for yy in y.saturating_sub(1)..=(y + 1).min(10) {
                    let cell = board.cells[xx * WIDTH + yy];
                    if cell >= 0 {
                        score += 1;
                    } else if cell < -1 && board.items[(-cell - 2) as usize][0] == 6 {
                        score -= 100;
                    }
                }
            }
            Some((option_index, score))
        })
        .max_by_key(|(index, score)| (*score, std::cmp::Reverse(*index)))
        .map(|(index, _)| index)
}

const OPTIONS: &[EventOptionSpec] = &[
    EventOptionSpec {
        option_id: "pay",
        loc_key: "CRYSTAL_SPHERE.pages.INITIAL.options.UNCOVER_FUTURE",
        default_label: "Uncover Future",
        default_description: "Pay gold for 3 divinations.",
        default_vars: &[
            ("UncoverFutureCost", EventVarSpec::Int(50)),
            ("UncoverFutureProphesizeCount", EventVarSpec::Int(3)),
        ],
        vars_fn: Some(cost_vars),
        title_vars_fn: None,
        handler: pay,
    },
    EventOptionSpec {
        option_id: "debt",
        loc_key: "CRYSTAL_SPHERE.pages.INITIAL.options.PAYMENT_PLAN",
        default_label: "Payment Plan",
        default_description: "Gain Debt and 6 divinations.",
        default_vars: &[
            ("CurseTitle", EventVarSpec::Text("Debt")),
            ("PaymentPlanCount", EventVarSpec::Int(6)),
        ],
        vars_fn: None,
        title_vars_fn: None,
        handler: debt,
    },
];
pub const CRYSTAL_SPHERE: EventSpec = EventSpec {
    id: EventId::CrystalSphere,
    python_name: "CrystalSphere",
    is_allowed: crystal_sphere_allowed,
    generate_options,
    options: OPTIONS,
};
const CELL_IDS: [&str; 121] = [
    "cell_0_0",
    "cell_0_1",
    "cell_0_2",
    "cell_0_3",
    "cell_0_4",
    "cell_0_5",
    "cell_0_6",
    "cell_0_7",
    "cell_0_8",
    "cell_0_9",
    "cell_0_10",
    "cell_1_0",
    "cell_1_1",
    "cell_1_2",
    "cell_1_3",
    "cell_1_4",
    "cell_1_5",
    "cell_1_6",
    "cell_1_7",
    "cell_1_8",
    "cell_1_9",
    "cell_1_10",
    "cell_2_0",
    "cell_2_1",
    "cell_2_2",
    "cell_2_3",
    "cell_2_4",
    "cell_2_5",
    "cell_2_6",
    "cell_2_7",
    "cell_2_8",
    "cell_2_9",
    "cell_2_10",
    "cell_3_0",
    "cell_3_1",
    "cell_3_2",
    "cell_3_3",
    "cell_3_4",
    "cell_3_5",
    "cell_3_6",
    "cell_3_7",
    "cell_3_8",
    "cell_3_9",
    "cell_3_10",
    "cell_4_0",
    "cell_4_1",
    "cell_4_2",
    "cell_4_3",
    "cell_4_4",
    "cell_4_5",
    "cell_4_6",
    "cell_4_7",
    "cell_4_8",
    "cell_4_9",
    "cell_4_10",
    "cell_5_0",
    "cell_5_1",
    "cell_5_2",
    "cell_5_3",
    "cell_5_4",
    "cell_5_5",
    "cell_5_6",
    "cell_5_7",
    "cell_5_8",
    "cell_5_9",
    "cell_5_10",
    "cell_6_0",
    "cell_6_1",
    "cell_6_2",
    "cell_6_3",
    "cell_6_4",
    "cell_6_5",
    "cell_6_6",
    "cell_6_7",
    "cell_6_8",
    "cell_6_9",
    "cell_6_10",
    "cell_7_0",
    "cell_7_1",
    "cell_7_2",
    "cell_7_3",
    "cell_7_4",
    "cell_7_5",
    "cell_7_6",
    "cell_7_7",
    "cell_7_8",
    "cell_7_9",
    "cell_7_10",
    "cell_8_0",
    "cell_8_1",
    "cell_8_2",
    "cell_8_3",
    "cell_8_4",
    "cell_8_5",
    "cell_8_6",
    "cell_8_7",
    "cell_8_8",
    "cell_8_9",
    "cell_8_10",
    "cell_9_0",
    "cell_9_1",
    "cell_9_2",
    "cell_9_3",
    "cell_9_4",
    "cell_9_5",
    "cell_9_6",
    "cell_9_7",
    "cell_9_8",
    "cell_9_9",
    "cell_9_10",
    "cell_10_0",
    "cell_10_1",
    "cell_10_2",
    "cell_10_3",
    "cell_10_4",
    "cell_10_5",
    "cell_10_6",
    "cell_10_7",
    "cell_10_8",
    "cell_10_9",
    "cell_10_10",
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    fn rng_state(v: &Value) -> sts2_core::dotnet_rng::RngState {
        sts2_core::dotnet_rng::RngState {
            counter: v["counter"].as_u64().unwrap() as u32,
            s0: v["s0"].as_u64().unwrap(),
            s1: v["s1"].as_u64().unwrap(),
            s2: v["s2"].as_u64().unwrap(),
            s3: v["s3"].as_u64().unwrap(),
        }
    }
    fn assert_grid(board: &Board, value: &Value) {
        for column in value.as_array().unwrap() {
            for cell in column.as_array().unwrap() {
                let x = cell["x"].as_u64().unwrap() as usize;
                let y = cell["y"].as_u64().unwrap() as usize;
                let v = board.cells[x * WIDTH + y];
                let item = if v >= 0 {
                    i64::from(v) - 1
                } else {
                    -i64::from(v) - 2
                };
                assert_eq!(item, cell["item"].as_i64().unwrap(), "cell {x},{y}");
                assert_eq!(v >= 0, cell["hidden"].as_bool().unwrap(), "cell {x},{y}");
            }
        }
    }
    #[test]
    fn crystal_matches_actual_csharp_board_reveals_and_full_reward_rng_vectors() {
        crate::cards::dispatch::ensure_installed();
        crate::relics::dispatch::ensure_installed();
        let data: Value = serde_json::from_str(include_str!(
            "../../../../../docs/beta-migration/crystal_sphere_csharp_vectors.json"
        ))
        .unwrap();
        for vector in data["vectors"].as_array().unwrap() {
            let mut state = RunState::new();
            state.character_id = sts2_core::CharacterId::Necrobinder;
            state.current_event = Some(EventId::CrystalSphere);
            let mut rng = Rng::new(0);
            rng.load_state(rng_state(&vector["initial_rng"]));
            state.event_state.rng = Some(rng);
            prepare(&mut state);
            assert_eq!(
                i64::from(state.event_state.scratch[0]),
                vector["cost"].as_i64().unwrap()
            );
            let debt = vector["payment_plan"].as_bool().unwrap();
            state.gold = 200;
            assert!(!begin(&mut state, debt).finished);
            let mut board = Board::load(&state);
            assert_eq!(
                state.event_state.rng.as_ref().unwrap().state(),
                rng_state(&vector["board_rng"])
            );
            assert_eq!(board.placed, vector["placed_all_items"].as_bool().unwrap());
            assert_grid(&board, &vector["initial_board"]);
            for (actual, expected) in board
                .items
                .iter()
                .zip(vector["placements"].as_array().unwrap())
            {
                assert_eq!(i64::from(actual[1]), expected["x"].as_i64().unwrap());
                assert_eq!(i64::from(actual[2]), expected["y"].as_i64().unwrap());
                let (w, h) = size(actual[0]);
                assert_eq!(w as u64, expected["width"].as_u64().unwrap());
                assert_eq!(h as u64, expected["height"].as_u64().unwrap());
            }
            for click in vector["clicks"].as_array().unwrap() {
                board.small = click["tool"] == "Small";
                board.click(
                    &mut state,
                    click["x"].as_u64().unwrap() as usize,
                    click["y"].as_u64().unwrap() as usize,
                );
                assert_eq!(
                    i64::from(board.divinations),
                    click["remaining"].as_i64().unwrap()
                );
                assert_eq!(
                    serde_json::to_value(&board.revealed).unwrap(),
                    click["revealed"]
                );
            }
            assert_grid(&board, &vector["final_board"]);
            // Isolate the oracle's one revealed relic's identity; its rarity
            // draw and exact RNG position are still compared independently.
            for rarity in [
                sts2_core::RelicRarity::Common,
                sts2_core::RelicRarity::Uncommon,
                sts2_core::RelicRarity::Rare,
            ] {
                state
                    .relic_grab_bag_by_rarity
                    .insert(rarity, vec![sts2_core::RelicId::BoneFlute]);
            }
            finish_rewards(&mut state, &board);
            assert_eq!(
                state.event_state.rng.as_ref().unwrap().state(),
                rng_state(&vector["final_rng"]),
                "{} payment_plan={debt}",
                vector["seed"]
            );
            let expected = vector["rewards"].as_array().unwrap();
            assert_eq!(state.pending_rewards.len(), expected.len());
            for (actual, expected) in state.pending_rewards.iter().zip(expected) {
                match actual {
                    Reward::Gold { amount } => {
                        assert_eq!(i64::from(*amount), expected["gold"].as_i64().unwrap());
                    }
                    Reward::Potion(potion) => assert_eq!(
                        potion.potion_kind.unwrap().python_name(),
                        expected["potion"].as_str().unwrap()
                    ),
                    Reward::Relic(relic) => {
                        assert_eq!(relic.relic_id, sts2_core::RelicId::BoneFlute);
                    }
                    Reward::Card(reward) => {
                        let actual:Vec<_>=reward.prepared_cards.iter().map(|card|serde_json::json!({"id":card.card_id.to_python_name(),"upgraded":card.upgraded})).collect();
                        assert_eq!(serde_json::to_value(actual).unwrap(), expected["cards"]);
                    }
                    _ => panic!("unexpected Crystal reward"),
                }
            }
        }
    }
    #[test]
    fn crystal_options_only_expose_visible_board_and_reject_revealed_cells() {
        let mut state = RunState::new();
        state.current_event = Some(EventId::CrystalSphere);
        state.gold = 200;
        prepare(&mut state);
        assert!(!pay(&mut state).finished);
        let before_rng = state.event_state.rng.clone();
        assert!(resolve_board(&mut state, "cell_0_0").is_none());
        let before = Board::load(&state).divinations;
        assert!(!resolve_board(&mut state, "tool_small").unwrap().finished);
        assert_eq!(Board::load(&state).divinations, before);
        assert_eq!(state.event_state.rng, before_rng);
        let options = generate_options(&state, &CRYSTAL_SPHERE);
        assert!(!options.iter().any(|o| o.option_id == "cell_0_0"));
        let mut alternative = state.clone();
        let mut b = Board::load(&alternative);
        // Hidden item identities must not change the visible decision.
        for item in &mut b.items {
            item[0] = (item[0] + 1) % 9;
        }
        b.save(&mut alternative);
        assert_eq!(policy_option(&state), policy_option(&alternative));
        assert_eq!(
            generate_options(&state, &CRYSTAL_SPHERE),
            generate_options(&alternative, &CRYSTAL_SPHERE)
        );
    }
}
