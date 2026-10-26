//! Extractor: parse a character card file and emit a per-card module tree.
pub fn run(character: &str) {
    let base = std::path::Path::new("crates/sts2-content/src/cards");
    let src_path = base.join(format!("{character}.rs"));
    let src = std::fs::read_to_string(&src_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", src_path.display()));

    let map = emit_tree_to_string_map(&src, character);

    let dir = base.join(character);
    std::fs::create_dir_all(&dir).expect("create char dir");
    for (name, contents) in &map {
        std::fs::write(dir.join(name), contents).expect("write file");
    }
    // Remove the original monolith only after the tree is written.
    std::fs::remove_file(&src_path).expect("remove monolith");
    eprintln!(
        "split {character}: wrote {} files to {}",
        map.len(),
        dir.display()
    );
}

use syn::spanned::Spanned;

/// Byte range covering an item INCLUDING its leading doc-comment attributes.
/// Requires the file to have been parsed via `syn::parse_file` with the
/// proc-macro2 `span-locations` feature on (gives real byte offsets).
pub fn item_span_bytes(item: &syn::Item) -> (usize, usize) {
    let self_range = item.span().byte_range();
    let mut start = self_range.start;
    let mut end = self_range.end;
    for attr in item_attrs(item) {
        let r = attr.span().byte_range();
        start = start.min(r.start);
        end = end.max(r.end);
    }
    (start, end)
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Fn(f) => &f.attrs,
        syn::Item::Const(c) => &c.attrs,
        syn::Item::Mod(m) => &m.attrs,
        syn::Item::Use(u) => &u.attrs,
        _ => &[],
    }
}

/// Prefix a `pub(crate)` visibility to an item slice that has none.
/// Operates on the text after any leading doc-comment lines.
pub fn bump_visibility(slice: &str) -> String {
    // Skip every leading comment line (`//`, which subsumes `///`/`//!`),
    // attribute (`#[`), and blank line until the item keyword. A plain `//`
    // line interleaved in a doc block (e.g. a `// Port-pin:` note between `///`
    // lines) must NOT stop the scan, or `pub(crate)` lands mid-doc-block and
    // syn rejects it (`expected fn`).
    let mut idx = 0usize;
    for line in slice.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with("#[") || t.is_empty() {
            idx += line.len();
        } else {
            break;
        }
    }
    let (head, tail) = slice.split_at(idx);
    if tail.trim_start().starts_with("pub") {
        slice.to_string()
    } else {
        // tail starts with `fn`/`const`/etc. on this line; insert before it.
        let lead_ws_len = tail.len() - tail.trim_start().len();
        let (ws, code) = tail.split_at(lead_ws_len);
        format!("{head}{ws}pub(crate) {code}")
    }
}

use crate::classify::{classify_item, Bucket};
use std::collections::BTreeMap;

