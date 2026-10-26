//! Bounded observation frontiers, one native feeder, and real asynchronous H2D copies.
//! CUDA calls are confined to setup, the feeder, and post-timing validation.
use super::{
    json, make_jobs, mpsc, numeric, thread, verify, Arc, ArrayQueue, AtomicUsize, Completion,
    Condvar, Duration, Instant, Job, Mode, Mutex, OnceLock, Ordering, OutputSlot, Path, Result,
    Schema, Session, Value, WorkItem, Workers,
};
use std::ffi::c_void;
use std::ptr::NonNull;

#[link(name = "cudart")]
extern "C" {
    fn cudaSetDevice(device: i32) -> i32;
    fn cudaHostAlloc(ptr: *mut *mut c_void, bytes: usize, flags: u32) -> i32;
    fn cudaFreeHost(ptr: *mut c_void) -> i32;
    fn cudaMalloc(ptr: *mut *mut c_void, bytes: usize) -> i32;
    fn cudaFree(ptr: *mut c_void) -> i32;
    fn cudaMemcpy(dst: *mut c_void, src: *const c_void, bytes: usize, kind: i32) -> i32;
    fn cudaMemcpyAsync(
        dst: *mut c_void,
        src: *const c_void,
        bytes: usize,
        kind: i32,
        stream: *mut c_void,
    ) -> i32;
    fn cudaStreamCreateWithFlags(stream: *mut *mut c_void, flags: u32) -> i32;
    fn cudaStreamDestroy(stream: *mut c_void) -> i32;
    fn cudaStreamSynchronize(stream: *mut c_void) -> i32;
    fn cudaEventCreate(event: *mut *mut c_void) -> i32;
    fn cudaEventDestroy(event: *mut c_void) -> i32;
    fn cudaEventRecord(event: *mut c_void, stream: *mut c_void) -> i32;
    fn cudaEventSynchronize(event: *mut c_void) -> i32;
    fn cudaEventElapsedTime(ms: *mut f32, start: *mut c_void, end: *mut c_void) -> i32;
}
fn check(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("CUDA error {code}"))
    }
}

#[derive(Debug)]
struct Pinned {
    ptr: NonNull<i32>,
    words: usize,
}
// SAFETY: allocations remain live through Arc ownership. Private APIs permit
// CPU writes only to unique job slices or feeder-owned staging buffers; DMA
// starts only after publication, and all streams finish before reuse/free.
unsafe impl Send for Pinned {}
unsafe impl Sync for Pinned {}
impl Pinned {
    fn new(words: usize) -> Result<Self> {
        let mut raw = std::ptr::null_mut();
        // SAFETY: CUDA initializes raw to an owned page-locked allocation.
        unsafe {
            check(cudaHostAlloc(&mut raw, words * 4, 0))?;
        }
        let ptr = NonNull::new(raw.cast::<i32>()).ok_or("null pinned allocation")?;
        // SAFETY: allocation has words writable i32 elements; pre-touch outside timing.
        unsafe {
            std::ptr::write_bytes(ptr.as_ptr(), 0, words);
        }
        Ok(Self { ptr, words })
    }
}
impl Drop for Pinned {
    fn drop(&mut self) {
        unsafe {
            cudaFreeHost(self.ptr.as_ptr().cast());
        }
    }
}

#[derive(Debug)]
pub(super) struct PinnedSlot {
    arena: Arc<Pinned>,
    offset: usize,
    rows: usize,
}
impl PinnedSlot {
    pub(super) fn write<F>(&mut self, f: F) -> Result<usize>
    where
        F: FnOnce(&mut [numeric::Row]) -> Result<usize>,
    {
        assert!(self.offset + self.rows * 16 <= self.arena.words);
        // SAFETY: each job receives exactly one disjoint slot. &mut self prevents
        // two writes by that job; the feeder waits for the batch's release counter.
        let rows = unsafe {
            std::slice::from_raw_parts_mut(
                self.arena
                    .ptr
                    .as_ptr()
                    .add(self.offset)
                    .cast::<numeric::Row>(),
                self.rows,
            )
        };
        f(rows)
    }
    pub(super) fn read(&self, len: usize) -> &[numeric::Row] {
        assert!(len <= self.rows);
        // SAFETY: read only after this slot's producer published its completed Job.
        unsafe {
            std::slice::from_raw_parts(
                self.arena
                    .ptr
                    .as_ptr()
                    .add(self.offset)
                    .cast::<numeric::Row>(),
                len,
            )
        }
    }
}

