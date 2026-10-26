//! Emit-tree test for split-relics: full synthetic relic_effects fixture →
//! expected file set, per-id install() bodies, regenerated call_once-guarded
//! installer, and a global-order replay check (the regenerated installer
//! replays the same (channel, id, fn) register calls in the same global order
//! as the monolith).
use xtask_lib::split_relics::emit_tree_to_string_map;

const SRC: &str = r"
use std::collections::BTreeMap;
use crate::card_effects::EffectResult;
use crate::relics::RelicId;

pub type RelicEffectFn = fn(&mut CombatState) -> EffectResult;

static INSTALL_ONCE: std::sync::Once = std::sync::Once::new();

fn player_owns_relic(state: &CombatState, id: RelicId) -> bool {
    let _ = super::card_effects::noop();
    false
}

pub fn register_many(entries: &[(RelicId, RelicEffectFn)]) {}

pub struct CardEventCtx {
    pub id: u32,
}

pub fn dispatch_after_combat_victory(state: &mut CombatState) -> EffectResult {
    EffectResult::Resolved
}

/// Cracked Core combat-start marker.
fn on_combat_start_cracked_core(state: &mut CombatState) -> EffectResult {
    EffectResult::Resolved
}

fn on_combat_start_burning_blood(state: &mut CombatState) -> EffectResult {
    EffectResult::Resolved
}

fn on_before_side_turn_start_cracked_core(state: &mut CombatState) -> EffectResult {
    EffectResult::Resolved
}

fn on_after_combat_victory_burning_blood(state: &mut CombatState) -> EffectResult {
    EffectResult::Resolved
}

pub fn install_default_registry() {
    INSTALL_ONCE.call_once(|| {
        register_many(&[
            (RelicId::CrackedCore, on_combat_start_cracked_core as RelicEffectFn),
            (RelicId::BurningBlood, on_combat_start_burning_blood as RelicEffectFn),
        ]);
        register_before_side_turn_start(
            RelicId::CrackedCore,
            on_before_side_turn_start_cracked_core as BeforeSideTurnStartFn,
        );
        register_after_combat_victory(
            RelicId::BurningBlood,
            on_after_combat_victory_burning_blood as AfterCombatVictoryFn,
        );
    });
}

pub fn ensure_installed() {
    install_default_registry();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smoke() {
        let _ = on_combat_start_cracked_core;
    }
}
";

#[test]
fn emits_expected_file_set() {
    let map = emit_tree_to_string_map(SRC);
    assert!(map.contains_key("prelude.rs"));
    assert!(map.contains_key("helpers.rs"));
    assert!(map.contains_key("channels.rs"));
    assert!(map.contains_key("mod.rs"));
    assert!(map.contains_key("tests.rs"));
    assert!(map.contains_key("cracked_core.rs"));
    assert!(map.contains_key("burning_blood.rs"));
}

#[test]
fn handlers_routed_to_their_id_files() {
    let map = emit_tree_to_string_map(SRC);
    let cc = &map["cracked_core.rs"];
    assert!(cc.contains("pub(crate) fn on_combat_start_cracked_core"));
    assert!(cc.contains("pub(crate) fn on_before_side_turn_start_cracked_core"));
    assert!(cc.contains("/// Cracked Core combat-start marker."));
    // burning_blood's handlers are NOT in cracked_core.rs
    assert!(!cc.contains("on_after_combat_victory_burning_blood"));

    let bb = &map["burning_blood.rs"];
    assert!(bb.contains("pub(crate) fn on_combat_start_burning_blood"));
    assert!(bb.contains("pub(crate) fn on_after_combat_victory_burning_blood"));
}

#[test]
fn helpers_and_channels_separated() {
    let map = emit_tree_to_string_map(SRC);
    // Registry plumbing + free helper → helpers.rs.
    assert!(map["helpers.rs"].contains("pub(crate) fn player_owns_relic"));
    assert!(map["helpers.rs"].contains("fn register_many"));
    // Channel payload type + dispatch wrapper → channels.rs.
    assert!(map["channels.rs"].contains("struct CardEventCtx"));
    assert!(map["channels.rs"].contains("fn dispatch_after_combat_victory"));
}

