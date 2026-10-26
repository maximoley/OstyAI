//! Per-input random-draw helpers — bead `sts2-rs-c9s5` (cliext F3).
//!
//! Implements `--random-all` and the per-input `random` value supported
//! by `--deck-preset`, `--relic-preset`, `--potion-preset`, and
//! `--encounter`. Each input gets its own independent fork of the
//! scenario RNG so the streams stay decorrelated AND `--seed`
//! reproduces the same composite scenario across runs.
//!
//! ## RNG salt scheme
//!
//! The user supplies one scalar seed (`args.seed: u64`). To produce
//! four independent streams without ever entangling them with the main
//! combat RNG (which is constructed inside
//! `build_starter_state_from_monsters` and drives monster-HP rolls),
//! we XOR the seed with a per-input constant salt:
//!
//! ```text
//! deck      = Xoshiro256PP::seed_from_u64(seed ^ SALT_DECK)
//! relic     = Xoshiro256PP::seed_from_u64(seed ^ SALT_RELIC)
//! potion    = Xoshiro256PP::seed_from_u64(seed ^ SALT_POTION)
//! encounter = Xoshiro256PP::seed_from_u64(seed ^ SALT_ENCOUNTER)
//! ```
//!
//! Salts are picked as fixed 64-bit constants with high Hamming
//! distance between them, derived from a hash of the input-name
//! ASCII bytes; the actual numeric values are arbitrary but MUST
//! stay stable across versions so reruns of a fixed `--seed`
//! reproduce the same scenario byte-for-byte. Changing a salt is a
//! breaking change to the random-all snapshot contract.
//!
//! The main combat RNG inside `build_starter_state_from_monsters`
//! is built independently from `args.seed` and is NOT salted —
//! monster-HP rolls keep their pre-F3 reproducibility.

use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;
use sts2_content::cards::CardSpec;
use sts2_content::names::{deck_card_spec_pool_for_class, PlayerClassHint};
use sts2_core::relics::RelicId;
use sts2_core::PotionKind;

use super::deck::DeckCardSpec;
use super::encounter::sts2_random_pool;
use super::relics::relic_id_to_kind;

// Salt constants — fixed 64-bit values, chosen for high Hamming
// distance. They are NOT derived at runtime; changing any value here
// breaks `--seed` snapshot reproducibility.
const SALT_DECK: u64 = 0xD11C_C0DE_DEC0_0001;
const SALT_RELIC: u64 = 0xC0DE_BEEF_4ABE_1002;
const SALT_POTION: u64 = 0xF00D_BABE_F1DE_1003;
const SALT_ENCOUNTER: u64 = 0xACE0_BACE_E11C_1004;
// Added in cliext F4 (bead sts2-rs-9yoh) — picks the class used by
// `--deck-preset random` when the user doesn't pin
// `--random-deck-character` explicitly. New salt so the deck-card
// stream is unaffected for any given (seed, class) pair.
const SALT_CLASS: u64 = 0xC1A5_5C0D_E0EE_1005;

/// Default number of cards for `--deck-preset random` when the user
/// doesn't pass `--random-deck-size`. The flag bounds-check is
/// performed at the CLI layer.
pub const DEFAULT_RANDOM_DECK_SIZE: usize = 10;

/// Build a forked `Xoshiro256PP` for the given salt. See module docs.
///
/// Note: this is CLI scenario seeding (DET, not parity-critical per
/// bead `sts2-rs-ykmk9` Stage 2 audit); the streams are decorrelation
/// helpers for `--random-all`, not engine-canonical, so Xoshiro is
/// retained here even after the engine-side DotNetRng migration.
fn fork(seed: u64, salt: u64) -> Xoshiro256PlusPlus {
    Xoshiro256PlusPlus::seed_from_u64(seed ^ salt)
}