/// Pure core: source text -> {filename -> file contents}. No disk IO.
pub fn emit_tree_to_string_map(src: &str, character: &str) -> BTreeMap<String, String> {
    let file = syn::parse_file(src).expect("card file must parse");

    let mut imports: Vec<String> = Vec::new();
    let mut helpers: Vec<String> = Vec::new();
    let mut specs: Vec<String> = Vec::new();
    let mut tests: Vec<String> = Vec::new();
    let mut cards: Vec<(String, String)> = Vec::new(); // (stem, slice)
                                                       // POOL entries in original order: (CardId::X, fn_name)
    let mut pool_entries: Vec<(String, String)> = Vec::new();
    // Module-root `pub`/`pub(crate)` items moved into helpers.rs that must stay
    // reachable at the pool root (e.g. `silent::silent_starter_deck`, or
    // colorless's `pub(crate)` `create_character_cards_into_arena` called from
    // other pools). Re-exported from the generated mod.rs with their original
    // visibility.
    let mut pub_helpers: Vec<(String, ReexportVis)> = Vec::new();
    // Names of fns relocated into helpers.rs — used to qualify any references
    // to them that survive in the regenerated `install_pool` body.
    let mut helper_fn_names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // Verbatim source of install_pool statements BEYOND `register_many(POOL)`
    // (e.g. regent's `register_many_late_effects(…)`), preserved so the
    // regenerated installer keeps every registration the monolith had.
    let mut install_pool_extra_raw = String::new();

    for item in &file.items {
        let (s, e) = item_span_bytes(item);
        let slice = &src[s..e];
        match classify_item(item) {
            // Top-level items lived directly under `cards`, so their `super::`
            // meant `crate::cards`. After relocation one level deeper they
            // must be rewritten to absolute `crate::cards::` (depth-independent).
            Bucket::Imports => imports.push(rewrite_super(slice)),
            Bucket::Helper => {
                // `impl`/`extern`/macro items can't take a visibility qualifier
                // (E0449) — relocate them verbatim; everything else is bumped.
                let emitted = if accepts_visibility(item) {
                    bump_visibility(slice)
                } else {
                    slice.to_string()
                };
                helpers.push(rewrite_super(&emitted));
                if let syn::Item::Fn(f) = item {
                    helper_fn_names.insert(f.sig.ident.to_string());
                }
                if let Some(name_vis) = pub_item_ident(item) {
                    pub_helpers.push(name_vis);
                }
            }
            Bucket::Specs => specs.push(rewrite_super(&bump_visibility(slice))),
            Bucket::Card { stem } => cards.push((stem, rewrite_super(&bump_visibility(slice)))),
            Bucket::Tests => {
                // The test module was nested ONE level under the pool module,
                // so its `super::*` already meant the pool module. Strip the
                // outer `mod tests { .. }` wrapper so `tests.rs` *is* that
                // module (depth-preserving) and DO NOT rewrite `super::`.
                if let syn::Item::Mod(m) = item {
                    if let Some((brace, _)) = &m.content {
                        let open = brace.span.open().byte_range();
                        let close = brace.span.close().byte_range();
                        tests.push(src[open.end..close.start].to_string());
                    }
                }
            }
            Bucket::Pool => {
                // POOL entries come from EITHER a top-level `const POOL`
                // (Ironclad) OR an inline `register_many(&[ … ])` call inside
                // `install_pool` (Silent and others). The const wins if both
                // exist. Any install_pool statements BEYOND the register_many
                // call (e.g. regent's register_many_late_effects) are preserved.
                match item {
                    syn::Item::Const(c) => pool_entries = parse_pool_entries(&c.expr),
                    syn::Item::Fn(f) if f.sig.ident == "install_pool" => {
                        if pool_entries.is_empty() {
                            if let Some(arr) = find_register_many_array(f) {
                                pool_entries = parse_pool_entries(arr);
                            }
                        }
                        install_pool_extra_raw = install_pool_extra_body(f, src);
                    }
                    _ => {}
                }
            }
        }
    }

    let mut out = BTreeMap::new();

    // prelude.rs — re-export every monolith import + the sibling helper/spec
    // modules so each per-card file gets them via `use super::prelude::*`.
    // Re-exports MUST be `pub(crate) use` (not `pub use`): some targets (the
    // generated spec modules) are `pub(crate)`, and a `pub use` of a less-
    // visible item is rejected (E0365).
    // `unused_imports` is allowed because the prelude re-exports the union of
    // the monolith's imports; any individual card file uses only a subset, and
    // some imports (e.g. PlayCardFn) are consumed only by the generated mod.rs.
    let mut prelude = String::from("//! @generated by xtask split-cards. Per-card prelude.\n#![allow(clippy::wildcard_imports, unused_imports)]\n\n");
    for i in &imports {
        prelude.push_str(&to_pub_crate_use(i));
        prelude.push('\n');
    }
    prelude.push_str("\npub(crate) use super::helpers::*;\npub(crate) use super::specs::*;\n");
    out.insert("prelude.rs".to_string(), prelude);

    // helpers.rs / specs.rs
    out.insert("helpers.rs".to_string(), wrap_module(&helpers));
    out.insert("specs.rs".to_string(), wrap_module(&specs));

    // card files
    use std::collections::BTreeSet;
    let all_card_fns: BTreeSet<String> = cards.iter().map(|(st, _)| format!("play_{st}")).collect();
    // Funcs actually registered in the POOL. A `play_*` card fn NOT in here is
    // an intentional unregistered placeholder (e.g. Silent's `play_marker`,
    // kept alive in the monolith by a `let _: PlayCardFn = play_marker;` in
    // install_pool, which the split drops) — mark it `#[allow(dead_code)]`.
    let registered: BTreeSet<&str> = pool_entries.iter().map(|(_, f)| f.as_str()).collect();

    for (stem, slice) in &cards {
        let own = format!("play_{stem}");
        let mut extra: Vec<String> = Vec::new();
        let parsed: syn::ItemFn = syn::parse_str(strip_vis(slice)).expect("card fn reparses");
        let mut found = BTreeSet::new();
        collect_called_idents(&parsed, &mut found);
        for f in &found {
            if f != &own && all_card_fns.contains(f) {
                let other_stem = f.strip_prefix("play_").unwrap();
                extra.push(format!("use super::{}::{f};", raw_mod(other_stem)));
            }
        }
        extra.sort();
        let body = if registered.contains(own.as_str()) {
            slice.clone()
        } else {
            format!("#[allow(dead_code)]\n{slice}")
        };
        out.insert(format!("{stem}.rs"), wrap_card(&body, &extra));
    }

    // tests.rs (only if present). The original test module called some
    // `play_*` fns directly (not only via the registry), so bring every
    // card module's items into scope. `unused_imports` is allowed because
    // only a subset is referenced.
    if !tests.is_empty() {
        let mut t = String::from("//! @generated by xtask split-cards. Moved test module.\n#![allow(clippy::wildcard_imports, unused_imports)]\nuse super::prelude::*;\nuse super::specs::*;\n");
        let mut stems: Vec<&String> = cards.iter().map(|(st, _)| st).collect();
        stems.sort();
        t.push_str("use super::{");
        for (i, st) in stems.iter().enumerate() {
            if i > 0 {
                t.push_str(", ");
            }
            t.push_str(&format!("{}::*", raw_mod(st)));
        }
        t.push_str("};\n");
        // Disambiguate prelude rename-aliases that collide with a sibling
        // module name (e.g. a monolith `use … as specs;` colliding with the
        // tool's own `pub mod specs;`): `use super::prelude::*` and
        // `use super::*` then both export that name → E0659. An explicit
        // import wins over both globs. Harmless when non-colliding
        // (`unused_imports` is allowed above).
        let mut renames = import_rename_idents(&imports);
        renames.sort();
        renames.dedup();
        for ident in &renames {
            t.push_str(&format!("use super::prelude::{ident};\n"));
        }
        t.push('\n');
        for slice in &tests {
            t.push_str(slice);
            t.push('\n');
        }
        out.insert("tests.rs".to_string(), t);
    }

    // install_pool extra statements: qualify references to relocated helper
    // fns (`make_it_so_late_effect` -> `helpers::make_it_so_late_effect`) and
    // card fns (`play_x` -> `x::play_x`). install_pool stays at the pool-module
    // level (cards::<char>), so `super::` paths in it are NOT rewritten.
    let install_pool_extra = if install_pool_extra_raw.trim().is_empty() {
        String::new()
    } else {
        qualify_install_pool_refs(&install_pool_extra_raw, &helper_fn_names, &all_card_fns)
    };

    // mod.rs
    out.insert(
        "mod.rs".to_string(),
        gen_mod_rs(
            character,
            &cards,
            &pool_entries,
            !tests.is_empty(),
            &pub_helpers,
            &install_pool_extra,
        ),
    );

    out
}

