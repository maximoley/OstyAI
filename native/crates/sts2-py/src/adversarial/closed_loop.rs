//! Native asynchronous action queue and reusable, page-locked observation batches.
//! The C ABI moves batch descriptors only. Simulation and observation writes are Rust-only.
use super::{numeric, Result, Schema, Session};
mod forced;
mod replay_queue;
mod search;
mod tier_budget;
use crossbeam_queue::ArrayQueue;
use replay_queue::{Queue, ReplayDescriptor, Sample};
use search::Search;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::ffi::{c_char, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

const CAPACITIES: [usize; 13] = [
    64, 96, 128, 192, 256, 384, 512, 768, 1024, 1536, 2048, 3072, 4096,
];
const FREE: usize = 0;
const FILLING: usize = 1;
const SEALED: usize = 2;
const READY: usize = 3;
const LEASED: usize = 4;

#[link(name = "cudart")]
extern "C" {
    fn cudaHostAlloc(ptr: *mut *mut c_void, bytes: usize, flags: u32) -> i32;
    fn cudaFreeHost(ptr: *mut c_void) -> i32;
}

#[derive(Debug)]
struct Arena {
    ptr: NonNull<i32>,
    words: usize,
}
// SAFETY: a slot is written only by its unique queued task. The release counter
// publishes a completed batch; no slot is recycled until its GPU lease is returned.
unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}
impl Arena {
    fn new(words: usize) -> Result<Self> {
        let mut ptr = std::ptr::null_mut();
        let code = unsafe {
            cudaHostAlloc(
                &mut ptr,
                words.checked_mul(4).ok_or("allocation overflow")?,
                0,
            )
        };
        if code != 0 {
            return Err(format!("cudaHostAlloc: {code}"));
        }
        let ptr = NonNull::new(ptr.cast::<i32>()).ok_or("null CUDA host allocation")?;
        unsafe {
            std::ptr::write_bytes(ptr.as_ptr(), 0, words);
        }
        Ok(Self { ptr, words })
    }
    fn address(&self, offset: usize) -> usize {
        assert!(offset < self.words);
        unsafe { self.ptr.as_ptr().add(offset) as usize }
    }
}
impl Drop for Arena {
    fn drop(&mut self) {
        unsafe {
            cudaFreeHost(self.ptr.as_ptr().cast());
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct Config {
    fixtures: String,
    schema: String,
    cases: usize,
    workers: usize,
    batch_size: usize,
    buffers: usize,
    row_capacity: usize,
    max_actions: usize,
    profile: bool,
    record_actions: bool,
    verify_exports: bool,
    simulations: usize,
    simulation_budgets: Option<tier_budget::Budgets>,
    skip_forced_actions: bool,
    considered_actions: usize,
    persistent_search: bool,
    search_cache_nodes: usize,
    search_seed: u64,
    reward_shaping: bool,
    fixed_potion_cost: Option<f32>,
    fixture_potion_costs: bool,
    record_replay: bool,
    replay_capacity: usize,
    actor_version: u64,
    episode: u64,
    fixture_offset: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            fixtures: "/public/maxim/Codex/spire-mentor-validation/late_act_8196_v1/fixtures.jsonl"
                .into(),
            schema: "/public/maxim/Codex/spire-mentor-native-arrays-20261024/schema".into(),
            cases: 8196,
            workers: 80,
            batch_size: 256,
            buffers: 48,
            row_capacity: 1024,
            max_actions: 500,
            profile: false,
            record_actions: true,
            verify_exports: false,
            simulations: 16,
            simulation_budgets: None,
            skip_forced_actions: true,
            considered_actions: 8,
            persistent_search: false,
            search_cache_nodes: 512,
            search_seed: 20261024,
            reward_shaping: true,
            fixed_potion_cost: None,
            fixture_potion_costs: false,
            record_replay: true,
            replay_capacity: 262144,
            actor_version: 0,
            episode: 0,
            fixture_offset: 0,
        }
    }
}

#[derive(Debug)]
struct Record {
    id: usize,
    simulation_budget: usize,
    search_tier: &'static str,
    searched_decisions: u64,
    forced_actions: u64,
    simulations: u64,
    session: Session,
    seed: u64,
    rows: usize,
    actions: Vec<u16>,
    search: Option<Search>,
    retained_search: Option<Search>,
    feedback: Option<(Vec<f32>, f32)>,
    root_rows: Vec<numeric::Row>,
    trajectory: Vec<Sample>,
}
impl Record {
    fn query(&self) -> &Session {
        self.search.as_ref().map_or(&self.session, Search::query)
    }
    fn query_mut(&mut self) -> &mut Session {
        self.search
            .as_mut()
            .map_or(&mut self.session, Search::query_mut)
    }
}
#[derive(Debug)]
struct Task {
    batch: usize,
    slot: usize,
    record: Box<Record>,
    action: Option<usize>,
}
#[derive(Debug)]
#[repr(align(64))]
struct Slot(Mutex<Option<Box<Record>>>);
#[derive(Debug)]
struct Batch {
    id: usize,
    generation: AtomicU64,
    status: AtomicUsize,
    count: AtomicUsize,
    left: AtomicUsize,
    max_rows: AtomicUsize,
    slots: Vec<Slot>,
}
impl Batch {
    fn new(id: usize, n: usize) -> Self {
        Self {
            id,
            generation: AtomicU64::new(0),
            status: AtomicUsize::new(FREE),
            count: AtomicUsize::new(0),
            left: AtomicUsize::new(0),
            max_rows: AtomicUsize::new(0),
            slots: (0..n).map(|_| Slot(Mutex::new(None))).collect(),
        }
    }
}
#[derive(Debug, Default)]
#[repr(align(64))]
struct WorkerStats {
    steps: AtomicU64,
    forced_actions: AtomicU64,
    exports: AtomicU64,
    rows: AtomicU64,
    step_ns: AtomicU64,
    export_ns: AtomicU64,
    max_rows: AtomicUsize,
    simulations: AtomicU64,
    search_transitions: AtomicU64,
    forced_search_transitions: AtomicU64,
    cached_edges: AtomicU64,
    chance_samples: AtomicU64,
    terminal_backups: AtomicU64,
    max_depth: AtomicUsize,
    search_ns: AtomicU64,
    peak_cache_nodes: AtomicUsize,
    transposition_hits: AtomicU64,
    persistent_hits: AtomicU64,
    root_reuses: AtomicU64,
    reused_nodes: AtomicU64,
    evicted_nodes: AtomicU64,
    cycle_backups: AtomicU64,
    root_evaluation_hits: AtomicU64,
}
#[derive(Debug)]
struct Shared {
    cfg: Config,
    schema: Arc<Schema>,
    arena: Arena,
    batches: Vec<Batch>,
    pending: ArrayQueue<Task>,
    ready: ArrayQueue<usize>,
    closed: AtomicBool,
    sleep: Mutex<()>,
    wake: Condvar,
    fatal: Mutex<Option<String>>,
    stats: Vec<WorkerStats>,
}
impl Shared {
    fn observation_offset(&self, b: usize, slot: usize) -> usize {
        (b * self.cfg.batch_size + slot) * self.cfg.row_capacity * 16
    }
    fn action_offset(&self, b: usize) -> usize {
        self.cfg.buffers * self.cfg.batch_size * self.cfg.row_capacity * 16
            + b * self.cfg.batch_size * (2 + self.cfg.row_capacity)
    }
    fn logits_offset(&self, b: usize) -> usize {
        self.action_offset(b) + self.cfg.batch_size * 2
    }
    fn value_offset(&self, b: usize) -> usize {
        self.action_offset(b) + self.cfg.batch_size
    }
    fn fail(&self, error: String) {
        let mut fatal = self.fatal.lock().expect("fatal lock");
        if fatal.is_none() {
            *fatal = Some(error);
        }
    }
    fn publish(&self, b: &Batch) {
        b.status.store(READY, Ordering::Release);
        self.ready
            .push(b.id)
            .expect("bounded ready queue cannot overflow");
    }
    fn finish_one(&self, b: &Batch) {
        if b.left.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.publish(b);
        }
    }
    fn execute(&self, task: &mut Task, worker: usize) -> Result<()> {
        let stats = &self.stats[worker];
        loop {
            let begun_search = self.cfg.profile.then(Instant::now);
            if task.record.simulation_budget > 0 {
                if let Some((logits, value)) = task.record.feedback.take() {
                    if let Some(search) = &mut task.record.search {
                        search.receive(logits, value);
                    } else {
                        let seed = sts2_core::run_rng_set::splitmix64(
                            self.cfg.search_seed
                                ^ (task.record.id as u64).wrapping_mul(0x9e3779b97f4a7c15)
                                ^ task.record.session.decisions as u64
                                ^ self.cfg.episode.wrapping_mul(0x85ebca6b),
                        );
                        task.record.search = Some(Search::with_reuse(
                            task.record.retained_search.take(),
                            &task.record.session,
                            logits,
                            value,
                            seed,
                            task.record.simulation_budget,
                            self.cfg.considered_actions,
                            if self.cfg.persistent_search {
                                self.cfg.search_cache_nodes
                            } else {
                                0
                            },
                            self.cfg.actor_version,
                        ));
                    }
                    let search = task.record.search.as_mut().expect("active search");
                    if let Some(action) = search.advance()? {
                        let m = &search.metrics;
                        task.record.searched_decisions += 1;
                        task.record.simulations += m.simulations;
                        stats
                            .simulations
                            .fetch_add(m.simulations, Ordering::Relaxed);
                        stats
                            .search_transitions
                            .fetch_add(m.transitions, Ordering::Relaxed);
                        stats
                            .cached_edges
                            .fetch_add(m.cached_edges, Ordering::Relaxed);
                        stats
                            .chance_samples
                            .fetch_add(m.chance_samples, Ordering::Relaxed);
                        stats
                            .terminal_backups
                            .fetch_add(m.terminal_backups, Ordering::Relaxed);
                        stats
                            .forced_search_transitions
                            .fetch_add(m.forced_transitions, Ordering::Relaxed);
                        stats.max_depth.fetch_max(m.max_depth, Ordering::Relaxed);
                        stats
                            .peak_cache_nodes
                            .fetch_max(m.peak_nodes, Ordering::Relaxed);
                        stats
                            .transposition_hits
                            .fetch_add(m.transposition_hits, Ordering::Relaxed);
                        stats
                            .persistent_hits
                            .fetch_add(m.persistent_hits, Ordering::Relaxed);
                        stats
                            .root_reuses
                            .fetch_add(m.root_reuses, Ordering::Relaxed);
                        stats
                            .reused_nodes
                            .fetch_add(m.reused_nodes, Ordering::Relaxed);
                        stats
                            .evicted_nodes
                            .fetch_add(m.evicted_nodes, Ordering::Relaxed);
                        stats
                            .cycle_backups
                            .fetch_add(m.cycle_backups, Ordering::Relaxed);

                        if self.cfg.record_replay {
                            task.record.trajectory.push(Sample {
                                raw: std::mem::take(&mut task.record.root_rows),
                                policy: search.policy_target(),
                                value: 0.0,
                                combat: (self.cfg.episode << 32) | task.record.id as u64,
                                version: self.cfg.actor_version,
                            });
                        }
                        if self.cfg.persistent_search {
                            task.record.retained_search = task.record.search.take();
                        } else {
                            task.record.search = None;
                        }
                        task.action = Some(action);
                    }
                }
            }
            if let Some(t) = begun_search {
                stats
                    .search_ns
                    .fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
            }
            if let Some(action) = task.action.take() {
                let begun = self.cfg.profile.then(Instant::now);
                task.record.session.step(action)?;
                if self.cfg.record_actions {
                    task.record
                        .actions
                        .push(u16::try_from(action).map_err(|e| e.to_string())?);
                }
                stats.steps.fetch_add(1, Ordering::Relaxed);
                if let Some(t) = begun {
                    stats
                        .step_ns
                        .fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
                }
            }
            forced::advance(&mut task.record, &self.cfg, &self.schema, stats)?;
            let begun = self.cfg.profile.then(Instant::now);
            let offset = self.observation_offset(task.batch, task.slot);
            // SAFETY: this task uniquely owns this final slot until publication.
            let target = unsafe {
                std::slice::from_raw_parts_mut(
                    self.arena.address(offset) as *mut numeric::Row,
                    self.cfg.row_capacity,
                )
            };
            let seed = task.record.seed;
            let used = task
                .record
                .query_mut()
                .numeric_direct_slice(&self.schema, target, seed)?;
            task.record.rows = used;
            stats.exports.fetch_add(1, Ordering::Relaxed);
            stats.rows.fetch_add(used as u64, Ordering::Relaxed);
            stats.max_rows.fetch_max(used, Ordering::Relaxed);
            if let Some(t) = begun {
                stats
                    .export_ns
                    .fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
            }
            if self.cfg.verify_exports {
                let mut expected = Vec::new();
                task.record
                    .query_mut()
                    .numeric_direct(&self.schema, &mut expected, seed)?;
                if expected.as_slice() != &target[..used] {
                    return Err("direct final-buffer export differs from Vec export".into());
                }
            }
            // Transport identity only; the neural decoder ignores header word 8.
            target[0][8] = task.record.id as i32;
            if self.cfg.record_replay && task.record.search.is_none() && !task.record.session.done()
            {
                task.record.root_rows = target[..used].to_vec();
            }
            self.batches[task.batch]
                .max_rows
                .fetch_max(used, Ordering::Relaxed);
            // Reuse a matching evaluation before sending another root to the GPU.
            // The replay observation above remains the freshly exported factual root.
            if task.record.search.is_none() && !task.record.session.done() {
                if let Some(eval) =
                    task.record.retained_search.as_ref().and_then(|s| {
                        s.cached_evaluation(&task.record.session, self.cfg.actor_version)
                    })
                {
                    stats.root_evaluation_hits.fetch_add(1, Ordering::Relaxed);
                    task.record.feedback = Some(eval);
                    continue;
                }
            }
            break;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Control {
    started: bool,
    free: VecDeque<usize>,
    filling: [Option<usize>; CAPACITIES.len()],
    waiting: VecDeque<(Box<Record>, Option<usize>)>,
    leased: usize,
    outcomes: Vec<Value>,
    total: usize,
    delivered_batches: u64,
}
impl Control {
    fn bucket(rows: usize) -> usize {
        CAPACITIES
            .iter()
            .position(|&n| rows <= n)
            .unwrap_or(CAPACITIES.len() - 1)
    }
    fn seal(&mut self, shared: &Shared, kind: usize) {
        if let Some(id) = self.filling[kind].take() {
            let b = &shared.batches[id];
            b.status.store(SEALED, Ordering::Release);
            shared.finish_one(b); // Remove the producer-registration sentinel.
        }
    }
    fn flush(&mut self, shared: &Shared) {
        for i in 0..self.filling.len() {
            self.seal(shared, i);
        }
    }
    fn pump(&mut self, shared: &Shared) {
        // Pair producer notifications with the worker's sleep-lock predicate.
        let _guard = shared.sleep.lock().expect("worker wake lock");
        while let Some((record, _)) = self.waiting.front() {
            let kind = Self::bucket(record.rows);
            if self.filling[kind].is_none() {
                let Some(id) = self.free.pop_front() else {
                    break;
                };
                let b = &shared.batches[id];
                assert_eq!(b.status.load(Ordering::Acquire), FREE);
                b.generation.fetch_add(1, Ordering::Relaxed);
                b.count.store(0, Ordering::Relaxed);
                b.max_rows.store(1, Ordering::Relaxed);
                b.left.store(1, Ordering::Relaxed);
                b.status.store(FILLING, Ordering::Release);
                self.filling[kind] = Some(id);
            }
            let id = self.filling[kind].expect("allocated batch");
            let b = &shared.batches[id];
            let slot = b.count.fetch_add(1, Ordering::Relaxed);
            b.left.fetch_add(1, Ordering::Relaxed);
            let (record, action) = self.waiting.pop_front().expect("nonempty waiting queue");
            shared
                .pending
                .push(Task {
                    batch: id,
                    slot,
                    record,
                    action,
                })
                .expect("bounded pending task capacity");
            if slot + 1 == shared.cfg.batch_size {
                self.seal(shared, kind);
            }
        }
        shared.wake.notify_all();
    }
}

/// A GPU lease. Host addresses remain valid until submit/release; each record
/// has row_capacity*64 byte pitch and starts with the NUM1 header+metadata.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct Descriptor {
    pub id: u64,
    pub generation: u64,
    pub observations: u64,
    pub actions: u64,
    pub values: u64,
    pub logits: u64,
    pub count: u32,
    pub max_rows: u32,
    pub row_capacity: u32,
    pub batch_capacity: u32,
}
#[derive(Debug)]
pub struct Loop {
    shared: Arc<Shared>,
    control: Mutex<Control>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    report: Mutex<CString>,
    replay: Mutex<Queue>,
}
impl Loop {
    fn new(cfg: Config) -> Result<Self> {
        // The C ABI bypasses PyInit_sts2_rs: install content explicitly.
        crate::allocator::never_purge();
        sts2_content::cards::dispatch::install_default_registry();
        sts2_content::relics::dispatch::ensure_installed();
        sts2_content::potions::dispatch::ensure_installed();
        if cfg.workers == 0
            || cfg.workers > 256
            || cfg.batch_size == 0
            || cfg.batch_size > 1024
            || cfg.buffers < 2
            || cfg.buffers > 256
            || cfg.row_capacity < 128
            || cfg.row_capacity > 4096
            || cfg.cases == 0
            || cfg.cases > 131072
            || cfg.max_actions == 0
            || cfg.simulations > 256
            || cfg.simulation_budgets.as_ref().is_some_and(|b| !b.valid())
            || cfg.search_cache_nodes == 0
            || cfg.search_cache_nodes > 4096
            || cfg.considered_actions == 0
            || cfg.replay_capacity == 0
            || cfg
                .fixed_potion_cost
                .is_some_and(|c| !c.is_finite() || !(0.0..=1.0).contains(&c))
            || (cfg.record_replay && cfg.simulations == 0)
        {
            return Err("invalid loop configuration".into());
        }
        let read = |name: &str| {
            std::fs::read_to_string(format!("{}/{}", cfg.schema, name)).map_err(|e| e.to_string())
        };
        let labels: Vec<Value> =
            serde_json::from_str(&read("registry.json")?).map_err(|e| e.to_string())?;
        let templates: Value =
            serde_json::from_str(&read("metadata.json")?).map_err(|e| e.to_string())?;
        let schema = Arc::new(Schema::new(&labels, templates)?);
        let source = std::fs::read_to_string(&cfg.fixtures).map_err(|e| e.to_string())?;
        let lines: Vec<&str> = source.lines().collect();
        if lines.is_empty() {
            return Err("empty fixtures".into());
        }
        let mut waiting = VecDeque::with_capacity(cfg.cases);
        for id in 0..cfg.cases {
            let f: Value = serde_json::from_str(lines[(id + cfg.fixture_offset) % lines.len()])
                .map_err(|e| e.to_string())?;
            let state =
                sts2_wire::state_from_fixture_json(&f["wire"]).map_err(|e| e.to_string())?;
            let seed = f["wire"]["rng"]["seed"].as_u64().unwrap_or(0)
                ^ (id as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
            let mut session =
                Session::from_parts(&schema, state, f["context"].clone(), cfg.max_actions)?;
            if cfg.reward_shaping {
                if session.state.player.current_hp <= 0 {
                    return Err("reward needs positive starting HP".into());
                }
                let seed = sts2_core::run_rng_set::splitmix64(
                    cfg.search_seed
                        ^ (id as u64).wrapping_mul(0x9e3779b97f4a7c15)
                        ^ cfg.episode.wrapping_mul(0x85ebca6b),
                );
                session.reward =
                    super::reward::Context::new(&session.state, seed, cfg.fixed_potion_cost);
            }
            if cfg.fixture_potion_costs {
                if !cfg.reward_shaping {
                    return Err("fixture potion costs require reward shaping".into());
                }
                let costs: Vec<f32> =
                    serde_json::from_value(f["evaluation"]["potion_costs"].clone())
                        .map_err(|e| format!("invalid fixture potion costs: {e}"))?;
                session.reward.set_initial_costs(&costs)?;
            }
            session.numeric_observation(&schema)?;
            let rows = session.numeric_cache.rows.len() + 1;
            if rows > cfg.row_capacity {
                return Err(format!("initial observation {id} needs {rows} rows"));
            }
            let (search_tier, simulation_budget) = match &cfg.simulation_budgets {
                Some(b) => b.resolve(&f)?,
                None => ("uniform", cfg.simulations),
            };
            waiting.push_back((
                Box::new(Record {
                    id,
                    simulation_budget,
                    search_tier,
                    searched_decisions: 0,
                    forced_actions: 0,
                    simulations: 0,
                    session,
                    seed,
                    rows,
                    root_rows: Vec::new(),
                    trajectory: Vec::new(),
                    search: None,
                    retained_search: None,
                    feedback: None,
                    actions: Vec::with_capacity(if cfg.record_actions {
                        cfg.max_actions
                    } else {
                        0
                    }),
                }),
                None,
            ));
        }
        let control = Control {
            started: false,
            free: (0..cfg.buffers).collect(),
            filling: [None; CAPACITIES.len()],
            waiting,
            leased: 0,
            outcomes: Vec::with_capacity(cfg.cases),
            total: cfg.cases,
            delivered_batches: 0,
        };
        let words = cfg.buffers * cfg.batch_size * (cfg.row_capacity * 17 + 2);
        let arena = Arena::new(words)?;
        let shared = Arc::new(Shared {
            schema,
            arena,
            batches: (0..cfg.buffers)
                .map(|i| Batch::new(i, cfg.batch_size))
                .collect(),
            pending: ArrayQueue::new(cfg.buffers * cfg.batch_size),
            ready: ArrayQueue::new(cfg.buffers),
            closed: AtomicBool::new(false),
            sleep: Mutex::new(()),
            wake: Condvar::new(),
            fatal: Mutex::new(None),
            stats: (0..cfg.workers).map(|_| WorkerStats::default()).collect(),
            cfg,
        });
        let (tx, rx) = mpsc::channel();
        let threads = (0..shared.cfg.workers)
            .map(|worker| {
                let shared = Arc::clone(&shared);
                let tx = tx.clone();
                thread::spawn(move || {
                    tx.send(()).expect("startup receiver");
                    loop {
                        if shared.closed.load(Ordering::Acquire) {
                            break;
                        }
                        let task = if let Some(task) = shared.pending.pop() {
                            Some(task)
                        } else {
                            let mut guard = shared.sleep.lock().expect("worker sleep lock");
                            loop {
                                if shared.closed.load(Ordering::Acquire) {
                                    break None;
                                }
                                if let Some(task) = shared.pending.pop() {
                                    break Some(task);
                                }
                                guard = shared.wake.wait(guard).expect("worker wait");
                            }
                        };
                        let Some(mut task) = task else {
                            break;
                        };
                        let result =
                            catch_unwind(AssertUnwindSafe(|| shared.execute(&mut task, worker)))
                                .unwrap_or_else(|_| Err("panic in native worker".into()));
                        if let Err(e) = result {
                            shared.fail(format!("combat {}: {e}", task.record.id));
                        }
                        let b = &shared.batches[task.batch];
                        let mut slot = b.slots[task.slot].0.lock().expect("slot completion lock");
                        assert!(slot.is_none());
                        *slot = Some(task.record);
                        drop(slot);
                        shared.finish_one(b);
                    }
                })
            })
            .collect();
        for _ in 0..shared.cfg.workers {
            rx.recv().map_err(|e| e.to_string())?;
        }
        Ok(Self {
            shared,
            control: Mutex::new(control),
            threads: Mutex::new(threads),
            report: Mutex::new(CString::default()),
            replay: Mutex::new(Queue::default()),
        })
    }
    fn start(&self) -> Result<()> {
        let mut c = self.control.lock().map_err(|e| e.to_string())?;
        if c.started {
            return Err("loop already started".into());
        }
        c.started = true;
        c.pump(&self.shared);
        c.flush(&self.shared);
        Ok(())
    }
    fn poll(&self, out: &mut Descriptor) -> Result<i32> {
        if let Some(e) = self.shared.fatal.lock().map_err(|e| e.to_string())?.clone() {
            return Err(e);
        }
        let mut c = self.control.lock().map_err(|e| e.to_string())?;
        if !c.started {
            return Err("loop not started".into());
        }
        let Some(id) = self.shared.ready.pop() else {
            if c.outcomes.len() == c.total {
                return Ok(2);
            }
            if c.leased == 0 {
                c.pump(&self.shared);
                c.flush(&self.shared);
            }
            return Ok(0);
        };
        let b = &self.shared.batches[id];
        b.status
            .compare_exchange(READY, LEASED, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "invalid ready batch state")?;
        *out = Descriptor {
            id: id as u64,
            generation: b.generation.load(Ordering::Acquire),
            observations: self
                .shared
                .arena
                .address(self.shared.observation_offset(id, 0)) as u64,
            actions: self.shared.arena.address(self.shared.action_offset(id)) as u64,
            values: self.shared.arena.address(self.shared.value_offset(id)) as u64,
            logits: self.shared.arena.address(self.shared.logits_offset(id)) as u64,
            count: b.count.load(Ordering::Acquire) as u32,
            max_rows: b.max_rows.load(Ordering::Acquire) as u32,
            row_capacity: self.shared.cfg.row_capacity as u32,
            batch_capacity: self.shared.cfg.batch_size as u32,
        };
        c.leased += 1;
        c.delivered_batches += 1;
        Ok(1)
    }
    fn submit(&self, id: usize, generation: u64) -> Result<()> {
        let mut c = self.control.lock().map_err(|e| e.to_string())?;
        let b = self.shared.batches.get(id).ok_or("invalid batch id")?;
        if b.status.load(Ordering::Acquire) != LEASED
            || b.generation.load(Ordering::Acquire) != generation
        {
            return Err("stale or duplicate GPU batch return".into());
        }
        let count = b.count.load(Ordering::Acquire);
        // SAFETY: the caller completed D2H into these pinned result arrays before
        // returning the lease. No worker owns this LEASED batch's buffers.
        let actions = unsafe {
            std::slice::from_raw_parts(
                self.shared.arena.address(self.shared.action_offset(id)) as *const i32,
                count,
            )
        };
        let values = unsafe {
            std::slice::from_raw_parts(
                self.shared.arena.address(self.shared.value_offset(id)) as *const f32,
                count,
            )
        };
        let logits = unsafe {
            std::slice::from_raw_parts(
                self.shared.arena.address(self.shared.logits_offset(id)) as *const f32,
                count * self.shared.cfg.row_capacity,
            )
        };
        // Validate the entire return transaction before consuming a single slot.
        for (i, &action) in actions.iter().enumerate() {
            let slot = b.slots[i].0.lock().map_err(|e| e.to_string())?;
            let r = slot.as_ref().ok_or("missing completed record")?;
            if r.simulation_budget > 0
                && logits[i * self.shared.cfg.row_capacity
                    ..i * self.shared.cfg.row_capacity + r.query().commands.len()]
                    .iter()
                    .any(|v| !v.is_finite())
            {
                return Err("nonfinite GPU policy logit".into());
            }
            if !values[i].is_finite() {
                return Err("nonfinite GPU value".into());
            }
            if (r.query().done() && action != -1)
                || (!r.query().done()
                    && (action < 0 || action as usize >= r.query().commands.len()))
            {
                return Err(format!(
                    "illegal GPU action {action} for combat {} with {} legal actions",
                    r.id,
                    r.query().commands.len()
                ));
            }
        }
        for (i, &action) in actions.iter().enumerate() {
            let mut r = b.slots[i]
                .0
                .lock()
                .map_err(|e| e.to_string())?
                .take()
                .ok_or("missing completed record")?;
            if r.query().done() {
                let s = &r.session;
                let mut replay = self.replay.lock().map_err(|e| e.to_string())?;
                if s.state.is_over {
                    replay.commit(
                        std::mem::take(&mut r.trajectory),
                        s.reward.utility(&s.state),
                        self.shared.cfg.replay_capacity,
                    );
                } else {
                    replay.censored += 1;
                }
                c.outcomes.push(json!({"id":r.id,"decisions":s.decisions,"search_tier":r.search_tier,"simulation_budget":r.simulation_budget,"searched_decisions":r.searched_decisions,"forced_actions":r.forced_actions,"simulations":r.simulations,"terminal":s.state.is_over,
                    "win":s.state.is_over&&s.state.player_won&&s.state.player.current_hp>0,
                    "truncated":!s.state.is_over,"hp":s.state.player.current_hp,"trace":s.numeric_cache.trace,
                    "return":s.reward.utility(&s.state),"spent_cost":s.reward.spent,"damage_fraction":s.reward.progress(),"initial_hp":s.reward.initial_hp,"initial_enemy_hp":s.reward.initial_enemy_hp(),
                    "potions":s.reward.ledger.iter().map(|p|json!({"id":p.id,"cost":p.cost,"used":p.used})).collect::<Vec<_>>(),
                    "state_hash":sts2_core::state_hash(&s.state),"actions":r.actions,"value":values[i]}));
            } else if r.simulation_budget > 0 {
                let start = i * self.shared.cfg.row_capacity;
                r.feedback = Some((
                    logits[start..start + r.query().commands.len()].to_vec(),
                    values[i],
                ));
                c.waiting.push_back((r, None));
            } else {
                c.waiting.push_back((r, Some(action as usize)));
            }
        }
        b.status.store(FREE, Ordering::Release);
        c.free.push_back(id);
        c.leased -= 1;
        c.pump(&self.shared);
        Ok(())
    }
    fn report(&self) -> Result<*const c_char> {
        let c = self.control.lock().map_err(|e| e.to_string())?;
        let sum = |f: fn(&WorkerStats) -> &AtomicU64| {
            self.shared
                .stats
                .iter()
                .map(|s| f(s).load(Ordering::Relaxed))
                .sum::<u64>()
        };
        let mut outcomes = c.outcomes.clone();
        outcomes.sort_by_key(|r| r["id"].as_u64());
        let replay = self.replay.lock().map_err(|e| e.to_string())?;
        let mut tier_totals = std::collections::BTreeMap::<String, Value>::new();
        for row in &outcomes {
            let tier = row["search_tier"].as_str().expect("record tier");
            let entry = tier_totals.entry(tier.to_owned()).or_insert_with(|| json!({"combats":0,"played_decisions":0,"searched_decisions":0,"forced_actions":0,"simulations":0,"budget":row["simulation_budget"]}));
            entry["combats"] = json!(entry["combats"].as_u64().unwrap() + 1);
            for (target, source) in [
                ("played_decisions", "decisions"),
                ("searched_decisions", "searched_decisions"),
                ("forced_actions", "forced_actions"),
                ("simulations", "simulations"),
            ] {
                entry[target] =
                    json!(entry[target].as_u64().unwrap() + row[source].as_u64().unwrap());
            }
        }
        let value = json!({"environment_revision":"native-ai-import-v1","replay_added":replay.added,"replay_queued":replay.samples.len(),"replay_evicted":replay.evicted,"replay_censored":replay.censored,"actor_version":self.shared.cfg.actor_version,"episode":self.shared.cfg.episode,"cases":c.total,"completed":c.outcomes.len(),"workers":self.shared.cfg.workers,
            "batch_size":self.shared.cfg.batch_size,"buffers":self.shared.cfg.buffers,"row_capacity":self.shared.cfg.row_capacity,
            "max_actions":self.shared.cfg.max_actions,"profile":self.shared.cfg.profile,"verify_exports":self.shared.cfg.verify_exports,
            "skip_forced_actions":self.shared.cfg.skip_forced_actions,"forced_actions":sum(|s|&s.forced_actions),
            "fixture_potion_costs":self.shared.cfg.fixture_potion_costs,
            "simulation_budget":if self.shared.cfg.simulation_budgets.is_some() {Value::Null} else {json!(self.shared.cfg.simulations)},"simulation_budgets":self.shared.cfg.simulation_budgets,"search_by_tier":tier_totals,"considered_actions":self.shared.cfg.considered_actions,
            "simulations":sum(|s|&s.simulations),"search_transitions":sum(|s|&s.search_transitions),"forced_search_transitions":sum(|s|&s.forced_search_transitions),
            "search_reuse":{"enabled":self.shared.cfg.persistent_search,"capacity_per_combat":self.shared.cfg.search_cache_nodes,
"transposition_hits":sum(|s|&s.transposition_hits),"persistent_hits":sum(|s|&s.persistent_hits),"root_reuses":sum(|s|&s.root_reuses),"reused_nodes":sum(|s|&s.reused_nodes),"evicted_nodes":sum(|s|&s.evicted_nodes),"cycle_backups":sum(|s|&s.cycle_backups),"root_evaluation_hits":sum(|s|&s.root_evaluation_hits),"peak_nodes_per_combat":self.shared.stats.iter().map(|s|s.peak_cache_nodes.load(Ordering::Relaxed)).max().unwrap_or(0)},
            "cached_edges":sum(|s|&s.cached_edges),"chance_samples":sum(|s|&s.chance_samples),
            "terminal_backups":sum(|s|&s.terminal_backups),"cpu_search_ns":sum(|s|&s.search_ns),
            "max_search_depth":self.shared.stats.iter().map(|s|s.max_depth.load(Ordering::Relaxed)).max(),
            "steps":sum(|s|&s.steps),"exports":sum(|s|&s.exports),"export_rows":sum(|s|&s.rows),
            "cpu_step_ns":sum(|s|&s.step_ns),"cpu_export_ns":sum(|s|&s.export_ns),
            "max_rows":self.shared.stats.iter().map(|s|s.max_rows.load(Ordering::Relaxed)).max(),
            "gpu_batches":c.delivered_batches,"outcomes":outcomes,"fatal":*self.shared.fatal.lock().map_err(|e|e.to_string())?});
        let mut report = self.report.lock().map_err(|e| e.to_string())?;
        *report = CString::new(value.to_string()).map_err(|e| e.to_string())?;
        Ok(report.as_ptr())
    }
}
impl Drop for Loop {
    fn drop(&mut self) {
        {
            let _g = self.shared.sleep.lock().expect("shutdown lock");
            self.shared.closed.store(true, Ordering::Release);
            self.shared.wake.notify_all();
        }
        for t in self.threads.get_mut().expect("threads lock").drain(..) {
            let _ = t.join();
        }
    }
}
thread_local! { static ERROR: std::cell::RefCell<CString> = std::cell::RefCell::new(CString::default()); }
fn ffi<T: Copy>(failure: T, f: impl FnOnce() -> Result<T>) -> T {
    match catch_unwind(AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err("panic at loop API boundary".into()))
    {
        Ok(v) => v,
        Err(e) => {
            ERROR.with(|s| {
                *s.borrow_mut() = CString::new(e.replace('\0', " ")).expect("sanitized error");
            });
            failure
        }
    }
}
/// Last error on the calling thread, valid until its next failed call.
#[no_mangle]
pub extern "C" fn sm_loop_error() -> *const c_char {
    ERROR.with(|s| s.borrow().as_ptr())
}
/// Construct native workers and pinned buffers without starting transitions.
/// # Safety
/// `config` must point to a live NUL-terminated UTF-8 JSON string.
#[no_mangle]
pub unsafe extern "C" fn sm_loop_create(config: *const c_char) -> *mut Loop {
    ffi(std::ptr::null_mut(), || {
        if config.is_null() {
            return Err("null config".into());
        }
        let text = unsafe { CStr::from_ptr(config) }
            .to_str()
            .map_err(|e| e.to_string())?;
        let cfg = serde_json::from_str(text).map_err(|e| e.to_string())?;
        Ok(Box::into_raw(Box::new(Loop::new(cfg)?)))
    })
}
/// # Safety
/// `pool` must be a live handle returned by create; no concurrent destruction.
#[no_mangle]
pub unsafe extern "C" fn sm_loop_start(pool: *const Loop) -> i32 {
    ffi(-1, || {
        unsafe { pool.as_ref() }.ok_or("null pool")?.start()?;
        Ok(0)
    })
}
/// Returns 1 for a GPU lease, 0 when not ready, 2 when all combats completed.
/// # Safety
/// `pool` must be live; `out` must reference a writable Descriptor.
#[no_mangle]
pub unsafe extern "C" fn sm_loop_poll(pool: *const Loop, out: *mut Descriptor) -> i32 {
    ffi(-1, || {
        unsafe { pool.as_ref() }
            .ok_or("null pool")?
            .poll(unsafe { out.as_mut() }.ok_or("null descriptor")?)
    })
}
/// # Safety
/// The caller must finish all GPU reads and result D2H writes for this lease
/// before calling. Native workers may immediately overwrite its buffers.
#[no_mangle]
pub unsafe extern "C" fn sm_loop_submit(pool: *const Loop, id: u64, generation: u64) -> i32 {
    ffi(-1, || {
        unsafe { pool.as_ref() }
            .ok_or("null pool")?
            .submit(id as usize, generation)?;
        Ok(0)
    })
}
/// # Safety
/// Live handle required. Returned text is borrowed until the next report call.
#[no_mangle]
pub unsafe extern "C" fn sm_loop_report(pool: *const Loop) -> *const c_char {
    ffi(std::ptr::null(), || {
        unsafe { pool.as_ref() }.ok_or("null pool")?.report()
    })
}
/// # Safety
/// Exclusive handle ownership; finish every GPU access before destruction.
#[no_mangle]
pub unsafe extern "C" fn sm_loop_destroy(pool: *mut Loop) {
    if !pool.is_null() {
        drop(unsafe { Box::from_raw(pool) });
    }
}

/// Drain up to limit completed-decision samples. Pointers are valid until the
/// next drain or destruction. The caller must copy before calling drain again.
/// # Safety
/// Pool and output must be live; serialize replay drains.
#[no_mangle]
pub unsafe extern "C" fn sm_loop_replay(
    pool: *const Loop,
    out: *mut ReplayDescriptor,
    limit: u32,
) -> i32 {
    ffi(-1, || {
        let p = unsafe { pool.as_ref() }.ok_or("null pool")?;
        let out = unsafe { out.as_mut() }.ok_or("null replay descriptor")?;
        Ok(p.replay
            .lock()
            .map_err(|e| e.to_string())?
            .drain(out, limit as usize)? as i32)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bucket_boundaries_keep_every_row() {
        for rows in 1..=1024 {
            assert!(CAPACITIES[Control::bucket(rows)] >= rows);
        }
    }
    #[test]
    fn invalid_configuration_rejected_before_cuda() {
        let c = Config {
            workers: 0,
            ..Config::default()
        };
        assert!(Loop::new(c).unwrap_err().contains("invalid loop"));
    }
    #[test]
    fn descriptor_abi_size_is_stable() {
        assert_eq!(std::mem::size_of::<Descriptor>(), 64);
    }
}
