//! Versioned feature schema for the M3 pick-ranker net (full-run-AI
//! plan §5/M3; bead `sts2-rs-40ikw.7`).
//!
//! The ranker's context encoder consumes a fixed-layout vector built
//! from [`VisibleRunView`] ONLY (§2.5 hard rule):
//!
//! - **Deck histogram** — one slot pair per [`CardId`] in a stable
//!   index (all `CardId::all()` entries sorted by Debug name):
//!   `[count, upgraded_count]` interleaved, `2 × n_cards()` wide.
//! - **Relic multi-hot** — one count slot per [`RelicId`] in the same
//!   stable-sort index, `n_relics()` wide.
//! - **Scalars** — [`SCALAR_NAMES`] (hp/gold/floor/act/…)
//!   followed by a boss-identity one-hot over the act-boss lead kinds
//!   ([`boss_kind_table`]), `n_scalars()` wide. Only this block is
//!   mean/std-normalized at inference (the histogram blocks are raw
//!   counts).
//!
//! Candidates are encoded as a card-entry index into the SAME card
//! table (plus a "skip"/null pseudo-row at index `n_cards()`), an
//! upgraded flag, and a mutation-kind one-hot ([`CandKind`]).
//!
//! # Schema pinning — [`feature_schema_id`]
//!
//! Rust inference and the Python trainer must never silently disagree
//! on the layout. The schema id is `FEATURE_SCHEMA_VERSION` plus a
//! 16-hex FNV-1a digest over the full ordered card list, relic list,
//! scalar names, boss-kind names, and candidate-kind names. Weight
//! files carry it; the net loaders ([`crate::eval::run_value::ValueNet`],
//! [`crate::eval::surrogate::SurrogateNet`]) refuse to load on mismatch. Any content change that shifts an index (new card,
//! new relic, new boss) changes the id automatically — and the golden
//! test below fails so the drift is noticed at build time, not at
//! sweep time.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use sts2_core::cards::CardId;
use sts2_core::{MonsterKind, RelicId};

use crate::eval::card_stats::card_stats;
use crate::rollout::CandidateMutation;
use crate::view::VisibleRunView;

/// Human-readable schema version prefix. Bump on any deliberate layout
/// change; the digest suffix catches accidental ones.
pub const FEATURE_SCHEMA_VERSION: &str = "sts2-runai-ranker-v1";

/// Number of candidate scalar features: `[upgraded, kind one-hot × 4]`.
pub const N_CAND_FEAT: usize = 5;

/// Fixed scalar feature names (pre-boss-one-hot block). Order is part
/// of the schema id.
pub const SCALAR_NAMES: &[&str] = &[
    "hp",
    "max_hp",
    "hp_frac",
    "gold",
    "gold_sqrt",
    "act_index",
    "act_floor",
    "act_progress",
    "boss_proximity",
    "ascension",
    "deck_size",
    "deck_upgraded",
    "deck_curses",
    "potion_count",
    "max_potion_slots",
    "relic_count",
];

/// Stable card index: every [`CardId`] sorted by Debug name. Sorted
/// (not enum-order) so the index survives enum-variant reordering;
/// additions still bump the schema id, by design.
pub fn card_index_table() -> &'static [CardId] {
    static TABLE: OnceLock<Vec<CardId>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut v: Vec<CardId> = CardId::all().to_vec();
        v.sort_by_key(|c| format!("{c:?}"));
        v
    })
}

/// Stable relic index (same construction as [`card_index_table`]).
pub fn relic_index_table() -> &'static [RelicId] {
    static TABLE: OnceLock<Vec<RelicId>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut v: Vec<RelicId> = RelicId::all().to_vec();
        v.sort_by_key(|r| format!("{r:?}"));
        v
    })
}

