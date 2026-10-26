//! Extractor: parse `crates/sts2-core/src/relic_effects.rs` and emit a
//! per-relic module tree `crates/sts2-core/src/relic_effects/`.
//!
//! Sibling of `split_cards.rs` (the per-card pilot). The structural deltas
//! from cards (see `docs/plans/2026-05-30-relics-handler-split-plan.md`):
//!   * `IdType = RelicId` (not `CardId`).
//!   * Registrations are attributed from the `install_default_registry()`
//!     statement list — a sequence of
//!     `register_<channel>(RelicId::X, handler as <ChannelFn>)` calls plus
//!     one legacy `register_many(&[...])` array — NOT a `const POOL` tuple
//!     array. Each statement routes verbatim to the relic named in its
//!     first argument; the `register_many` array is split per element.
//!   * The install body is wrapped in `INSTALL_ONCE.call_once(|| { … })`;
//!     the closure block is descended into and the `call_once` guard is
//!     preserved in the regenerated top-level installer.
//!   * `super::` in relocated bodies means `crate` (the module lives
//!     directly under the crate root), so it is rewritten to `crate::`.
//!   * An extra shared `channels.rs` module holds the `relic_hook_channel!`
//!     macro + invocations + `dispatch_*` wrappers + payload types.

use crate::split_cards::{bump_visibility, item_span_bytes, raw_mod, to_pub_crate_use};
use std::collections::{BTreeMap, BTreeSet};
use syn::spanned::Spanned;

/// Path prefix that `super::` resolves to for relocated relic_effects items.
/// The module lives directly under the crate root, so `super::` means
/// `crate::` (cf. cards, where it meant `crate::cards::`).
pub const RELIC_SUPER_TARGET: &str = "crate::";

/// Rewrite `super::` path prefixes to an absolute `target` path. Parameterized
/// so the shared split core works for both cards (`crate::cards::`) and relics
/// (`crate::`). `super` is a keyword and can never be a substring of an
/// identifier, so a plain textual replace is safe.
#[must_use]
pub fn rewrite_super_to(slice: &str, target: &str) -> String {
    slice.replace("super::", target)
}

// ---------------------------------------------------------------------------
// CLASSIFY
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelicBucket {
    /// A registered handler fn, grouped under the RelicId it is registered for.
    Handler { id_snake: String },
    /// `install_default_registry` / `ensure_installed` — regenerated.
    Install,
    /// The `relic_hook_channel!` macro def + invocations, `dispatch_*`
    /// wrappers, and channel payload types → `channels.rs`.
    Channels,
    /// Registry plumbing + free helpers → `helpers.rs`.
    Helper,
    /// `#[cfg(test)] mod tests` → `tests.rs`.
    Tests,
    /// `use` imports → `prelude.rs`.
    Imports,
    /// A top-level `pub use` re-export → emitted verbatim at `mod.rs` root
    /// (and also downgraded into the prelude for internal references).
    PubUse,
}

fn has_cfg_test(attrs: &[syn::Attribute]) -> bool {
    use quote::ToTokens;
    attrs
        .iter()
        .any(|a| a.path().is_ident("cfg") && a.to_token_stream().to_string().contains("test"))
}

/// Names that are channel plumbing (live in `channels.rs`). These are the
/// `dispatch_*` wrappers and channel payload-context types; the macro def +
/// invocations are detected structurally.
fn is_channel_fn_name(name: &str) -> bool {
    name.starts_with("dispatch_")
}

/// `true` for a struct/enum/type that is a channel payload context type. The
/// recon-named payloads all end in `Ctx`, `Kind`, or `Source`; conservatively
/// route the small known set into channels. Anything else stays in helpers.
fn is_channel_payload_type(name: &str) -> bool {
    name.ends_with("Ctx")
        || name.ends_with("Kind")
        || name.ends_with("Source")
        || name.ends_with("Props")
}

