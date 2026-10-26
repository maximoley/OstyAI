//! [`WireValue`] — the one accessor surface the wire parser reads through.
//!
//! The parser was originally written straight against `PyO3` (`PyDict`
//! lookups, `.extract::<i32>()`, `PyList` downcasts), which pinned the whole
//! reconstruction path to having a Python interpreter in the process. The
//! shapes it reads are plain JSON, so this trait names exactly the operations
//! it needs and nothing else; two implementations supply them:
//!
//! * `&serde_json::Value` — here. The interpreter-free path: a captured
//!   SpireMentor/adapter dict off disk or off a socket.
//! * `Bound<'_, PyAny>` — in `sts2-py`. The historical path, unchanged for
//!   every existing caller.
//!
//! ## Matching Python's acceptance set
//!
//! The two impls must accept the *same* dicts, or a differential harness is
//! comparing two dialects rather than one implementation. Python's coercions
//! are the reference, because that is what has been in production:
//!
//! * **`bool` is an `int` in Python.** `True.__index__()` is 1, so
//!   `.extract::<i64>()` accepts a `PyBool`. [`WireValue::as_i64`] therefore
//!   accepts `Value::Bool` too, rather than being gratuitously stricter than
//!   the path it replaces. `tests/differential.rs` pins the agreement.
//! * **A float is never an int.** `PyO3` refuses `.extract::<i64>()` on a
//!   `PyFloat` (no silent truncation), and `serde_json`'s `as_i64` likewise
//!   returns `None` for `3.0`. Both reject.
//! * **Absent vs null are different.** `PyDict::get_item` returns `Some(None)`
//!   for a key explicitly set to `None`, and `None` only when the key is
//!   missing; `require_keys` accepts the former. [`WireValue::get`] keeps that
//!   split — `Some(v)` with `v.is_null()` for an explicit null.

/// A JSON-shaped value the wire parser can read.
///
/// `Clone` is required because both impls hand back owned handles from
/// [`get`](WireValue::get) / [`as_array`](WireValue::as_array) — a refcount
/// bump for `Bound`, a pointer copy for `&Value`. Neither is a deep copy.
pub trait WireValue: Sized + Clone {
    /// Look a key up in an object. `None` when the key is absent **or** the
    /// receiver is not an object; `Some(v)` with `v.is_null()` when the key
    /// is present and explicitly null.
    fn get(&self, key: &str) -> Option<Self>;

    /// True for JSON `null` / Python `None`.
    fn is_null(&self) -> bool;

    /// True for a JSON object / Python `dict`.
    fn is_object(&self) -> bool;

    /// Every `(key, value)` pair of an object, **sorted by key**, or `None`
    /// if this is not an object.
    ///
    /// The sort is part of the contract, not an implementation detail: a
    /// `PyDict` iterates in insertion order while `serde_json`'s default
    /// `Map` iterates sorted, so an unordered `entries` would hand the two
    /// impls different `Vec` orders for the same input and make any
    /// structural comparison between them meaningless. Both current callers
    /// (`rng.counters`, a card's `combat_vars`) collect independent
    /// key→value assignments where order carries no meaning, so imposing one
    /// costs nothing and buys determinism.
    fn entries(&self) -> Option<Vec<(String, Self)>>;

    /// The string value, or `None` if this is not a string.
    fn as_str(&self) -> Option<String>;

    /// The integer value. Accepts booleans (see the module note on Python's
    /// `bool`-is-`int`); rejects floats.
    fn as_i64(&self) -> Option<i64>;

    /// The unsigned integer value.
    ///
    /// Distinct from [`as_i64`](WireValue::as_i64) because `$.rng.seed` is a
    /// genuine `u64`: a seed above `i64::MAX` is ordinary, and reading it
    /// through `as_i64` would reject it. The `PyO3` path always accepted
    /// those (`.extract::<u64>()`), so this one must too.
    fn as_u64(&self) -> Option<u64>;

    /// The float value. Accepts integers, matching `.extract::<f64>()`.
    fn as_f64(&self) -> Option<f64>;

    /// The boolean value, or `None` if this is not a boolean.
    fn as_bool(&self) -> Option<bool>;

    /// The elements of a JSON array / Python `list`, or `None` if this is
    /// neither.
    fn as_array(&self) -> Option<Vec<Self>>;

    /// A human-readable rendering, for error details only.
    ///
    /// The one caller is the mid-resolution guard, which names the offending
    /// value in its message. Each impl uses its own host's spelling (`str(v)`
    /// under Python, JSON text under `serde_json`) — this is the single place
    /// the two error surfaces are allowed to differ, and only inside the
    /// detail text, never the path or the error class.
    fn render(&self) -> String;
}

impl WireValue for &serde_json::Value {
    fn get(&self, key: &str) -> Option<Self> {
        self.as_object()?.get(key)
    }

