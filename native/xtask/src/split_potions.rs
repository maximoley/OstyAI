//! Extractor: parse `crates/sts2-content/src/potions/dispatch.rs` and emit a
//! per-potion module tree `crates/sts2-content/src/potions/dispatch/`.
//!
//! Sibling of `split_cards.rs` (per-card pilot) and `split_relics.rs` (per-relic
//! registry). Potions are the SIMPLEST of the three on the attribution axis —
//! the whole registration set is a single
//! `let entries: [(PotionKind, PotionEffectFn); 63] = [ … ];` literal inside
//! `install_default_registry`, followed by `potion_effects::register_many(&entries)`.
//! See `docs/plans/2026-05-30-potions-handler-split-plan.md`.
//!
//! Structural deltas from the relic sibling:
//!   * `IdType = PotionKind`, handler prefix `drink_`.
//!   * Attribution binds the `let entries` ARRAY directly (no
//!     `INSTALL_ONCE.call_once` to descend into — the install body is flat).
//!   * Single `register_many` channel → the central `entries` array is
//!     regenerated in `mod.rs` (the card flat-array shape, NOT the relic
//!     per-id-`install()` shape). NO per-id `install()` fn.
//!   * NO `specs` module (potion spec data lives in `potions/mod.rs`, untouched).
//!   * `super::` in relocated bodies means `crate::potions::` (the module lives
//!     under `crate::potions::dispatch`).
//!   * Two test-referenced consts (`KINGS_COURAGE_FORGE_AMOUNT`,
//!     `BONE_BREW_SUMMON_AMOUNT`) are `pub(crate) use`-re-exported at mod.rs
//!     root so the relocated test module's `super::<CONST>` keeps resolving.

use crate::split_cards::{
    bump_visibility, item_span_bytes, parse_pool_entries, raw_mod, to_pub_crate_use,
};
use crate::split_relics::{id_to_snake, rewrite_super_to};
use std::collections::{BTreeMap, BTreeSet};

/// Path prefix that `super::` resolves to for relocated dispatch items. The
/// module lives under `crate::potions::dispatch`; relocated one level deeper,
/// `super::` (which meant `crate::potions::dispatch`) is rewritten to the
/// absolute `crate::potions::`. (Practically a no-op — production handler/helper
/// bodies contain no `super::` — but kept for parity with the sibling tools.)
pub const POTION_SUPER_TARGET: &str = "crate::potions::";

// ---------------------------------------------------------------------------
// CLASSIFY
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PotionBucket {
    /// A registered `drink_<id>` handler, grouped under the PotionKind it is
    /// registered for. `id_snake` is derived from the registration's FIRST arg
    /// (`PotionKind::KingsCourage` → `kings_courage`), NOT by stripping the fn
    /// name — this defeats the `drink_character_card_potion` decoy.
    Handler { id_snake: String },
    /// `install_default_registry` / `ensure_installed` — regenerated.
    Install,
    /// Free helper fns (incl. the `drink_*`-named decoy) + handler-local consts
    /// → `helpers.rs`.
    Helper,
    /// `#[cfg(test)] mod tests` → `tests.rs`.
    Tests,
    /// `use` imports → `prelude.rs`.
    Imports,
}

fn has_cfg_test(attrs: &[syn::Attribute]) -> bool {
    use quote::ToTokens;
    attrs
        .iter()
        .any(|a| a.path().is_ident("cfg") && a.to_token_stream().to_string().contains("test"))
}

/// Classify a top-level item. `handler_to_id` maps a registered handler fn name
/// → its `<id_snake>` (built from the `install_default_registry` `entries`
/// array); it is the AUTHORITATIVE classifier for handler fns (a `drink_*`-named
/// fn ABSENT from the array — the `drink_character_card_potion` decoy — is a
/// shared helper, not a handler).
pub fn classify_potion_item(
    item: &syn::Item,
    handler_to_id: &BTreeMap<String, String>,
) -> PotionBucket {
    match item {
        syn::Item::Use(_) => PotionBucket::Imports,
        syn::Item::Mod(m) if has_cfg_test(&m.attrs) => PotionBucket::Tests,
        syn::Item::Fn(f) => {
            let name = f.sig.ident.to_string();
            if name == "install_default_registry" || name == "ensure_installed" {
                PotionBucket::Install
            } else if let Some(id_snake) = handler_to_id.get(&name) {
                PotionBucket::Handler {
                    id_snake: id_snake.clone(),
                }
            } else {
                // Every other free fn (the 5 helpers incl. the decoy) → helpers.
                PotionBucket::Helper
            }
        }
        // const items + anything else (impl, etc.) → helpers.
        _ => PotionBucket::Helper,
    }
}

// ---------------------------------------------------------------------------
// ATTRIBUTE — parse install_default_registry `let entries` array
// ---------------------------------------------------------------------------