/// Classify a top-level item. `handler_to_id` maps a registered handler fn
/// name → its `<id_snake>` (built from the install statement list); this is
/// the authoritative classifier for handler fns.
pub fn classify_relic_item(
    item: &syn::Item,
    handler_to_id: &BTreeMap<String, String>,
) -> RelicBucket {
    match item {
        syn::Item::Use(u) => {
            // A top-level `pub use` is a crate-external re-export surface (e.g.
            // `relic_effects::Reward`, consumed by integration tests). Keep it
            // public at mod.rs root; a plain `use` is internal → prelude.
            if matches!(u.vis, syn::Visibility::Public(_)) {
                RelicBucket::PubUse
            } else {
                RelicBucket::Imports
            }
        }
        syn::Item::Mod(m) if has_cfg_test(&m.attrs) => RelicBucket::Tests,
        // The `relic_hook_channel!` macro definition itself.
        syn::Item::Macro(mac) if mac.mac.path.is_ident("macro_rules") => {
            // `macro_rules! relic_hook_channel { … }` — the ident sits in the
            // tokens; treat any macro_rules def in this file as channel
            // plumbing (the file has exactly one: relic_hook_channel).
            RelicBucket::Channels
        }
        // A bare `relic_hook_channel!( … );` invocation at item position.
        syn::Item::Macro(mac) if mac.mac.path.is_ident("relic_hook_channel") => {
            RelicBucket::Channels
        }
        syn::Item::Struct(s) if is_channel_payload_type(&s.ident.to_string()) => {
            RelicBucket::Channels
        }
        syn::Item::Enum(e) if is_channel_payload_type(&e.ident.to_string()) => {
            RelicBucket::Channels
        }
        syn::Item::Fn(f) => {
            let name = f.sig.ident.to_string();
            if name == "install_default_registry" || name == "ensure_installed" {
                RelicBucket::Install
            } else if let Some(id_snake) = handler_to_id.get(&name) {
                RelicBucket::Handler {
                    id_snake: id_snake.clone(),
                }
            } else if is_channel_fn_name(&name) {
                RelicBucket::Channels
            } else {
                RelicBucket::Helper
            }
        }
        _ => RelicBucket::Helper,
    }
}

// ---------------------------------------------------------------------------
// ATTRIBUTE — parse install_default_registry → per-id register statements
// ---------------------------------------------------------------------------

/// One register statement routed to a relic id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegStmt {
    /// Ordinal of this register-call in the closure body, used for global
    /// ordering. The legacy `register_many` array's elements each get a
    /// distinct (fractional-but-monotone) index so per-element order is kept.
    pub global_index: usize,
    /// `RelicId::X` literal text.
    pub id: String,
    /// `<id_snake>` derived from `id`.
    pub id_snake: String,
    /// Verbatim register-call text routed to this id's `install()`.
    pub text: String,
    /// The handler fn name (second arg, modulo `as <Fn>` cast).
    pub handler: String,
}

/// `RelicId::SnappingJaxfruit` → `snapping_jaxfruit`.
#[must_use]
pub fn id_to_snake(id_path: &str) -> String {
    let variant = id_path.rsplit("::").next().unwrap_or(id_path).trim();
    let mut out = String::with_capacity(variant.len() + 4);
    for (i, ch) in variant.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Locate the `install_default_registry` fn, descend into the
/// `INSTALL_ONCE.call_once(|| { … })` closure, and walk its statements in
/// source order, routing each `register_*` call to the relic id named in its
/// first argument. The legacy `register_many(&[ … ])` array is split per
/// element. Returns the routed statements in global order.
#[must_use]
pub fn parse_install_statements(install_fn: &syn::ItemFn, src: &str) -> Vec<RegStmt> {
    let closure_block = find_call_once_closure_block(install_fn)
        .expect("install_default_registry must contain INSTALL_ONCE.call_once(|| { … })");
    let mut out = Vec::new();
    let mut gi = 0usize;
    for stmt in &closure_block.stmts {
        let syn::Stmt::Expr(syn::Expr::Call(call), _) = stmt else {
            continue;
        };
        let fn_name = match &*call.func {
            syn::Expr::Path(p) => p
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default(),
            _ => continue,
        };
        if fn_name == "register_many" {
            // Legacy array — split per element, one single-element
            // register_many(&[…]) per id, preserving element order.
            for (id, handler) in register_many_elements(call) {
                let id_snake = id_to_snake(&id);
                let text = format!("register_many(&[({id}, {handler} as RelicEffectFn)]);");
                out.push(RegStmt {
                    global_index: gi,
                    id,
                    id_snake,
                    text,
                    handler,
                });
                gi += 1;
            }
        } else if fn_name.starts_with("register_") {
            // register_<channel>(RelicId::X, handler as <Fn>)
            let (id, handler) = per_channel_call_id_and_handler(call);
            let id_snake = id_to_snake(&id);
            let range = stmt.span().byte_range();
            let text = src[range.start..range.end].to_string();
            out.push(RegStmt {
                global_index: gi,
                id,
                id_snake,
                text,
                handler,
            });
            gi += 1;
        }
    }
    out
}

/// Find the block of the closure passed to `INSTALL_ONCE.call_once(|| { … })`.
fn find_call_once_closure_block(f: &syn::ItemFn) -> Option<&syn::Block> {
    use syn::visit::Visit;
    struct V<'ast>(Option<&'ast syn::Block>);
    impl<'ast> Visit<'ast> for V<'ast> {
        fn visit_expr_method_call(&mut self, c: &'ast syn::ExprMethodCall) {
            if c.method == "call_once" && self.0.is_none() {
                if let Some(syn::Expr::Closure(cl)) = c.args.first() {
                    if let syn::Expr::Block(b) = &*cl.body {
                        self.0 = Some(&b.block);
                    }
                }
            }
            syn::visit::visit_expr_method_call(self, c);
        }
    }
    let mut v = V(None);
    v.visit_item_fn(f);
    v.0
}