    fn is_null(&self) -> bool {
        serde_json::Value::is_null(self)
    }

    fn is_object(&self) -> bool {
        serde_json::Value::is_object(self)
    }

    fn entries(&self) -> Option<Vec<(String, Self)>> {
        let mut out: Vec<(String, Self)> = self
            .as_object()?
            .iter()
            .map(|(k, v)| (k.clone(), v))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Some(out)
    }

    fn as_str(&self) -> Option<String> {
        serde_json::Value::as_str(self).map(ToOwned::to_owned)
    }

    fn as_i64(&self) -> Option<i64> {
        // Python's `bool` subclasses `int`, so the PyO3 path accepts `True`
        // here and this one must too.
        if let serde_json::Value::Bool(b) = self {
            return Some(i64::from(*b));
        }
        // Fully qualified, NOT `self.as_i64()`: `Self` is `&Value`, so `self`
        // is `&&Value` and method syntax resolves back to *this* trait method
        // rather than the inherent one — an infinite recursion that shows up
        // as a stack overflow, not a type error.
        serde_json::Value::as_i64(self)
    }

    fn as_u64(&self) -> Option<u64> {
        if let serde_json::Value::Bool(b) = self {
            return Some(u64::from(*b));
        }
        serde_json::Value::as_u64(self)
    }

    fn as_f64(&self) -> Option<f64> {
        serde_json::Value::as_f64(self)
    }

    fn as_bool(&self) -> Option<bool> {
        serde_json::Value::as_bool(self)
    }

    fn as_array(&self) -> Option<Vec<Self>> {
        Some(serde_json::Value::as_array(self)?.iter().collect())
    }

    fn render(&self) -> String {
        self.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Every assertion below goes through `WireValue::…` explicitly rather
    /// than method syntax. `serde_json::Value` has inherent methods of the
    /// same names, and on a concrete `&Value` those win method resolution —
    /// so `v.as_i64()` here would silently test `serde_json`'s behaviour
    /// instead of this impl's, and the `bool`-is-`int` case (where the two
    /// deliberately differ) would pass while asserting nothing.
    fn wv(v: &serde_json::Value) -> &serde_json::Value {
        v
    }

    #[test]
    fn absent_and_null_are_distinguishable() {
        let v = json!({"present": null});
        assert!(
            WireValue::get(&wv(&v), "missing").is_none(),
            "absent key must read as None"
        );
        let present = WireValue::get(&wv(&v), "present").expect("explicit null is still present");
        assert!(WireValue::is_null(&present));
    }

    #[test]
    fn get_on_a_non_object_is_none() {
        let v = json!([1, 2, 3]);
        assert!(WireValue::get(&wv(&v), "anything").is_none());
    }

    #[test]
    fn bool_reads_as_int_like_python() {
        assert_eq!(WireValue::as_i64(&wv(&json!(true))), Some(1));
        assert_eq!(WireValue::as_i64(&wv(&json!(false))), Some(0));
        assert_eq!(WireValue::as_u64(&wv(&json!(true))), Some(1));
    }

    #[test]
    fn float_is_never_an_int() {
        let v = json!(3.0);
        assert_eq!(
            WireValue::as_i64(&wv(&v)),
            None,
            "no silent truncation, matching PyO3"
        );
        assert_eq!(WireValue::as_f64(&wv(&v)), Some(3.0));
    }

    #[test]
    fn int_reads_as_float() {
        assert_eq!(WireValue::as_f64(&wv(&json!(3))), Some(3.0));
    }

    /// `$.rng.seed` is a genuine u64; reading it through `as_i64` would
    /// reject every seed above `i64::MAX`, which the PyO3 path accepted.
    #[test]
    fn u64_above_i64_max_survives() {
        let v = json!(18_446_744_073_709_551_615u64);
        assert_eq!(WireValue::as_u64(&wv(&v)), Some(u64::MAX));
        assert_eq!(WireValue::as_i64(&wv(&v)), None);
    }

    #[test]
    fn arrays_yield_element_handles() {
        let v = json!([{"a": 1}, {"a": 2}]);
        let items = WireValue::as_array(&wv(&v)).expect("array");
        assert_eq!(items.len(), 2);
        assert_eq!(
            WireValue::get(&items[1], "a").and_then(|x| WireValue::as_i64(&x)),
            Some(2)
        );
        assert!(WireValue::as_array(&wv(&json!({}))).is_none());
    }

    #[test]
    fn entries_are_key_sorted() {
        let v = json!({"zebra": 1, "alpha": 2, "mid": 3});
        let got: Vec<String> = WireValue::entries(&wv(&v))
            .expect("object")
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(got, ["alpha", "mid", "zebra"]);
        assert!(WireValue::entries(&wv(&json!([]))).is_none());
    }
}