/// Sample [`RANDOM_DECK_SIZE`] cards uniformly with replacement from
/// the deck-buildable pool exposed by `sts2-content::names::deck_card_spec_pool`.
///
/// "With replacement" is deliberate: STS2 decks routinely contain
/// duplicates (5× Strike + 4× Defend is a starter shape) so sampling
/// with replacement matches the natural distribution. A future bead
/// could swap to weighted-by-rarity sampling without changing the
/// public seam.
#[must_use]
pub fn random_deck(seed: u64, size: usize, class: PlayerClassHint) -> Vec<DeckCardSpec> {
    let mut rng = fork(seed, SALT_DECK);
    let pool: Vec<CardSpec> = deck_card_spec_pool_for_class(class);
    assert!(
        !pool.is_empty(),
        "deck pool for class {class:?} must be non-empty — extend deck_card_spec_registry"
    );
    (0..size)
        .map(|_| {
            let idx = rng.gen_range(0..pool.len());
            DeckCardSpec {
                spec: pool[idx],
                upgrade_level: 0,
                enchantment: None,
            }
        })
        .collect()
}

/// Pick a class uniformly from
/// [`PlayerClassHint::SUPPORTED_FOR_RANDOM_DECK`]. Used by
/// `--random-all` (and by `--deck-preset random` without an explicit
/// `--random-deck-character`) so the random class is reproducible from
/// `--seed` alone but decorrelated from the card-pool stream.
#[must_use]
pub fn random_class(seed: u64) -> PlayerClassHint {
    let mut rng = fork(seed, SALT_CLASS);
    let pool = PlayerClassHint::SUPPORTED_FOR_RANDOM_DECK;
    pool[rng.gen_range(0..pool.len())]
}

/// Sample 1..=3 relics uniformly without replacement from the runtime
/// pool — every `RelicId` whose [`relic_id_to_kind`] returns `Some`.
/// Mirrors the alphabet of seedable relics (gating on runtime-kind
/// availability matches `seed_relics`'s requirement; sampling an
/// unseedable id would just fail later).
#[must_use]
pub fn random_relics(seed: u64) -> Vec<RelicId> {
    let mut rng = fork(seed, SALT_RELIC);
    let pool: Vec<RelicId> = RelicId::all()
        .iter()
        .copied()
        .filter(|&id| relic_id_to_kind(id).is_some())
        .collect();
    let count = rng.gen_range(1..=3usize).min(pool.len());
    pool.choose_multiple(&mut rng, count).copied().collect()
}

/// Sample 3 distinct non-event-only potions for `--potion-preset random`.
///
/// The bead spec calls this an "alias for the existing `random-3`
/// preset". The *shape* matches (3 distinct picks from the non-event
/// pool) but the *stream* differs: `--potion-preset random-3` seeds
/// its RNG with `args.seed` directly inside `resolve_potions`, while
/// this function uses the salted fork `seed ^ SALT_POTION` so it
/// stays decorrelated from the other `--random-all` streams (deck /
/// relic / encounter) when they fire on the same seed. Net effect:
/// for a given `--seed`, `--potion-preset random` and
/// `--potion-preset random-3` will pick different concrete potions.
#[must_use]
pub fn random_potions(seed: u64) -> Vec<PotionKind> {
    let mut rng = fork(seed, SALT_POTION);
    let pool: Vec<PotionKind> = PotionKind::ALL
        .iter()
        .copied()
        .filter(|k| !k.is_event_only())
        .collect();
    pool.choose_multiple(&mut rng, 3).copied().collect()
}

