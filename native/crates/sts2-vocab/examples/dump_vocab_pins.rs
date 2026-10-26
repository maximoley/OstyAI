//! Regenerate the pinned index-order files under
//! `crates/sts2-vocab/src/vocab_pins/`.
//!
//! Run from `sts2-rs/`:
//!
//! ```text
//! cargo run -p sts2-vocab --example dump_vocab_pins
//! ```
//!
//! The pins are what stop a content addition from re-numbering embedding rows
//! that a deployed champion already learned (GH #544). This tool only ever
//! **appends** ids the pin is missing — it never reorders or drops a line, so
//! running it can not invalidate a net. Review the diff, then commit it in the
//! same change as the content that added the ids.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/vocab_pins");
    fs::create_dir_all(&dir).expect("create vocab_pins dir");
    for kind in sts2_vocab::PINNED_KINDS {
        let path = dir.join(format!("{kind}.txt"));
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let pinned: Vec<&str> = sts2_vocab::parse_pin(&existing).collect();
        let already: BTreeSet<&str> = pinned.iter().copied().collect();
        let content =
            sts2_vocab::content_ids(kind).unwrap_or_else(|| panic!("unknown pinned kind {kind}"));
        let missing: Vec<&str> = content
            .iter()
            .copied()
            .filter(|id| !already.contains(id))
            .collect();

        let mut out = String::new();
        out.push_str(&header(kind));
        for id in pinned.iter().chain(missing.iter()) {
            out.push_str(id);
            out.push('\n');
        }
        fs::write(&path, out).expect("write pin file");
        println!(
            "{kind}: {} pinned, {} appended{}",
            pinned.len(),
            missing.len(),
            if missing.is_empty() {
                String::new()
            } else {
                format!(" ({})", missing.join(", "))
            }
        );
    }
}

fn header(kind: &str) -> String {
    format!(
        "# PINNED EMBEDDING-ROW ORDER for the `{kind}` PPO registry. APPEND-ONLY.\n\
         #\n\
         # Line N (comments and blanks skipped) is embedding row N. Reordering or\n\
         # deleting a line silently repoints a deployed champion's learned row at a\n\
         # different id -- the failure GH #544 documents. Adding content appends here;\n\
         # regenerate with `cargo run -p sts2-vocab --example dump_vocab_pins`.\n\
         #\n\
         # Ids present here but absent from the content are kept: the row stays\n\
         # unreachable rather than shifting every id after it.\n"
    )
}
