//! Regenerate the `maps` + `golden_path` sections of
//! `tests/parity/map_generator_snapshot.json` from current Rust map-gen
//! output.
//!
//! Bead context (2026-05-28 audit cleanup): the snapshot at
//! `tests/parity/map_generator_snapshot.json` was originally produced by
//! `tests/parity/map_generator_snapshot.py` which calls into the Python sim
//! at `sts2-rl-agent@01c094d`. Bead `cde5196` (StandardActMap RNG-order
//! fix: `_pointTypeCounts` rolled BEFORE `PathGenerate`) brought the Rust
//! map-generator to C#-faithful behaviour; the Python sim still carries the
//! old order, so the Python-anchored snapshot diverges from both Rust and
//! the actual C# game. Per CLAUDE.md the C# is the source of truth.
//!
//! This dumper re-anchors the `maps` + `golden_path` sections to the
//! C#-faithful Rust output, keeping the `rng_sequences` + `hash_codes`
//! sections (which match across all three) untouched.
//!
//! Run:
//!   cargo run --example regen_map_snapshot -p sts2-core \
//!       > tests/parity/map_generator_snapshot.json.new
//! Then diff + replace.

use sts2_core::{
    generate_act_map, generate_golden_path_map, DotNetRng as Rng, MapCoord, MapPointType,
};

fn point_type_name(t: MapPointType) -> &'static str {
    match t {
        MapPointType::Unassigned => "UNASSIGNED",
        MapPointType::Unknown => "UNKNOWN",
        MapPointType::Monster => "MONSTER",
        MapPointType::Elite => "ELITE",
        MapPointType::Boss => "BOSS",
        MapPointType::Shop => "SHOP",
        MapPointType::RestSite => "REST_SITE",
        MapPointType::Treasure => "TREASURE",
        MapPointType::Ancient => "ANCIENT",
    }
}

fn dump_map(seed: i32) -> serde_json::Value {
    let mut rng = Rng::new(seed);
    let map = generate_act_map(15, &mut rng, 0, false, 0, None);

    // Sort points by (col, row) so the dump is canonical and stable.
    let mut points: Vec<(MapCoord, _)> = map
        .grid
        .iter()
        .map(|(coord, &idx)| (*coord, map.points[idx as usize].point_type))
        .collect();
    points.sort_by_key(|a| (a.0.col, a.0.row));

    let point_json: Vec<serde_json::Value> = points
        .iter()
        .map(|(coord, pt)| {
            serde_json::json!({
                "col": coord.col,
                "row": coord.row,
                "type": point_type_name(*pt),
            })
        })
        .collect();

    // Edges = (col, row) → [(child_col, child_row)] flattened, sorted.
    let mut edges: Vec<(i32, i32, i32, i32)> = Vec::new();
    for (coord, &idx) in &map.grid {
        for &child_idx in &map.points[idx as usize].children {
            let child = &map.points[child_idx as usize];
            edges.push((coord.col, coord.row, child.coord.col, child.coord.row));
        }
    }
    // Also include start → row-1 children + row-(N-1) → boss children,
    // matching what the existing snapshot's edges array carried.
    if let Some(start_idx) = map.start {
        let start = &map.points[start_idx as usize];
        for &child_idx in &start.children {
            let child = &map.points[child_idx as usize];
            edges.push((
                start.coord.col,
                start.coord.row,
                child.coord.col,
                child.coord.row,
            ));
        }
    }
    edges.sort_unstable();
    edges.dedup();

    let edge_json: Vec<serde_json::Value> = edges
        .iter()
        .map(|(c1, r1, c2, r2)| serde_json::json!([c1, r1, c2, r2]))
        .collect();

    serde_json::json!({
        "seed": seed,
        "map_length": map.map_length,
        "point_count": point_json.len(),
        "points": point_json,
        "edges": edge_json,
    })
}

fn dump_golden_path() -> serde_json::Value {
    let map = generate_golden_path_map(1);
    let mut points: Vec<(MapCoord, _)> = map
        .grid
        .iter()
        .map(|(coord, &idx)| (*coord, map.points[idx as usize].point_type))
        .collect();
    points.sort_by_key(|a| (a.0.col, a.0.row));
    let point_json: Vec<serde_json::Value> = points
        .iter()
        .map(|(coord, pt)| {
            serde_json::json!({
                "col": coord.col,
                "row": coord.row,
                "type": point_type_name(*pt),
            })
        })
        .collect();
    serde_json::json!({
        "map_length": map.map_length,
        "points": point_json,
    })
}

fn main() {
    // Preserve the existing rng_sequences + hash_codes sections, only
    // replace maps + golden_path with C#-faithful Rust output.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/parity/map_generator_snapshot.json");
    let existing: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read existing snapshot"))
            .expect("parse existing snapshot");

    let maps: Vec<serde_json::Value> = [0_i32, 42, 2026].iter().map(|&s| dump_map(s)).collect();
    let golden = dump_golden_path();

    let mut out = existing;
    out["maps"] = serde_json::Value::Array(maps);
    out["golden_path"] = golden;

    let pretty = serde_json::to_string_pretty(&out).expect("serialize");
    println!("{pretty}");
}