/// Pick one encounter for `--encounter random` / `--random-all`.
///
/// Originally (B2) this drew from the `ENCOUNTERS` alias table, but
/// 8 of the 9 aliases reference STS1-name Phase-4 scaffolding stubs
/// (Cultist/JawWorm/AcidSlimeS/GreenLouse/Sentry/two-sentries/
/// three-louses/slimes-2x-acid). Cliext H5 retargets the random
/// path to STS2-native monsters only: by default it returns a
/// single-monster encounter drawn from
/// [`sts2_random_pool`](super::encounter::sts2_random_pool), or
/// `nibbit` if the pool is somehow empty. STS1 stubs stay reachable
/// via explicit `--encounter cultist` / `--monsters cultist`.
#[must_use]
pub fn random_encounter(seed: u64) -> String {
    let mut rng = fork(seed, SALT_ENCOUNTER);
    let pool = sts2_random_pool();
    if pool.is_empty() {
        // Defensive: SELECTABLE_MONSTERS always contains ≥1 STS2 entry
        // today (Nibbit), but if the table is ever pruned this avoids
        // a gen_range(0..0) panic on the random path.
        return "nibbit".to_string();
    }
    // Look up the canonical tag for the chosen kind by walking
    // SELECTABLE_MONSTERS — the caller (--random-all in main.rs)
    // assigns this string to args.encounter, then resolve_encounter
    // routes it through the SELECTABLE_MONSTERS fallback.
    let kind = pool[rng.gen_range(0..pool.len())];
    super::encounter::SELECTABLE_MONSTERS
        .iter()
        .find(|(_, k, _)| *k == kind)
        .map(|(tag, _, _)| (*tag).to_string())
        .expect("kind in SELECTABLE_MONSTERS by construction")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_deck_is_deterministic_and_correct_size() {
        let a = random_deck(7, DEFAULT_RANDOM_DECK_SIZE, PlayerClassHint::Ironclad);
        let b = random_deck(7, DEFAULT_RANDOM_DECK_SIZE, PlayerClassHint::Ironclad);
        assert_eq!(a.len(), DEFAULT_RANDOM_DECK_SIZE);
        assert_eq!(
            a.iter().map(|c| c.spec.id).collect::<Vec<_>>(),
            b.iter().map(|c| c.spec.id).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn random_deck_different_seeds_diverge() {
        let a = random_deck(1, DEFAULT_RANDOM_DECK_SIZE, PlayerClassHint::Ironclad);
        let b = random_deck(2, DEFAULT_RANDOM_DECK_SIZE, PlayerClassHint::Ironclad);
        // Ironclad pool has > 10 entries so this is overwhelmingly
        // likely. If the pool shrinks below 4 entries this test may
        // need rethinking.
        assert_ne!(
            a.iter().map(|c| c.spec.id).collect::<Vec<_>>(),
            b.iter().map(|c| c.spec.id).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn random_deck_respects_size_argument() {
        for size in [1usize, 5, 10, 30, 50] {
            let deck = random_deck(42, size, PlayerClassHint::Silent);
            assert_eq!(deck.len(), size, "size={size}");
        }
    }

    #[test]
    fn random_deck_is_class_locked() {
        // Every card sampled with class=Ironclad must come from the
        // Ironclad section of the registry; same for Silent. Verified
        // by intersecting with the per-class pool.
        let ironclad_ids: std::collections::HashSet<_> =
            deck_card_spec_pool_for_class(PlayerClassHint::Ironclad)
                .iter()
                .map(|s| s.id)
                .collect();
        let silent_ids: std::collections::HashSet<_> =
            deck_card_spec_pool_for_class(PlayerClassHint::Silent)
                .iter()
                .map(|s| s.id)
                .collect();
        // The two pools must be disjoint for the lock to be meaningful.
        assert!(ironclad_ids.is_disjoint(&silent_ids));
        for seed in [0u64, 1, 7, 99, 31337] {
            for card in random_deck(seed, 30, PlayerClassHint::Ironclad) {
                assert!(
                    ironclad_ids.contains(&card.spec.id),
                    "leaked non-Ironclad card {:?}",
                    card.spec.id
                );
            }
            for card in random_deck(seed, 30, PlayerClassHint::Silent) {
                assert!(
                    silent_ids.contains(&card.spec.id),
                    "leaked non-Silent card {:?}",
                    card.spec.id
                );
            }
        }
    }

    /// Regression test for bead `sts2-rs-ttte`: `--deck-preset random` with
    /// no explicit `--random-deck-character` must produce a single-class deck
    /// for every seed. Enumerates seeds 0..1000 via the `random_class()` +
    /// `random_deck()` pipeline that `main.rs` uses on the default path.
    #[test]
    fn random_deck_default_class_is_single_class_invariant() {
        use std::collections::HashSet;
        // Pre-build per-class id sets for fast membership checks.
        // (PlayerClassHint doesn't derive Hash so we store as Vec pairs.)
        let class_pool_pairs: Vec<(PlayerClassHint, HashSet<_>)> =
            PlayerClassHint::SUPPORTED_FOR_RANDOM_DECK
                .iter()
                .map(|&class| {
                    let ids: HashSet<_> = deck_card_spec_pool_for_class(class)
                        .iter()
                        .map(|s| s.id)
                        .collect();
                    (class, ids)
                })
                .collect();

        for seed in 0u64..1000 {
            let class = random_class(seed);
            let deck = random_deck(seed, DEFAULT_RANDOM_DECK_SIZE, class);
            let pool = class_pool_pairs
                .iter()
                .find(|(c, _)| *c == class)
                .map(|(_, ids)| ids)
                .expect("random_class always returns a SUPPORTED_FOR_RANDOM_DECK member");
            for card in &deck {
                assert!(
                    pool.contains(&card.spec.id),
                    "seed {seed}: random_class picked {class:?} but card {:?} is not in that \
                     class's pool — registry mistag in deck_card_spec_registry (sts2-rs-ttte)",
                    card.spec.id
                );
            }
        }
    }

    #[test]
    fn random_class_is_deterministic_and_in_supported_set() {
        let a = random_class(11);
        let b = random_class(11);
        assert_eq!(a, b);
        assert!(PlayerClassHint::SUPPORTED_FOR_RANDOM_DECK.contains(&a));
    }

    #[test]
    fn random_relics_is_deterministic_and_in_range() {
        let a = random_relics(42);
        let b = random_relics(42);
        assert_eq!(a, b);
        assert!((1..=3).contains(&a.len()));
    }

    #[test]
    fn random_relics_all_have_runtime_kinds() {
        for id in random_relics(99) {
            assert!(relic_id_to_kind(id).is_some());
        }
    }

    #[test]
    fn random_potions_is_deterministic_and_three() {
        let a = random_potions(13);
        let b = random_potions(13);
        assert_eq!(a, b);
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn initial_potion_scenarios_include_normal_non_combat_generatable_potions() {
        let mut seen = std::collections::HashSet::new();
        for seed in 0..2048 {
            for potion in random_potions(seed) {
                assert!(!potion.is_event_only());
                seen.insert(potion);
            }
        }
        for kind in [
            PotionKind::FairyInABottle,
            PotionKind::FruitJuice,
            PotionKind::RegenPotion,
        ] {
            assert!(seen.contains(&kind));
            assert!(!kind.can_be_generated_in_combat());
        }
    }

    #[test]
    fn random_encounter_only_picks_sts2_native_monsters() {
        // Cliext H5: random rolls must never surface STS1-name stubs.
        // Walk 200 seeds; every result must resolve to a kind flagged
        // sts2_native=true in SELECTABLE_MONSTERS.
        use super::super::encounter::SELECTABLE_MONSTERS;
        for seed in 0..200u64 {
            let tag = random_encounter(seed);
            let entry = SELECTABLE_MONSTERS
                .iter()
                .find(|(t, _, _)| *t == tag.as_str())
                .unwrap_or_else(|| panic!("random_encounter returned unknown tag {tag:?}"));
            assert!(
                entry.2,
                "random_encounter returned STS1 stub {tag} (seed {seed})"
            );
        }
    }

    #[test]
    fn random_encounter_is_deterministic() {
        let a = random_encounter(101);
        let b = random_encounter(101);
        assert_eq!(a, b);
        // Returned tag must be a known SELECTABLE_MONSTERS entry.
        use super::super::encounter::SELECTABLE_MONSTERS;
        assert!(SELECTABLE_MONSTERS.iter().any(|(t, _, _)| *t == a.as_str()));
    }

    #[test]
    fn streams_are_independent() {
        // Smoke check: per-input salts produce decorrelated streams.
        // Build the four streams from the same seed and assert that
        // tweaking ONE salt does not move the others. (Indirectly
        // demonstrated by the per-stream determinism tests above; this
        // one exercises that distinct salts produce distinct first
        // u64s for the same input seed.)
        let seed = 31337u64;
        let deck_u = fork(seed, SALT_DECK).gen::<u64>();
        let relic_u = fork(seed, SALT_RELIC).gen::<u64>();
        let potion_u = fork(seed, SALT_POTION).gen::<u64>();
        let enc_u = fork(seed, SALT_ENCOUNTER).gen::<u64>();
        assert_ne!(deck_u, relic_u);
        assert_ne!(deck_u, potion_u);
        assert_ne!(deck_u, enc_u);
        assert_ne!(relic_u, potion_u);
        assert_ne!(relic_u, enc_u);
        assert_ne!(potion_u, enc_u);
    }
}