/// Boss-identity one-hot universe: the distinct lead [`MonsterKind`]s
/// (first roster entry — the `PinnedActBoss::lead_kind` convention,
/// `run_callbacks.rs`) of every Boss-tier encounter across the three
/// act tables, sorted by Debug name.
pub fn boss_kind_table() -> &'static [MonsterKind] {
    static TABLE: OnceLock<Vec<MonsterKind>> = OnceLock::new();
    TABLE.get_or_init(|| {
        use sts2_content::encounters::EncounterTier;
        let tables = [
            sts2_content::encounters::act1_overgrowth::ACT1_ENCOUNTERS,
            sts2_content::encounters::act2_hive::ACT2_ENCOUNTERS,
            sts2_content::encounters::act3_glory::ACT3_ENCOUNTERS,
        ];
        let mut kinds: Vec<MonsterKind> = tables
            .iter()
            .flat_map(|t| t.iter())
            .filter(|e| e.tier == EncounterTier::Boss)
            .filter_map(|e| e.monsters.first().map(|&(k, _)| k))
            .collect();
        kinds.sort_by_key(|k| format!("{k:?}"));
        kinds.dedup();
        kinds
    })
}

fn card_index_lookup() -> &'static std::collections::BTreeMap<CardId, u32> {
    static MAP: OnceLock<std::collections::BTreeMap<CardId, u32>> = OnceLock::new();
    MAP.get_or_init(|| {
        card_index_table()
            .iter()
            .enumerate()
            .map(|(i, &c)| (c, i as u32))
            .collect()
    })
}

fn relic_index_lookup() -> &'static std::collections::BTreeMap<RelicId, u32> {
    static MAP: OnceLock<std::collections::BTreeMap<RelicId, u32>> = OnceLock::new();
    MAP.get_or_init(|| {
        relic_index_table()
            .iter()
            .enumerate()
            .map(|(i, &r)| (r, i as u32))
            .collect()
    })
}

/// Stable index of `card_id` in the schema's card universe.
#[must_use]
pub fn card_index_of(card_id: CardId) -> Option<u32> {
    card_index_lookup().get(&card_id).copied()
}

/// Stable index of `relic_id` in the schema's relic universe.
#[must_use]
pub fn relic_index_of(relic_id: RelicId) -> Option<u32> {
    relic_index_lookup().get(&relic_id).copied()
}

/// Card-universe width (deck histogram slots; the candidate embedding
/// table has `n_cards() + 1` rows — the extra row is the skip/null
/// pseudo-candidate).
#[must_use]
pub fn n_cards() -> usize {
    card_index_table().len()
}

/// Relic multi-hot width.
#[must_use]
pub fn n_relics() -> usize {
    relic_index_table().len()
}

/// Scalar-block width: fixed scalars + boss one-hot.
#[must_use]
pub fn n_scalars() -> usize {
    SCALAR_NAMES.len() + boss_kind_table().len()
}

/// Total dense context width: `2·n_cards + n_relics + n_scalars`.
#[must_use]
pub fn context_dim() -> usize {
    2 * n_cards() + n_relics() + n_scalars()
}

/// FNV-1a 64 (matches `rollout::fnv1a`; local copy keeps the schema
/// module dependency-free for review).
fn fnv1a(h: u64, s: &str) -> u64 {
    let mut h = h;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// The pinned schema id: version prefix + digest over the complete
/// ordered universes. See module docs.
pub fn feature_schema_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv1a(h, "cards\0");
        for c in card_index_table() {
            h = fnv1a(h, &format!("{c:?}\0"));
        }
        h = fnv1a(h, "relics\0");
        for r in relic_index_table() {
            h = fnv1a(h, &format!("{r:?}\0"));
        }
        h = fnv1a(h, "scalars\0");
        for s in SCALAR_NAMES {
            h = fnv1a(h, &format!("{s}\0"));
        }
        h = fnv1a(h, "bosses\0");
        for k in boss_kind_table() {
            h = fnv1a(h, &format!("{k:?}\0"));
        }
        h = fnv1a(h, "candkinds\0add\0remove\0upgrade\0skip\0");
        format!("{FEATURE_SCHEMA_VERSION}:{h:016x}")
    })
}

/// Candidate mutation kind (one-hot block of the candidate features).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CandKind {
    Add,
    Remove,
    Upgrade,
    Skip,
}

