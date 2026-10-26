//! Phase-3 belief-state draw pile (information-set representation).
//!
//! Plan: `docs/plans/2026-06-09-combat-phase3-information-set-search-implementation.md` (Task 3.1).
//! Bounded inaccuracies: `docs/reality-checks/2026-06-09-belief-pile-bounded-inaccuracies.md`.
//! Reference: `sts_lightspeed` `include/combat/CardManager.h` (pile ops live inline on `CardManager`).
//!
//! Where the live engine models the draw pile as a *concrete fully-ordered*
//! `PlayerCombatState.draw` and hides the true order from search by
//! re-shuffling once per PIMC simulation, this type models the **information
//! set the searcher actually knows**: a deterministic `known_top` stack, a
//! deterministic `known_bottom` stack, and an order-unobserved `unknown`
//! multiset whose shuffle randomness is **deferred to draw time**. Drawing
//! from `unknown` samples uniformly without replacement, which induces the
//! exact same distribution as "uniformly shuffle the multiset, then draw
//! sequentially" (exchangeability) — see the `verify_draw_dist` test, which
//! gates this against the engine's `DotNetRng` Fisher-Yates shuffle.
//!
//! This module is **not** wired into the live combat engine; it is consumed
//! only by the Phase-3 information-set search (Tasks 3.2/3.3), so it adds no
//! cost to the hot engine path.

use crate::CardUid;
use rand::Rng;
use smallvec::SmallVec;

/// Draw order is: `known_top` (in order) → `unknown` (uniform random,
/// deferred) → `known_bottom` (in order). Index 0 of each stack is the
/// element drawn first within that stack.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BeliefDrawPile {
    /// Cards with a known top position; `known_top[0]` is drawn next.
    /// Maps from `PlayerCombatState.pinned_top` (Headbutt-style "on top").
    pub known_top: SmallVec<[CardUid; 4]>,
    /// Cards with a known bottom position; drawn only after `known_top`
    /// and `unknown` are exhausted, `known_bottom[0]` first. Maps from
    /// `PlayerCombatState.pinned_bottom`.
    pub known_bottom: SmallVec<[CardUid; 4]>,
    /// Cards whose order is unobserved. A `draw_top` from here samples a
    /// uniformly random element (deferred shuffle).
    pub unknown: SmallVec<[CardUid; 32]>,
    /// Frozen-Eye analog: when the player can observe the true draw order
    /// the information set collapses to a concrete order (the caller puts
    /// that order in `known_top`). Latent at the v0.103.2 pin — see the
    /// bounded-inaccuracies doc; do not wire to a mechanic without
    /// confirming a C# analog exists.
    pub order_observed: bool,
}

impl BeliefDrawPile {
    /// Construct the information set implied by a live player's draw pile,
    /// discarding the unobserved middle order (the whole point). Pinned
    /// cards keep their known placements.
    #[must_use]
    pub fn from_player(player: &crate::PlayerCombatState) -> Self {
        let known_top: SmallVec<[CardUid; 4]> = player.pinned_top.iter().copied().collect();
        let known_bottom: SmallVec<[CardUid; 4]> = player.pinned_bottom.iter().copied().collect();
        let mut unknown: SmallVec<[CardUid; 32]> = SmallVec::new();
        for &uid in &player.draw {
            if !known_top.contains(&uid) && !known_bottom.contains(&uid) {
                unknown.push(uid);
            }
        }
        Self {
            known_top,
            known_bottom,
            unknown,
            order_observed: false,
        }
    }

    /// Construct a fully-observed pile in concrete draw order (Frozen-Eye
    /// analog): every card is deterministic, drawn front-to-back.
    #[must_use]
    pub fn with_observed_order<I: IntoIterator<Item = CardUid>>(order: I) -> Self {
        Self {
            known_top: order.into_iter().collect(),
            known_bottom: SmallVec::new(),
            unknown: SmallVec::new(),
            order_observed: true,
        }
    }

    /// Total number of cards remaining in the pile.
    #[must_use]
    pub fn len(&self) -> usize {
        self.known_top.len() + self.unknown.len() + self.known_bottom.len()
    }

