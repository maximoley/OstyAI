//! Reproducible run-trajectory recording.
//!
//! [`RecordingPolicy`] wraps the run policy and captures every decision;
//! [`RecordingPolicy::into_run_file`] projects a native-`.run`-schema
//! [`RunFile`] carrying our `sts2sim` extension (repro metadata + the
//! decision stream + combat detail). [`write_run_file`] gzips it to
//! `<dir>/<seed>.run.gz` — parseable by `run_files.load_run` and the
//! spire-codex / sts2replays corpus tooling.

pub(crate) mod recorder;
pub mod schema;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub use recorder::{Detail, RecordingPolicy, RECORDER_VERSION};
pub use schema::{Meta, RunFile};

use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};

/// Env var naming the directory to write per-seed `.run.gz` files into.
pub const RECORD_DIR_ENV: &str = "STS2_RUNAI_RECORD_DIR";
/// Env var naming the record detail level (`summary`/`decisions`/`full`).
pub const RECORD_DETAIL_ENV: &str = "STS2_RUNAI_RECORD_DETAIL";
/// Env var carrying the engine git sha (the sweep stamps it once).
pub const ENGINE_SHA_ENV: &str = "STS2_RUNAI_ENGINE_SHA";

/// The record configuration in effect, read from the environment.
/// `None` when recording is off.
#[must_use]
pub fn record_config() -> Option<(PathBuf, Detail)> {
    let dir = std::env::var(RECORD_DIR_ENV).ok()?;
    if dir.is_empty() {
        return None;
    }
    let detail = std::env::var(RECORD_DETAIL_ENV)
        .ok()
        .and_then(|s| Detail::parse(&s).ok())
        .unwrap_or(Detail::Full);
    Some((PathBuf::from(dir), detail))
}

/// Gzip-serialize `run` to `<dir>/<seed>.run.gz`. Creates `dir`.
pub fn write_run_file(dir: &Path, seed: u64, run: &RunFile) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{seed}.run.gz"));
    let json = serde_json::to_vec(run)?;
    let file = std::fs::File::create(&path)?;
    let mut enc = GzEncoder::new(file, Compression::default());
    enc.write_all(&json)?;
    enc.finish()?;
    Ok(path)
}

/// SHA-256 of a file's contents (hex), or `None` on any read error.
#[must_use]
pub fn file_sha256(path: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Some(format!("{:x}", hasher.finalize()))
}

/// Collect the reproducibility-relevant environment (`STS2_RUNAI_*` /
/// `STS2_PPO_*`) plus a `build_id` marker, into a stable map.
#[must_use]
pub fn repro_env(build_id: &str) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = std::env::vars()
        .filter(|(k, _)| {
            (k.starts_with("STS2_RUNAI_") || k.starts_with("STS2_PPO_"))
                // The record dir/detail are harness knobs, not run inputs.
                && k != RECORD_DIR_ENV
                && k != RECORD_DETAIL_ENV
        })
        .collect();
    env.insert("build_id".to_string(), build_id.to_string());
    env
}