impl CandKind {
    /// One-hot position within the candidate feature vector (after the
    /// upgraded flag).
    #[must_use]
    pub fn one_hot_pos(self) -> usize {
        match self {
            CandKind::Add => 0,
            CandKind::Remove => 1,
            CandKind::Upgrade => 2,
            CandKind::Skip => 3,
        }
    }
}

/// One rankable candidate: card-entry index (`n_cards()` = the
/// skip/null embedding row), upgraded flag, mutation kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateFeatures {
    pub card_idx: u32,
    pub upgraded: bool,
    pub kind: CandKind,
}

impl CandidateFeatures {
    /// Encode a [`CandidateMutation`] (the tier-2/labeler candidate
    /// unit). Returns `None` for mutations the ranker does not cover
    /// (relics / gold / potions) — callers fall back to the heuristic
    /// for those decisions. `view` resolves deck indices to card ids.
    /// `TransformDeck` candidates arrive as `RemoveCard` (the M1
    /// "remove + unknown replacement" framing) and encode as `Remove`.
    #[must_use]
    pub fn from_mutation(m: &CandidateMutation, view: &VisibleRunView) -> Option<Self> {
        match *m {
            CandidateMutation::Baseline => Some(Self {
                card_idx: n_cards() as u32,
                upgraded: false,
                kind: CandKind::Skip,
            }),
            CandidateMutation::AddCard { card_id, upgraded } => Some(Self {
                card_idx: card_index_of(card_id)?,
                upgraded,
                kind: CandKind::Add,
            }),
            CandidateMutation::RemoveCard { deck_idx } => {
                let card = view.deck.get(deck_idx)?;
                Some(Self {
                    card_idx: card_index_of(card.card_id)?,
                    upgraded: card.upgraded,
                    kind: CandKind::Remove,
                })
            }
            CandidateMutation::UpgradeCard { deck_idx } => {
                let card = view.deck.get(deck_idx)?;
                Some(Self {
                    card_idx: card_index_of(card.card_id)?,
                    upgraded: card.upgraded,
                    kind: CandKind::Upgrade,
                })
            }
            CandidateMutation::AddRelic { .. }
            | CandidateMutation::GoldDelta { .. }
            | CandidateMutation::AddPotion { .. }
            | CandidateMutation::HealHp { .. } => None,
        }
    }

    /// Dense candidate scalar vector: `[upgraded, kind one-hot × 4]`.
    #[must_use]
    pub fn feat_vec(&self) -> [f32; N_CAND_FEAT] {
        let mut v = [0.0; N_CAND_FEAT];
        v[0] = f32::from(u8::from(self.upgraded));
        v[1 + self.kind.one_hot_pos()] = 1.0;
        v
    }
}

/// Sparse context features of one decision (the export/training row
/// form; densify via [`ContextFeatures::write_dense`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContextFeatures {
    /// `(card_idx, count, upgraded_count)`, ascending `card_idx`.
    pub deck: Vec<(u32, f32, f32)>,
    /// `(relic_idx, count)`, ascending `relic_idx`.
    pub relics: Vec<(u32, f32)>,
    /// Dense scalar block, `n_scalars()` long (see [`SCALAR_NAMES`] +
    /// boss one-hot tail). NOT normalized — the net's stored
    /// `scalar_mean`/`scalar_std` apply at forward time.
    pub scalars: Vec<f32>,
}

