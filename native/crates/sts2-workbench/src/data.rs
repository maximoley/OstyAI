//! The workbench's embedded runtime data.
//!
//! The Python workbench reads its display data from the filesystem at
//! startup — the vendored `sts2-pycli/localization_eng/*.json` dumps, the
//! sibling wiki's `sts2-wiki/data/v0.107.1/` manifests and the art manifest
//! `scripts/build_asset_manifest.py` derives from the extracted art tree.
//! In the browser there is no filesystem, so this crate `include_str!`s a
//! bundle of exactly those inputs instead, produced by
//! `sts2-pycli/scripts/bundle_workbench_data.py` (`just workbench-data`;
//! `just check-workbench-data` fails on drift).
//!
//! The game-derived files live under `crates/sts2-workbench/data/game/`
//! (extracted from a game install — never published; `just import-game`
//! regenerates them) and are staged by `build.rs`, which substitutes empty
//! stubs when the directory is absent so the crate always compiles. Tests
//! that assert on real game text are gated on `cfg(workbench_game_data)`.
//! Only the curated `data/card_tags.json` is unconditionally committed.
//!
//! Layout of `crates/sts2-workbench/data/game/`:
//!
//! * `localization_eng/*.json` — byte-for-byte copies of the game's flat
//!   `{"KEY.field": "string"}` localization dumps (plus `BUILD.txt`, the
//!   game build they were vendored from).
//! * `wiki_cards.json` / `wiki_monsters.json` / `wiki_owned.json` — the wiki
//!   manifests trimmed to the fields the Python loaders read
//!   (`localization.load_card_localization` / `load_card_meta` /
//!   `load_monster_hp_ranges` / `load_owned_localization`).
//! * `asset_manifest.json` — the sorted list of art (+ audio) paths that
//!   exist in the art store; [`crate::assets::AssetResolver`] runs in
//!   manifest mode against it.
//! * `creature_scales.json` — per-creature authored scene scale + Bounds box.
//!
//! Everything is parsed once, lazily, into [`Data`] (see [`data`]).

use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

/// The game build the engine simulates and every bundled file describes.
/// Mirrors `workbench.localization._ENGINE_WIKI_VERSION`; flip in lockstep
/// when the engine pin moves.
pub const ENGINE_WIKI_VERSION: &str = "v0.111.0";

/// A flat `{"KEY.field": "string"}` localization dump.
pub type LocMap = BTreeMap<String, String>;

/// One creature's authored scene scale + Bounds box
/// (`{"scale", "bounds_w", "bounds_h"}`; a bound can be `null`).
///
/// Kept as a map rather than a struct so the JSON round-trips with exactly
/// the keys the file carries — the Python resolver hands the dict through
/// untouched and the frontend reads it.
pub type CreatureScale = BTreeMap<String, Option<f64>>;

/// One keyword-only upgrade delta of a wiki card (`upgrades[]` entry with a
/// `keyword`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct WikiCardUpgrade {
    #[serde(default)]
    pub keyword: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
}

/// One template variable of a wiki card (`vars[]` entry).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct WikiCardVar {
    #[serde(rename = "type", default)]
    pub var_type: Option<String>,
    #[serde(default)]
    pub base_value: Option<serde_json::Number>,
    #[serde(default)]
    pub upgraded_value: Option<serde_json::Number>,
}

/// A wiki `cards.json` entry, trimmed to the fields the loaders read.
///
/// Every field but `loc_key` is optional and defaults to absent, so a
/// hand-written test fixture can be as sparse as the Python ones — the
/// loaders apply the same truthiness rules as the Python code.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct WikiCard {
    #[serde(default)]
    pub loc_key: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub character: Option<String>,
    #[serde(default)]
    pub star_cost: Option<i64>,
    #[serde(default)]
    pub x_star_cost: Option<bool>,
    #[serde(default)]
    pub description_template: Option<String>,
    #[serde(default)]
    pub description_plain: Option<String>,
    #[serde(default)]
    pub upgraded_description_plain: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub upgrades: Vec<WikiCardUpgrade>,
    #[serde(default)]
    pub vars: Vec<WikiCardVar>,
    #[serde(rename = "type", default)]
    pub card_type: Option<String>,
    #[serde(default)]
    pub rarity: Option<String>,
    #[serde(default)]
    pub energy_cost: Option<i64>,
    #[serde(default)]
    pub upgraded_cost: Option<i64>,
}

