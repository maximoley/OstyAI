//! Persistent Rust workers, a shared MPMC action queue, and owned final buffers.
//! No Python API calls or interpreter initialization occur in this module.
use super::{numeric, Result, Schema, Session};
#[cfg(feature = "native-gpu-feeder")]
pub(crate) mod gpu_feed;
use crossbeam_queue::ArrayQueue;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
enum Mode {
    Direct,
    CachedCopy,
    StepOnly,
}

#[derive(Debug)]
struct Job {
    id: usize,
    session: Session,
    action: usize,
    seed: u64,
    output: Vec<numeric::Row>,
    #[cfg(feature = "native-gpu-feeder")]
    pinned: Option<gpu_feed::PinnedSlot>,
    #[cfg(feature = "native-gpu-feeder")]
    pinned_len: usize,
    mode: Mode,
    profile: bool,
    worker: usize,
    step_ns: u64,
    export_ns: u64,
    grew: bool,
    error: Option<String>,
}

impl Job {
    fn output_rows(&self) -> &[numeric::Row] {
        #[cfg(feature = "native-gpu-feeder")]
        if let Some(slot) = &self.pinned {
            return slot.read(self.pinned_len);
        }
        &self.output
    }

    fn execute(&mut self, schema: &Schema) -> Result<()> {
        let before = self.output.capacity();
        let start = self.profile.then(Instant::now);
        self.session.step(self.action)?;
        let stepped = self.profile.then(Instant::now);
        #[cfg(feature = "native-gpu-feeder")]
        if let Some(slot) = &mut self.pinned {
            self.pinned_len =
                slot.write(|rows| self.session.numeric_direct_slice(schema, rows, self.seed))?;
            return Ok(());
        }
        match self.mode {
            Mode::Direct => self
                .session
                .numeric_direct(schema, &mut self.output, self.seed)?,
            Mode::CachedCopy => {
                self.session.numeric_observation(schema)?;
                self.output.clear();
                self.output.push(
                    self.session
                        .numeric_frame(self.session.numeric_cache.rows.len(), self.seed),
                );
                self.output
                    .extend_from_slice(&self.session.numeric_cache.rows);
            }
            Mode::StepOnly => (),
        }
        if let (Some(start), Some(stepped)) = (start, stepped) {
            self.step_ns = stepped.duration_since(start).as_nanos() as u64;
            self.export_ns = stepped.elapsed().as_nanos() as u64;
        }
        self.grew = self.output.capacity() != before;
        Ok(())
    }
}

#[derive(Debug)]
#[repr(align(64))]
struct OutputSlot(OnceLock<Box<Job>>);

#[derive(Debug)]
struct Completion {
    slots: Vec<OutputSlot>,
    remaining: AtomicUsize,
    #[cfg(feature = "native-gpu-feeder")]
    feed: Option<Arc<gpu_feed::FeedState>>,
    sleep: Mutex<()>,
    wake: Condvar,
}

impl Completion {
    fn len(&self) -> usize {
        self.slots.len()
    }
    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
    fn iter(&self) -> impl Iterator<Item = &Job> {
        self.slots
            .iter()
            .map(|slot| slot.0.get().expect("completed slot").as_ref())
    }
}

#[derive(Debug)]
struct WorkItem {
    job: Box<Job>,
    completion: Arc<Completion>,
}

#[derive(Debug)]
struct Shared {
    pending: ArrayQueue<WorkItem>,
    closed: Mutex<bool>,
    wake: Condvar,
}

#[derive(Debug)]
struct Workers {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

impl Workers {
    fn new(schema: Arc<Schema>, count: usize, capacity: usize) -> Self {
        let shared = Arc::new(Shared {
            pending: ArrayQueue::new(capacity),
            closed: Mutex::new(false),
            wake: Condvar::new(),
        });
        let (started_tx, started_rx) = mpsc::channel();
        let threads = (0..count)
            .map(|worker| {
                let shared = Arc::clone(&shared);
                let schema = Arc::clone(&schema);
                let started = started_tx.clone();
                thread::spawn(move || {
                    started.send(()).expect("startup receiver alive");
                    loop {
                        let item = if let Some(item) = shared.pending.pop() {
                            Some(item)
                        } else {
                            let mut closed = shared.closed.lock().expect("queue sleep lock");
                            loop {
                                if let Some(item) = shared.pending.pop() {
                                    break Some(item);
                                }
                                if *closed {
                                    break None;
                                }
                                closed = shared.wake.wait(closed).expect("queue sleep lock");
                            }
                        };
                        let Some(WorkItem {
                            mut job,
                            completion,
                        }) = item
                        else {
                            break;
                        };
                        job.worker = worker;
                        job.error = job.execute(&schema).err();
                        let id = job.id;
                        completion.slots[id]
                            .0
                            .set(job)
                            .expect("one producer per output slot");
                        #[cfg(feature = "native-gpu-feeder")]
                        if let Some(feed) = &completion.feed {
                            feed.publish(id);
                        }
                        if completion.remaining.fetch_sub(1, Ordering::AcqRel) == 1 {
                            let _guard = completion.sleep.lock().expect("completion sleep lock");
                            completion.wake.notify_one();
                        }
                    }
                })
            })
            .collect();
        for _ in 0..count {
            started_rx.recv().expect("worker startup");
        }
        Self { shared, threads }
    }