    /// True when no cards remain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.known_top.is_empty() && self.unknown.is_empty() && self.known_bottom.is_empty()
    }

    /// Draw the top card, resolving hidden order lazily. Returns `None`
    /// when the pile is empty (the caller then performs reshuffle). A
    /// single-element `unknown` is drawn deterministically with **no** RNG
    /// read (singleton normalization).
    pub fn draw_top(&mut self, rng: &mut impl Rng) -> Option<CardUid> {
        if !self.known_top.is_empty() {
            return Some(self.known_top.remove(0));
        }
        match self.unknown.len() {
            0 => {
                if self.known_bottom.is_empty() {
                    None
                } else {
                    Some(self.known_bottom.remove(0))
                }
            }
            1 => Some(self.unknown.swap_remove(0)),
            n => {
                let idx = rng.gen_range(0..n);
                // swap_remove is O(1); `unknown` is order-free so the
                // disturbance to the remaining elements is irrelevant.
                Some(self.unknown.swap_remove(idx))
            }
        }
    }

    /// Add a card to the unobserved region (`CardManager::shuffleIntoDrawPile`
    /// without a fixed position).
    pub fn add(&mut self, uid: CardUid) {
        self.unknown.push(uid);
    }

    /// Place a card on top so it draws next (`addToTop`). Later calls push
    /// earlier top cards down (LIFO), matching pin-to-top semantics.
    pub fn add_to_top(&mut self, uid: CardUid) {
        self.known_top.insert(0, uid);
    }

    /// Place a card at the very bottom (`addToBottom`).
    pub fn add_to_bottom(&mut self, uid: CardUid) {
        self.known_bottom.push(uid);
    }

    /// Collapse all known placements back into the unobserved region and
    /// absorb `cards` (`shuffleIn` / `reshuffleSelf` + `absorb`). After this
    /// the whole pile is order-unobserved.
    pub fn shuffle_in<I: IntoIterator<Item = CardUid>>(&mut self, cards: I) {
        let top = std::mem::take(&mut self.known_top);
        let bottom = std::mem::take(&mut self.known_bottom);
        self.unknown.extend(top);
        self.unknown.extend(bottom);
        self.unknown.extend(cards);
        self.order_observed = false;
    }

    /// Collapse known placements into the unobserved region without
    /// absorbing new cards (`reshuffleSelf`).
    pub fn reshuffle_self(&mut self) {
        let top = std::mem::take(&mut self.known_top);
        let bottom = std::mem::take(&mut self.known_bottom);
        self.unknown.extend(top);
        self.unknown.extend(bottom);
        self.order_observed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dotnet_rng::Rng as DotNetRng;
    use rand::SeedableRng;
    use rand_xoshiro::Xoshiro256PlusPlus;
    use std::collections::BTreeMap;

    fn uid(n: u32) -> CardUid {
        CardUid::new(n).expect("non-zero")
    }

    /// χ²(df=23) ≈ 49.73 at p=0.001 — a deliberately generous threshold so
    /// the distributional test has a vanishing false-failure rate while
    /// still catching any real divergence between the two samplers.
    const CHI2_23_P001: f64 = 49.73;

    /// 4 distinct cards → 4! = 24 orderings. Tally the full draw-sequence
    /// distribution from (a) the engine's `DotNetRng` Fisher-Yates shuffle
    /// drained front-to-back and (b) the belief pile's sequential
    /// uniform-without-replacement draw, and assert a two-sample χ² shows
    /// them statistically indistinguishable (exchangeability / Task 3.1
    /// acceptance #1).
    #[test]
    fn verify_draw_dist_matches_engine_shuffle() {
        const N: i32 = 120_000;
        let cards = [uid(1), uid(2), uid(3), uid(4)];

        let mut engine: BTreeMap<[u32; 4], u64> = BTreeMap::new();
        let mut belief: BTreeMap<[u32; 4], u64> = BTreeMap::new();

        for seed in 0..N {
            // Engine arm: concrete shuffle, drain front-to-back.
            let mut v = cards;
            DotNetRng::new(seed).shuffle(&mut v);
            let key = [v[0].get(), v[1].get(), v[2].get(), v[3].get()];
            *engine.entry(key).or_insert(0) += 1;

            // Belief arm: sequential uniform-without-replacement draw.
            let mut pile = BeliefDrawPile {
                unknown: cards.iter().copied().collect(),
                ..Default::default()
            };
            let mut rng = Xoshiro256PlusPlus::seed_from_u64(seed as u64);
            let mut drawn = [0u32; 4];
            for slot in &mut drawn {
                *slot = pile.draw_top(&mut rng).expect("non-empty").get();
            }
            assert!(pile.is_empty(), "belief pile fully drained");
            *belief.entry(drawn).or_insert(0) += 1;
        }

        // All 24 orderings must appear in both arms.
        assert_eq!(engine.len(), 24, "engine covered all permutations");
        assert_eq!(belief.len(), 24, "belief covered all permutations");

        // Two-sample χ² homogeneity (equal sample sizes): Σ (a-b)²/(a+b).
        let mut chi2 = 0.0_f64;
        for (key, &a) in &engine {
            let b = *belief.get(key).unwrap_or(&0);
            let (a, b) = (a as f64, b as f64);
            if a + b > 0.0 {
                chi2 += (a - b).powi(2) / (a + b);
            }
        }
        assert!(
            chi2 < CHI2_23_P001,
            "belief draw distribution diverges from engine shuffle: χ²={chi2:.2} (df=23, threshold={CHI2_23_P001})"
        );
    }

    #[test]
    fn known_top_draws_first_in_order() {
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(7);
        for _ in 0..1000 {
            let mut pile = BeliefDrawPile {
                known_top: [uid(10), uid(11)].into_iter().collect(),
                unknown: [uid(1), uid(2), uid(3)].into_iter().collect(),
                ..Default::default()
            };
            assert_eq!(pile.draw_top(&mut rng), Some(uid(10)));
            assert_eq!(pile.draw_top(&mut rng), Some(uid(11)));
            // remaining three are the unknown set in some order
            let mut rest = [
                pile.draw_top(&mut rng).unwrap().get(),
                pile.draw_top(&mut rng).unwrap().get(),
                pile.draw_top(&mut rng).unwrap().get(),
            ];
            rest.sort_unstable();
            assert_eq!(rest, [1, 2, 3]);
            assert!(pile.draw_top(&mut rng).is_none());
        }
    }

    #[test]
    fn known_bottom_draws_last_in_order() {
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(99);
        for _ in 0..1000 {
            let mut pile = BeliefDrawPile {
                unknown: [uid(1), uid(2)].into_iter().collect(),
                known_bottom: [uid(20), uid(21)].into_iter().collect(),
                ..Default::default()
            };
            // first two are the unknown set
            let mut first = [
                pile.draw_top(&mut rng).unwrap().get(),
                pile.draw_top(&mut rng).unwrap().get(),
            ];
            first.sort_unstable();
            assert_eq!(first, [1, 2]);
            // bottom block in order, last
            assert_eq!(pile.draw_top(&mut rng), Some(uid(20)));
            assert_eq!(pile.draw_top(&mut rng), Some(uid(21)));
            assert!(pile.draw_top(&mut rng).is_none());
        }
    }

    #[test]
    fn singleton_unknown_is_deterministic_without_rng() {
        // A 1-element unknown must not consume the RNG; seed the rng and
        // confirm its state is untouched by comparing to a fresh stream.
        let mut pile = BeliefDrawPile {
            unknown: [uid(42)].into_iter().collect(),
            ..Default::default()
        };
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(123);
        let mut witness = Xoshiro256PlusPlus::seed_from_u64(123);
        assert_eq!(pile.draw_top(&mut rng), Some(uid(42)));
        assert!(pile.is_empty());
        // The pile's draw did not advance `rng`, so it still matches the
        // untouched witness stream.
        use rand::RngCore;
        assert_eq!(rng.next_u64(), witness.next_u64());
    }

    #[test]
    fn observed_order_draws_concrete_sequence() {
        let mut rng = Xoshiro256PlusPlus::seed_from_u64(1);
        let order = [uid(5), uid(6), uid(7)];
        let mut pile = BeliefDrawPile::with_observed_order(order);
        assert!(pile.order_observed);
        for &c in &order {
            assert_eq!(pile.draw_top(&mut rng), Some(c));
        }
        assert!(pile.is_empty());
    }

    #[test]
    fn shuffle_in_collapses_known_stacks() {
        let mut pile = BeliefDrawPile {
            known_top: [uid(1)].into_iter().collect(),
            known_bottom: [uid(2)].into_iter().collect(),
            unknown: [uid(3)].into_iter().collect(),
            order_observed: true,
        };
        pile.shuffle_in([uid(4), uid(5)]);
        assert!(pile.known_top.is_empty());
        assert!(pile.known_bottom.is_empty());
        assert!(!pile.order_observed);
        assert_eq!(pile.unknown.len(), 5);
        let mut got: Vec<u32> = pile.unknown.iter().map(|c| c.get()).collect();
        got.sort_unstable();
        assert_eq!(got, [1, 2, 3, 4, 5]);
    }
}
