//! Potion on-drink dispatch registry — bead `sts2-rs-13q` (P7-6).
//!
//! Mirrors `relic_effects` (bead `sts2-rs-l7l` P7-1). Per-potion
//! handlers are keyed by [`PotionKind`]; the extension-point façade
//! lives in `sts2-content::potions::dispatch::install_default_registry`.
//! Phase-7 hook integration (`hooks::dispatch_on_drink_potion`) lands
//! with the umbrella bead `sts2-rs-h9u`; for now the registry is
//! consumed by per-potion unit tests.

use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

use crate::card_effects::EffectResult;
use crate::potions::{PotionKind, PotionTargetType};
use crate::CombatState;

/// Handler signature for a potion on-drink effect. The `target` is the
/// `Option<u8>` carried by [`crate::Action::DrinkPotion`].
pub type PotionEffectFn = fn(&mut CombatState, target: Option<u8>) -> EffectResult;

static REGISTRY: OnceLock<RwLock<BTreeMap<PotionKind, PotionEffectFn>>> = OnceLock::new();

fn registry() -> &'static RwLock<BTreeMap<PotionKind, PotionEffectFn>> {
    REGISTRY.get_or_init(|| RwLock::new(BTreeMap::new()))
}

/// Register a single handler. Overwrites any previous entry.
pub fn register(kind: PotionKind, handler: PotionEffectFn) {
    registry()
        .write()
        .expect("potion dispatch registry poisoned")
        .insert(kind, handler);
}

/// Register many handlers in one go.
pub fn register_many(entries: &[(PotionKind, PotionEffectFn)]) {
    let mut guard = registry()
        .write()
        .expect("potion dispatch registry poisoned");
    for &(id, f) in entries {
        guard.insert(id, f);
    }
}

/// Look up the registered handler for `kind`, or `None` if absent.
#[must_use]
pub fn lookup(kind: PotionKind) -> Option<PotionEffectFn> {
    registry()
        .read()
        .expect("potion dispatch registry poisoned")
        .get(&kind)
        .copied()
}

/// Dispatch the on-drink effect for `kind`. Returns
/// [`EffectResult::NotImplemented`] when no handler is registered.
pub fn dispatch(state: &mut CombatState, kind: PotionKind, target: Option<u8>) -> EffectResult {
    // Event log (C# `PotionUsedEntry`), recorded before the effect so whatever
    // the potion deals or applies follows it.
    crate::event_log::record(
        state,
        crate::event_log::EventLogEntry::PotionUsed {
            potion: kind,
            target,
        },
    );
    match lookup(kind) {
        Some(f) => f(state, target),
        None => EffectResult::NotImplemented,
    }
}

/// Number of currently-registered handlers.
#[must_use]
pub fn registered_count() -> usize {
    registry()
        .read()
        .expect("potion dispatch registry poisoned")
        .len()
}

// ── Per-potion target-type registry (bead `sts2-rs-ybtmu`) ──────────────
//
// `PotionKind` (core) carries no target shape; the canonical
// `PotionKind → PotionTargetType` mapping lives in
// `sts2-content::potions::all::spec_for`. Core needs it for
// `SurroundedPower.BeforePotionUsed`: C# re-faces the surrounded player
// toward an enemy-targeted potion's victim (and only those — Self /
// AllEnemies potions pass a null single-target and never flip). Content
// populates this registry in `install_default_registry`; an unpopulated
// lookup (content not installed, e.g. a core-only unit test) returns
// `None`, which the caller treats as "no flip" — safe.

static TARGET_TYPES: OnceLock<RwLock<BTreeMap<PotionKind, PotionTargetType>>> = OnceLock::new();

fn target_types() -> &'static RwLock<BTreeMap<PotionKind, PotionTargetType>> {
    TARGET_TYPES.get_or_init(|| RwLock::new(BTreeMap::new()))
}

/// Register many `PotionKind → PotionTargetType` entries (content-side).
pub fn register_many_target_types(entries: &[(PotionKind, PotionTargetType)]) {
    let mut guard = target_types()
        .write()
        .expect("potion target-type registry poisoned");
    for &(id, t) in entries {
        guard.insert(id, t);
    }
}

/// Look up the registered target type for `kind`, or `None` if absent.
#[must_use]
pub fn lookup_target_type(kind: PotionKind) -> Option<PotionTargetType> {
    target_types()
        .read()
        .expect("potion target-type registry poisoned")
        .get(&kind)
        .copied()
}
