//! Flamegraph one `fight_delta` call (plan §5/M2 labeling-cost
//! breakdown): is the probe wall-clock dominated by MCTS state
//! cloning / allocation (copy-on-write / undo-stack / arena work would
//! pay off) or by the transition function itself (it wouldn't)?
//!
//! Uses the in-process `pprof` sampler because the box runs with
//! `kernel.perf_event_paranoid=4` (unprivileged `perf` disabled).
//!
//! ```sh
//! cargo build -p sts2-runai --profile bench --example profile_fight_delta
//! ./target/release/examples/profile_fight_delta
//! ```
//!
//! Writes `fight_delta_flamegraph.svg` + `fight_delta_folded.txt`
//! (standard collapsed-stack format) into the current directory.

// The `pprof` sampler is Unix-only (ucontext_t / pthread / POSIX pipes), so
// the profiling body compiles only on Unix. Off Unix this is a no-op stub,
// which lets the workspace build/test on Windows.
#[cfg(not(unix))]
fn main() {
    eprintln!(
        "profile_fight_delta relies on the Unix-only `pprof` sampler; \
         not supported on this platform."
    );
}

#[cfg(unix)]
fn main() {
    use std::fs::File;
    use std::io::Write as _;
    use std::time::{Duration, Instant};

    use sts2_content::run_callbacks::new_run_manager_for_character;
    use sts2_core::cards::CardId;
    use sts2_core::potions::CharacterId;
    use sts2_runai::rollout::{
        ensure_content_installed, fight_delta, CandidateMutation, EncounterPlan, FightCache,
        ProbeBudget,
    };

    ensure_content_installed();

    // The labeler's canonical shape: starter Ironclad at a card-shaped
    // decision, default probe budget (100 sims, K=16 paired seeds,
    // T=5 turn truncation), elite + boss encounters.
    let rm = new_run_manager_for_character(17, CharacterId::Ironclad);
    let plan = EncounterPlan::for_decision(&rm, 99);
    assert!(plan.elite.is_some() && plan.boss.is_some(), "act-1 plan");
    let budget = ProbeBudget::default();
    let cand = CandidateMutation::AddCard {
        card_id: CardId::Inflame,
        upgraded: false,
    };

    // Warm-up outside the profile (registries, allocator, page-in).
    let t0 = Instant::now();
    let warm = fight_delta(&rm, &cand, &plan, &budget, &FightCache::default());
    eprintln!("warm-up: {:?}/call, delta={warm:?}", t0.elapsed());

    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(997)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .expect("profiler guard");

    // Fresh cache every call so all 32 probes (2 encounters x K=16)
    // run live each iteration; loop until ~15 s of samples.
    let t0 = Instant::now();
    let mut iters = 0u32;
    while t0.elapsed() < Duration::from_secs(15) {
        let d = fight_delta(&rm, &cand, &plan, &budget, &FightCache::default());
        std::hint::black_box(d);
        iters += 1;
    }
    let wall = t0.elapsed();
    eprintln!(
        "{iters} fight_delta calls in {wall:.2?} ({:?}/call)",
        wall / iters.max(1)
    );

    let report = guard.report().build().expect("report");
    let mut svg = File::create("fight_delta_flamegraph.svg").expect("svg file");
    report.flamegraph(&mut svg).expect("flamegraph render");

    // Collapsed stacks (root;...;leaf count) for top-N self-time work.
    let mut folded = File::create("fight_delta_folded.txt").expect("folded file");
    for (frames, count) in &report.data {
        let mut line = String::new();
        for frame in frames.frames.iter().rev() {
            for symbol in frame.iter().rev() {
                if !line.is_empty() {
                    line.push(';');
                }
                line.push_str(&symbol.to_string());
            }
        }
        writeln!(folded, "{line} {count}").expect("write folded");
    }
    eprintln!("wrote fight_delta_flamegraph.svg + fight_delta_folded.txt");
}
