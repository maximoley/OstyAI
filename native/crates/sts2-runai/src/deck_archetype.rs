//! Coarse deck-archetype classifier for stratification and diversity
//! (deck-oracle task 0.1).
//!
//! [`classify_archetype`] maps a [`VisibleRunView`] to one of six coarse
//! buckets. It is a *stratifier*, not a precise label — thresholds are
//! intentionally lenient so that every real-world deck lands in a
//! meaningful bucket while edge cases fall to [`Archetype::Generic`].
//!
//! The classification uses the same card-feature vocabulary as
//! [`crate::eval::card_stats`] / [`crate::eval::heuristic_v`] so "scaling
//! card" means exactly the same thing across the whole crate.
//!
//! # Bucket definitions (thresholds are documented inline)
//!
//! | Bucket | Primary signal |
//! |--------|---------------|
//! | `Thin` | deck ≤ 7 cards (starter ± 1–2 picks) |
//! | `Scaling` | scaling-card fraction ≥ 0.20 |
//! | `Block` | block-per-card ≥ 2.0 AND block-card fraction ≥ 0.40 |
//! | `Exhaust` | exhaust-card fraction ≥ 0.30 |
//! | `Aggro` | attack fraction ≥ 0.60 AND avg damage-per-card ≥ 5.0 |
//! | `Generic` | none of the above |
//!
//! Priorities are applied in the order listed above.

use crate::eval::card_stats::card_stats;
use crate::view::VisibleRunView;

/// Coarse deck-archetype bucket for stratification/diversity.
///
/// Derived via [`PartialOrd`]/[`Ord`] ordering == declaration order; do NOT
/// rely on numeric ordering for comparisons — use the explicit enum variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Archetype {
    Thin,
    Scaling,
    Block,
    Exhaust,
    Aggro,
    Generic,
}

impl Archetype {
    /// All archetype variants in declaration order. Used by `sampling` to
    /// pick a uniform random archetype without hardcoding a fragile match.
    pub const ALL: [Self; 6] = [
        Self::Thin,
        Self::Scaling,
        Self::Block,
        Self::Exhaust,
        Self::Aggro,
        Self::Generic,
    ];

    /// Short ASCII token usable as a CSV/JSON key or diversity key.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Thin => "thin",
            Self::Scaling => "scaling",
            Self::Block => "block",
            Self::Exhaust => "exhaust",
            Self::Aggro => "aggro",
            Self::Generic => "generic",
        }
    }
}

impl std::fmt::Display for Archetype {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.token())
    }
}

// Compile-time guard: if a new variant is added without updating ALL, the
// length assertion fires as a const-eval panic.
const _: () = assert!(
    Archetype::ALL.len() == 6,
    "Archetype::ALL length mismatch — update ALL when adding variants"
);

/// Card-type/keyword ratio features over the deck, usable for bias
/// regression in the 0a oracle or as a diversity key.
///
/// All fields are ratios in [0, 1] (or raw counts/scalars where noted).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArchetypeFeatures {
    /// Number of cards in the deck.
    pub deck_size: usize,
    /// Fraction of cards with `scaling == true` (HeuristicV vocabulary).
    pub scaling_frac: f64,
    /// Average block produced per card play.
    pub block_per_card: f64,
    /// Fraction of deck cards whose block contribution > 0.
    pub block_card_frac: f64,
    /// Fraction of cards with the Exhaust keyword (from CardSpec).
    pub exhaust_frac: f64,
    /// Fraction of cards that are attacks (`is_attack`).
    pub attack_frac: f64,
    /// Average single-target damage per card play (across the whole deck).
    pub damage_per_card: f64,
}