impl ContextFeatures {
    /// Extract from a [`VisibleRunView`] (§2.5: player-visible only).
    #[must_use]
    pub fn from_view(view: &VisibleRunView) -> Self {
        let mut deck_map: std::collections::BTreeMap<u32, (f32, f32)> =
            std::collections::BTreeMap::new();
        let mut curses = 0.0_f32;
        let mut upgraded_total = 0.0_f32;
        for card in &view.deck {
            if card_stats(card.card_id, card.upgraded).curse_like {
                curses += 1.0;
            }
            if card.upgraded {
                upgraded_total += 1.0;
            }
            if let Some(idx) = card_index_of(card.card_id) {
                let e = deck_map.entry(idx).or_insert((0.0, 0.0));
                e.0 += 1.0;
                if card.upgraded {
                    e.1 += 1.0;
                }
            }
        }
        let deck: Vec<(u32, f32, f32)> =
            deck_map.into_iter().map(|(i, (c, u))| (i, c, u)).collect();

        let mut relic_map: std::collections::BTreeMap<u32, f32> = std::collections::BTreeMap::new();
        for r in &view.relics {
            if let Some(idx) = relic_index_of(*r) {
                *relic_map.entry(idx).or_insert(0.0) += 1.0;
            }
        }
        let relics: Vec<(u32, f32)> = relic_map.into_iter().collect();

        let hp_frac = if view.max_hp <= 0 {
            0.0
        } else {
            (view.hp as f32 / view.max_hp as f32).clamp(0.0, 1.0)
        };
        let mut scalars = Vec::with_capacity(n_scalars());
        scalars.extend_from_slice(&[
            view.hp as f32,
            view.max_hp as f32,
            hp_frac,
            view.gold as f32,
            (view.gold.max(0) as f32).sqrt(),
            f32::from(view.act_index),
            view.act_floor as f32,
            view.act_progress() as f32,
            view.boss_proximity() as f32,
            f32::from(view.ascension),
            view.deck.len() as f32,
            upgraded_total,
            curses,
            view.potions.len() as f32,
            view.max_potion_slots as f32,
            view.relics.len() as f32,
        ]);
        debug_assert_eq!(scalars.len(), SCALAR_NAMES.len());
        for kind in boss_kind_table() {
            let hot = view.boss.is_some_and(|b| b.lead_kind == *kind);
            scalars.push(if hot { 1.0 } else { 0.0 });
        }

        Self {
            deck,
            relics,
            scalars,
        }
    }

    /// Densify into `out` (length [`context_dim`]): deck pairs, relic
    /// counts, raw scalar block. The layout the trainer mirrors.
    pub fn write_dense(&self, out: &mut [f32]) {
        assert_eq!(out.len(), context_dim(), "dense buffer width");
        out.fill(0.0);
        let nc = n_cards();
        for &(idx, count, upgraded) in &self.deck {
            let i = idx as usize;
            if i < nc {
                out[2 * i] = count;
                out[2 * i + 1] = upgraded;
            }
        }
        let relic_base = 2 * nc;
        let nr = n_relics();
        for &(idx, count) in &self.relics {
            let i = idx as usize;
            if i < nr {
                out[relic_base + i] = count;
            }
        }
        let scalar_base = relic_base + nr;
        for (i, &s) in self.scalars.iter().enumerate().take(n_scalars()) {
            out[scalar_base + i] = s;
        }
    }
}