/// Extract `(RelicId::X, handler_fn_name)` from a per-channel register call.
fn per_channel_call_id_and_handler(call: &syn::ExprCall) -> (String, String) {
    use quote::ToTokens;
    let id = call
        .args
        .first()
        .map(|a| a.to_token_stream().to_string().replace(' ', ""))
        .unwrap_or_default();
    let handler = call
        .args
        .iter()
        .nth(1)
        .map(|a| match a {
            syn::Expr::Cast(c) => c.expr.to_token_stream().to_string(),
            other => other.to_token_stream().to_string(),
        })
        .unwrap_or_default()
        .replace(' ', "");
    (id, handler)
}

/// Split a `register_many(&[ (RelicId::A, fa as RelicEffectFn), … ])` call into
/// per-element `(RelicId::X, handler_fn_name)` pairs, preserving order.
fn register_many_elements(call: &syn::ExprCall) -> Vec<(String, String)> {
    use quote::ToTokens;
    let mut v = Vec::new();
    let Some(arg) = call.args.first() else {
        return v;
    };
    let arr = match arg {
        syn::Expr::Reference(r) => match &*r.expr {
            syn::Expr::Array(a) => a,
            _ => return v,
        },
        syn::Expr::Array(a) => a,
        _ => return v,
    };
    for elem in &arr.elems {
        if let syn::Expr::Tuple(t) = elem {
            if t.elems.len() < 2 {
                continue;
            }
            let id = t.elems[0].to_token_stream().to_string().replace(' ', "");
            let handler = match &t.elems[1] {
                syn::Expr::Cast(c) => c.expr.to_token_stream().to_string(),
                other => other.to_token_stream().to_string(),
            }
            .replace(' ', "");
            v.push((id, handler));
        }
    }
    v
}

// ---------------------------------------------------------------------------
// EMIT
// ---------------------------------------------------------------------------