/// Classify the deck in `view` into a coarse [`Archetype`] bucket.
///
/// See the module-level table for the exact thresholds.
#[must_use]
pub fn classify_archetype(view: &VisibleRunView) -> Archetype {
    let f = archetype_features(view);

    // Priority 1: tiny deck — stratifies early-run and removal-heavy decks.
    // Threshold: ≤ 7 cards (starter is 5; 7 gives one or two safe additions).
    if f.deck_size <= 7 {
        return Archetype::Thin;
    }

    // Priority 2: scaling — power/buff cards dominate the deck.
    // Threshold: ≥ 20 % scaling cards (consistent with HeuristicV's `scaling_count` feature).
    if f.scaling_frac >= 0.20 {
        return Archetype::Scaling;
    }

    // Priority 3: block/defense — high block density.
    // Threshold: ≥ 2.0 block per card (averaged over the full deck) AND
    // ≥ 40 % of cards produce block. The 2.0 floor rejects pure-attack
    // decks that happen to have a Defend or two; 40 % rejects hybrid builds
    // where block is a side effect rather than the primary strategy.
    // Example: 4 Defend (5 each) + 6 Strike → block_per_card=20/10=2.0,
    // block_card_frac=4/10=0.40 → Block.
    if f.block_per_card >= 2.0 && f.block_card_frac >= 0.40 {
        return Archetype::Block;
    }

    // Priority 4: exhaust synergy — at least 30 % of cards have Exhaust.
    if f.exhaust_frac >= 0.30 {
        return Archetype::Exhaust;
    }

    // Priority 5: aggressive — majority attacks with meaningful damage output.
    // Threshold: ≥ 60 % attacks AND ≥ 5.0 average damage per card.
    if f.attack_frac >= 0.60 && f.damage_per_card >= 5.0 {
        return Archetype::Aggro;
    }

    Archetype::Generic
}

/// Extract the numeric feature vector used by [`classify_archetype`] and
/// potentially by downstream bias-regression (oracle task 0a).
#[must_use]
pub fn archetype_features(view: &VisibleRunView) -> ArchetypeFeatures {
    let n = view.deck.len();
    if n == 0 {
        return ArchetypeFeatures {
            deck_size: 0,
            scaling_frac: 0.0,
            block_per_card: 0.0,
            block_card_frac: 0.0,
            exhaust_frac: 0.0,
            attack_frac: 0.0,
            damage_per_card: 0.0,
        };
    }

    let mut scaling = 0usize;
    let mut block_total = 0.0f64;
    let mut block_cards = 0usize;
    let mut exhaust_cards = 0usize;
    let mut attacks = 0usize;
    let mut damage_total = 0.0f64;

    for dc in &view.deck {
        let s = card_stats(dc.card_id, dc.upgraded);
        if s.scaling {
            scaling += 1;
        }
        if s.block_per_play > 0.0 {
            block_total += s.block_per_play;
            block_cards += 1;
        }
        // Exhaust: queried from the spec via card_stats re-exports —
        // card_stats::exhausts() wraps CardSpec::exhausts() so the taxonomy
        // stays in one place.
        if card_exhausts_for_deck(dc.card_id, dc.upgraded) {
            exhaust_cards += 1;
        }
        if s.is_attack {
            attacks += 1;
            damage_total += s.damage_per_play;
        }
    }

    let nf = n as f64;
    ArchetypeFeatures {
        deck_size: n,
        scaling_frac: scaling as f64 / nf,
        block_per_card: block_total / nf,
        block_card_frac: block_cards as f64 / nf,
        exhaust_frac: exhaust_cards as f64 / nf,
        attack_frac: attacks as f64 / nf,
        damage_per_card: damage_total / nf,
    }
}

// ── Per-card archetype membership ────────────────────────────────────────

/// Returns `true` if `card_id` (at the given upgrade level) belongs to the
/// "family" of cards that advance the given [`Archetype`].
///
/// Uses the same [`card_stats`] fields as [`classify_archetype`] and
/// [`archetype_features`] so the vocabulary is consistent across the whole
/// crate. This is intentionally a per-card approximation — unlike the
/// deck-level classifier it cannot use deck-size (which is a deck property,
/// not a card property), so:
///
/// - `Thin`    → always `false` (no single card is intrinsically "thin");
/// - `Scaling` → `card_stats.scaling`;
/// - `Block`   → `card_stats.block_per_play > 0.0`;
/// - `Exhaust` → card has the Exhaust keyword;
/// - `Aggro`   → `is_attack && damage_per_play >= 5.0` (matches the classifier's
///               "meaningful damage" threshold);
/// - `Generic` → always `true` (a catch-all; every card nominally fits).
#[must_use]
pub fn card_matches_archetype(
    card_id: sts2_core::cards::CardId,
    upgraded: bool,
    archetype: Archetype,
) -> bool {
    match archetype {
        Archetype::Thin => false,
        Archetype::Scaling => {
            let s = card_stats(card_id, upgraded);
            s.scaling
        }
        Archetype::Block => {
            let s = card_stats(card_id, upgraded);
            s.block_per_play > 0.0
        }
        Archetype::Exhaust => card_exhausts_for_deck(card_id, upgraded),
        Archetype::Aggro => {
            let s = card_stats(card_id, upgraded);
            s.is_attack && s.damage_per_play >= 5.0
        }
        Archetype::Generic => true,
    }
}

