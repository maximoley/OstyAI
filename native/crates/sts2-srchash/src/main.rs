//! `sts2-srchash <rs_root>` — print the aggregate engine source hash for an
//! sts2-rs checkout, computed from SOURCE alone (no built wheel needed).
//!
//! Issue #15 / bead `sts2-rs-cg5ci`, Phase 2. The parity harness's
//! `wheel-venv.sh` content-addresses each worktree's venv as `.venv-h/<H>`,
//! where `<H>` is this hash. Because it is computed before any `.so` exists, it
//! cannot come from the wheel's `recompute_src_hash` (that needs the extension
//! loaded) — hence this tiny CLI over the very same [`sts2_srchash`] hasher, so
//! the venv name can never drift from the `src_hash()` the wheel bakes in.
//!
//! Prints the 16-hex hash and exits 0; on a missing/invalid `rs_root` prints a
//! usage line to stderr and exits 2.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(rs_root) = std::env::args().nth(1) else {
        eprintln!(
            "usage: sts2-srchash <rs_root>\n\n\
             Prints the 16-hex aggregate engine source hash of the sts2-rs\n\
             checkout at <rs_root> (the same hash sts2_rs.src_hash() bakes in)."
        );
        return ExitCode::from(2);
    };
    let root = Path::new(&rs_root);
    if !root.join("crates").is_dir() {
        eprintln!("sts2-srchash: not an sts2-rs checkout (no crates/ under {rs_root})");
        return ExitCode::from(2);
    }
    println!("{}", sts2_srchash::recompute_engine_hash(root));
    ExitCode::SUCCESS
}