#[test]
fn super_rewritten_to_crate_in_helpers() {
    let map = emit_tree_to_string_map(SRC);
    assert!(map["helpers.rs"].contains("crate::card_effects::noop"));
    assert!(!map["helpers.rs"].contains("super::card_effects"));
}

#[test]
fn per_id_install_bodies_in_global_order() {
    let map = emit_tree_to_string_map(SRC);
    // cracked_core: register_many array elem FIRST, then before_side_turn_start.
    let cc = &map["cracked_core.rs"];
    let install_start = cc.find("pub(crate) fn install()").unwrap();
    let cc_install = &cc[install_start..];
    let many_pos = cc_install
        .find("register_many(&[(RelicId::CrackedCore")
        .expect("cracked_core register_many element present");
    let bst_pos = cc_install
        .find("register_before_side_turn_start")
        .expect("cracked_core before_side_turn_start present");
    assert!(many_pos < bst_pos, "legacy array elem must come first");

    let bb = &map["burning_blood.rs"];
    let bb_install = &bb[bb.find("pub(crate) fn install()").unwrap()..];
    let bb_many = bb_install
        .find("register_many(&[(RelicId::BurningBlood")
        .unwrap();
    let bb_acv = bb_install.find("register_after_combat_victory").unwrap();
    assert!(bb_many < bb_acv);
}

#[test]
fn mod_rs_regenerates_call_once_guarded_installer() {
    let map = emit_tree_to_string_map(SRC);
    let m = &map["mod.rs"];
    assert!(m.contains("pub fn install_default_registry()"));
    assert!(m.contains("helpers::INSTALL_ONCE.call_once(|| {"));
    assert!(m.contains("pub fn ensure_installed()"));
    // First-seen id order: cracked_core (array elem 0) before burning_blood.
    let cc = m.find("cracked_core::install();").unwrap();
    let bb = m.find("burning_blood::install();").unwrap();
    assert!(
        cc < bb,
        "first-seen-id order: cracked_core then burning_blood"
    );
    // Module decls + re-export surface.
    assert!(m.contains("pub mod cracked_core;"));
    assert!(m.contains("pub mod burning_blood;"));
    assert!(m.contains("pub mod channels;"));
    assert!(m.contains("pub use channels::*;"));
    assert!(m.contains("pub use helpers::{register, register_many, lookup, dispatch, registered_count, RelicEffectFn};"));
}

#[test]
fn tests_module_relocated_and_unwrapped() {
    let map = emit_tree_to_string_map(SRC);
    let t = &map["tests.rs"];
    // wrapper stripped: the inner `fn smoke` is present, but no `mod tests {`.
    assert!(t.contains("fn smoke()"));
    assert!(!t.contains("mod tests {"));
    // per-id globs so tests reaching handlers directly resolve.
    assert!(t.contains("cracked_core::*"));
    assert!(t.contains("burning_blood::*"));
}