// ── Exhaust lookup ────────────────────────────────────────────────────────

/// Returns `true` if the card (at the given upgrade level) has the Exhaust
/// keyword. Uses a one-time-initialised static map built from the content
/// spec index so repeated calls are O(1) lookups with no registry dependency.
///
/// Upgrade handling: `card_pod_from_id` bakes the spec-upgrade keyword
/// delta into `Card::keyword_override` for cards whose keywords change on
/// upgrade (e.g. BurningPact drops EXHAUST). `card_exhausts_inst` then
/// honours that override. For cards with no keyword delta the base
/// `CardSpec::exhausts()` is used directly, which avoids any dependency on
/// the global exhaust registry being installed.
fn card_exhausts_for_deck(card_id: sts2_core::cards::CardId, upgraded: bool) -> bool {
    use std::collections::BTreeMap;
    use std::sync::OnceLock;
    use sts2_content::cards::spec_index::{
        card_pod_from_id, specs_for_character, COLORLESS_SPECS, STATUS_SPECS,
    };
    use sts2_core::cards::KeywordOverride;
    use sts2_core::potions::CharacterId;

    static CACHE: OnceLock<BTreeMap<sts2_core::cards::CardId, [bool; 2]>> = OnceLock::new();
    let index = CACHE.get_or_init(|| {
        let mut map = BTreeMap::new();
        let char_slices = [
            CharacterId::Ironclad,
            CharacterId::Silent,
            CharacterId::Defect,
            CharacterId::Regent,
            CharacterId::Necrobinder,
        ]
        .into_iter()
        .map(specs_for_character);
        for slice in char_slices.chain([COLORLESS_SPECS, STATUS_SPECS]) {
            for factory in slice {
                let spec = factory();
                let base_exhaust = spec.exhausts();
                // For the upgraded variant, check whether the upgrade delta
                // overrides keywords. `card_pod_from_id` applies `apply_spec_upgrade`
                // which stamps `keyword_override` when the delta has a `keywords`
                // field. We replicate the `card_exhausts_inst` logic inline.
                let upg_exhaust = card_pod_from_id(spec.id, true).map_or(base_exhaust, |card| {
                    match card.keyword_override {
                        Some(ko) => ko.contains(KeywordOverride::EXHAUST),
                        None => base_exhaust, // no keyword delta → same as base
                    }
                });
                map.insert(spec.id, [base_exhaust, upg_exhaust]);
            }
        }
        map
    });
    index
        .get(&card_id)
        .is_some_and(|pair| pair[usize::from(upgraded)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{DeckCard, VisibleRunView};
    use sts2_content::run_callbacks::new_run_manager_for_character;
    use sts2_core::cards::CardId;
    use sts2_core::potions::CharacterId;

    #[test]
    fn test_archetype_all_exhaustive() {
        // Exhaustive match — adding a variant without updating ALL
        // causes a compile error here (non-exhaustive pattern).
        for v in Archetype::ALL {
            let _name = match v {
                Archetype::Thin => "thin",
                Archetype::Scaling => "scaling",
                Archetype::Block => "block",
                Archetype::Exhaust => "exhaust",
                Archetype::Aggro => "aggro",
                Archetype::Generic => "generic",
            };
        }
        // Every variant in Archetype::ALL must be unique.
        let mut seen = std::collections::HashSet::new();
        for v in Archetype::ALL {
            assert!(seen.insert(v), "Archetype::ALL contains duplicate: {v:?}");
        }
        assert_eq!(Archetype::ALL.len(), 6, "Archetype::ALL length mismatch");
    }

    /// Build a minimal `VisibleRunView` whose deck is exactly the supplied
    /// cards. All other fields come from a fresh Ironclad run-manager so they
    /// are valid (non-default) but irrelevant to the archetype classifier.
    fn view_with(deck: Vec<(CardId, bool)>) -> VisibleRunView {
        let rm = new_run_manager_for_character(42, CharacterId::Ironclad);
        let mut view = VisibleRunView::from_run_manager(&rm);
        view.deck = deck
            .into_iter()
            .map(|(card_id, upgraded)| DeckCard { card_id, upgraded })
            .collect();
        view
    }

    // ── RED → GREEN: brief's required tests ───────────────────────────────

    #[test]
    fn scaling_deck_classifies_scaling() {
        // 3 scaling cards + 5 filler = 8 total, 37.5 % >> 20 % threshold.
        // Uses 8 cards to exceed the Thin cutoff (≤ 7).
        let v = view_with(vec![
            (CardId::Inflame, false),
            (CardId::DemonFormCard, false),
            (CardId::RuptureCard, false),
            (CardId::StrikeIronclad, false),
            (CardId::StrikeIronclad, false),
            (CardId::StrikeIronclad, false),
            (CardId::DefendIronclad, false),
            (CardId::Bash, false),
        ]);
        assert_eq!(classify_archetype(&v), Archetype::Scaling);
    }

    #[test]
    fn small_deck_classifies_thin() {
        // 3 cards ≤ 7 threshold → Thin.
        let v = view_with(vec![
            (CardId::StrikeIronclad, false),
            (CardId::DefendIronclad, false),
            (CardId::Bash, false),
        ]);
        assert_eq!(classify_archetype(&v), Archetype::Thin);
    }

    // ── Additional coverage ───────────────────────────────────────────────

    #[test]
    fn block_deck_classifies_block() {
        // 8 cards: 4 Defend (5 block each) + 4 Strike.
        // block_per_card = 20/8 = 2.5 ≥ 2.0; block_card_frac = 4/8 = 0.50 ≥ 0.40.
        let v = view_with(vec![
            (CardId::DefendIronclad, false),
            (CardId::DefendIronclad, false),
            (CardId::DefendIronclad, false),
            (CardId::DefendIronclad, false),
            (CardId::StrikeIronclad, false),
            (CardId::StrikeIronclad, false),
            (CardId::StrikeIronclad, false),
            (CardId::StrikeIronclad, false),
        ]);
        assert_eq!(classify_archetype(&v), Archetype::Block);
    }

    #[test]
    fn aggro_deck_classifies_aggro() {
        // 9 attacks + 1 Bash → 100 % attack frac; Bash and StrikeIronclad have > 5 avg damage.
        let mut cards = vec![(CardId::Bash, false)];
        for _ in 0..9 {
            cards.push((CardId::StrikeIronclad, false));
        }
        let v = view_with(cards);
        assert_eq!(classify_archetype(&v), Archetype::Aggro);
    }

    #[test]
    fn token_round_trips() {
        for a in [
            Archetype::Thin,
            Archetype::Scaling,
            Archetype::Block,
            Archetype::Exhaust,
            Archetype::Aggro,
            Archetype::Generic,
        ] {
            assert!(!a.token().is_empty());
            assert_eq!(a.to_string(), a.token());
        }
    }

    #[test]
    fn archetype_features_zero_deck() {
        let rm = new_run_manager_for_character(1, CharacterId::Ironclad);
        let mut view = VisibleRunView::from_run_manager(&rm);
        view.deck.clear();
        let f = archetype_features(&view);
        assert_eq!(f.deck_size, 0);
        assert!(f.scaling_frac < f64::EPSILON);
    }

    #[test]
    fn scaling_frac_matches_count() {
        // 2 scaling + 6 non-scaling = 25 % ≥ 20 % threshold.
        let v = view_with(vec![
            (CardId::Inflame, false),
            (CardId::DemonFormCard, false),
            (CardId::StrikeIronclad, false),
            (CardId::StrikeIronclad, false),
            (CardId::StrikeIronclad, false),
            (CardId::StrikeIronclad, false),
            (CardId::StrikeIronclad, false),
            (CardId::StrikeIronclad, false),
        ]);
        let f = archetype_features(&v);
        assert!((f.scaling_frac - 0.25).abs() < 1e-9);
        assert_eq!(classify_archetype(&v), Archetype::Scaling);
    }
}