fn wrap_module(slices: &[String]) -> String {
    // `unused_imports` allowed: a pool may have no spec-sampler fns, leaving an
    // empty specs.rs whose `use super::prelude::*` would otherwise be unused.
    let mut s = String::from("//! @generated by xtask split-cards.\n#![allow(unused_imports)]\nuse super::prelude::*;\n\n");
    for sl in slices {
        s.push_str(sl);
        s.push_str("\n\n");
    }
    s
}

fn wrap_card(slice: &str, extra_uses: &[String]) -> String {
    let mut s = String::from("//! @generated by xtask split-cards.\nuse super::prelude::*;\n");
    for u in extra_uses {
        s.push_str(u);
        s.push('\n');
    }
    s.push('\n');
    s.push_str(slice);
    s.push('\n');
    s
}

fn strip_vis(slice: &str) -> &str {
    // reparse needs a standalone fn; the doc-comment + pub(crate) are fine for syn.
    slice
}

/// Rewrite `super::` path prefixes to absolute `crate::cards::`. Top-level
/// items in the monolith lived directly under `crate::cards`, so their
/// `super::` meant `cards`; after relocation into `cards/<char>/<item>.rs`
/// the relative path would resolve one level too shallow. Absolute paths are
/// depth-independent and unambiguous. (`super` is a keyword, so it can never
/// be a substring of an identifier — a plain textual replace is safe.)
pub fn rewrite_super(slice: &str) -> String {
    slice.replace("super::", "crate::cards::")
}

