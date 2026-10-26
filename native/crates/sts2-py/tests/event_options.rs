//! `event_options()` `PyO3` surface (bead `sts2-rs-h9qn3`, W4).
//!
//! At run start the engine lands in `Phase::Event` with
//! `current_event = Some(Neow)`. `RunManager.event_options()` should
//! return a non-empty list of `(option_id, label)` tuples whose
//! `option_id`s match the lowercased `TextKey` path segments the scraper
//! records (`<EVENT_ID>.pages.INITIAL.options.<OPT>.title` → `<opt>`).

#![allow(clippy::missing_panics_doc)]

use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::sync::Once;

use sts2_rs::sts2_rs;

fn init_sts2_rs_module() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        pyo3::append_to_inittab!(sts2_rs);
        pyo3::prepare_freethreaded_python();
    });
}

fn build_run_manager(py: Python<'_>) -> PyResult<Py<PyAny>> {
    let sts2_rs = py.import("sts2_rs")?;
    let cls = sts2_rs.getattr("RunManager")?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("seed", 42i32)?;
    kwargs.set_item("character", "ironclad")?;
    kwargs.set_item("neow_start", true)?;
    cls.call((), Some(&kwargs)).map(pyo3::Bound::unbind)
}

#[test]
fn event_options_returns_neow_options_at_run_start() {
    init_sts2_rs_module();
    Python::with_gil(|py| {
        let rm = build_run_manager(py).expect("build RunManager");
        let opts_obj = rm
            .bind(py)
            .call_method0("event_options")
            .expect("event_options()");
        assert!(
            !opts_obj.is_none(),
            "expected Phase::Event with Neow current_event at run start"
        );
        let opts: Vec<(String, String)> =
            opts_obj.extract().expect("List[Tuple[str, str]] extract");
        assert!(!opts.is_empty(), "Neow options should be non-empty");
        for (option_id, label) in &opts {
            assert_eq!(
                option_id.to_lowercase(),
                *option_id,
                "option_id `{option_id}` must be lowercase for `TextKey` mapping"
            );
            assert!(!label.is_empty(), "label should be non-empty");
        }
    });
}

#[test]
fn event_options_returns_none_outside_event_phase() {
    init_sts2_rs_module();
    Python::with_gil(|py| {
        let sts2_rs = py.import("sts2_rs").expect("import sts2_rs");
        let cls = sts2_rs.getattr("RunManager").expect("RunManager class");
        let kwargs = PyDict::new(py);
        kwargs.set_item("seed", 7i32).unwrap();
        kwargs.set_item("character", "ironclad").unwrap();
        // neow_start defaults to false → run starts at Phase::MapChoice,
        // current_event is None.
        let rm = cls.call((), Some(&kwargs)).expect("RunManager()");
        let opts_obj = rm.call_method0("event_options").expect("event_options()");
        assert!(
            opts_obj.is_none(),
            "expected None when no event is materialized"
        );
    });
}
