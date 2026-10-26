//! Seed canonicalization shared by both backends.
//!
//! Port of `sts2-pycli/python/workbench/seed.py`.
//!
//! The C# engine's `RunRngSet` hashes the seed string as-is, while the Rust
//! port's `RunManager(string_seed=…)` canonicalizes first (uppercase + `O` →
//! `0` + `I` → `1`) before hashing. For non-canonical user input the two
//! engines would otherwise compute different `Seed` ints and diverge from
//! step zero — same input, totally different runs.
//!
//! The workbench applies [`canonicalize_seed`] once at session start so both
//! backends see the same canonical string. Idempotent: a string that is
//! already canonical (e.g. a game-generated 10-char alphanumeric like
//! `"Y3X2YZSBQJ"`) is returned unchanged.
//!
//! The Python function is `seed.strip().upper().replace("O", "0").replace("I",
//! "1")`; `sts2_core::canonicalize_seed` (a port of the C#
//! `SeedHelper.CanonicalizeSeed`) applies the same four rules in the order
//! upper → fold → trim. The order cannot change the result — case-mapping
//! never produces or consumes whitespace, and the folded letters are not
//! whitespace — so the core function is re-exported rather than duplicated.

pub use sts2_core::canonicalize_seed;

#[cfg(test)]
mod tests {
    //! Pins the four rules the Python docstring states: trim, uppercase,
    //! `O` → `0`, `I` → `1`.

    use super::canonicalize_seed;

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(canonicalize_seed("  ABC123  "), "ABC123");
        assert_eq!(canonicalize_seed("\tABC123\n"), "ABC123");
    }

    #[test]
    fn uppercases() {
        assert_eq!(canonicalize_seed("abc123xyz"), "ABC123XYZ");
    }

    #[test]
    fn folds_o_to_zero() {
        assert_eq!(canonicalize_seed("OoO"), "000");
    }

    #[test]
    fn folds_i_to_one() {
        assert_eq!(canonicalize_seed("IiI"), "111");
    }

    #[test]
    fn all_four_rules_compose_like_the_python() {
        // `" oi2AbI ".strip().upper().replace("O", "0").replace("I", "1")`.
        assert_eq!(canonicalize_seed(" oi2AbI "), "012AB1");
    }

    #[test]
    fn a_game_generated_seed_is_a_fixed_point() {
        assert_eq!(canonicalize_seed("Y3X2YZSBQJ"), "Y3X2YZSBQJ");
        assert_eq!(canonicalize_seed("2J2DEKXJP9"), "2J2DEKXJP9");
    }
}

/// Canonical beta numeric seeds are u64; negative signed-32 values are
/// accepted only as an explicit compatibility bridge for old faithful logs.
pub fn parse_numeric_seed(seed: &str) -> Result<u64, std::num::ParseIntError> {
    seed.parse::<u64>()
        .or_else(|unsigned_error| match seed.parse::<i32>() {
            Ok(old) if old < 0 => Ok(u64::from(old as u32)),
            _ => Err(unsigned_error),
        })
}

#[cfg(test)]
mod numeric_tests {
    #[test]
    fn unsigned_max_and_explicit_legacy_signed_seed_are_lossless() {
        assert_eq!(
            super::parse_numeric_seed("18446744073709551615").unwrap(),
            u64::MAX
        );
        assert_eq!(
            super::parse_numeric_seed("-1").unwrap(),
            u64::from(u32::MAX)
        );
        assert!(super::parse_numeric_seed("18446744073709551616").is_err());
    }
}
