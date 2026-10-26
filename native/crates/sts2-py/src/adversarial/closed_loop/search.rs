//! Small cached stochastic Gumbel tree. Selection equations follow mctx;
//! simulator states and chance outcomes live on CPU, only leaf evaluation waits on GPU.
use super::super::{Command, Result, Session};
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
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
// Hashes select buckets only. Full canonical state equality and the complete
// Session continuation/reward/history signature decide identity. Never use the
// observation alone: it omits simulator continuation and reward-ledger state.
#[derive(Debug)]
struct Key {
    digest: u64,
    state: sts2_core::CombatState,
    session: String,
}
impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.session == other.session && self.state == other.state
    }
}
impl Eq for Key {}
impl Key {
    fn new(s: &Session) -> Self {
        let mut state = s.state.clone();
        // Strip ONLY information already hidden by belief search. Pins survive.
        sts2_core::blindfold::belief_root(&mut state, 0);
        // Immutable context and initial-cost maps are shared by Arc within
        // one combat. Owner identity avoids serializing the large frozen JSON.
        let session = format!(
            "{:?}",
            (
                (
                    &s.reward,
                    std::sync::Arc::as_ptr(&s.context),
                    s.initial_hp,
                    std::sync::Arc::as_ptr(&s.initial_costs)
                ),
                (&s.commands, &s.selected, &s.played_turn, &s.played_counts),
                (
                    s.decisions,
                    s.drawn,
                    s.exhausted,
                    s.discarded,
                    s.damage_events
                ),
                (
                    s.history_round,
                    s.log_cursor,
                    &s.last_potion,
                    &s.opening_prompt
                ),
            )
        );
        let mut h = std::collections::hash_map::DefaultHasher::new();
        sts2_core::state_hash(&state).hash(&mut h);
        session.hash(&mut h);
        Self {
            state,
            session,
            digest: h.finish(),
        }
    }
    // Identity is immutable. Reindexing an eviction must not rehash every state.
    fn hash(&self) -> u64 {
        self.digest
    }
}
#[derive(Debug)]
struct Node {
    session: Session,
    key: Option<Key>,
    value: f32,
    edges: Vec<Edge>,
}
impl Node {
    fn new(session: Session) -> Self {
        Self {
            session,
            key: None,
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
    pub transposition_hits: u64,
    pub persistent_hits: u64,
    pub root_reuses: u64,
    pub reused_nodes: u64,
    pub evicted_nodes: u64,
    pub cycle_backups: u64,
    pub peak_nodes: usize,
}
#[derive(Debug)]
pub(super) struct Search {
    nodes: Vec<Node>,
    table: HashMap<u64, Vec<usize>>,
    root: usize,
    root_visits: Vec<u32>,
    capacity: usize,
    actor_version: u64,
    previous_nodes: usize,
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
            root_visits: vec![0; node.edges.len()],
            nodes: vec![node],
            table: HashMap::new(),
            root: 0,
            capacity: 0,
            actor_version: 0,
            previous_nodes: 0,
            pending: 0,
            path: Vec::with_capacity(budget),
            gumbels,
            schedule: schedule(m, budget),
            seed,
            rng: splitmix64(seed ^ 0x14ac),
            metrics: Metrics::default(),
        }
    }
    fn lookup(&self, key: &Key) -> Option<usize> {
        self.table
            .get(&key.hash())?
            .iter()
            .copied()
            .find(|&i| self.nodes[i].key.as_ref() == Some(key))
    }
    pub fn cached_evaluation(&self, root: &Session, actor: u64) -> Option<(Vec<f32>, f32)> {
        if self.capacity == 0 || self.actor_version != actor {
            return None;
        }
        let i = self.lookup(&Key::new(root))?;
        let n = &self.nodes[i];
        if n.edges.is_empty() {
            return None;
        }
        Some((n.edges.iter().map(|e| e.logit).collect(), n.value))
    }
    // Bounded per-combat cache. Prefer the new root and descendants, then recent
    // alternatives. Never keep a biased subset of a sampled chance edge: if an
    // outcome is evicted, invalidate its whole sample list, retaining Q evidence.
    fn compact(&mut self, root: Option<usize>) -> u64 {
        if self.nodes.len() <= self.capacity {
            return 0;
        }
        let mut order = Vec::new();
        let mut seen = HashSet::new();
        let mut queue: VecDeque<usize> = root.into_iter().collect();
        while let Some(i) = queue.pop_front() {
            if !seen.insert(i) {
                continue;
            }
            order.push(i);
            if order.len() == self.capacity {
                break;
            }
            for e in &self.nodes[i].edges {
                queue.extend(e.outcomes.iter().copied());
            }
        }
        for i in (0..self.nodes.len()).rev() {
            if order.len() == self.capacity {
                break;
            }
            if seen.insert(i) {
                order.push(i);
            }
        }
        let mut remap = vec![None; self.nodes.len()];
        for (new, &old) in order.iter().enumerate() {
            remap[old] = Some(new);
        }
        let evicted = self.nodes.len() - order.len();
        let mut old: Vec<Option<Node>> = std::mem::take(&mut self.nodes)
            .into_iter()
            .map(Some)
            .collect();
        self.nodes = order.into_iter().map(|i| old[i].take().unwrap()).collect();
        self.table.clear();
        for (i, n) in self.nodes.iter_mut().enumerate() {
            for e in &mut n.edges {
                if e.outcomes.iter().all(|&c| remap[c].is_some()) {
                    for c in &mut e.outcomes {
                        *c = remap[*c].unwrap();
                    }
                } else {
                    e.outcomes.clear();
                }
            }
            self.table
                .entry(n.key.as_ref().unwrap().hash())
                .or_default()
                .push(i);
        }
        evicted as u64
    }
    pub fn with_reuse(
        previous: Option<Self>,
        root: &Session,
        logits: Vec<f32>,
        value: f32,
        seed: u64,
        budget: usize,
        considered: usize,
        capacity: usize,
        actor: u64,
    ) -> Self {
        if capacity == 0 {
            return Self::new(root, logits, value, seed, budget, considered);
        }
        let mut fresh = Self::new(root, logits.clone(), value, seed, budget, considered);
        fresh.capacity = capacity;
        fresh.actor_version = actor;
        let key = Key::new(root);
        if let Some(mut old) = previous.filter(|s| s.actor_version == actor && s.capacity > 0) {
            old.capacity = capacity;
            let root_id = old.lookup(&key);
            fresh.metrics.evicted_nodes = old.compact(root_id);
            fresh.nodes = old.nodes;
            fresh.table = old.table;
            fresh.previous_nodes = fresh.nodes.len();
            fresh.metrics.reused_nodes = fresh.nodes.len() as u64;
            if let Some(i) = fresh.lookup(&key) {
                fresh.root = i;
                fresh.pending = i;
                fresh.metrics.root_reuses = 1;
                // Keep outgoing visits/Q and descendants, but use today's root
                // evaluation. Only fresh root visits drive sequential halving.
                let n = &mut fresh.nodes[i];
                assert_eq!(n.edges.len(), logits.len());
                n.value = value;
                for ((e, l), p) in n.edges.iter_mut().zip(logits.iter()).zip(softmax(&logits)) {
                    e.logit = *l;
                    e.prior = p;
                }
            } else {
                let mut n = Node::new(root.clone());
                sts2_core::blindfold::belief_root(&mut n.session.state, seed);
                n.session.max_actions = usize::MAX;
                n.evaluate(logits, value);
                fresh.root = fresh.nodes.len();
                fresh.pending = fresh.root;
                n.key = Some(key);
                fresh
                    .table
                    .entry(n.key.as_ref().unwrap().hash())
                    .or_default()
                    .push(fresh.root);
                fresh.nodes.push(n);
            }
        } else {
            fresh.nodes[0].key = Some(key);
            fresh
                .table
                .entry(fresh.nodes[0].key.as_ref().unwrap().hash())
                .or_default()
                .push(0);
        }
        fresh.metrics.peak_nodes = fresh.nodes.len();
        fresh
    }
    pub fn policy_target(&self) -> Vec<f32> {
        let root = &self.nodes[self.root];
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
        if let Some(&(_, a)) = self.path.first() {
            self.root_visits[a] += 1;
        }
        self.metrics.simulations += 1;
        self.path.clear();
    }
    fn root_action(&self, final_choice: bool) -> usize {
        let root = &self.nodes[self.root];
        let count = if final_choice {
            self.root_visits.iter().copied().max().unwrap_or(0)
        } else {
            self.schedule[self.metrics.simulations as usize]
        };
        argmax(
            root.edges
                .iter()
                .zip(root.qtransform())
                .enumerate()
                .map(|(a, (e, q))| {
                    if self.root_visits[a] == count {
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
            let mut node = self.root;
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
                    let key = (self.capacity > 0).then(|| Key::new(&session));
                    let found = key.as_ref().and_then(|k| self.lookup(k));
                    let id = found.unwrap_or(self.nodes.len());
                    if found.is_some() {
                        self.metrics.transposition_hits += 1;
                        self.metrics.persistent_hits += u64::from(id < self.previous_nodes);
                    } else {
                        let mut n = Node::new(session);
                        if let Some(k) = key {
                            self.table.entry(k.hash()).or_default().push(id);
                            n.key = Some(k);
                        }
                        self.nodes.push(n);
                        self.metrics.peak_nodes = self.metrics.peak_nodes.max(self.nodes.len());
                    }
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
                // A transposition can close a cycle. Bootstrap its evaluated V
                // once, rather than endlessly traversing a cyclic graph.
                if self.path.iter().any(|&(n, _)| n == child) {
                    self.metrics.cycle_backups += 1;
                    self.backup(self.nodes[child].value);
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
    fn transposition_key_hides_rng_but_preserves_continuations_costs_and_pins() {
        let a = fixture();
        let mut b = a.clone();
        b.state.player.draw.reverse();
        b.state.run_rng.reseed_combat_streams(99, 17);
        b.state.rng = sts2_core::dotnet_rng::Rng::from_search_seed(91);
        assert_eq!(Key::new(&a), Key::new(&b));
        b.state.pending_turn_setup_resume = !a.state.pending_turn_setup_resume;
        assert_ne!(Key::new(&a), Key::new(&b));
        b = a.clone();
        b.reward.spent += 0.5;
        assert_ne!(Key::new(&a), Key::new(&b));
        b = a.clone();
        b.selected.push(0);
        assert_ne!(Key::new(&a), Key::new(&b));
        b = a.clone();
        b.state.player.pinned_top.push(b.state.player.draw[0]);
        assert_ne!(Key::new(&a), Key::new(&b));
        b = a.clone();
        b.played_counts.insert("SOUL".into(), 7);
        assert_ne!(Key::new(&a), Key::new(&b));
    }
    #[test]
    fn persistent_reroot_preserves_graph_and_spends_fresh_budget() {
        let root = fixture();
        let mut a = Search::with_reuse(
            None,
            &root,
            vec![0.0; root.commands.len()],
            0.25,
            77,
            32,
            8,
            512,
            9,
        );
        finish(&mut a);
        let i = a
            .nodes
            .iter()
            .position(|n| {
                !n.session.state.is_over
                    && !n.edges.is_empty()
                    && n.session.decisions > root.decisions
            })
            .unwrap();
        let next = a.nodes[i].session.clone();
        let old_visits: u32 = a.nodes[i].edges.iter().map(|e| e.visits).sum();
        let eval = a.cached_evaluation(&next, 9).unwrap();
        assert!(a.cached_evaluation(&next, 10).is_none());
        let mut b = Search::with_reuse(Some(a), &next, eval.0, eval.1, 88, 32, 8, 512, 9);
        assert_eq!(b.metrics.root_reuses, 1);
        assert_eq!(
            b.nodes[b.root].edges.iter().map(|e| e.visits).sum::<u32>(),
            old_visits
        );
        finish(&mut b);
        assert_eq!(b.metrics.simulations, 32);
        assert_eq!(b.root_visits.iter().sum::<u32>(), 32);
        assert_eq!(
            b.nodes[b.root].edges.iter().map(|e| e.visits).sum::<u32>(),
            old_visits + 32
        );
        assert!(b.metrics.reused_nodes > 1);
    }
    #[test]
    fn new_actor_discards_persistent_evaluations_and_statistics() {
        let root = fixture();
        let mut a = Search::with_reuse(
            None,
            &root,
            vec![0.0; root.commands.len()],
            0.25,
            77,
            16,
            8,
            512,
            1,
        );
        finish(&mut a);
        let b = Search::with_reuse(
            Some(a),
            &root,
            vec![1.0; root.commands.len()],
            0.75,
            88,
            16,
            8,
            512,
            2,
        );
        assert_eq!(b.nodes.len(), 1);
        assert_eq!(b.nodes[0].value, 0.75);
        assert_eq!(b.metrics.reused_nodes, 0);
        assert_eq!(b.metrics.root_reuses, 0);
    }
    #[test]
    fn transposition_hash_collision_requires_full_equality() {
        let root = fixture();
        let mut a = Search::with_reuse(
            None,
            &root,
            vec![0.0; root.commands.len()],
            0.25,
            77,
            16,
            8,
            512,
            1,
        );
        let mut other = root.clone();
        other.state.pending_turn_setup_resume = true;
        let k = Key::new(&other);
        a.table.entry(k.hash()).or_default().push(0);
        assert!(a.lookup(&k).is_none());
        assert_eq!(a.lookup(&Key::new(&root)), Some(0));
    }
    #[test]
    fn cyclic_graph_bootstraps_once_per_simulation() {
        let root = fixture();
        let mut a = Search::with_reuse(
            None,
            &root,
            vec![0.0; root.commands.len()],
            0.25,
            77,
            32,
            8,
            512,
            1,
        );
        for e in &mut a.nodes[0].edges {
            e.outcomes = vec![0];
            e.stochastic = false;
        }
        assert!(a.advance().unwrap().is_some());
        assert_eq!(a.metrics.cycle_backups, 32);
        assert_eq!(a.metrics.simulations, 32);
        assert_eq!(a.metrics.max_depth, 1);
    }
    #[test]
    fn eviction_is_bounded_and_invalidates_complete_chance_sample_lists() {
        let root = fixture();
        let mut a = Search::with_reuse(
            None,
            &root,
            vec![0.0; root.commands.len()],
            0.25,
            77,
            32,
            8,
            512,
            1,
        );
        finish(&mut a);
        let before: Vec<usize> = a.nodes[0].edges.iter().map(|e| e.outcomes.len()).collect();
        a.capacity = 2;
        assert!(a.compact(Some(0)) > 0);
        assert_eq!(a.nodes.len(), 2);
        for (e, n) in a.nodes[0].edges.iter().zip(before) {
            assert!(e.outcomes.is_empty() || e.outcomes.len() == n);
            assert!(e.outcomes.iter().all(|&id| id < 2));
        }
        assert_eq!(a.lookup(&Key::new(&root)), Some(0));
    }
    #[test]
    fn persistent_search_remains_hidden_information_invariant() {
        let root = fixture();
        let mut hidden = root.clone();
        hidden.state.player.draw.reverse();
        hidden.state.run_rng.reseed_combat_streams(999, 9);
        let mut a = Search::with_reuse(
            None,
            &root,
            vec![0.0; root.commands.len()],
            0.25,
            77,
            32,
            8,
            512,
            1,
        );
        let mut b = Search::with_reuse(
            None,
            &hidden,
            vec![0.0; hidden.commands.len()],
            0.25,
            77,
            32,
            8,
            512,
            1,
        );
        assert_eq!(finish(&mut a), finish(&mut b));
        a = Search::with_reuse(
            Some(a),
            &root,
            vec![0.0; root.commands.len()],
            0.25,
            78,
            32,
            8,
            512,
            1,
        );
        b = Search::with_reuse(
            Some(b),
            &hidden,
            vec![0.0; hidden.commands.len()],
            0.25,
            78,
            32,
            8,
            512,
            1,
        );
        assert_eq!(finish(&mut a), finish(&mut b));
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