/// Pure core: source text → {filename → file contents}. No disk IO.
#[must_use]
pub fn emit_tree_to_string_map(src: &str) -> BTreeMap<String, String> {
    let file = syn::parse_file(src).expect("relic_effects.rs must parse");

    // PASS 1: find install_default_registry, parse its statement list, build
    // the handler→id classifier map.
    let install_fn = file
        .items
        .iter()
        .find_map(|it| match it {
            syn::Item::Fn(f) if f.sig.ident == "install_default_registry" => Some(f),
            _ => None,
        })
        .expect("install_default_registry must exist");
    let reg_stmts = parse_install_statements(install_fn, src);
    let mut handler_to_id: BTreeMap<String, String> = BTreeMap::new();
    for rs in &reg_stmts {
        handler_to_id
            .entry(rs.handler.clone())
            .or_insert_with(|| rs.id_snake.clone());
    }

    // First-seen id order (drives cross-id install() call order) + per-id
    // statements in ascending global_index.
    let mut first_seen: Vec<String> = Vec::new();
    let mut by_id: BTreeMap<String, Vec<RegStmt>> = BTreeMap::new();
    for rs in &reg_stmts {
        if !by_id.contains_key(&rs.id_snake) {
            first_seen.push(rs.id_snake.clone());
        }
        by_id
            .entry(rs.id_snake.clone())
            .or_default()
            .push(rs.clone());
    }

    // PASS 2: classify + slice every top-level item.
    let mut imports: Vec<String> = Vec::new();
    let mut pub_uses: Vec<String> = Vec::new();
    let mut helpers: Vec<String> = Vec::new();
    let mut channels: Vec<String> = Vec::new();
    let mut tests: Vec<String> = Vec::new();
    // id_snake → handler slices
    let mut handler_slices: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for item in &file.items {
        let (s, e) = item_span_bytes(item);
        let slice = &src[s..e];
        match classify_relic_item(item, &handler_to_id) {
            RelicBucket::Imports => imports.push(rewrite_super_to(slice, RELIC_SUPER_TARGET)),
            RelicBucket::PubUse => {
                // Preserve internal availability (handler/helper/channel bodies
                // reference these names via the prelude glob) AND the crate-
                // external `pub use` surface at mod.rs root.
                let rewritten = rewrite_super_to(slice, RELIC_SUPER_TARGET);
                imports.push(rewritten.clone());
                pub_uses.push(rewritten);
            }
            RelicBucket::Helper => {
                let emitted = if accepts_visibility(item) {
                    bump_visibility(slice)
                } else {
                    slice.to_string()
                };
                helpers.push(rewrite_super_to(&emitted, RELIC_SUPER_TARGET));
            }
            RelicBucket::Channels => {
                // The macro def, its invocations, and payload types: relocate
                // verbatim (macro/impl items reject a vis qualifier; structs/
                // fns are already `pub`). Bump only when it accepts vis and is
                // not already public.
                let emitted = if accepts_visibility(item) {
                    bump_visibility(slice)
                } else {
                    slice.to_string()
                };
                let mut rewritten = rewrite_super_to(&emitted, RELIC_SUPER_TARGET);
                // `relic_hook_channel!` generates a module-private
                // `fn $registry_fn()`. Post-split, channels.rs and tests.rs are
                // siblings, so test-body calls to the generated registry
                // accessors are E0425-inaccessible. Widen the generated fn to
                // `pub(crate)` (visibility-only — runtime/digest unchanged) so
                // the `use super::channels::*` glob re-exports it into tests.rs.
                if rewritten.contains("macro_rules! relic_hook_channel") {
                    rewritten =
                        rewritten.replace("fn $registry_fn()", "pub(crate) fn $registry_fn()");
                }
                channels.push(rewritten);
            }
            RelicBucket::Handler { id_snake } => {
                handler_slices
                    .entry(id_snake)
                    .or_default()
                    .push(rewrite_super_to(
                        &bump_visibility(slice),
                        RELIC_SUPER_TARGET,
                    ));
            }
            RelicBucket::Tests => {
                // Strip the outer `mod tests { … }` wrapper; tests.rs *is* the
                // module. Its `super::*` already meant the relic_effects
                // module → do NOT rewrite.
                if let syn::Item::Mod(m) = item {
                    if let Some((brace, _)) = &m.content {
                        let open = brace.span.open().byte_range();
                        let close = brace.span.close().byte_range();
                        tests.push(src[open.end..close.start].to_string());
                    }
                }
            }
            RelicBucket::Install => { /* regenerated; original dropped */ }
        }
    }

    let mut out = BTreeMap::new();

    // prelude.rs
    let mut prelude = String::from(
        "//! @generated by xtask split-relics. Per-relic prelude.\n#![allow(clippy::wildcard_imports, unused_imports)]\n\n",
    );
    for i in &imports {
        prelude.push_str(&to_pub_crate_use(i));
        prelude.push('\n');
    }
    prelude.push_str("\npub(crate) use super::helpers::*;\npub(crate) use super::channels::*;\n");
    out.insert("prelude.rs".to_string(), prelude);

    // helpers.rs / channels.rs
    out.insert("helpers.rs".to_string(), wrap_module(&helpers));
    out.insert("channels.rs".to_string(), wrap_module(&channels));

    // per-id <id_snake>.rs — handler slices + a pub(crate) fn install().
    // A relic that is registered but whose handler body lives elsewhere
    // (none expected) still gets a file via its by_id key.
    let mut all_ids: BTreeSet<String> = BTreeSet::new();
    all_ids.extend(handler_slices.keys().cloned());
    all_ids.extend(by_id.keys().cloned());
    for id_snake in &all_ids {
        let mut body =
            String::from("//! @generated by xtask split-relics.\nuse super::prelude::*;\n\n");
        if let Some(slices) = handler_slices.get(id_snake) {
            for sl in slices {
                body.push_str(sl);
                body.push_str("\n\n");
            }
        }
        body.push_str("pub(crate) fn install() {\n");
        if let Some(stmts) = by_id.get(id_snake) {
            for st in stmts {
                body.push_str("    ");
                body.push_str(&st.text);
                body.push('\n');
            }
        }
        body.push_str("}\n");
        out.insert(format!("{id_snake}.rs"), body);
    }

    // tests.rs (only if present).
    if !tests.is_empty() {
        let mut t = String::from(
            "//! @generated by xtask split-relics. Moved test module.\n#![allow(clippy::wildcard_imports, unused_imports)]\nuse super::prelude::*;\nuse super::channels::*;\nuse super::helpers::*;\n",
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
        gen_mod_rs(&all_ids, &first_seen, &by_id, &pub_uses, !tests.is_empty()),
    );

    out
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
        "//! @generated by xtask split-relics.\n#![allow(unused_imports)]\nuse super::prelude::*;\n\n",
    );
    for sl in slices {
        s.push_str(sl);
        s.push_str("\n\n");
    }
    s
}

