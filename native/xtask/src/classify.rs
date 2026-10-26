//! Pure classification of a card file's top-level items.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bucket {
    Card { stem: String },
    Helper,
    Specs,
    Pool,
    Tests,
    Imports,
}

fn has_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .any(|a| a.path().is_ident("cfg") && a.to_token_stream().to_string().contains("test"))
}

use quote::ToTokens;

pub fn classify_item(item: &syn::Item) -> Bucket {
    match item {
        syn::Item::Use(_) => Bucket::Imports,
        syn::Item::Mod(m) if has_cfg_test(&m.attrs) => Bucket::Tests,
        syn::Item::Const(c) if c.ident == "POOL" => Bucket::Pool,
        syn::Item::Fn(f) => {
            let name = f.sig.ident.to_string();
            if name == "install_pool" {
                Bucket::Pool
            } else if let Some(stem) = name.strip_prefix("play_") {
                Bucket::Card {
                    stem: stem.to_string(),
                }
            } else if name.ends_with("_specs")
                || (name.starts_with("random_") && name.ends_with("_spec"))
            {
                Bucket::Specs
            } else {
                Bucket::Helper
            }
        }
        _ => Bucket::Helper,
    }
}