/// Convert a monolith `use …;` statement into a `pub(crate) use …;` re-export.
pub fn to_pub_crate_use(use_stmt: &str) -> String {
    // Skip leading comment/attribute lines (e.g. a `#[cfg(test)]` on its own
    // line) to find the `use`/`pub use` keyword, then bump it to `pub(crate)`
    // while preserving the prefix. Otherwise an attributed alias stays a private
    // `use` and any `super::prelude::<alias>` reference is E0603-inaccessible
    // (colorless: `#[cfg(test)] use … as sspecs;`, referenced from tests.rs).
    let mut idx = 0usize;
    for line in use_stmt.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with("#[") || t.is_empty() {
            idx += line.len();
        } else {
            break;
        }
    }
    let (head, tail) = use_stmt.split_at(idx);
    let lead_ws_len = tail.len() - tail.trim_start().len();
    let (ws, code) = tail.split_at(lead_ws_len);
    if code.starts_with("pub(crate)") {
        use_stmt.to_string()
    } else if let Some(rest) = code.strip_prefix("pub use") {
        format!("{head}{ws}pub(crate) use{rest}")
    } else if let Some(rest) = code.strip_prefix("use") {
        format!("{head}{ws}pub(crate) use{rest}")
    } else {
        use_stmt.to_string()
    }
}

/// Visibility to re-export a relocated module-root item with, so its original
/// `crate::cards::<char>::<name>` path keeps resolving from elsewhere.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReexportVis {
    Pub,
    PubCrate,
}

impl ReexportVis {
    fn keyword(self) -> &'static str {
        match self {
            ReexportVis::Pub => "pub use",
            ReexportVis::PubCrate => "pub(crate) use",
        }
    }
}

/// `true` for items that accept a leading visibility qualifier. `impl` blocks,
/// `extern {}` blocks and freestanding macro invocations do NOT (`pub(crate)
/// impl …` is `error[E0449]`), so they must be relocated verbatim. Only the
/// Helper bucket can carry such items (Card/Specs are always `fn`).
fn accepts_visibility(item: &syn::Item) -> bool {
    !matches!(
        item,
        syn::Item::Impl(_) | syn::Item::ForeignMod(_) | syn::Item::Macro(_)
    )
}