/// A wiki `monsters.json` row, trimmed to the HP band fields.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct WikiMonster {
    pub class_name: String,
    #[serde(default)]
    pub min_hp: Option<i64>,
    #[serde(default)]
    pub max_hp: Option<i64>,
    #[serde(default)]
    pub min_hp_base: Option<i64>,
    #[serde(default)]
    pub max_hp_base: Option<i64>,
    #[serde(default)]
    pub hp_ascension: Option<i64>,
}

/// A wiki per-relic / per-potion file, trimmed to what
/// `load_owned_localization` reads (relics first, then potions, each sorted
/// by file name).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct WikiOwned {
    #[serde(default)]
    pub class_name: String,
    #[serde(default)]
    pub description: String,
}

/// A flat JSON object of strings read in DOCUMENT order.
///
/// `serde_json`'s map type sorts its keys, but `sts2_hint.canonical_maps`
/// builds its title maps by iterating `powers.json` in file order with
/// first-wins / last-wins collision rules, so the order has to survive.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrderedPairs(pub Vec<(String, String)>);

impl<'de> Deserialize<'de> for OrderedPairs {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PairsVisitor;

        impl<'de> Visitor<'de> for PairsVisitor {
            type Value = OrderedPairs;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a flat JSON object whose values are strings")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut out = Vec::with_capacity(map.size_hint().unwrap_or(0));
                while let Some((key, value)) = map.next_entry::<String, String>()? {
                    out.push((key, value));
                }
                Ok(OrderedPairs(out))
            }
        }

        deserializer.deserialize_map(PairsVisitor)
    }
}

/// Every bundled input, parsed.
#[derive(Clone, Debug, Default)]
pub struct Data {
    /// First line of `localization_eng/BUILD.txt` — the vendored build.
    pub loc_build: String,
    /// `localization_eng/cards.json`.
    pub loc_cards: LocMap,
    /// `localization_eng/card_keywords.json`.
    pub loc_card_keywords: LocMap,
    /// `localization_eng/afflictions.json`.
    pub loc_afflictions: LocMap,
    /// `localization_eng/rest_site_ui.json`.
    pub loc_rest_site_ui: LocMap,
    /// `localization_eng/relics.json`.
    pub loc_relics: LocMap,
    /// `localization_eng/potions.json`.
    pub loc_potions: LocMap,
    /// `localization_eng/characters.json`.
    pub loc_characters: LocMap,
    /// `localization_eng/events.json`.
    pub loc_events: LocMap,
    /// `localization_eng/monsters.json`.
    pub loc_monsters: LocMap,
    /// `localization_eng/orbs.json`.
    pub loc_orbs: LocMap,
    /// `localization_eng/powers.json`, in file order (see [`OrderedPairs`]).
    pub loc_powers: OrderedPairs,
    /// Trimmed wiki `cards.json`, in manifest order.
    pub wiki_cards: Vec<WikiCard>,
    /// Trimmed wiki `monsters.json`, in manifest order.
    pub wiki_monsters: Vec<WikiMonster>,
    /// Trimmed wiki `relics/*.json` + `potions/*.json`.
    pub wiki_owned: Vec<WikiOwned>,
    /// Sorted relative paths of every file in the art store.
    pub asset_manifest: Vec<String>,
    /// `creature_scales.json`, keyed by creature-visual slug.
    pub creature_scales: BTreeMap<String, CreatureScale>,
    /// `card_db.json` — the deck analyzer's mechanical card stats, keyed by
    /// loc-key (`workbench/deck_analysis.py`).
    pub deck_card_db: BTreeMap<String, serde_json::Value>,
    /// `card_tags.json` — the deck analyzer's curated role / velocity tags
    /// (its `_meta` entry dropped, as the Python loader does).
    pub deck_card_tags: BTreeMap<String, serde_json::Value>,
}

/// A game-derived bundle file, staged by `build.rs` into `$OUT_DIR/game/`
/// (a copy of `data/game/<name>` when present, an empty stub otherwise).
macro_rules! bundled {
    ($name:literal) => {
        include_str!(concat!(env!("OUT_DIR"), "/game/", $name))
    };
}

