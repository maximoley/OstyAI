//! The extension's global allocator: mimalloc (A1), told never to purge (A1b).
//!
//! Design-doc items A1/A1b in `docs/perf/2026-10-13-engine-speed-exact-design.md` §7.

use std::ffi::{c_int, c_long};
use std::sync::Once;

// Rust allocations inside the extension go through mimalloc, as in the
// `sts2-cli` binaries (`sts2-cli/src/main.rs`).
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// `mi_option_purge_delay`: index 15 of mimalloc.h's `mi_option_t`.
///
/// libmimalloc-sys 0.1.47 compiles its vendored mimalloc v3.3.1
/// (`c_src/mimalloc/v3`); its `v2` feature, which nothing in this workspace
/// enables, would select v2.3.1. Both headers put `purge_delay` at 15, but the
/// crate's `extended` bindings do not name it, so the index is written out
/// here. `tests/allocator_options.rs` reads the option back by its NAME, so a
/// wrong index fails there instead of quietly setting some other option.
pub const MI_OPTION_PURGE_DELAY: c_int = 15;

// mimalloc.h declares these as
//   void mi_option_set(mi_option_t option, long value);
//   long mi_option_get(mi_option_t option);
// They sit in the same static library as `MiMalloc`'s own calls, which
// libmimalloc-sys links into this cdylib on every target. The header gives
// them C linkage, also on MSVC where the crate compiles mimalloc as C++.
// The cdylib does not export them, so the calls bind to this extension's
// mimalloc, never to another copy in the process (CPython 3.13 bundles one).
// `long` is `c_long`: 64-bit on Linux, 32-bit on Windows.
extern "C" {
    fn mi_option_set(option: c_int, value: c_long);
    fn mi_option_get(option: c_int) -> c_long;
}

/// Tell mimalloc never to purge (decommit or reset) memory it has freed.
///
/// mimalloc v3 purges freed memory `purge_delay` ms after the free (default
/// 1,000). An advisor runs its 8-thread search seconds apart, so each call
/// faulted the previous tree's pages back in. Measured with 1.5 s between
/// calls and pinned workers, each call took 46.1 ms and about 10,500 page
/// faults. With purging off it takes 39.7 ms and none, the same as the system
/// allocator (39.8 ms). On the trainer wheel the same binary gains ×1.009 wall
/// at 250/1000 sims and ×1.026 at the deployed 1000/4000.
///
/// The cost is that the process keeps its peak heap committed until it exits.
/// Before an advisor call, RSS is 186 MB with purging off, 157 MB with it on,
/// and 209 MB on the system allocator.
///
/// This overrides `MIMALLOC_PURGE_DELAY`. It runs once; the module init calls
/// it before anything else.
pub fn never_purge() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: a plain C call with an in-range option index; mimalloc
        // only stores the value. The store is not synchronized, which is
        // safe here: it runs inside the module init, before any other thread
        // can be running code in this extension.
        unsafe { mi_option_set(MI_OPTION_PURGE_DELAY, -1) };
    });
}

/// mimalloc's current purge delay in ms; `-1` means it never purges.
// `c_long` is already `i64` on Linux, but `i32` on Windows.
#[allow(clippy::useless_conversion)]
pub fn purge_delay_ms() -> i64 {
    // SAFETY: a plain C call with an in-range option index.
    i64::from(unsafe { mi_option_get(MI_OPTION_PURGE_DELAY) })
}
