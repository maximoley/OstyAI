//! Ascension-level stat scaling, ported 1:1 from C#
//! `../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Helpers/AscensionHelper.cs`.
//!
//! C# encodes per-monster ascension scaling inline at every stat via
//! `AscensionHelper.GetValueIfAscension(level, ascensionValue, fallbackValue)`,
//! which returns `ascensionValue` iff `RunManager.HasAscension(level)` —
//! and `AscensionManager.HasLevel(level)` is `_level >= (int)level`
//! (`../sts2-wiki/decompiled/v0.103.2/MegaCrit.Sts2.Core.Entities.Ascension/AscensionManager.cs:24`).
//! So the threshold is the enum *discriminant*: a stat tagged
//! `ToughEnemies` (discriminant 8) scales at ascension `>= 8`,
//! `DeadlyEnemies` (discriminant 9) at `>= 9`.
//!
//! Only two of the eleven `AscensionLevel` variants touch monster stats:
//! `ToughEnemies` (monster HP) and `DeadlyEnemies` (monster attack
//! damage). The remaining variants affect the run/map/shop layer, not
//! combat-side creature stats, so they are intentionally omitted here —
//! add them if a combat-side consumer appears.

/// The two ascension thresholds that scale combat-side monster stats.
/// The discriminant is the C# `AscensionLevel` enum ordinal, which is
/// exactly the ascension level at which the tier activates (`HasLevel`
/// is `_level >= (int)level`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AscensionLevel {
    /// `AscensionLevel.ToughEnemies` — monster HP scaling, active at
    /// ascension `>= 8`.
    ToughEnemies = 8,
    /// `AscensionLevel.DeadlyEnemies` — monster attack-damage scaling,
    /// active at ascension `>= 9`.
    DeadlyEnemies = 9,
}

/// Port of C# `AscensionHelper.GetValueIfAscension(level, ascensionValue,
/// fallbackValue)`: return `asc_value` when the run's `ascension` has
/// reached `level`'s threshold, else `base_value`.
///
/// Faithful to the C# argument order — `asc_value` (the scaled value)
/// first, `base_value` (the non-ascension fallback) second — so a
/// transcription from a C# `GetValueIfAscension(Tier, A, B)` call maps
/// directly to `get_value_if_ascension(asc, Tier, A, B)`.
#[must_use]
pub fn get_value_if_ascension(
    ascension: u8,
    level: AscensionLevel,
    asc_value: i32,
    base_value: i32,
) -> i32 {
    if ascension >= level as u8 {
        asc_value
    } else {
        base_value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tough_enemies_hp_scales_at_ascension_8() {
        // C# FuzzyWurmCrawler MinInitialHp =>
        // GetValueIfAscension(ToughEnemies, 58, 55).
        let f = |asc| get_value_if_ascension(asc, AscensionLevel::ToughEnemies, 58, 55);
        assert_eq!(f(0), 55, "A0 uses the non-ascension fallback");
        assert_eq!(f(7), 55, "A7 is still below the ToughEnemies threshold");
        assert_eq!(f(8), 58, "A8 activates ToughEnemies");
        assert_eq!(f(10), 58, "A10 (>= 8) keeps the scaled value");
    }

    #[test]
    fn deadly_enemies_damage_scales_at_ascension_9() {
        // C# FuzzyWurmCrawler AcidGoopDamage =>
        // GetValueIfAscension(DeadlyEnemies, 6, 4).
        let f = |asc| get_value_if_ascension(asc, AscensionLevel::DeadlyEnemies, 6, 4);
        assert_eq!(f(0), 4, "A0 uses the non-ascension fallback");
        assert_eq!(f(8), 4, "A8 is below the DeadlyEnemies threshold");
        assert_eq!(f(9), 6, "A9 activates DeadlyEnemies");
        assert_eq!(f(10), 6, "A10 (>= 9) keeps the scaled value");
    }
}