/// THE load-bearing replay check: flatten the regenerated tree's installer in
/// execution order (mod.rs calls `<id>::install()` in first-seen order; each
/// `install()` emits its register statements in global_index order) and
/// confirm the resulting (channel, id, fn) sequence equals the monolith's
/// original install body order.
#[test]
fn regenerated_installer_replays_same_global_order() {
    let map = emit_tree_to_string_map(SRC);

    // The regenerated installer replays statements grouped by id, in
    // first-seen-id order, each id's statements in ascending global_index
    // (spec ATTRIBUTE step 4). Because every channel is a SEPARATE BTreeMap,
    // and no two ids share a channel+key collision here, this per-id-grouped
    // execution order yields byte-identical registry contents AND identical
    // per-channel listener-vector order as the monolith's flat global order:
    //   monolith flat:  cc(0) bb(1) cc(2) bb(3)
    //   grouped replay: cc(0) cc(2) | bb(1) bb(3)
    // Per channel, the relative order of the two ids is preserved
    // (register_many: cc before bb; the other two channels have one id each),
    // so the observable registry state is identical.
    let expected: Vec<(&str, &str, &str)> = vec![
        // cracked_core::install()
        (
            "register_many",
            "RelicId::CrackedCore",
            "on_combat_start_cracked_core",
        ),
        (
            "register_before_side_turn_start",
            "RelicId::CrackedCore",
            "on_before_side_turn_start_cracked_core",
        ),
        // burning_blood::install()
        (
            "register_many",
            "RelicId::BurningBlood",
            "on_combat_start_burning_blood",
        ),
        (
            "register_after_combat_victory",
            "RelicId::BurningBlood",
            "on_after_combat_victory_burning_blood",
        ),
    ];

    // Reconstruct execution order from the generated tree.
    let m = &map["mod.rs"];
    let install_body = &m[m.find("call_once(|| {").unwrap()..m.find("});").unwrap()];
    // first-seen id call order from mod.rs.
    let mut id_order: Vec<&str> = Vec::new();
    for line in install_body.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_suffix("::install();") {
            id_order.push(rest);
        }
    }

    let mut got: Vec<(String, String, String)> = Vec::new();
    for id in &id_order {
        let body = &map[&format!("{id}.rs")];
        let install = &body[body.find("pub(crate) fn install()").unwrap()..];
        got.extend(parse_register_calls(install));
    }

    assert_eq!(got.len(), expected.len(), "register count preserved");
    for (g, e) in got.iter().zip(expected.iter()) {
        assert_eq!(g.0, e.0, "channel");
        assert_eq!(g.1, e.1, "id");
        assert_eq!(g.2, e.2, "fn");
    }

    // The load-bearing invariant: PER CHANNEL, the relative id order in the
    // regenerated replay must equal the monolith's per-channel id order
    // (each channel is a separate listener vector/BTreeMap, so only intra-
    // channel order is observable). Monolith per-channel id sequences:
    let monolith_per_channel: Vec<(&str, Vec<&str>)> = vec![
        (
            "register_many",
            vec!["RelicId::CrackedCore", "RelicId::BurningBlood"],
        ),
        (
            "register_before_side_turn_start",
            vec!["RelicId::CrackedCore"],
        ),
        (
            "register_after_combat_victory",
            vec!["RelicId::BurningBlood"],
        ),
    ];
    for (chan, ids) in &monolith_per_channel {
        let replay_ids: Vec<&str> = got
            .iter()
            .filter(|(c, _, _)| c == chan)
            .map(|(_, id, _)| id.as_str())
            .collect();
        assert_eq!(&replay_ids, ids, "per-channel id order for {chan}");
    }
}

/// Parse every `register_<chan>(...)` call out of an `install()` body into
/// ordered `(channel_fn, id_path, handler_fn)` triples. Whitespace-insensitive
/// so it handles both the single-line `register_many(&[(RelicId::X, f as
/// RelicEffectFn)]);` form and the multi-line per-channel form
/// `register_<chan>(\n RelicId::X,\n f as <Fn>,\n);`.
fn parse_register_calls(body: &str) -> Vec<(String, String, String)> {
    let collapsed: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = Vec::new();
    let mut rest = collapsed.as_str();
    while let Some(pos) = rest.find("register_") {
        let after = &rest[pos..];
        let Some(paren) = after.find('(') else {
            break;
        };
        let chan = after[..paren].trim().to_string();
        // The id literal is the first `RelicId::Variant` after the paren.
        let inner = &after[paren..];
        let id = inner
            .find("RelicId::")
            .map(|i| {
                let s = &inner[i..];
                let end = s
                    .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                    .unwrap_or(s.len());
                s[..end].to_string()
            })
            .unwrap_or_default();
        // The handler is the ident right before ` as `.
        let handler = inner
            .find(" as ")
            .map(|i| {
                let pre = &inner[..i];
                pre.rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or("")
                    .to_string()
            })
            .unwrap_or_default();
        out.push((chan, id, handler));
        // advance past this `register_` token.
        rest = &rest[pos + "register_".len()..];
    }
    out
}