/// If `item` is a module-root item that was reachable as
/// `crate::cards::<char>::<name>` (i.e. `pub` or `pub(crate)`), return its
/// identifier and the visibility to re-export it with — these must be
/// re-exported from the generated `mod.rs` to preserve that path after
/// relocation into helpers.rs. Private (`fn`) items are reached by siblings via
/// the `super::helpers::*` glob and need no re-export.
fn pub_item_ident(item: &syn::Item) -> Option<(String, ReexportVis)> {
    fn vis_of(v: &syn::Visibility) -> Option<ReexportVis> {
        match v {
            syn::Visibility::Public(_) => Some(ReexportVis::Pub),
            syn::Visibility::Restricted(r) if r.path.is_ident("crate") => {
                Some(ReexportVis::PubCrate)
            }
            _ => None,
        }
    }
    match item {
        syn::Item::Fn(f) => vis_of(&f.vis).map(|v| (f.sig.ident.to_string(), v)),
        syn::Item::Const(c) => vis_of(&c.vis).map(|v| (c.ident.to_string(), v)),
        syn::Item::Static(s) => vis_of(&s.vis).map(|v| (s.ident.to_string(), v)),
        syn::Item::Struct(s) => vis_of(&s.vis).map(|v| (s.ident.to_string(), v)),
        syn::Item::Enum(e) => vis_of(&e.vis).map(|v| (e.ident.to_string(), v)),
        syn::Item::Type(t) => vis_of(&t.vis).map(|v| (t.ident.to_string(), v)),
        _ => None,
    }
}

/// Rust keywords that can legitimately appear as a `play_<stem>` stem and
/// would be invalid as a bare module identifier (e.g. `play_break` → `break`).
fn is_rust_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "yield"
    )
}

/// Module identifier for a card stem, raw-escaped if it is a keyword. The
/// on-disk file is still named `<stem>.rs` (Rust resolves `mod r#break` to
/// `break.rs`), so only the `mod`/path token is escaped.
pub fn raw_mod(stem: &str) -> String {
    if is_rust_keyword(stem) {
        format!("r#{stem}")
    } else {
        stem.to_string()
    }
}

fn collect_called_idents(f: &syn::ItemFn, out: &mut std::collections::BTreeSet<String>) {
    use syn::visit::{self, Visit};
    struct V<'a>(&'a mut std::collections::BTreeSet<String>);
    impl<'ast> Visit<'ast> for V<'_> {
        fn visit_path(&mut self, p: &'ast syn::Path) {
            if let Some(seg) = p.segments.last() {
                let id = seg.ident.to_string();
                if id.starts_with("play_") {
                    self.0.insert(id);
                }
            }
            visit::visit_path(self, p);
        }
    }
    V(out).visit_item_fn(f);
}