#[derive(Debug)]
#[repr(align(64))]
struct Left(AtomicUsize);
#[derive(Debug)]
pub(super) struct FeedState {
    batch: usize,
    left: Vec<Left>,
    ready: ArrayQueue<usize>,
    sleep: Mutex<()>,
    wake: Condvar,
}
impl FeedState {
    fn new(count: usize, batch: usize) -> Self {
        let n = count.div_ceil(batch);
        Self {
            batch,
            left: (0..n)
                .map(|b| Left(AtomicUsize::new((count - b * batch).min(batch))))
                .collect(),
            ready: ArrayQueue::new(n.max(1)),
            sleep: Mutex::new(()),
            wake: Condvar::new(),
        }
    }
    pub(super) fn publish(&self, id: usize) {
        let b = id / self.batch;
        if self.left[b].0.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.ready.push(b).expect("bounded frontier ready queue");
            let _guard = self.sleep.lock().expect("feed sleep lock");
            self.wake.notify_one();
        }
    }
    fn next(&self) -> Result<usize> {
        if let Some(b) = self.ready.pop() {
            return Ok(b);
        }
        let mut guard = self.sleep.lock().map_err(|e| e.to_string())?;
        loop {
            if let Some(b) = self.ready.pop() {
                return Ok(b);
            }
            let (g, t) = self
                .wake
                .wait_timeout(guard, Duration::from_secs(60))
                .map_err(|e| e.to_string())?;
            guard = g;
            if t.timed_out() {
                return Err("GPU feeder timed out waiting for completed batch".into());
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathMode {
    CpuOnly,
    SerialCompact,
    CompactOverlap,
    PinnedOverlap,
}

#[derive(Debug, Default)]
struct TransferStats {
    pack_ns: u64,
    wait_ns: u64,
    h2d_ms: f64,
    bytes: usize,
    copies: usize,
}
#[derive(Debug)]
struct Gpu {
    pinned: Arc<Pinned>,
    staging: Vec<Pinned>,
    device: *mut c_void,
    stream: *mut c_void,
    starts: [*mut c_void; 3],
    ends: [*mut c_void; 3],
    max_words: usize,
    max_batch: usize,
    slot_rows: usize,
}
// SAFETY: scoped feeder has exclusive &mut Gpu access; CUDA primary context is
// selected on that thread before use. Threads are joined before validation/drop.
unsafe impl Send for Gpu {}
impl Gpu {
    fn new(count: usize, batch: usize, slot_rows: usize) -> Result<Self> {
        unsafe {
            check(cudaSetDevice(0))?;
        }
        let max_words = count * slot_rows * 16;
        let pinned = Arc::new(Pinned::new(max_words)?);
        let staging = (0..3)
            .map(|_| Pinned::new(batch * slot_rows * 16))
            .collect::<Result<Vec<_>>>()?;
        let mut device = std::ptr::null_mut();
        let mut stream = std::ptr::null_mut();
        let mut starts = [std::ptr::null_mut(); 3];
        let mut ends = starts;
        unsafe {
            check(cudaMalloc(&mut device, max_words * 4))?;
            check(cudaStreamCreateWithFlags(&mut stream, 1))?;
            for i in 0..3 {
                check(cudaEventCreate(&mut starts[i]))?;
                check(cudaEventCreate(&mut ends[i]))?;
            }
        }
        Ok(Self {
            pinned,
            staging,
            device,
            stream,
            starts,
            ends,
            max_words,
            max_batch: batch,
            slot_rows,
        })
    }
    fn drain(&self, slot: usize, stats: &mut TransferStats) -> Result<()> {
        let mut ms = 0.0;
        unsafe {
            check(cudaEventSynchronize(self.ends[slot]))?;
            check(cudaEventElapsedTime(
                &mut ms,
                self.starts[slot],
                self.ends[slot],
            ))?;
        }
        stats.h2d_ms += f64::from(ms);
        Ok(())
    }
    fn transfer(
        &mut self,
        completion: &Completion,
        feed: &FeedState,
        mode: PathMode,
    ) -> Result<TransferStats> {
        let count = completion.len();
        let mut stats = TransferStats::default();
        let mut active = [false; 3];
        if mode == PathMode::SerialCompact {
            let mut g = completion.sleep.lock().map_err(|e| e.to_string())?;
            while completion.remaining.load(Ordering::Acquire) != 0 {
                g = completion.wake.wait(g).map_err(|e| e.to_string())?;
            }
        }
        for sequence in 0..count.div_ceil(feed.batch) {
            let t = Instant::now();
            let b = feed.next()?;
            stats.wait_ns += t.elapsed().as_nanos() as u64;
            let ring = sequence % 3;
            if active[ring] {
                self.drain(ring, &mut stats)?;
            }
            let first = b * feed.batch;
            let n = (count - first).min(feed.batch);
            let offset = first * self.slot_rows * 16;
            let (src, words) = if mode == PathMode::PinnedOverlap {
                // SAFETY: the release counter proves all writers in this batch finished.
                (
                    unsafe { self.pinned.ptr.as_ptr().add(offset) },
                    n * self.slot_rows * 16,
                )
            } else {
                let t = Instant::now();
                let header = n + 2;
                let mut cursor = header;
                let target = self.staging[ring].ptr.as_ptr();
                // Ring slot's prior DMA completed above; this thread owns staging writes.
                unsafe {
                    *target = n as i32;
                }
                for i in 0..n {
                    let job = completion.slots[first + i]
                        .0
                        .get()
                        .ok_or("unpublished observation")?;
                    if let Some(error) = &job.error {
                        return Err(error.clone());
                    }
                    let rows = job.output_rows();
                    let words = rows.len() * 16;
                    if cursor + words > self.staging[ring].words {
                        return Err("compact staging overflow".into());
                    }
                    unsafe {
                        *target.add(i + 1) = cursor as i32;
                        std::ptr::copy_nonoverlapping(
                            rows.as_ptr().cast::<i32>(),
                            target.add(cursor),
                            words,
                        );
                    }
                    cursor += words;
                }
                unsafe {
                    *target.add(n + 1) = cursor as i32;
                }
                stats.pack_ns += t.elapsed().as_nanos() as u64;
                (target, cursor)
            };
            if offset + words > self.max_words {
                return Err("device capacity exceeded".into());
            }
            // Host buffer is pinned and remains untouched until its completion event.
            unsafe {
                check(cudaEventRecord(self.starts[ring], self.stream))?;
                check(cudaMemcpyAsync(
                    self.device.cast::<i32>().add(offset).cast(),
                    src.cast(),
                    words * 4,
                    1,
                    self.stream,
                ))?;
                check(cudaEventRecord(self.ends[ring], self.stream))?;
            }
            active[ring] = true;
            stats.bytes += words * 4;
            stats.copies += 1;
        }
        for (slot, used) in active.into_iter().enumerate() {
            if used {
                self.drain(slot, &mut stats)?;
            }
        }
        Ok(stats)
    }
    fn validate(&self, completion: &Completion, batch: usize, mode: PathMode) -> Result<usize> {
        let mut readback = vec![0i32; completion.len() * self.slot_rows * 16];
        unsafe {
            check(cudaMemcpy(
                readback.as_mut_ptr().cast(),
                self.device,
                readback.len() * 4,
                2,
            ))?;
        }
        for b in 0..completion.len().div_ceil(batch) {
            let first = b * batch;
            let n = (completion.len() - first).min(batch);
            let base = first * self.slot_rows * 16;
            if mode != PathMode::PinnedOverlap && readback[base] != n as i32 {
                return Err("GPU compact batch header mismatch".into());
            }
            for i in 0..n {
                let job = completion.slots[first + i]
                    .0
                    .get()
                    .ok_or("missing validation slot")?;
                let rows = job.output_rows();
                let start = if mode == PathMode::PinnedOverlap {
                    base + i * self.slot_rows * 16
                } else {
                    base + readback[base + i + 1] as usize
                };
                let end = start + rows.len() * 16;
                let actual = readback.get(start..end).ok_or("GPU row range invalid")?;
                if !actual.iter().copied().eq(rows.iter().flatten().copied()) {
                    return Err(format!("GPU observation mismatch job {}", job.id));
                }
            }
        }
        Ok(completion.len())
    }
}
impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            cudaStreamSynchronize(self.stream);
            for i in 0..3 {
                cudaEventDestroy(self.starts[i]);
                cudaEventDestroy(self.ends[i]);
            }
            cudaStreamDestroy(self.stream);
            cudaFree(self.device);
        }
    }
}

