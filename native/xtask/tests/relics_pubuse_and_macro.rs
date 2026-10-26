//! Regression locks for the two APPLY-phase defects (run wf_bbb8dd6a-ed5):
//!  1. A top-level `pub use` re-export must survive at `mod.rs` ROOT as `pub
//!     use` (crate-external surface, e.g. `relic_effects::Reward`) — NOT be
//!     downgraded to `pub(crate) use` and buried in the private prelude.
//!  2. The `relic_hook_channel!` macro generates a module-private
//!     `fn $registry_fn()`; post-split, `channels.rs` and `tests.rs` are
//!     siblings, so the generated registry accessor must be widened to
//!     `pub(crate)` for the `use super::channels::*` glob to re-export it.
use xtask_lib::split_relics::emit_tree_to_string_map;

const SRC: &str = r"
use std::collections::BTreeMap;
use crate::card_effects::EffectResult;
use crate::relics::RelicId;

pub use crate::runstate::RoomType;
pub use crate::runstate::{CardReward, RelicReward, Reward};

pub type RelicEffectFn = fn(&mut CombatState) -> EffectResult;

static INSTALL_ONCE: std::sync::Once = std::sync::Once::new();

pub fn register_many(entries: &[(RelicId, RelicEffectFn)]) {}

macro_rules! relic_hook_channel {
    ($registry_fn:ident, $fn_alias:ident) => {
        fn $registry_fn() -> &'static RwLock<BTreeMap<RelicId, $fn_alias>> {
            static R: OnceLock<RwLock<BTreeMap<RelicId, $fn_alias>>> = OnceLock::new();
            R.get_or_init(|| RwLock::new(BTreeMap::new()))
        }
    };
}

relic_hook_channel!(registry_modify_power_amount_received, ModifyPowerFn);

fn on_combat_start_burning_blood(state: &mut CombatState) -> EffectResult {
    EffectResult::Resolved
}

pub fn install_default_registry() {
    INSTALL_ONCE.call_once(|| {
        register_many(&[
            (RelicId::BurningBlood, on_combat_start_burning_blood as RelicEffectFn),
        ]);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smoke() {
        let _ = registry_modify_power_amount_received();
    }
}
";

#[test]
fn pub_use_reexport_lands_at_mod_root_verbatim() {
    let map = emit_tree_to_string_map(SRC);
    let m = &map["mod.rs"];
    // Both monolith `pub use` re-exports preserved verbatim at the root.
    assert!(
        m.contains("pub use crate::runstate::RoomType;"),
        "RoomType pub-use missing from mod.rs root:\n{m}"
    );
    assert!(
        m.contains("pub use crate::runstate::{CardReward, RelicReward, Reward};"),
        "Reward pub-use missing from mod.rs root:\n{m}"
    );
    // It must NOT be the only copy: internal bodies still reference these via
    // the prelude glob, so the prelude keeps a downgraded `pub(crate) use`.
    let prelude = &map["prelude.rs"];
    assert!(
        prelude.contains("pub(crate) use crate::runstate::{CardReward, RelicReward, Reward};"),
        "prelude must retain a pub(crate) copy for internal references:\n{prelude}"
    );
    // And a plain `use` must NOT have been promoted to a root pub use.
    assert!(!m.contains("pub use crate::card_effects::EffectResult;"));
}

#[test]
fn channel_macro_registry_fn_is_pub_crate() {
    let map = emit_tree_to_string_map(SRC);
    let ch = &map["channels.rs"];
    assert!(
        ch.contains("pub(crate) fn $registry_fn()"),
        "macro registry fn must be widened to pub(crate):\n{ch}"
    );
    // The original private form must be gone (no bare `fn $registry_fn()`).
    assert!(
        !ch.contains("\n        fn $registry_fn()"),
        "private `fn $registry_fn()` must have been rewritten"
    );
}
