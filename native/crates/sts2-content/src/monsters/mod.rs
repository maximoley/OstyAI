// port-pin-skip — registration shim (no Python counterpart).
pub mod dsl;

// Act-1 has two zone variants in STS2: Overgrowth (default) and
// Underdocks (alternate). The Python pin labels Underdocks as `act4.py`
// — a historical misnomer; the C# game has only 3 acts. We use C#
// canonical zone names (see `MegaCrit.Sts2.Core.Models.Acts.*`).
pub mod act1_overgrowth;
pub mod act1_underdocks;
pub mod act1_weak;
pub mod act2_hive;
pub mod act3_glory;
pub mod shared;

mod legacy_power_surface;

pub(crate) use legacy_power_surface::{
    apply_power_to_player_via_legacy_surface, apply_signed_power_to_player,
};