/// A curated (non-game) file committed under `data/`.
macro_rules! curated {
    ($name:literal) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/", $name))
    };
}

fn parse<T: for<'de> Deserialize<'de>>(name: &str, text: &str) -> T {
    serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("sts2-workbench bundle `data/{name}` is malformed: {e}"))
}

impl Data {
    /// Parse the committed bundle. Prefer [`data`], which does this once.
    #[must_use]
    pub fn load_bundled() -> Self {
        Self {
            loc_build: bundled!("localization_eng/BUILD.txt")
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .to_string(),
            loc_cards: parse(
                "localization_eng/cards.json",
                bundled!("localization_eng/cards.json"),
            ),
            loc_card_keywords: parse(
                "localization_eng/card_keywords.json",
                bundled!("localization_eng/card_keywords.json"),
            ),
            loc_afflictions: parse(
                "localization_eng/afflictions.json",
                bundled!("localization_eng/afflictions.json"),
            ),
            loc_rest_site_ui: parse(
                "localization_eng/rest_site_ui.json",
                bundled!("localization_eng/rest_site_ui.json"),
            ),
            loc_relics: parse(
                "localization_eng/relics.json",
                bundled!("localization_eng/relics.json"),
            ),
            loc_potions: parse(
                "localization_eng/potions.json",
                bundled!("localization_eng/potions.json"),
            ),
            loc_characters: parse(
                "localization_eng/characters.json",
                bundled!("localization_eng/characters.json"),
            ),
            loc_events: parse(
                "localization_eng/events.json",
                bundled!("localization_eng/events.json"),
            ),
            loc_monsters: parse(
                "localization_eng/monsters.json",
                bundled!("localization_eng/monsters.json"),
            ),
            loc_orbs: parse(
                "localization_eng/orbs.json",
                bundled!("localization_eng/orbs.json"),
            ),
            loc_powers: parse(
                "localization_eng/powers.json",
                bundled!("localization_eng/powers.json"),
            ),
            wiki_cards: parse("wiki_cards.json", bundled!("wiki_cards.json")),
            wiki_monsters: parse("wiki_monsters.json", bundled!("wiki_monsters.json")),
            wiki_owned: parse("wiki_owned.json", bundled!("wiki_owned.json")),
            asset_manifest: parse("asset_manifest.json", bundled!("asset_manifest.json")),
            creature_scales: parse("creature_scales.json", bundled!("creature_scales.json")),
            deck_card_db: parse("card_db.json", bundled!("card_db.json")),
            deck_card_tags: {
                let mut tags: BTreeMap<String, serde_json::Value> =
                    parse("card_tags.json", curated!("card_tags.json"));
                tags.remove("_meta");
                tags
            },
        }
    }
}

/// The parsed bundle, built on first use and shared thereafter.
pub fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(Data::load_bundled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(workbench_game_data)]
    #[test]
    fn bundle_parses_and_is_the_pinned_build() {
        let d = data();
        assert_eq!(d.loc_build, ENGINE_WIKI_VERSION);
        assert!(!d.loc_cards.is_empty());
        assert!(!d.wiki_cards.is_empty());
        assert!(!d.wiki_monsters.is_empty());
        assert!(!d.wiki_owned.is_empty());
        assert!(!d.asset_manifest.is_empty());
        assert!(!d.creature_scales.is_empty());
        assert!(!d.loc_powers.0.is_empty());
        // The art manifest is what the resolver's existence checks run
        // against; it is sorted so lookups by prefix are deterministic.
        assert!(d.asset_manifest.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn ordered_pairs_keep_document_order() {
        let pairs: OrderedPairs = serde_json::from_str(r#"{"Z.title":"z","A.title":"a"}"#).unwrap();
        assert_eq!(
            pairs.0,
            vec![
                ("Z.title".to_string(), "z".to_string()),
                ("A.title".to_string(), "a".to_string())
            ]
        );
    }

    #[test]
    fn wiki_card_fixture_can_be_sparse() {
        let cards: Vec<WikiCard> =
            serde_json::from_value(serde_json::json!([{"loc_key": "BASH", "title": "Bash"}]))
                .unwrap();
        assert_eq!(cards[0].loc_key, "BASH");
        assert_eq!(cards[0].title.as_deref(), Some("Bash"));
        assert!(cards[0].vars.is_empty());
    }
}
