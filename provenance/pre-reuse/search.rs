//! Small cached stochastic Gumbel tree. Selection equations follow mctx;
//! simulator states and chance outcomes live on CPU, only leaf evaluation waits on GPU.
use super::super::{Command, Result, Session};
use sts2_core::run_rng_set::{outcome_stream_seed_u64, splitmix64, LEGACY_RNG_SALT};

#[derive(Debug)]
struct Edge {
    logit: f32,
    prior: f32,
    visits: u32,
    sum: f32,
    outcomes: Vec<usize>,
    stochastic: bool,
    widening: (f32, f32),
}
#[derive(Debug)]
struct Node {
    session: Session,
    value: f32,
    edges: Vec<Edge>,
}
impl Node {
    fn new(session: Session) -> Self {
        Self {
            session,
            value: 0.0,
            edges: Vec::new(),
        }
    }
    fn evaluate(&mut self, logits: Vec<f32>, value: f32) {
        let probs = softmax(&logits);
        self.value = value;
        self.edges = logits
            .into_iter()
            .zip(probs)
            .map(|(logit, prior)| Edge {
                logit,
                prior,
                visits: 0,
                sum: 0.0,
                outcomes: Vec::new(),
                stochastic: false,
                widening: (1.0, 0.5),
            })
            .collect();
    }
    fn qtransform(&self) -> Vec<f32> {
        let visits: u32 = self.edges.iter().map(|e| e.visits).sum();
        let p: f32 = self
            .edges
            .iter()
            .filter(|e| e.visits > 0)
            .map(|e| e.prior.max(f32::MIN_POSITIVE))
            .sum();
        let weighted: f32 = self
            .edges
            .iter()
            .filter(|e| e.visits > 0)
            .map(|e| e.prior.max(f32::MIN_POSITIVE) / p * e.sum / e.visits as f32)
            .sum();
        let mixed = (self.value + visits as f32 * weighted) / (1 + visits) as f32;
        let mut q: Vec<f32> = self
            .edges
            .iter()
            .map(|e| {
                if e.visits == 0 {
                    mixed
                } else {
                    e.sum / e.visits as f32
                }
            })
            .collect();
        let lo = q.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = q.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let scale = (50.0 + self.edges.iter().map(|e| e.visits).max().unwrap_or(0) as f32) * 0.1
            / (hi - lo).max(1e-8);
        for x in &mut q {
            *x = (*x - lo) * scale;
        }
        q
    }
    fn interior(&self) -> usize {
        let scores: Vec<f32> = self
            .edges
            .iter()
            .zip(self.qtransform())
            .map(|(e, q)| e.logit + q)
            .collect();
        let probs = softmax(&scores);
        let visits: u32 = self.edges.iter().map(|e| e.visits).sum();
        argmax(
            probs
                .iter()
                .zip(&self.edges)
                .map(|(p, e)| p - e.visits as f32 / (1 + visits) as f32),
        )
    }
}
fn softmax(x: &[f32]) -> Vec<f32> {
    let max = x.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut p: Vec<f32> = x.iter().map(|v| (v - max).exp()).collect();
    let sum: f32 = p.iter().sum();
    for v in &mut p {
        *v /= sum;
    }
    p
}
fn argmax(xs: impl Iterator<Item = f32>) -> usize {
    let mut best = 0;
    let mut score = f32::NEG_INFINITY;
    for (i, x) in xs.enumerate() {
        if x > score {
            score = x;
            best = i;
        }
    }
    best
}
fn schedule(m: usize, budget: usize) -> Vec<u32> {
    if m <= 1 {
        return (0..budget as u32).collect();
    }
    let log = (m as f64).log2().ceil() as usize;
    let mut visits = vec![0; m];
    let mut n = m;
    let mut result = Vec::with_capacity(budget + m);
    while result.len() < budget {
        for _ in 0..(budget / (log * n)).max(1) {
            result.extend_from_slice(&visits[..n]);
            for v in &mut visits[..n] {
                *v += 1;
            }
        }
        n = (n / 2).max(2);
    }
    result.truncate(budget);
    result
}
#[derive(Debug, Default)]
pub(super) struct Metrics {
    pub simulations: u64,
    pub transitions: u64,
    pub forced_transitions: u64,
    pub cached_edges: u64,
    pub chance_samples: u64,
    pub terminal_backups: u64,
    pub max_depth: usize,
}
#[derive(Debug)]
pub(super) struct Search {
    nodes: Vec<Node>,
    pending: usize,
    path: Vec<(usize, usize)>,
    gumbels: Vec<f32>,
    schedule: Vec<u32>,
    seed: u64,
    rng: u64,
    pub metrics: Metrics,
}
impl Search {
    pub fn new(
        root: &Session,
        logits: Vec<f32>,
        value: f32,
        seed: u64,
        budget: usize,
        considered: usize,
    ) -> Self {
        let mut state = root.clone();
        // Search seed never depends on hidden RNG or actual draw order.
        sts2_core::blindfold::belief_root(&mut state.state, seed);
        state.max_actions = usize::MAX; // no search depth/rollout cap
        let m = considered.min(budget).min(logits.len());
        let mut node = Node::new(state);
        node.evaluate(logits, value);
        let gumbels = (0..node.edges.len())
            .map(|i| {
                let bits = splitmix64(seed ^ (i as u64).wrapping_mul(0x9e3779b97f4a7c15));
                let u = ((bits >> 11) as f64 + 0.5) / (1u64 << 53) as f64;
                (-(-u.ln()).ln()) as f32
            })
            .collect();
        Self {
            nodes: vec![node],
            pending: 0,
            path: Vec::with_capacity(budget),
            gumbels,
            schedule: schedule(m, budget),
            seed,
            rng: splitmix64(seed ^ 0x14ac),
            metrics: Metrics::default(),
        }
    }
    pub fn policy_target(&self) -> Vec<f32> {
        let root = &self.nodes[0];
        softmax(
            &root
                .edges
                .iter()
                .zip(root.qtransform())
                .map(|(e, q)| e.logit + q)
                .collect::<Vec<_>>(),
        )
    }
    pub fn query(&self) -> &Session {
        &self.nodes[self.pending].session
    }
    pub fn query_mut(&mut self) -> &mut Session {
        &mut self.nodes[self.pending].session
    }
    pub fn receive(&mut self, logits: Vec<f32>, value: f32) {
        self.nodes[self.pending].evaluate(logits, value);
        self.backup(value);
    }
    fn backup(&mut self, value: f32) {
        self.metrics.max_depth = self.metrics.max_depth.max(self.path.len());
        for &(n, a) in &self.path {
            let e = &mut self.nodes[n].edges[a];
            e.visits += 1;
            e.sum += value;
        }
        self.metrics.simulations += 1;
        self.path.clear();
    }
    fn root_action(&self, final_choice: bool) -> usize {
        let root = &self.nodes[0];
        let count = if final_choice {
            root.edges.iter().map(|e| e.visits).max().unwrap_or(0)
        } else {
            self.schedule[self.metrics.simulations as usize]
        };
        argmax(
            root.edges
                .iter()
                .zip(root.qtransform())
                .enumerate()
                .map(|(a, (e, q))| {
                    if e.visits == count {
                        self.gumbels[a] + e.logit + q
                    } else {
                        f32::NEG_INFINITY
                    }
                }),
        )
    }
    /// Some(action) when budget completed, None when one new leaf needs GPU.
    pub fn advance(&mut self) -> Result<Option<usize>> {
        while (self.metrics.simulations as usize) < self.schedule.len() {
            let mut node = 0;
            let mut action = self.root_action(false);
            loop {
                self.path.push((node, action));
                let edge = &self.nodes[node].edges[action];
                let cap = (edge.widening.0 * ((edge.visits + 1) as f32).powf(edge.widening.1))
                    .ceil()
                    .max(1.0) as usize;
                let expand =
                    edge.outcomes.is_empty() || (edge.stochastic && edge.outcomes.len() < cap);
                let child = if expand {
                    let outcome = edge.outcomes.len() as u32;
                    let base = splitmix64(
                        self.seed
                            ^ (node as u64).wrapping_mul(0x9e3779b97f4a7c15)
                            ^ (action as u64).wrapping_mul(0x85ebca6b),
                    );
                    let mut session = self.nodes[node].session.clone();
                    session.state.run_rng.reseed_combat_streams(base, outcome);
                    session.state.rng = sts2_core::dotnet_rng::Rng::from_search_seed(
                        outcome_stream_seed_u64(base, outcome, LEGACY_RNG_SALT),
                    );
                    let before = session.state.run_rng.combat_counter_fingerprint();
                    let legacy = session.state.rng.counter();
                    let draws = session.state.cards_drawn;
                    let mut endturn = matches!(
                        session.commands[action],
                        Command::Core(sts2_core::Action::EndTurn)
                    );
                    session.step(action)?;
                    self.metrics.transitions += 1;
                    session.refresh_commands();
                    // Collapse forced factual transitions within this belief outcome.
                    // Keep its RNG streams running; do not reseed or evaluate intermediates.
                    let mut forced_chain = 0usize;
                    while !session.state.is_over && session.commands.len() == 1 {
                        forced_chain += 1;
                        if forced_chain == 1000 {
                            eprintln!("diagnostic long forced chain: decisions={} command={:?} selected={:?} pending={:?} hp={} enemies={:?}", session.decisions, session.commands, session.selected, session.state.pending_choice, session.state.player.current_hp, session.state.creatures);
                        }
                        endturn |= matches!(
                            session.commands[0],
                            Command::Core(sts2_core::Action::EndTurn)
                        );
                        session.step(0)?;
                        self.metrics.transitions += 1;
                        self.metrics.forced_transitions += 1;
                        session.refresh_commands();
                    }
                    let stochastic = before != session.state.run_rng.combat_counter_fingerprint()
                        || legacy != session.state.rng.counter();
                    let is_draw = session.state.cards_drawn > draws;
                    let id = self.nodes.len();
                    let edge = &mut self.nodes[node].edges[action];
                    edge.stochastic = stochastic;
                    edge.widening = if endturn {
                        sts2_mcts::BELIEF_DRAW_ENDTURN_WIDENING
                    } else if is_draw {
                        (sts2_mcts::BELIEF_DRAW_DRAW_WIDENING_C, 0.5)
                    } else {
                        (1.0, 0.5)
                    };
                    edge.outcomes.push(id);
                    self.metrics.chance_samples += u64::from(stochastic);
                    self.nodes.push(Node::new(session));
                    id
                } else {
                    self.metrics.cached_edges += 1;
                    self.rng = splitmix64(self.rng);
                    edge.outcomes[(self.rng % edge.outcomes.len() as u64) as usize]
                };
                if self.nodes[child].session.state.is_over {
                    let session = &self.nodes[child].session;
                    let s = &session.state;
                    let value = if session.reward.enabled {
                        session.reward.utility(s)
                    } else if s.player_won && s.player.current_hp > 0 {
                        1.0
                    } else {
                        -1.0
                    };
                    self.metrics.terminal_backups += 1;
                    self.backup(value);
                    break;
                }
                if self.nodes[child].edges.is_empty() {
                    self.pending = child;
                    return Ok(None);
                }
                node = child;
                action = self.nodes[node].interior();
            }
        }
        Ok(Some(self.root_action(true)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sequential_halving_16_simulations() {
        assert_eq!(
            schedule(8, 16),
            vec![0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 3]
        );
        assert_eq!(schedule(1, 16), (0..16).collect::<Vec<_>>());
        for m in 1..=16 {
            assert_eq!(schedule(m, 16).len(), 16);
        }
    }
    #[test]
    fn stable_softmax_and_ties() {
        let p = softmax(&[1000.0, 1000.0]);
        assert_eq!(p, vec![0.5, 0.5]);
        assert_eq!(argmax([1.0, 1.0, 0.0].into_iter()), 0);
    }
    fn fixture() -> Session {
        sts2_content::cards::dispatch::install_default_registry();
        sts2_content::relics::dispatch::ensure_installed();
        sts2_content::potions::dispatch::ensure_installed();
        let f: serde_json::Value =
            serde_json::from_str(include_str!("search_fixture.json")).unwrap();
        let schema = super::super::super::Schema::new(&[], serde_json::json!({})).unwrap();
        let state = sts2_wire::state_from_json(&f["wire"]).unwrap();
        let mut s = Session::from_parts(&schema, state, f["context"].clone(), 500).unwrap();
        s.refresh_commands();
        s
    }
    fn finish(s: &mut Search) -> (usize, Vec<u64>) {
        let mut hashes = Vec::new();
        loop {
            if let Some(a) = s.advance().unwrap() {
                return (a, hashes);
            }
            let q = s.query_mut();
            q.refresh_commands();
            hashes.push(sts2_core::state_hash(&q.state));
            let logits = vec![0.0; q.commands.len()];
            let v = q.state.player.current_hp as f32 / q.state.player.max_hp as f32;
            s.receive(logits, v);
        }
    }
    #[test]
    fn real_search_budget_cache_and_hidden_information_invariance() {
        let root = fixture();
        let original = sts2_core::state_hash(&root.state);
        let mut hidden = root.clone();
        assert!(
            hidden.state.player.pinned_top.is_empty()
                && hidden.state.player.pinned_bottom.is_empty()
        );
        hidden.state.player.draw.reverse();
        hidden.state.run_rng.reseed_combat_streams(92341, 9);
        hidden.state.rng = sts2_core::dotnet_rng::Rng::from_search_seed(934);
        let logits = vec![0.0; root.commands.len()];
        let mut a = Search::new(&root, logits.clone(), 0.25, 777, 16, 8);
        let mut b = Search::new(&hidden, logits, 0.25, 777, 16, 8);
        assert_eq!(finish(&mut a), finish(&mut b));
        assert_eq!(a.metrics.simulations, 16);
        assert_eq!(a.nodes[0].edges.iter().map(|e| e.visits).sum::<u32>(), 16);
        assert_eq!(
            (a.metrics.transitions - a.metrics.forced_transitions) as usize,
            a.nodes.len() - 1
        );
        assert!(a.metrics.cached_edges > 0);
        assert!(a.metrics.max_depth > 1);
        assert_eq!(sts2_core::state_hash(&root.state), original);
    }
    #[test]
    fn forced_belief_states_are_not_tree_nodes() {
        let mut root = fixture();
        root.state.player.hand.clear();
        root.state.player.draw.clear();
        root.state.player.discard.clear();
        root.state.player.potions.clear();
        root.state
            .player
            .potions
            .push(sts2_core::PotionKind::FirePotion);
        root.state.player.potion_slot_indices.clear();
        root.state.player.potion_slot_indices.push(0);
        root.refresh_commands();
        assert!(root.commands.len() > 1);
        let original = sts2_core::state_hash(&root.state);
        let mut s = Search::new(&root, vec![0.0; root.commands.len()], 0.25, 777, 256, 8);
        finish(&mut s);
        assert_eq!(s.metrics.simulations, 256);
        assert!(s.metrics.forced_transitions > 0);
        for node in &s.nodes {
            assert!(node.session.state.is_over || node.session.commands.len() > 1);
        }
        assert_eq!(
            (s.metrics.transitions - s.metrics.forced_transitions) as usize,
            s.nodes.len() - 1
        );
        assert_eq!(sts2_core::state_hash(&root.state), original);
    }
    #[test]
    fn belief_pins_and_chance_outcomes_preserved() {
        let mut root = fixture();
        let top = root.state.player.draw[0];
        root.state.player.pinned_top.push(top);
        let mut s = Search::new(&root, vec![0.0; root.commands.len()], 0.0, 31, 16, 1);
        assert_eq!(s.nodes[0].session.state.player.pinned_top[0], top);
        assert_eq!(s.nodes[0].session.state.player.draw[0], top);
        finish(&mut s);
        assert_eq!(s.metrics.simulations, 16);
    }
    #[test]
    fn preset_potion_costs_preserve_instance_identity_and_future_cost() {
        let mut s = fixture();
        s.state.player.potions.clear();
        s.state.player.potion_slot_indices.clear();
        s.state
            .player
            .potions
            .push(sts2_core::PotionKind::FirePotion);
        s.state
            .player
            .potions
            .push(sts2_core::PotionKind::FirePotion);
        s.state.player.potion_slot_indices.push(0);
        s.state.player.potion_slot_indices.push(1);
        s.reward = super::super::super::reward::Context::new(&s.state, 123, Some(1.0));
        s.reward.set_initial_costs(&[0.0, 1.0]).unwrap();
        assert!(s.reward.set_initial_costs(&[0.0]).is_err());
        assert!(s.reward.set_initial_costs(&[0.0, f32::NAN]).is_err());
        let mut c = s.clone();
        assert_eq!(c.reward.ledger[0].cost, 0.0);
        assert_eq!(c.reward.ledger[1].cost, 1.0);
        c.state
            .player
            .potions
            .push(sts2_core::PotionKind::FirePotion);
        c.state.player.potion_slot_indices.push(2);
        c.reward.update(&c.state, None);
        assert_eq!(c.reward.ledger[2].cost, 1.0);
        assert_eq!(s.reward.ledger.len(), 2);
    }
    #[test]
    fn potion_identity_cost_tokens_and_damage_context_follow_clones() {
        use sts2_core::PotionKind;
        let mut s = fixture();
        s.state.player.potions.clear();
        s.state.player.potion_slot_indices.clear();
        s.state.player.potions.push(PotionKind::FirePotion);
        s.state.player.potions.push(PotionKind::FirePotion);
        s.state.player.potion_slot_indices.push(0);
        s.state.player.potion_slot_indices.push(1);
        s.reward = super::super::super::reward::Context::new(&s.state, 5, Some(0.7));
        s.refresh_commands();
        let a = s
            .commands
            .iter()
            .position(|a| {
                matches!(
                    a,
                    Command::Core(
                        sts2_core::Action::DrinkPotion { potion_slot: 0, .. }
                            | sts2_core::Action::UsePotion { slot: 0, .. }
                    )
                )
            })
            .unwrap();
        s.step(a).unwrap();
        assert!((s.reward.spent - 0.7).abs() < 1e-6);
        assert_eq!(s.reward.active, vec![1]);
        assert!(s.reward.ledger[0].used);
        let mut cloned = s.clone();
        cloned.state.player.potions.push(PotionKind::FirePotion);
        cloned.state.player.potion_slot_indices.push(0);
        cloned.reward.update(&cloned.state, None);
        assert_eq!(cloned.reward.ledger.len(), 3);
        assert_eq!(s.reward.ledger.len(), 2);
        let mut names: Vec<String> = Vec::new();
        for c in &s.state.creatures {
            if let Some(ai) = &c.ai {
                names.push(ai.current_move().id.into());
                names.extend(ai.move_history().iter().map(|v| (*v).to_owned()));
            }
        }
        for power in &s.state.powers {
            names.push(power.kind.python_name().to_owned());
        }
        let schema = super::super::super::Schema::new(
            &[],
            serde_json::json!({"names":names,"context":*s.context}),
        )
        .unwrap();
        let mut rows = Vec::new();
        s.numeric_direct(&schema, &mut rows, 0).unwrap();
        assert_eq!(rows[0][1], 2);
        let context = rows.iter().find(|r| r[0] == 17).unwrap();
        assert!((f32::from_bits(context[4] as u32) - 0.7).abs() < 1e-6);
        let cost = rows.iter().find(|r| r[0] == 18).unwrap();
        assert_eq!(cost[3], 1);
        assert!((f32::from_bits(cost[4] as u32) - 0.7).abs() < 1e-6);
        let uid = s.state.enemy_uids[0].0;
        let enemy = s.state.creatures.iter_mut().find(|c| c.id == uid).unwrap();
        let hp = enemy.current_hp;
        enemy.current_hp = (hp - 5).max(0);
        s.reward.update(&s.state, None);
        let progress = s.reward.progress();
        s.state
            .creatures
            .iter_mut()
            .find(|c| c.id == uid)
            .unwrap()
            .current_hp = hp;
        s.reward.update(&s.state, None);
        assert!((s.reward.progress() - progress).abs() < 1e-6);
    }
}
