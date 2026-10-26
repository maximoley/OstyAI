//! Multi-seed act-map digest gate for `map_path_pruning` perf work (GH #468).
//!
//! `generate_act_map` must stay byte-identical (points, types, edges,
//! x-offsets, and RNG draw count) across any optimization of the pruning
//! pass, because prune order feeds `map_rng` consumption and thus every
//! downstream roll of the run. This test hashes the full generated map +
//! `rng.counter()` for a few thousand seeds across several
//! (act, ascension, elite-replace) configurations and compares against a
//! golden file recorded from the pre-optimization build (`7981d198b`).
//!
//! Regenerate (ONLY when the map generator intentionally changes behavior):
//!
//! ```text
//! MAP_GEN_DIGEST_RECORD=1 cargo nextest run --release --profile slow \
//!     -E 'test(slow_map_gen_digest_matches_golden)' -p sts2-core
//! ```

use sts2_core::{generate_act_map, DotNetRng as Rng};

const GOLDEN_REL_PATH: &str = "../../tests/parity/map_gen_digests_v1.txt";

/// (label, seed count, ascension, act_index, replace_treasure_with_elite)
const CONFIGS: &[(&str, i32, i32, i32, bool)] = &[
    ("act0", 1500, 0, 0, false),
    ("act1", 250, 0, 1, false),
    ("act2", 250, 0, 2, false),
    ("asc1", 250, 1, 0, false),
    ("elite_replace", 250, 0, 0, true),
];

/// Extra adversarial seeds appended to every config's `0..n` range.
const EXTRA_SEEDS: &[i32] = &[-1, -2026, i32::MIN, i32::MAX];

fn fnv1a_update(hash: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *hash ^= u64::from(b);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
}

/// Hash every observable field of the generated map, in deterministic
/// (grid-iteration) order, plus the number of RNG draws consumed.
fn map_digest(seed: i32, ascension: i32, act_index: i32, replace_treasure: bool) -> u64 {
    let mut rng = Rng::new(seed);
    let map = generate_act_map(15, &mut rng, ascension, replace_treasure, act_index, None);

    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    fnv1a_update(&mut h, &map.map_length.to_le_bytes());
    for (coord, &idx) in &map.grid {
        let p = map.point(idx);
        fnv1a_update(&mut h, &coord.col.to_le_bytes());
        fnv1a_update(&mut h, &coord.row.to_le_bytes());
        fnv1a_update(&mut h, &[p.point_type as u8]);
        fnv1a_update(&mut h, &p.x_offset.to_bits().to_le_bytes());
        // Children in stored order — edge order is observable via
        // map-choice surfaces, so it must be identical too.
        for &c in &p.children {
            let cc = map.point(c).coord;
            fnv1a_update(&mut h, &cc.col.to_le_bytes());
            fnv1a_update(&mut h, &cc.row.to_le_bytes());
        }
        fnv1a_update(&mut h, b"|");
    }
    for meta in [map.start, map.boss, map.second_boss] {
        match meta {
            Some(idx) => {
                let c = map.point(idx).coord;
                fnv1a_update(&mut h, &c.col.to_le_bytes());
                fnv1a_update(&mut h, &c.row.to_le_bytes());
            }
            None => fnv1a_update(&mut h, b"-"),
        }
    }
    fnv1a_update(&mut h, &rng.counter().to_le_bytes());
    h
}

fn golden_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN_REL_PATH)
}

fn compute_all() -> Vec<String> {
    let mut lines = Vec::new();
    for &(label, n, asc, act, replace) in CONFIGS {
        let seeds = (0..n).chain(EXTRA_SEEDS.iter().copied());
        for seed in seeds {
            let d = map_digest(seed, asc, act, replace);
            lines.push(format!("{label}:{seed}:{d:016x}"));
        }
    }
    lines
}

#[test]
fn slow_map_gen_digest_matches_golden() {
    let path = golden_path();
    let lines = compute_all();
    if std::env::var_os("MAP_GEN_DIGEST_RECORD").is_some() {
        std::fs::write(&path, lines.join("\n") + "\n").expect("write golden");
        eprintln!("recorded {} digests to {}", lines.len(), path.display());
        return;
    }
    let golden = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "golden {} missing — record it from a known-good build with \
             MAP_GEN_DIGEST_RECORD=1 (see module docs): {e}",
            path.display()
        )
    });
    let golden_lines: Vec<&str> = golden.lines().collect();
    assert_eq!(
        golden_lines.len(),
        lines.len(),
        "digest count mismatch vs golden — CONFIGS changed without re-recording?"
    );
    let mut mismatches = 0;
    for (got, want) in lines.iter().zip(&golden_lines) {
        if got != want {
            if mismatches < 20 {
                eprintln!("MISMATCH: got {got} want {want}");
            }
            mismatches += 1;
        }
    }
    assert_eq!(
        mismatches, 0,
        "{mismatches} map digests diverged from the pre-optimization golden \
         — pruning behavior changed (prune order feeds map_rng consumption)"
    );
}

/// Fast smoke slice of the golden (first 25 act0 seeds) so the default
/// nextest profile still guards the digest contract on every run.
#[test]
fn map_gen_digest_smoke_prefix() {
    let path = golden_path();
    let Ok(golden) = std::fs::read_to_string(&path) else {
        // Golden not recorded yet (fresh checkout mid-recording) — the
        // slow_ test owns the missing-file diagnostics.
        return;
    };
    for line in golden.lines().take(25) {
        let mut parts = line.split(':');
        let label = parts.next().expect("label");
        assert_eq!(label, "act0");
        let seed: i32 = parts.next().expect("seed").parse().expect("seed int");
        let want = parts.next().expect("digest");
        let got = format!("{:016x}", map_digest(seed, 0, 0, false));
        assert_eq!(got, want, "digest mismatch for act0 seed {seed}");
    }
}