fn run_pipeline(
    pool: &Workers,
    jobs: Vec<Box<Job>>,
    gpu: &mut Gpu,
    batch: usize,
    mode: PathMode,
) -> Result<(Duration, Arc<Completion>, TransferStats)> {
    if mode == PathMode::CpuOnly {
        let (t, c) = pool.run(jobs)?;
        return Ok((t, c, TransferStats::default()));
    }
    if batch > gpu.max_batch {
        return Err("batch exceeds allocation".into());
    }
    let count = jobs.len();
    if count > pool.shared.pending.capacity() {
        return Err("frontier exceeds action queue capacity".into());
    }
    let feed = Arc::new(FeedState::new(count, batch));
    let completion = Arc::new(Completion {
        slots: (0..count).map(|_| OutputSlot(OnceLock::new())).collect(),
        remaining: AtomicUsize::new(count),
        sleep: Mutex::new(()),
        wake: Condvar::new(),
        feed: Some(Arc::clone(&feed)),
    });
    let (ready_tx, ready_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel();
    thread::scope(|scope| {
        let feed_job = Arc::clone(&feed);
        let complete_job = Arc::clone(&completion);
        let handle = scope.spawn(move || -> Result<TransferStats> {
            let init = unsafe { check(cudaSetDevice(0)) };
            if let Err(error) = init {
                ready_tx
                    .send(Err(error.clone()))
                    .map_err(|e| e.to_string())?;
                return Err(error);
            }
            ready_tx.send(Ok(())).map_err(|e| e.to_string())?;
            go_rx.recv().map_err(|e| e.to_string())?;
            gpu.transfer(&complete_job, &feed_job, mode)
        });
        ready_rx.recv().map_err(|e| e.to_string())??;
        let start = Instant::now();
        go_tx.send(()).map_err(|e| e.to_string())?;
        {
            let _guard = pool.shared.closed.lock().map_err(|e| e.to_string())?;
            for job in jobs {
                pool.shared
                    .pending
                    .push(WorkItem {
                        job,
                        completion: Arc::clone(&completion),
                    })
                    .map_err(|_| "pending queue full")?;
            }
            pool.shared.wake.notify_all();
        }
        let stats = handle.join().map_err(|_| "GPU feeder panicked")??;
        {
            let mut g = completion.sleep.lock().map_err(|e| e.to_string())?;
            while completion.remaining.load(Ordering::Acquire) != 0 {
                g = completion.wake.wait(g).map_err(|e| e.to_string())?;
            }
        }
        let elapsed = start.elapsed();
        // Feeder transfer completion proves every published batch is on-device.
        for job in completion.iter() {
            if let Some(error) = &job.error {
                return Err(error.clone());
            }
        }
        Ok((elapsed, Arc::clone(&completion), stats))
    })
}

pub(crate) fn run(root: &Path) -> Result<()> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| e.to_string());
    let schema_path = Path::new("/public/maxim/Codex/spire-mentor-native-arrays-20261024/schema");
    let labels: Vec<Value> = serde_json::from_str(&read(&schema_path.join("registry.json"))?)
        .map_err(|e| e.to_string())?;
    let templates = serde_json::from_str(&read(&schema_path.join("metadata.json"))?)
        .map_err(|e| e.to_string())?;
    let schema = Arc::new(Schema::new(&labels, templates)?);
    let source = read(Path::new(
        "/public/maxim/Codex/spire-mentor-validation/late_act_8196_v1/fixtures.jsonl",
    ))?;
    let mut roots = Vec::new();
    for line in source.lines() {
        let f: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let s = sts2_wire::state_from_json(&f["wire"]).map_err(|e| e.to_string())?;
        let mut s = Session::from_parts(&schema, s, f["context"].clone(), 500)?;
        s.numeric_observation(&schema)?;
        roots.push(s);
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
        .map(|b| i32::from_le_bytes(b.try_into().expect("4 bytes")))
        .collect();
    let quick = std::env::var_os("FEED_SMOKE").is_some();
    let workloads = if quick {
        vec![(64, false)]
    } else {
        vec![(8192, true), (8196, false), (32784, false)]
    };
    let configs = [
        (PathMode::CpuOnly, 256),
        (PathMode::SerialCompact, 256),
        (PathMode::CompactOverlap, 64),
        (PathMode::CompactOverlap, 256),
        (PathMode::CompactOverlap, 1024),
        (PathMode::CompactOverlap, 2048),
        (PathMode::PinnedOverlap, 64),
        (PathMode::PinnedOverlap, 256),
        (PathMode::PinnedOverlap, 1024),
        (PathMode::PinnedOverlap, 2048),
    ];
    let pool = Workers::new(Arc::clone(&schema), 80, 65536);
    let mut results = Vec::new();
    for (count, fixed) in workloads {
        let mut gpu = Gpu::new(count, 2048, if fixed { 64 } else { 128 })?;
        for trial in 0..if quick { 2 } else { 6 } {
            // Rotate mode order to reduce systematic warmup/allocator/order bias.
            for shift in 0..configs.len() {
                let (mode, batch) = configs[(shift + trial) % configs.len()];
                let mut jobs =
                    make_jobs(&roots, &actions, &seeds, count, fixed, Mode::Direct, false);
                if mode == PathMode::PinnedOverlap {
                    for job in &mut jobs {
                        job.output = Vec::new();
                        job.pinned = Some(PinnedSlot {
                            arena: Arc::clone(&gpu.pinned),
                            offset: job.id * gpu.slot_rows * 16,
                            rows: gpu.slot_rows,
                        });
                    }
                }
                let (elapsed, complete, stats) = run_pipeline(&pool, jobs, &mut gpu, batch, mode)?;
                // Safety/lifetime guard: a completed wave is joined before any arena reuse.
                for job in complete.iter() {
                    let at = if fixed { 0 } else { job.id % roots.len() };
                    verify(job, at, &oracle, &reference["proof"][at])?;
                }
                let checked = if mode == PathMode::CpuOnly {
                    0
                } else {
                    gpu.validate(&complete, batch, mode)?
                };
                let row = json!({"mode":format!("{mode:?}"),"batch":batch,"workers":80,"feeder_workers":usize::from(mode!=PathMode::CpuOnly),
                    "jobs":count,"unique_starting_states":if fixed {1}else{roots.len().min(count)},"fixed":fixed,"trial":trial,"warmup":trial==0,"seconds":elapsed.as_secs_f64(),
                    "gpu_ready_per_second":count as f64/elapsed.as_secs_f64(),"cpu_pack_ms":stats.pack_ns as f64/1e6,
                    "feeder_wait_ms":stats.wait_ns as f64/1e6,"h2d_event_ms":stats.h2d_ms,"h2d_bytes":stats.bytes,"transfer_calls":stats.copies,
                    "useful_bytes":complete.iter().map(|j|j.output_rows().len()*64).sum::<usize>(),"gpu_observations_checked":checked,
                    "pinned_host_bytes":gpu.pinned.words*4+gpu.staging.iter().map(|x|x.words*4).sum::<usize>(),"device_bytes":gpu.max_words*4});
                println!("{row}");
                results.push(row);
                std::fs::write(
                    root.join(if quick {
                        "smoke-results.json"
                    } else {
                        "results.json"
                    }),
                    serde_json::to_vec_pretty(&results).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_batch_publishes_once_and_only_after_all_writes() {
        let f = FeedState::new(5, 2);
        f.publish(0);
        assert!(f.ready.is_empty());
        f.publish(4);
        assert_eq!(f.ready.pop(), Some(2));
        f.publish(1);
        assert_eq!(f.ready.pop(), Some(0));
        f.publish(3);
        assert!(f.ready.is_empty());
        f.publish(2);
        assert_eq!(f.ready.pop(), Some(1));
        assert!(f.ready.is_empty());
    }
}
