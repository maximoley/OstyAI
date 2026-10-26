//! The wire-parse error surface, decoupled from `PyO3`.
//!
//! `state_dict.rs` used to build `PyErr`s directly through two factories:
//! `unsupported()` → the custom `UnsupportedStateError` Python exception,
//! and `schema_err()` → `ValueError`. That distinction is load-bearing --
//! `sts2-py/tests/fail_loud.rs` asserts the *type* per failure class, not
//! just the message -- so it survives here as the two [`WireError`]
//! variants, and the `PyO3` shim maps them back 1:1.
//!
//! The rendered message is `"{path}: {detail}"` in both cases, byte-identical
//! to what the `PyErr` factories produced. Downstream assertions (and the
//! Python adapter's own error scraping) key off that shape.

use std::fmt;

/// A wire-dict rejection, carrying the offending JSON path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    /// Structural/validation failure: missing key, wrong type, invariant
    /// violation (`hp > max_hp`, negative energy, …). Maps to `ValueError`.
    Schema { path: String, detail: String },
    /// The dict is well-formed but references an id, power, or shape the
    /// engine cannot model. Maps to `UnsupportedStateError`.
    Unsupported { path: String, detail: String },
}

impl WireError {
    /// The JSON path the failure was found at (`$.player.relics[3]`).
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Schema { path, .. } | Self::Unsupported { path, .. } => path,
        }
    }

    /// The human-readable reason, without the path prefix.
    #[must_use]
    pub fn detail(&self) -> &str {
        match self {
            Self::Schema { detail, .. } | Self::Unsupported { detail, .. } => detail,
        }
    }

    /// True for the [`WireError::Unsupported`] class (unmodelled content),
    /// which the `PyO3` shim raises as `UnsupportedStateError`.
    #[must_use]
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Self::Unsupported { .. })
    }
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path(), self.detail())
    }
}

impl std::error::Error for WireError {}

pub type WireResult<T> = Result<T, WireError>;

/// Unmodelled content/shape → `UnsupportedStateError` on the Python side.
pub fn unsupported<P: AsRef<str>>(path: P, detail: &str) -> WireError {
    WireError::Unsupported {
        path: path.as_ref().to_owned(),
        detail: detail.to_owned(),
    }
}

/// Structural/validation failure → `ValueError` on the Python side.
pub fn schema_err<P: AsRef<str>>(path: P, detail: &str) -> WireError {
    WireError::Schema {
        path: path.as_ref().to_owned(),
        detail: detail.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rendered form is what `fail_loud.rs` greps for a path substring
    /// in, and what the Python adapter surfaces to the advisor log.
    #[test]
    fn display_is_path_colon_detail() {
        let e = unsupported("$.player.relics[3]", "GHIBBET_MAW");
        assert_eq!(e.to_string(), "$.player.relics[3]: GHIBBET_MAW");
        let e = schema_err("$", "missing required key `phase`");
        assert_eq!(e.to_string(), "$: missing required key `phase`");
    }

    #[test]
    fn classes_are_distinguishable() {
        assert!(unsupported("$", "x").is_unsupported());
        assert!(!schema_err("$", "x").is_unsupported());
    }
}