/// One attributed registration: a single `(PotionKind::X, drink_y as
/// PotionEffectFn)` array element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PotionReg {
    /// Position in the `entries` array (= install call order).
    pub array_index: usize,
    /// `PotionKind::X` literal text.
    pub id: String,
    /// `<id_snake>` derived from `id` (`PotionKind::KingsCourage` →
    /// `kings_courage`).
    pub id_snake: String,
    /// The handler fn name (second arg, modulo the `as PotionEffectFn` cast).
    pub handler: String,
}

/// Locate the `let entries: [(PotionKind, PotionEffectFn); N] = [ … ];` local in
/// `install_default_registry` and return its array elements in source order.
/// Binds the `let`'s array literal directly (NOT the `&entries` arg of
/// `register_many`, which is a reference to the binding). Reuses
/// `parse_pool_entries` (shared with the card tool — it strips the `as <Fn>`
/// cast and whitespace).
#[must_use]
pub fn parse_install_entries(install_fn: &syn::ItemFn) -> Vec<PotionReg> {
    let array_expr = find_entries_array(install_fn)
        .expect("install_default_registry must contain `let entries = [ … ];`");
    let mut out = Vec::new();
    for (array_index, (id, handler)) in parse_pool_entries(array_expr).into_iter().enumerate() {
        let id_snake = id_to_snake(&id);
        out.push(PotionReg {
            array_index,
            id,
            id_snake,
            handler,
        });
    }
    out
}

