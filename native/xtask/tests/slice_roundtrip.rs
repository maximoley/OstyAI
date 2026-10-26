use xtask_lib::split_cards::{bump_visibility, item_span_bytes};

const SRC: &str = "\
/// doc line
fn play_anger(x: i32) -> i32 { x + 1 }
";

#[test]
fn slice_captures_doc_comment_and_body() {
    let file = syn::parse_file(SRC).unwrap();
    let item = &file.items[0];
    let (start, end) = item_span_bytes(item);
    let slice = &SRC[start..end];
    assert!(slice.contains("/// doc line"));
    assert!(slice.trim_end().ends_with("{ x + 1 }"));
}

#[test]
fn bump_visibility_prefixes_private_fn() {
    assert_eq!(
        bump_visibility("fn play_anger() {}"),
        "pub(crate) fn play_anger() {}"
    );
    assert_eq!(
        bump_visibility("pub fn install_pool() {}"),
        "pub fn install_pool() {}"
    );
    assert_eq!(
        bump_visibility("/// d\nfn f(){}"),
        "/// d\npub(crate) fn f(){}"
    );
}

#[test]
fn bump_visibility_skips_plain_line_comment_in_doc_block() {
    // A plain `//` note interleaved in a `///` doc block must not stop the
    // visibility-insertion scan (regression: necrobinder play_drain_power).
    let slice = "/// doc one\n// Port-pin: foo@abc\n/// doc two\nfn play_x() {}";
    let out = bump_visibility(slice);
    assert!(out.contains("pub(crate) fn play_x"), "got: {out}");
    assert!(!out.contains("pub(crate) // Port-pin"));
    syn::parse_str::<syn::ItemFn>(&out).expect("bumped fn reparses");
}