    fn run(&self, jobs: Vec<Box<Job>>) -> Result<(Duration, Arc<Completion>)> {
        let count = jobs.len();
        if count > self.shared.pending.capacity() {
            return Err("queue capacity exceeded".into());
        }
        let mut seen = vec![false; count];
        for job in &jobs {
            if job.id >= count || seen[job.id] {
                return Err("duplicate/out-of-range job".into());
            }
            seen[job.id] = true;
        }
        let completion = Arc::new(Completion {
            slots: (0..count).map(|_| OutputSlot(OnceLock::new())).collect(),
            remaining: AtomicUsize::new(count),
            #[cfg(feature = "native-gpu-feeder")]
            feed: None,
            sleep: Mutex::new(()),
            wake: Condvar::new(),
        });
        let start = Instant::now();
        {
            let _guard = self.shared.closed.lock().map_err(|e| e.to_string())?;
            for job in jobs {
                self.shared
                    .pending
                    .push(WorkItem {
                        job,
                        completion: Arc::clone(&completion),
                    })
                    .map_err(|_| "pending queue overflow")?;
            }
            self.shared.wake.notify_all();
        }
        {
            let mut guard = completion.sleep.lock().map_err(|e| e.to_string())?;
            while completion.remaining.load(Ordering::Acquire) != 0 {
                let (next, timeout) = completion
                    .wake
                    .wait_timeout(guard, Duration::from_secs(60))
                    .map_err(|e| e.to_string())?;
                guard = next;
                if timeout.timed_out() && completion.remaining.load(Ordering::Acquire) != 0 {
                    return Err("worker timeout".into());
                }
            }
        }
        let elapsed = start.elapsed();
        for job in completion.iter() {
            if let Some(error) = &job.error {
                return Err(format!("job {}: {error}", job.id));
            }
        }
        Ok((elapsed, completion))
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        *self.shared.closed.lock().expect("queue sleep lock") = true;
        self.shared.wake.notify_all();
        for handle in self.threads.drain(..) {
            handle.join().expect("native worker panicked");
        }
    }
}

fn make_jobs(
    roots: &[Session],
    actions: &[usize],
    seeds: &[u64],
    count: usize,
    fixed: bool,
    mode: Mode,
    profile: bool,
) -> Vec<Box<Job>> {
    (0..count)
        .map(|id| {
            let at = if fixed { 0 } else { id % roots.len() };
            let session = roots[at].clone();
            // Pre-touch final buffer storage outside processing time; length/capacity remain distinct.
            let mut output = vec![[0; numeric::WIDTH]; session.numeric_cache.rows.len() * 2 + 128];
            output.clear();
            Box::new(Job {
                id,
                session,
                action: actions[at],
                seed: seeds[at],
                output,
                #[cfg(feature = "native-gpu-feeder")]
                pinned: None,
                #[cfg(feature = "native-gpu-feeder")]
                pinned_len: 0,
                mode,
                profile,
                worker: 0,
                step_ns: 0,
                export_ns: 0,
                grew: false,
                error: None,
            })
        })
        .collect()
}

fn verify(job: &Job, at: usize, oracle: &[i32], expected: &Value) -> Result<()> {
    let b = oracle[2] as usize;
    let m = &oracle[8 + at * 8..16 + at * 8];
    let n = m[2] as usize;
    let first = 8 + b * 8 + m[1] as usize * 16;
    let mut header = [0; 16];
    header[..8].copy_from_slice(&[0x4e55_4d31, 1, 1, n as i32, 16, 8, 0, 0]);
    header[8..].copy_from_slice(m);
    header[8] = 0;
    header[9] = 0;
    let rows = job.output_rows();
    if rows.first() != Some(&header) || rows.len() != n + 1 {
        return Err(format!("job {} header/length differs", job.id));
    }
    for (row, reference) in rows[1..]
        .iter()
        .zip(oracle[first..first + n * 16].chunks_exact(16))
    {
        if row.as_slice() != reference {
            return Err(format!("job {} observation differs", job.id));
        }
    }
    let state_hash = sts2_core::state_hash(&job.session.state);
    if Some(state_hash) != expected["state_hash"].as_u64()
        || Some(job.session.numeric_cache.trace) != expected["trace"].as_u64()
    {
        return Err(format!("job {} state/action trace differs", job.id));
    }
    job.session.state.assert_invariants()?;
    Ok(())
}

pub(crate) fn run_benchmark(root: &Path) -> Result<()> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| e.to_string());
    let base = Path::new("ostyai/schema");
    let labels: Vec<Value> =
        serde_json::from_str(&read(&base.join("registry.json"))?).map_err(|e| e.to_string())?;
    let templates =
        serde_json::from_str(&read(&base.join("metadata.json"))?).map_err(|e| e.to_string())?;
    let schema = Arc::new(Schema::new(&labels, templates)?);
    let source = read(Path::new("ostyai/validation/fixtures.jsonl"))?;
    let mut roots = Vec::new();
    for line in source.lines() {
        let fixture: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let state = sts2_wire::state_from_json(&fixture["wire"]).map_err(|e| e.to_string())?;
        let mut session = Session::from_parts(&schema, state, fixture["context"].clone(), 500)?;
        session.numeric_observation(&schema)?;
        roots.push(session);
    }
    let reference: Value =
        serde_json::from_str(&read(&root.join("oracle.json"))?).map_err(|e| e.to_string())?;
    let actions: Vec<usize> =
        serde_json::from_value(reference["actions"].clone()).map_err(|e| e.to_string())?;
    let seeds: Vec<u64> =
        serde_json::from_value(reference["seeds"].clone()).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(root.join("oracle.bin")).map_err(|e| e.to_string())?;
    let oracle: Vec<i32> = bytes
        .chunks_exact(4)
        .map(|b| i32::from_le_bytes(b.try_into().expect("four bytes")))
        .collect();
    let mut results = Vec::new();
    for workers in [1, 80] {
        let pool = Workers::new(Arc::clone(&schema), workers, 32768);
        let mut invalid = make_jobs(&roots, &actions, &seeds, 1, true, Mode::Direct, false);
        invalid[0].action = usize::MAX;
        if pool.run(invalid).is_ok() {
            return Err("invalid action unexpectedly accepted".into());
        }
        // Frozen OLD extension is independent of the refactored row writer.
        let (_, jobs) = pool.run(make_jobs(
            &roots,
            &actions,
            &seeds,
            roots.len(),
            false,
            Mode::Direct,
            false,
        ))?;
        for job in jobs.iter() {
            verify(job, job.id, &oracle, &reference["proof"][job.id])?;
        }
        println!(
            "{}",
            json!({"validation_workers":workers,"states":jobs.len(),"exact_records_actions_state_hash":true})
        );
        drop(jobs);
        for fixed in [true, false] {
            let count = if fixed { 8192 } else { roots.len() };
            for mode in [Mode::StepOnly, Mode::CachedCopy, Mode::Direct] {
                for trial in 0..6 {
                    let (elapsed, jobs) = pool.run(make_jobs(
                        &roots, &actions, &seeds, count, fixed, mode, false,
                    ))?;
                    if !matches!(mode, Mode::StepOnly) {
                        for job in jobs.iter() {
                            let at = if fixed { 0 } else { job.id };
                            verify(job, at, &oracle, &reference["proof"][at])?;
                        }
                    }
                    let workers_used: BTreeSet<_> = jobs.iter().map(|j| j.worker).collect();
                    let row = json!({"workers":workers,"workload":if fixed {"fixed_strike"}else{"8196_distinct"},
                        "mode":format!("{mode:?}"),"trial":trial,"warmup":trial==0,"jobs":count,
                        "seconds":elapsed.as_secs_f64(),"decisions_per_second":count as f64/elapsed.as_secs_f64(),
                        "workers_used":workers_used.len(),"output_bytes":jobs.iter().map(|j|j.output.len()*64).sum::<usize>(),
                        "output_buffer_growths":jobs.iter().filter(|j|j.grew).count()});
                    println!("{row}");
                    results.push(row);
                }
            }
            // Separate instrumented run: throughput above has no per-job timers.
            let (_, jobs) = pool.run(make_jobs(
                &roots,
                &actions,
                &seeds,
                count,
                fixed,
                Mode::Direct,
                true,
            ))?;
            let row = json!({"profile":true,"workers":workers,"workload":if fixed {"fixed_strike"}else{"8196_distinct"},
                "jobs":count,"step_ns_sum":jobs.iter().map(|j|j.step_ns).sum::<u64>(),
                "export_ns_sum":jobs.iter().map(|j|j.export_ns).sum::<u64>()});
            println!("{row}");
            results.push(row);
        }
    }
    std::fs::write(
        root.join("results.json"),
        serde_json::to_vec_pretty(&results).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_workers_shutdown_and_empty_waves_do_not_deadlock() {
        let schema = Arc::new(Schema::new(&[], json!({})).unwrap());
        let pool = Workers::new(schema, 2, 8);
        for _ in 0..4 {
            assert!(pool.run(Vec::new()).unwrap().1.is_empty());
        }
        drop(pool);
    }
}