/// Regenerate `mod.rs`: module decls, the `pub use` re-export surface, and the
/// `call_once`-guarded top-level installer replaying each id's `install()` in
/// first-seen-id order.
fn gen_mod_rs(
    all_ids: &BTreeSet<String>,
    first_seen: &[String],
    by_id: &BTreeMap<String, Vec<RegStmt>>,
    pub_uses: &[String],
    has_tests: bool,
) -> String {
    let _ = by_id;
    let mut s = String::new();
    s.push_str("// @generated by xtask split-relics. DO NOT EDIT relic list by hand;\n");
    s.push_str("// re-run `cargo run -p xtask -- split-relics` after adding a relic handler.\n");
    s.push_str("//! Per-relic module aggregator.\n");
    s.push_str("#![allow(unused_imports)]\n\n");
    s.push_str("mod prelude;\npub mod helpers;\npub mod channels;\n");
    if has_tests {
        s.push_str("#[cfg(test)]\nmod tests;\n");
    }
    let mut ids: Vec<&String> = all_ids.iter().collect();
    ids.sort();
    for id in &ids {
        s.push_str(&format!("pub mod {};\n", raw_mod(id)));
    }
    s.push('\n');

    // Re-export the registry surface external callers expect at the module
    // root (sts2-content::relics::* and sts2-core integration tests call these
    // as relic_effects::<name>).
    s.push_str("// Re-export the registry surface external callers expect at the module root.\n");
    s.push_str(
        "pub use helpers::{register, register_many, lookup, dispatch, registered_count, RelicEffectFn};\n",
    );
    s.push_str(
        "// `pub use channels::*` re-exports every generated `register_<chan>`/\n// `dispatch_<chan>`/`<Chan>Fn` so `relic_effects::<name>` keeps resolving.\n",
    );
    s.push_str("pub use channels::*;\n\n");

    // Top-level `pub use` re-exports lifted verbatim from the monolith so
    // crate-external paths (e.g. `relic_effects::Reward`) keep resolving.
    for pu in pub_uses {
        s.push_str(pu);
        s.push('\n');
    }
    if !pub_uses.is_empty() {
        s.push('\n');
    }

    // call_once-guarded installer.
    s.push_str("/// Install every relic handler. Idempotent (guarded by INSTALL_ONCE).\n");
    s.push_str("pub fn install_default_registry() {\n");
    s.push_str("    helpers::INSTALL_ONCE.call_once(|| {\n");
    for id in first_seen {
        s.push_str(&format!("        {}::install();\n", raw_mod(id)));
    }
    s.push_str("    });\n");
    s.push_str("}\n\n");
    s.push_str("/// Idempotent alias for [`install_default_registry`].\n");
    s.push_str("pub fn ensure_installed() {\n    install_default_registry();\n}\n");
    s
}

// ---------------------------------------------------------------------------
// Disk driver
// ---------------------------------------------------------------------------

/// Run the split against the real `relic_effects.rs`. NOT exercised by the
/// unit tests (which use synthetic fixtures); applied under the determinism
/// lock in the APPLY phase.
pub fn run() {
    let base = std::path::Path::new("crates/sts2-core/src");
    let src_path = base.join("relic_effects.rs");
    let src = std::fs::read_to_string(&src_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", src_path.display()));

    let map = emit_tree_to_string_map(&src);

    let dir = base.join("relic_effects");
    std::fs::create_dir_all(&dir).expect("create relic_effects dir");
    for (name, contents) in &map {
        std::fs::write(dir.join(name), contents).expect("write file");
    }
    std::fs::remove_file(&src_path).expect("remove monolith");
    eprintln!(
        "split-relics: wrote {} files to {}",
        map.len(),
        dir.display()
    );
}
