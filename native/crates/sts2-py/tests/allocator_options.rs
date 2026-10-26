//! Importing `sts2_rs` turns mimalloc's purging off (design-doc item A1b).
//!
//! The option is read back two ways. `allocator::purge_delay_ms` reads the
//! same index the init path writes. mimalloc's own option dump
//! (`mi_options_print_out`) reads it by NAME. Only the second catches a wrong
//! index: with one, the init writes -1 into some other option, the index reads
//! that -1 back, and the option named `purge_delay` keeps its default.

use pyo3::prelude::*;
use std::ffi::{c_char, c_void, CStr};

// `::` because the second import, the `#[pymodule]` entry point, is also a
// module named `sts2_rs` and would shadow the crate.
use ::sts2_rs::allocator;
use ::sts2_rs::sts2_rs;

type OutputFn = unsafe extern "C" fn(msg: *const c_char, arg: *mut c_void);

extern "C" {
    // mimalloc.h (v3): a version line, then one `option '<name>': <value>`
    // line per option, each passed to `out`.
    fn mi_options_print_out(out: Option<OutputFn>, arg: *mut c_void);
}

unsafe extern "C" fn append(msg: *const c_char, arg: *mut c_void) {
    // SAFETY: `arg` is the `String` that `options_by_name` passes, alive for
    // the whole dump; `msg` is a NUL-terminated string mimalloc owns for
    // the duration of this call.
    let (out, msg) = unsafe { (&mut *arg.cast::<String>(), CStr::from_ptr(msg)) };
    out.push_str(&msg.to_string_lossy());
}

/// The value mimalloc's option table holds for the option called `name`.
fn option_by_name(name: &str) -> i64 {
    let mut dump = String::new();
    // SAFETY: `append` only writes into `dump`, which outlives the call.
    unsafe { mi_options_print_out(Some(append), std::ptr::from_mut(&mut dump).cast()) };
    let prefix = format!("option '{name}': ");
    dump.lines()
        .find_map(|line| line.strip_prefix(prefix.as_str()))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("no option '{name}' in mimalloc's dump:\n{dump}"))
}

#[test]
fn importing_the_module_turns_mimalloc_purging_off() {
    // Before the import: purging is on, and the index names `purge_delay`.
    let before = option_by_name("purge_delay");
    assert_ne!(
        before, -1,
        "purging is already off before the import; is MIMALLOC_PURGE_DELAY=-1 set?"
    );
    assert_eq!(
        allocator::purge_delay_ms(),
        before,
        "MI_OPTION_PURGE_DELAY does not index the option named `purge_delay`"
    );

    // The real init path: `import sts2_rs` runs the `#[pymodule]` function.
    pyo3::append_to_inittab!(sts2_rs);
    pyo3::prepare_freethreaded_python();
    Python::with_gil(|py| py.import("sts2_rs").map(drop)).expect("import sts2_rs");

    assert_eq!(allocator::purge_delay_ms(), -1);
    assert_eq!(
        option_by_name("purge_delay"),
        -1,
        "the import must turn off the option named `purge_delay`"
    );
}