#[cfg(test)]
// Exact float comparison is intentional here: the features under test
// are integer counts/flags stored as f32, written without arithmetic.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use sts2_content::run_callbacks::new_run_manager_for_character;
    use sts2_core::potions::CharacterId;

    /// GOLDEN: the schema id is pinned. If this fails, the feature
    /// universe changed (new card/relic/boss or reordered scalars) —
    /// update the literal AND retrain/re-export any ranker weights
    /// (old weight files will loudly refuse to load, by design).
    #[test]
    fn schema_id_is_pinned() {
        assert_eq!(
            feature_schema_id(),
            "sts2-runai-ranker-v1:2f04a391d9b68abc",
            "feature universe drifted — bump weights + retrain (see module docs)"
        );
    }

    /// GOLDEN: known view → known sparse indices. A fresh Ironclad
    /// starter deck is 5×Strike, 4×Defend, 1×Bash, all unupgraded.
    #[test]
    fn fresh_ironclad_view_maps_to_known_indices() {
        let rm = new_run_manager_for_character(3, CharacterId::Ironclad);
        let view = VisibleRunView::from_run_manager(&rm);
        let ctx = ContextFeatures::from_view(&view);

        let idx_of = |c: CardId| card_index_of(c).expect("card in universe");
        let mut expected: Vec<(u32, f32, f32)> = vec![
            (idx_of(CardId::StrikeIronclad), 5.0, 0.0),
            (idx_of(CardId::DefendIronclad), 4.0, 0.0),
            (idx_of(CardId::Bash), 1.0, 0.0),
        ];
        expected.sort_by_key(|e| e.0);
        assert_eq!(ctx.deck, expected, "starter-deck histogram");

        // Starter relic present in the multi-hot.
        assert_eq!(ctx.relics.len(), view.relics.len());
        // Scalar block shape + a few known values.
        assert_eq!(ctx.scalars.len(), n_scalars());
        assert_eq!(ctx.scalars[0], view.hp as f32);
        assert_eq!(ctx.scalars[10], view.deck.len() as f32);
        // Boss one-hot: exactly one hot slot (boss pinned on a fresh run).
        let boss_block = &ctx.scalars[SCALAR_NAMES.len()..];
        assert_eq!(
            boss_block.iter().filter(|&&v| v == 1.0).count(),
            1,
            "pinned boss must one-hot exactly once"
        );

        // Dense round-trip: deck pair lands at 2·idx / 2·idx+1.
        let mut dense = vec![0.0_f32; context_dim()];
        ctx.write_dense(&mut dense);
        let bash = idx_of(CardId::Bash) as usize;
        assert_eq!(dense[2 * bash], 1.0);
        assert_eq!(dense[2 * bash + 1], 0.0);
        assert_eq!(
            dense[2 * n_cards() + n_relics()],
            view.hp as f32,
            "scalar block offset"
        );
    }

    /// Candidate encoding covers card-shaped mutations and rejects
    /// non-card ones.
    #[test]
    fn candidate_encoding_covers_card_shapes_only() {
        let rm = new_run_manager_for_character(7, CharacterId::Ironclad);
        let view = VisibleRunView::from_run_manager(&rm);

        let add = CandidateFeatures::from_mutation(
            &CandidateMutation::AddCard {
                card_id: CardId::Inflame,
                upgraded: true,
            },
            &view,
        )
        .expect("add encodes");
        assert_eq!(add.kind, CandKind::Add);
        assert!(add.upgraded);
        assert_eq!(add.card_idx, card_index_of(CardId::Inflame).unwrap());
        assert_eq!(add.feat_vec(), [1.0, 1.0, 0.0, 0.0, 0.0]);

        let skip = CandidateFeatures::from_mutation(&CandidateMutation::Baseline, &view)
            .expect("skip encodes");
        assert_eq!(skip.card_idx, n_cards() as u32, "skip = null row");
        assert_eq!(skip.feat_vec(), [0.0, 0.0, 0.0, 0.0, 1.0]);

        let upg = CandidateFeatures::from_mutation(
            &CandidateMutation::UpgradeCard { deck_idx: 0 },
            &view,
        )
        .expect("upgrade encodes");
        assert_eq!(upg.kind, CandKind::Upgrade);
        assert_eq!(
            upg.card_idx,
            card_index_of(view.deck[0].card_id).unwrap(),
            "deck idx resolves through the view"
        );

        assert!(
            CandidateFeatures::from_mutation(
                &CandidateMutation::AddRelic {
                    relic_id: RelicId::Anchor
                },
                &view
            )
            .is_none(),
            "relic candidates are not rankable"
        );
        assert!(CandidateFeatures::from_mutation(
            &CandidateMutation::GoldDelta { delta: 10 },
            &view
        )
        .is_none());
    }

    /// Index tables are sorted, deduplicated, and self-consistent.
    #[test]
    fn index_tables_are_stable() {
        let cards = card_index_table();
        assert!(cards
            .windows(2)
            .all(|w| format!("{:?}", w[0]) < format!("{:?}", w[1])));
        let relics = relic_index_table();
        assert!(relics
            .windows(2)
            .all(|w| format!("{:?}", w[0]) < format!("{:?}", w[1])));
        assert!(!boss_kind_table().is_empty(), "boss universe non-empty");
        assert_eq!(
            context_dim(),
            2 * n_cards() + n_relics() + n_scalars(),
            "dense layout arithmetic"
        );
        for (i, c) in cards.iter().enumerate() {
            assert_eq!(card_index_of(*c), Some(i as u32));
        }
    }
}
