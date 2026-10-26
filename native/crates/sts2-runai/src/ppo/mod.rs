//! PPO `RunPolicyNet` Rust inference (2026-07-07 ppo-rust-inference plan).
//!
//! Porting authority: `sts2-pycli/python/ppo/{vocab,mapfeat,env,encoder,
//! model,net_policy}.py` — those files are never modified by this port;
//! drift is caught by the committed parity fixtures under
//! `tests/fixtures/`. Modules: the tract-onnx runtime wrapper ([`net`]),
//! the categorical-id registries ([`vocab`]), the observation data model
//! ([`obs`]), the map-feature port ([`mapfeat`]), the featurizer
//! ([`encoder`]), and the deployed policy ([`policy`]).

pub mod encoder;
pub mod features;
pub mod mapfeat;
#[cfg(feature = "ppo")]
pub mod net;
pub mod obs;
pub mod policy;
/// Re-export of the vocab crate: the registries moved to `sts2-vocab` so
/// `sts2-wire` (and the wasm workbench) can name them without this crate's
/// tract-onnx / rayon graph. Every `ppo::vocab::*` path keeps working.
pub mod vocab {
    pub use sts2_vocab::*;
}
pub mod wire;

use std::path::{Path, PathBuf};

/// Resolve a committed test fixture under `tests/fixtures/<name>`,
/// relative to this crate's manifest dir. Mirrors the skip-if-absent
/// idiom in `tests/run_policy_net_parity.rs`: callers should check
/// `.exists()` (or just try to read it) and skip gracefully rather than
/// fail when a fixture hasn't been generated/committed yet.
#[must_use]
pub fn test_fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Every committed fixture whose file name starts with `prefix`, sorted.
///
/// The parity fixtures come in variants — the default one pins the encoder's
/// default schema, and `ppo.dump_parity_fixtures` writes one more per gated
/// `STS2_PPO_FEATURES` set (`ppo_encoder_parity_reward_upgrade.jsonl`), each
/// row recording the schema it was dumped at. A gate that reads only the
/// default path would silently stop covering the gated arm.
#[must_use]
pub fn test_fixture_paths(prefix: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "jsonl")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(prefix))
        })
        .collect();
    out.sort();
    out
}
