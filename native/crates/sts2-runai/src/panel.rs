//! Seed panels — fixed, version-pinned benchmark seed sets (plan §3.1).
//!
//! Bead `sts2-rs-40ikw.4` (M0). Three committed panels live at
//! `data/seed-panels/{dev-200,main-500,release-2000}.json`:
//!
//! | panel | seeds | use |
//! |---|---|---|
//! | `dev-200` | 200 | every iteration loop |
//! | `main-500` | 500 | A/B decisions, milestone gates |
//! | `release-2000` | 2000 | published numbers (held out — never tuned on) |
//!
//! The three panels are **disjoint** by construction: one
//! deterministically-seeded stream ([`PANEL_GEN_SEED`]) produces 2700
//! distinct seeds; the first 200 are `dev-200`, the next 500 `main-500`,
//! the last 2000 `release-2000`. Re-running the generator always
//! reproduces the committed files byte-for-byte.
//!
//! Rows are `{seed, ascension, character}`. The committed files pin
//! `character = "ironclad"` (program scope, plan §0) and `ascension = 0`;
//! sweeps may override ascension via the `run_arena --ascension` flag so
//! ascension comparisons stay **paired by seed**.

use std::collections::HashSet;
use std::path::Path;

use rand::Rng;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
use serde::{Deserialize, Serialize};

/// The fixed RNG constant the panel generator is seeded with. ASCII
/// `"STS2PANL"` as a big-endian u64 — chosen once for M0 and frozen;
/// changing it regenerates every panel and invalidates all published
/// numbers, so don't.
pub const PANEL_GEN_SEED: u64 = u64::from_be_bytes(*b"STS2PANL");

/// Panel sizes, in stream order: (`name`, `len`).
pub const PANEL_SPECS: [(&str, usize); 3] =
    [("dev-200", 200), ("main-500", 500), ("release-2000", 2000)];

/// One panel row. `seed` is the engine's unsigned 64-bit beta run seed
/// (`new_run_manager_for_character` input); `character` is the lowercase
/// token (`"ironclad"`, …) understood by [`crate::arena::parse_character`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelRow {
    pub seed: u64,
    pub ascension: u8,
    pub character: String,
}

/// A named seed panel (the JSON file's in-memory form).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Panel {
    /// Panel name (`"dev-200"`, …).
    pub name: String,
    /// Generator constant the rows were drawn from, recorded for
    /// provenance (must equal [`PANEL_GEN_SEED`] for the committed files).
    pub gen_seed: u64,
    pub rows: Vec<PanelRow>,
}

impl Panel {
    /// Parse a panel from its JSON text.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// Serialize to the canonical committed JSON form (pretty, trailing
    /// newline) — byte-stable across re-runs.
    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).expect("panel serializes");
        s.push('\n');
        s
    }

    /// Load a panel JSON file from disk.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::from_json(&text).map_err(std::io::Error::other)
    }
}

/// Generate the three canonical panels (`dev-200`, `main-500`,
/// `release-2000`) from [`PANEL_GEN_SEED`]. Deterministic and disjoint:
/// one Xoshiro256++ stream yields the historical 2700 distinct non-negative
/// 31-bit panel values (rejection-sampled on duplicates), stored as beta u64.
/// Keep these published panel identities unchanged; callers may load any u64.
#[must_use]
pub fn generate_panels() -> Vec<Panel> {
    let total: usize = PANEL_SPECS.iter().map(|(_, n)| n).sum();
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(PANEL_GEN_SEED);
    let mut seen: HashSet<u64> = HashSet::with_capacity(total);
    let mut seeds: Vec<u64> = Vec::with_capacity(total);
    while seeds.len() < total {
        // Preserve the published panel, rather than redraw its identities.
        // This is a bounded sample, not a narrowing of an incoming master seed.
        let s = u64::try_from(rng.gen_range(0..=i32::MAX)).expect("panel sample is non-negative");
        if seen.insert(s) {
            seeds.push(s);
        }
    }

    let mut panels = Vec::with_capacity(PANEL_SPECS.len());
    let mut cursor = 0usize;
    for (name, len) in PANEL_SPECS {
        let rows = seeds[cursor..cursor + len]
            .iter()
            .map(|&seed| PanelRow {
                seed,
                ascension: 0,
                character: "ironclad".to_string(),
            })
            .collect();
        cursor += len;
        panels.push(Panel {
            name: name.to_string(),
            gen_seed: PANEL_GEN_SEED,
            rows,
        });
    }
    panels
}

/// Write the three canonical panel files into `dir`
/// (`<dir>/<name>.json`), creating it if needed. Returns the written
/// paths.
pub fn write_panels(dir: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut paths = Vec::new();
    for panel in generate_panels() {
        let path = dir.join(format!("{}.json", panel.name));
        std::fs::write(&path, panel.to_json())?;
        paths.push(path);
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same constant → same panels, run-to-run (the byte-identical
    /// acceptance criterion for committed panel files).
    #[test]
    fn beta_panel_json_preserves_full_unsigned_seed() {
        let row = PanelRow {
            seed: u64::MAX,
            ascension: 10,
            character: "necrobinder".to_owned(),
        };
        let encoded = serde_json::to_string(&row).unwrap();
        let decoded: PanelRow = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, row);
        assert!(encoded.contains("18446744073709551615"));
    }

    #[test]
    fn panel_generation_is_deterministic() {
        let a = generate_panels();
        let b = generate_panels();
        assert_eq!(a, b);
        for (panel, (name, len)) in a.iter().zip(PANEL_SPECS) {
            assert_eq!(panel.name, name);
            assert_eq!(panel.rows.len(), len);
            assert_eq!(panel.gen_seed, PANEL_GEN_SEED);
        }
    }

    /// The three panels share no seed (release-2000 is held out; overlap
    /// would leak tuning decisions into published numbers).
    #[test]
    fn panels_are_disjoint() {
        let panels = generate_panels();
        let mut seen = HashSet::new();
        for panel in &panels {
            for row in &panel.rows {
                assert!(
                    seen.insert(row.seed),
                    "seed {} appears in more than one panel",
                    row.seed
                );
                assert_eq!(row.character, "ironclad");
                assert_eq!(row.ascension, 0);
            }
        }
        assert_eq!(seen.len(), 2700);
    }

    /// JSON round-trip preserves the panel exactly.
    #[test]
    fn panel_json_round_trip() {
        let panels = generate_panels();
        let text = panels[0].to_json();
        let back = Panel::from_json(&text).expect("parses");
        assert_eq!(back, panels[0]);
    }
}