/// Collect the rename idents (`… as <ident>`) from a set of `use` statements.
/// These bindings can collide with sibling module names in `tests.rs`.
fn import_rename_idents(imports: &[String]) -> Vec<String> {
    fn walk(tree: &syn::UseTree, out: &mut Vec<String>) {
        match tree {
            syn::UseTree::Rename(r) => out.push(r.rename.to_string()),
            syn::UseTree::Path(p) => walk(&p.tree, out),
            syn::UseTree::Group(g) => g.items.iter().for_each(|t| walk(t, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for imp in imports {
        if let Ok(u) = syn::parse_str::<syn::ItemUse>(imp.trim()) {
            walk(&u.tree, &mut out);
        }
    }
    out
}

/// Find the `&[ … ]` array argument of a `register_many(&[ … ])` call inside
/// `install_pool` (the inline-registration form used by pools without a
/// top-level `const POOL`). Returns the array-reference expr for
/// `parse_pool_entries`.
fn find_register_many_array(f: &syn::ItemFn) -> Option<&syn::Expr> {
    use syn::visit::Visit;
    struct V<'ast>(Option<&'ast syn::Expr>);
    impl<'ast> Visit<'ast> for V<'ast> {
        fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
            let is_register_many = matches!(&*c.func, syn::Expr::Path(p)
                if p.path.segments.last().is_some_and(|s| s.ident == "register_many"));
            if is_register_many && self.0.is_none() {
                if let Some(arg @ syn::Expr::Reference(_)) = c.args.first() {
                    self.0 = Some(arg);
                }
            }
            syn::visit::visit_expr_call(self, c);
        }
    }
    let mut v = V(None);
    v.visit_item_fn(f);
    v.0
}

/// `true` if `stmt` is a `register_many(…)` call statement (exact last path
/// segment `register_many`, so `register_many_late_effects` does NOT match).
fn stmt_is_register_many(stmt: &syn::Stmt) -> bool {
    if let syn::Stmt::Expr(syn::Expr::Call(c), _) = stmt {
        if let syn::Expr::Path(p) = &*c.func {
            return p
                .path
                .segments
                .last()
                .is_some_and(|s| s.ident == "register_many");
        }
    }
    false
}

/// Verbatim source of every install_pool statement AFTER the `register_many`
/// call (e.g. regent's `register_many_late_effects(…)`), preserving comments
/// and indentation. The pipeline re-emits the `register_many(POOL)` line
/// itself, so it is excluded here. Empty when there are no extra statements.
fn install_pool_extra_body(f: &syn::ItemFn, src: &str) -> String {
    let block_range = f.block.span().byte_range();
    let close_pos = block_range.end.saturating_sub(1); // the `}`
    let mut after = None;
    for stmt in &f.block.stmts {
        if stmt_is_register_many(stmt) {
            after = Some(stmt.span().byte_range().end);
            break;
        }
    }
    let Some(after) = after else {
        return String::new();
    };
    src.get(after..close_pos).unwrap_or("").to_string()
}

/// Replace whole-word occurrences of `name` with `prefix`+`name`, skipping
/// occurrences that are already part of a larger identifier or path (preceded
/// by an identifier char or `:`, or followed by an identifier char).
fn qualify_ident(text: &str, name: &str, prefix: &str) -> String {
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i..].starts_with(name) {
            let before_ok = i == 0 || !(is_ident(bytes[i - 1]) || bytes[i - 1] == b':');
            let after = i + name.len();
            let after_ok = after >= text.len() || !is_ident(bytes[after]);
            if before_ok && after_ok {
                out.push_str(prefix);
                out.push_str(name);
                i = after;
                continue;
            }
        }
        let ch = text[i..].chars().next().expect("char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Qualify references in the preserved install_pool body: relocated helper fns
/// gain a `helpers::` prefix; card fns `play_x` become `x::play_x`. install_pool
/// lives in mod.rs (cards::<char>), so these submodule paths resolve.
fn qualify_install_pool_refs(
    extra: &str,
    helper_fns: &std::collections::BTreeSet<String>,
    card_fns: &std::collections::BTreeSet<String>,
) -> String {
    let mut s = extra.to_string();
    for name in helper_fns {
        s = qualify_ident(&s, name, "helpers::");
    }
    for name in card_fns {
        let stem = name.strip_prefix("play_").unwrap_or(name);
        s = qualify_ident(&s, name, &format!("{}::", raw_mod(stem)));
    }
    s
}

/// Parse the `&[ (CardId::X, play_x as PlayCardFn), ... ]` initializer into
/// (`CardId::X`, `play_x`) pairs, preserving order. Also accepts a bare array
/// literal `[ (PotionKind::X, drink_x as PotionEffectFn), … ]` (the potion
/// `let entries = [ … ]` form, which has no leading `&`).
pub fn parse_pool_entries(expr: &syn::Expr) -> Vec<(String, String)> {
    use quote::ToTokens;
    let mut v = Vec::new();
    // expr is `& [ tuple, tuple, ... ]` OR a bare `[ tuple, ... ]`.
    let arr_expr = match expr {
        syn::Expr::Reference(r) => &*r.expr,
        other => other,
    };
    {
        if let syn::Expr::Array(arr) = arr_expr {
            for elem in &arr.elems {
                if let syn::Expr::Tuple(t) = elem {
                    let id = t.elems[0].to_token_stream().to_string().replace(' ', "");
                    // second elem is `play_x as PlayCardFn`
                    let func = match &t.elems[1] {
                        syn::Expr::Cast(c) => c.expr.to_token_stream().to_string(),
                        other => other.to_token_stream().to_string(),
                    };
                    v.push((id, func.replace(' ', "")));
                }
            }
        }
    }
    v
}

fn gen_mod_rs(
    _character: &str,
    cards: &[(String, String)],
    pool: &[(String, String)],
    has_tests: bool,
    pub_helpers: &[(String, ReexportVis)],
    install_pool_extra: &str,
) -> String {
    let mut s = String::new();
    s.push_str("// @generated by xtask split-cards. DO NOT EDIT card list by hand;\n");
    s.push_str("// re-run `cargo run -p xtask -- split-cards <char>` after adding a card file.\n");
    s.push_str("//! Per-card module aggregator.\n");
    // Module-root re-exports below preserve every `crate::cards::<char>::<name>`
    // path from the monolith, including `pub(crate)` items only some of which
    // are referenced cross-pool — the unreferenced ones are intentionally
    // unused re-exports, so allow it on the generated aggregator.
    s.push_str("#![allow(unused_imports)]\n\n");
    s.push_str(
        "use sts2_core::card_effects::{self, PlayCardFn};\nuse sts2_core::cards::CardId;\n\n",
    );
    s.push_str("mod prelude;\npub mod helpers;\npub mod specs;\n");
    if has_tests {
        s.push_str("#[cfg(test)]\nmod tests;\n");
    }
    // sorted, stable module declarations
    let mut stems: Vec<&String> = cards.iter().map(|(st, _)| st).collect();
    stems.sort();
    for st in stems {
        s.push_str(&format!("pub mod {};\n", raw_mod(st)));
    }
    // Preserve the pool's public API: re-export module-root `pub`/`pub(crate)`
    // items that were relocated into helpers.rs (e.g. `<char>_starter_deck`, or
    // colorless's `pub(crate)` arena builders), so external
    // `crate::cards::<char>::<name>` paths keep resolving with their original
    // visibility.
    if !pub_helpers.is_empty() {
        let mut names: Vec<&(String, ReexportVis)> = pub_helpers.iter().collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        s.push('\n');
        for (name, vis) in names {
            s.push_str(&format!("{} helpers::{name};\n", vis.keyword()));
        }
    }
    s.push('\n');

    // installer — register_many(POOL) plus any extra registrations the
    // monolith's install_pool had (verbatim, helper refs qualified). cargo fmt
    // (run by the pipeline before the lock) normalizes the spliced indentation.
    s.push_str("/// Register every handler in this pool. Called from\n/// [`crate::cards::dispatch::install_default_registry`].\npub fn install_pool() {\n    card_effects::register_many(POOL);");
    if install_pool_extra.is_empty() {
        s.push_str("\n}\n\n");
    } else {
        s.push_str(install_pool_extra);
        s.push_str("\n}\n\n");
    }

    // POOL in original order
    s.push_str("#[allow(clippy::type_complexity)]\nconst POOL: &[(CardId, PlayCardFn)] = &[\n");
    for (id, func) in pool {
        // func is `play_anger`; module stem is func minus `play_`
        let stem = func.strip_prefix("play_").unwrap_or(func);
        s.push_str(&format!(
            "    ({id}, {}::{func} as PlayCardFn),\n",
            raw_mod(stem)
        ));
    }
    s.push_str("];\n");
    s
}