/// Find the `let entries = <array>` local's init array expr in the install fn.
fn find_entries_array(f: &syn::ItemFn) -> Option<&syn::Expr> {
    for stmt in &f.block.stmts {
        if let syn::Stmt::Local(local) = stmt {
            if let Some(init) = &local.init {
                if let syn::Expr::Array(_) = &*init.expr {
                    return Some(&init.expr);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// EMIT
// ---------------------------------------------------------------------------

/// Pure core: source text → {filename → file contents}. No disk IO.
#[must_use]
pub fn emit_tree_to_string_map(src: &str) -> BTreeMap<String, String> {
    let file = syn::parse_file(src).expect("potions/dispatch.rs must parse");

    // PASS 1: find install_default_registry, parse its `let entries` array,
    // build the handler→id classifier map.
    let install_fn = file
        .items
        .iter()
        .find_map(|it| match it {
            syn::Item::Fn(f) if f.sig.ident == "install_default_registry" => Some(f),
            _ => None,
        })
        .expect("install_default_registry must exist");
    let regs = parse_install_entries(install_fn);
    let mut handler_to_id: BTreeMap<String, String> = BTreeMap::new();
    for r in &regs {
        handler_to_id
            .entry(r.handler.clone())
            .or_insert_with(|| r.id_snake.clone());
    }

    // PASS 2: classify + slice every top-level item.
    let mut imports: Vec<String> = Vec::new();
    let mut helpers: Vec<String> = Vec::new();
    let mut tests: Vec<String> = Vec::new();
    // id_snake → handler slices.
    let mut handler_slices: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // UPPER_SNAKE const idents relocated into helpers.rs (for the test-ref scan).
    let mut relocated_consts: BTreeSet<String> = BTreeSet::new();

    for item in &file.items {
        let (s, e) = item_span_bytes(item);
        let slice = &src[s..e];
        match classify_potion_item(item, &handler_to_id) {
            PotionBucket::Imports => {
                imports.push(rewrite_super_to(slice, POTION_SUPER_TARGET));
            }
            PotionBucket::Helper => {
                let emitted = if accepts_visibility(item) {
                    bump_visibility(slice)
                } else {
                    slice.to_string()
                };
                helpers.push(rewrite_super_to(&emitted, POTION_SUPER_TARGET));
                if let syn::Item::Const(c) = item {
                    relocated_consts.insert(c.ident.to_string());
                }
            }
            PotionBucket::Handler { id_snake } => {
                handler_slices
                    .entry(id_snake)
                    .or_default()
                    .push(rewrite_super_to(
                        &bump_visibility(slice),
                        POTION_SUPER_TARGET,
                    ));
            }
            PotionBucket::Tests => {
                // Strip the outer `mod tests { … }` wrapper; tests.rs *is* the
                // module. Its `super::` already meant the dispatch module → do
                // NOT rewrite.
                if let syn::Item::Mod(m) = item {
                    if let Some((brace, _)) = &m.content {
                        let open = brace.span.open().byte_range();
                        let close = brace.span.close().byte_range();
                        tests.push(src[open.end..close.start].to_string());
                    }
                }
            }
            PotionBucket::Install => { /* regenerated; original dropped */ }
        }
    }

    // Every id that has a handler slice; in registration (array) order all 63
    // appear. A registered id whose handler body is missing (none expected)
    // still gets a file keyed by its id_snake so the central array resolves.
    let mut all_ids: BTreeSet<String> = BTreeSet::new();
    all_ids.extend(handler_slices.keys().cloned());
    for r in &regs {
        all_ids.insert(r.id_snake.clone());
    }

    // Test-referenced consts: scan the test slice(s) for `super::<UPPER_SNAKE>`
    // matching a relocated const → re-export at mod.rs root.
    let test_referenced_consts = scan_test_referenced_consts(&tests, &relocated_consts);

    let mut out = BTreeMap::new();

    // prelude.rs — re-export every monolith import + the helpers glob.
    // `pub(crate) use` (not `pub use`) to avoid E0365 on pub(crate) targets.
    // NO `specs` glob (delta from cards).
    let mut prelude = String::from(
        "//! @generated by xtask split-potions. Per-potion prelude.\n#![allow(clippy::wildcard_imports, unused_imports)]\n\n",
    );
    for i in &imports {
        prelude.push_str(&to_pub_crate_use(i));
        prelude.push('\n');
    }
    prelude.push_str("\npub(crate) use super::helpers::*;\n");
    out.insert("prelude.rs".to_string(), prelude);

    // helpers.rs
    out.insert("helpers.rs".to_string(), wrap_module(&helpers));

    // per-id <id_snake>.rs — JUST that potion's handler slice(s). NO per-id
    // install() fn (delta from relics).
    for id_snake in &all_ids {
        let mut body =
            String::from("//! @generated by xtask split-potions.\nuse super::prelude::*;\n\n");
        if let Some(slices) = handler_slices.get(id_snake) {
            for sl in slices {
                body.push_str(sl);
                body.push_str("\n\n");
            }
        }
        out.insert(format!("{id_snake}.rs"), body);
    }

    // tests.rs (only if present).
    if !tests.is_empty() {
        let mut t = String::from(
            "//! @generated by xtask split-potions. Moved test module.\n#![allow(clippy::wildcard_imports, unused_imports)]\nuse super::prelude::*;\n",
        );
        let mut ids: Vec<&String> = all_ids.iter().collect();
        ids.sort();
        if !ids.is_empty() {
            t.push_str("use super::{");
            for (i, id) in ids.iter().enumerate() {
                if i > 0 {
                    t.push_str(", ");
                }
                t.push_str(&format!("{}::*", raw_mod(id)));
            }
            t.push_str("};\n");
        }
        t.push('\n');
        for slice in &tests {
            t.push_str(slice);
            t.push('\n');
        }
        out.insert("tests.rs".to_string(), t);
    }

    // mod.rs aggregator.
    out.insert(
        "mod.rs".to_string(),
        gen_mod_rs(&all_ids, &regs, &test_referenced_consts, !tests.is_empty()),
    );

    out
}

/// Scan relocated test slices for `super::<UPPER_SNAKE>` references that match a
/// relocated const ident; return the matching const idents (sorted, deduped).
/// These must be re-exported at `mod.rs` root so the test module's `super::`
/// keeps resolving after the const moved into `helpers.rs`.
fn scan_test_referenced_consts(
    tests: &[String],
    relocated_consts: &BTreeSet<String>,
) -> Vec<String> {
    let mut found: BTreeSet<String> = BTreeSet::new();
    for slice in tests {
        let bytes = slice.as_bytes();
        let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut i = 0usize;
        while let Some(rel) = slice[i..].find("super::") {
            let start = i + rel + "super::".len();
            // collect the ident immediately after `super::`.
            let mut end = start;
            while end < bytes.len() && is_ident(bytes[end]) {
                end += 1;
            }
            let ident = &slice[start..end];
            if relocated_consts.contains(ident) {
                found.insert(ident.to_string());
            }
            i = start.max(i + rel + 1);
        }
    }
    found.into_iter().collect()
}

/// `true` for items that accept a leading visibility qualifier. `impl`,
/// `extern {}`, and freestanding macro invocations/defs do NOT (E0449).
fn accepts_visibility(item: &syn::Item) -> bool {
    !matches!(
        item,
        syn::Item::Impl(_) | syn::Item::ForeignMod(_) | syn::Item::Macro(_)
    )
}

fn wrap_module(slices: &[String]) -> String {
    let mut s = String::from(
        "//! @generated by xtask split-potions.\n#![allow(unused_imports)]\nuse super::prelude::*;\n\n",
    );
    for sl in slices {
        s.push_str(sl);
        s.push_str("\n\n");
    }
    s
}

/// Regenerate `mod.rs`: module decls, the two-const `pub(crate) use` re-export,
/// and the flat `let entries = [ … ]; register_many(&entries)` installer with
/// each handler path qualified to `<id_snake>::drink_<id>`.
fn gen_mod_rs(
    all_ids: &BTreeSet<String>,
    regs: &[PotionReg],
    test_referenced_consts: &[String],
    has_tests: bool,
) -> String {
    let mut s = String::new();
    s.push_str("// @generated by xtask split-potions. DO NOT EDIT the potion list by hand;\n");
    s.push_str("// re-run `cargo run -p xtask -- split-potions` after adding a potion handler.\n");
    s.push_str("//! Per-potion dispatch module aggregator.\n");
    s.push_str("#![allow(unused_imports)]\n\n");
    s.push_str("use sts2_core::potion_effects::{self, PotionEffectFn};\n");
    s.push_str("use sts2_core::PotionKind;\n\n");

    s.push_str("mod prelude;\npub mod helpers;\n");
    if has_tests {
        s.push_str("#[cfg(test)]\nmod tests;\n");
    }
    let mut ids: Vec<&String> = all_ids.iter().collect();
    ids.sort();
    for id in &ids {
        s.push_str(&format!("pub mod {};\n", raw_mod(id)));
    }
    s.push('\n');

    // Re-export the two consts the relocated test module reaches at the module
    // root via `super::<CONST>`. `pub(crate)` keeps them in-crate-visible
    // without E0365.
    if !test_referenced_consts.is_empty() {
        s.push_str(
            "// Re-export the consts the relocated test module reaches at the module\n// root via `super::<CONST>`.\n",
        );
        s.push_str("pub(crate) use helpers::{");
        for (i, c) in test_referenced_consts.iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            s.push_str(c);
        }
        s.push_str("};\n\n");
    }

    // Flat installer — the card-flat-array shape (NO call_once guard).
    s.push_str("/// Install every potion on-drink handler into the global registry.\n");
    s.push_str("/// (`register_many` overwrites existing entries with the same value.)\n");
    s.push_str("pub fn install_default_registry() {\n");
    s.push_str(&format!(
        "    let entries: [(PotionKind, PotionEffectFn); {}] = [\n",
        regs.len()
    ));
    for r in regs {
        let stem = r.handler.strip_prefix("drink_").unwrap_or(&r.handler);
        // The module is named by the registration's id_snake; the handler fn
        // keeps its original name. Resolve via `<id_snake>::<handler>`.
        let _ = stem;
        s.push_str(&format!(
            "        ({}, {}::{} as PotionEffectFn),\n",
            r.id,
            raw_mod(&r.id_snake),
            r.handler
        ));
    }
    s.push_str("    ];\n");
    s.push_str("    potion_effects::register_many(&entries);\n\n");
    // Bead `sts2-rs-y81ne`: also publish the canonical `PotionKind →
    // PotionTargetType` mapping (from `all::spec_for`) into core so
    // `SurroundedPower.BeforePotionUsed` can re-face the surrounded player
    // toward an enemy-targeted potion's victim. Core carries no
    // target-shape data of its own. Emitted by the generator (not
    // hand-edited) so a `split-potions` regen preserves it.
    s.push_str(concat!(
        "    // Bead `sts2-rs-y81ne`: publish the canonical `PotionKind ->\n",
        "    // PotionTargetType` mapping (from `all::spec_for`) into core so\n",
        "    // `SurroundedPower.BeforePotionUsed` can re-face the surrounded\n",
        "    // player toward an enemy-targeted potion's victim. Core carries no\n",
        "    // target-shape data of its own.\n",
        "    let target_types: Vec<(PotionKind, sts2_core::potions::PotionTargetType)> = PotionKind::ALL\n",
        "        .iter()\n",
        "        .map(|&k| (k, crate::potions::all::spec_for(k).target_type))\n",
        "        .collect();\n",
        "    potion_effects::register_many_target_types(&target_types);\n",
    ));
    s.push_str("}\n\n");

    s.push_str("/// Idempotent alias for [`install_default_registry`].\n");
    s.push_str("pub fn ensure_installed() {\n    install_default_registry();\n}\n");
    s
}

// ---------------------------------------------------------------------------
// Disk driver
// ---------------------------------------------------------------------------

/// Run the split against the real `potions/dispatch.rs`. NOT exercised by the
/// unit tests (which use synthetic fixtures); applied under the determinism
/// lock in the APPLY phase.
pub fn run() {
    let base = std::path::Path::new("crates/sts2-content/src/potions");
    let src_path = base.join("dispatch.rs");
    let src = std::fs::read_to_string(&src_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", src_path.display()));

    let map = emit_tree_to_string_map(&src);

    let dir = base.join("dispatch");
    std::fs::create_dir_all(&dir).expect("create dispatch dir");
    for (name, contents) in &map {
        std::fs::write(dir.join(name), contents).expect("write file");
    }
    std::fs::remove_file(&src_path).expect("remove monolith");
    eprintln!(
        "split-potions: wrote {} files to {}",
        map.len(),
        dir.display()
    );
}
