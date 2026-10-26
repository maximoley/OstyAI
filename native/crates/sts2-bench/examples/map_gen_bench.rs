//! Act-map construction throughput probe (GH #468).
//!
//! `RunManager::new` was measured at ~45 ms/run at `7981d198b`, ~99.6% of
//! it inside `map_path_pruning::find_matching_segments`. This harness
//! times `generate_act_map` alone so the pruning optimization can be
//! A/B'd without the full xsim harness:
//!
//! ```text
//! cargo run --release -p sts2-bench --example map_gen_bench -- [seeds]
//! ```

use std::time::Instant;
use sts2_core::{generate_act_map, DotNetRng as Rng};

fn main() {
    let seeds: i32 = std::env::args()
        .nth(1)
        .map_or(200, |s| s.parse().expect("seed count"));

    // Warm-up + checksum so the work can't be optimized away.
    let mut checksum: u64 = 0;
    let start = Instant::now();
    for seed in 0..seeds {
        let mut rng = Rng::new(seed);
        let map = generate_act_map(15, &mut rng, 0, false, 0, None);
        checksum = checksum
            .wrapping_mul(31)
            .wrapping_add(map.grid.len() as u64)
            .wrapping_add(u64::from(rng.counter()));
    }
    let elapsed = start.elapsed();

    let per_map = elapsed / u32::try_from(seeds).expect("seeds fits u32");
    println!(
        "{seeds} maps in {:.3}s — {:.1} maps/s, {:.3} ms/map (checksum {checksum:x})",
        elapsed.as_secs_f64(),
        f64::from(seeds) / elapsed.as_secs_f64(),
        per_map.as_secs_f64() * 1e3,
    );
}
