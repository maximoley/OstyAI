//! Build script — adds an rpath to libpython for non-extension-module
//! builds (e.g. `cargo test`).
//!
//! When `extension-module` is active (maturin builds), the cdylib is
//! loaded by an already-running Python process and libpython is
//! provided by that process — no link or rpath needed.
//!
//! When tests run via `cargo test`, the integration-test binary uses
//! pyo3's `auto-initialize` feature, which links against libpython at
//! load time. Without an rpath the binary can't find libpython unless
//! `LD_LIBRARY_PATH` is set externally. We discover the Python install
//! prefix via `pyo3-build-config` (the same crate pyo3 uses) and emit
//! the corresponding rpath.
//!
//! Bead `sts2-rs-92sk` (hint-T1).

fn main() {
    if std::env::var_os("CARGO_FEATURE_NATIVE_GPU_FEEDER").is_some() {
        println!("cargo:rerun-if-env-changed=CUDA_LIB_DIR");
        let cuda_lib = std::env::var("CUDA_LIB_DIR")
            .unwrap_or_else(|_| "/usr/local/cuda/lib64".to_string());
        println!("cargo:rustc-link-search=native={cuda_lib}");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{cuda_lib}");
    }
    emit_git_build_stamp();
    emit_src_hash_stamp();

    // Only emit rpath when NOT building an extension module (i.e. when
    // libpython will be linked dynamically). Cargo passes the feature
    // via `CARGO_FEATURE_EXTENSION_MODULE` when active.
    if std::env::var_os("CARGO_FEATURE_EXTENSION_MODULE").is_some() {
        // maturin path — libpython is already loaded by the host.
        return;
    }

    if let Ok(prefix) = std::env::var("PYO3_PYTHON_PREFIX") {
        emit_rpath(&prefix);
        return;
    }

    // Ask `python3-config` for the install prefix. This matches the
    // interpreter pyo3 will link against (driven by `PYO3_PYTHON` or
    // the `python3` on `PATH`).
    if let Ok(output) = std::process::Command::new("python3-config")
        .arg("--prefix")
        .output()
    {
        if output.status.success() {
            let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !prefix.is_empty() {
                emit_rpath(&prefix);
            }
        }
    }
}

/// Stamp the git commit this extension is built from into `STS2_BUILD_GIT`,
/// so the parity harness (`assert_wheel_fresh`) can reject a wheel built
/// from a stale checkout — the failure mode the mtime + content probes
/// cannot see (a `.so` with a fresh mtime and a valid `rng_counters` probe
/// that nonetheless lags `HEAD`'s features; a worktree agent installing
/// into the shared venv is the usual culprit). Degrades to `"unknown"` when
/// git is unavailable (CI / release-tarball builds), which the harness
/// treats as skip-with-warning, never a hard abort. Bead `sts2-rs-ybtmu`
/// follow-up.
fn emit_git_build_stamp() {
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=STS2_BUILD_GIT={sha}");

    // Re-run this build script when HEAD moves (commit / checkout) so the
    // stamp tracks the current commit. `logs/HEAD` is appended on every
    // HEAD movement, making it a reliable rerun trigger across branches.
    for p in ["HEAD", "logs/HEAD"] {
        if let Ok(out) = std::process::Command::new("git")
            .args(["rev-parse", "--git-path", p])
            .output()
        {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() {
                    println!("cargo:rerun-if-changed={path}");
                }
            }
        }
    }
}

/// Stamp this crate's source-content hash (`STS2_PY_SRC_HASH`, INCLUDING the
/// `include_bytes!` value/policy-net weights that live outside `src`) and the
/// workspace inputs (`STS2_WORKSPACE_HASH` = the workspace Cargo.toml +
/// Cargo.lock + rust-toolchain.toml) for the content-addressed engine-identity
/// check (issue #15, bead
/// `sts2-rs-cg5ci`). Unlike sts2-core/content/mcts, this build script already
/// opts out of cargo's default file tracking (it emits git `rerun-if-changed`),
/// so it must name its own inputs explicitly to keep the stamp fresh.
fn emit_src_hash_stamp() {
    let crate_dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo"),
    );
    // <root>/crates/sts2-py -> <root>
    let rs_root = crate_dir
        .parent()
        .and_then(std::path::Path::parent)
        .map_or_else(|| crate_dir.clone(), std::path::Path::to_path_buf);
    // `PY_DATA_INPUTS` is empty since the bundled policy prior was deleted
    // (2026-08-15); the mapping is kept so a future `include_bytes!` asset is
    // hashed into the stamp automatically rather than by remembering to.
    let py_extra: Vec<(&str, std::path::PathBuf)> = sts2_srchash::PY_DATA_INPUTS
        .iter()
        .map(|(label, rel)| (*label, rs_root.join(rel)))
        .collect();

    let py_hash = sts2_srchash::hash_crate(&crate_dir, &py_extra);
    let ws_hash = sts2_srchash::hash_workspace(&rs_root);
    println!("cargo:rustc-env=STS2_PY_SRC_HASH={py_hash}");
    println!("cargo:rustc-env=STS2_WORKSPACE_HASH={ws_hash}");

    // Keep the stamp fresh: re-run when any hashed input changes. Cargo scans a
    // directory path recursively, so `src` covers every source file.
    let mut watched = vec![
        crate_dir.join("src"),
        crate_dir.join("Cargo.toml"),
        rs_root.join("Cargo.toml"),
        rs_root.join("Cargo.lock"),
        rs_root.join("rust-toolchain.toml"),
    ];
    watched.extend(py_extra.into_iter().map(|(_, path)| path));
    for p in watched {
        println!("cargo:rerun-if-changed={}", p.display());
    }
}

fn emit_rpath(prefix: &str) {
    let lib_dir = format!("{prefix}/lib");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{lib_dir}");
}
