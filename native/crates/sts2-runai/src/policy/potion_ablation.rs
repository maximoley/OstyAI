//! Potion headroom ablations (`STS2_RUNAI_POTION_ABLATION`).
//!
//! Both the OC charge and the swap rule measured ≈0–1.5pp, none of it
//! significant. Before spending more on potion *policy*, these two seams
//! bound what potion policy can be worth at all:
//!
//! - `no-drinks` — the search may never take a `DrinkPotion` at the root,
//!   for every fight of the run. Baseline − this arm = **the total value
//!   of the whole potion system** to the agent (the denominator every
//!   potion result should be read against).
//! - `boss-oracle` — every boss fight is entered with a FULL belt, free.
//!   Baseline vs this arm = the **upper bound on all acquisition work**
//!   (skip / discard / swap / shop-buy combined): if perfect availability
//!   is worth little, that whole line is dead without building it.
//!
//! Measurement-only seams: like the other potion env vars they PANIC on
//! an unknown value rather than silently running an unablated arm.
//!
//! Known bias, deliberately accepted (`no-drinks`): held potions are
//! still acquired, so gold spent on shop potions is wasted in that arm,
//! which can only make the potion system look MORE valuable than it is.
//! The expected finding (small value) is therefore robust to it; a large
//! reading would need this refined before it could be trusted.
//!
//! `boss-oracle` fills with the GAME's own drop distribution — rarity
//! rolled 65% common / 25% uncommon / 10% rare, then uniform within the
//! class, mirroring `sts2_content::potions::all::create_random_potions_
//! from_pool` — off a deterministic per-(seed, floor) `splitmix64`
//! stream. It does NOT touch any engine RNG stream, so the rest of the
//! run is unperturbed and the arm is reproducible. The belt is therefore
//! "what the acquisition stream actually offers", not "the best
//! potions": this bounds perfect *availability*, not perfect
//! *selection*.
//!
//! (The first cut drew uniformly over common+uncommon only. That
//! over-sampled uncommons ~50/50 against the real 72/28 while dropping
//! rares entirely — two opposite biases of similar size. Matching the
//! real table removes the question instead of arguing it cancels.)

use sts2_core::potions::{CharacterId, PotionKind, PotionRarity};

/// Resolved `STS2_RUNAI_POTION_ABLATION` mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PotionAblation {
    /// No drink may be taken at the search root, all fights.
    NoDrinks,
    /// Boss fights start with a full belt.
    BossOracle,
    /// +2 potion slots for the whole run (`slots+2`).
    ///
    /// The realism upgrade on `boss-oracle`: that mode conjures potions,
    /// which no policy can do. Slots ARE acquirable in game (the Potion
    /// Belt relic, +2), so whatever this arm gains is reachable by a
    /// shop / boss-relic preference. Measured 2026-08-23: the A8 base
    /// cap is **2** (3 minus A4 TightBelt) and the belt sits full 8.6%
    /// (stack off) to 20.4% (stack on) of decisions, so capacity — not
    /// acquisition — is the binding constraint. The agent already BUYS
    /// more potions than winning humans (0.67–0.87/run vs 0.4).
    SlotsPlus2,
}

impl PotionAblation {
    /// Pure resolver (env value injected for testability). Unset ⇒ `None`
    /// ⇒ deployed behavior untouched; unknown ⇒ panic.
    pub(crate) fn resolve(env: Option<&str>) -> Option<Self> {
        let raw = env?.trim();
        match raw {
            "no-drinks" => Some(Self::NoDrinks),
            "boss-oracle" => Some(Self::BossOracle),
            "slots+2" => Some(Self::SlotsPlus2),
            other => panic!(
                "STS2_RUNAI_POTION_ABLATION: unknown mode {other:?} \
                 (expected \"no-drinks\", \"boss-oracle\" or \"slots+2\")"
            ),
        }
    }
}

/// The pool a `boss-oracle` belt is filled from: every character-eligible,
/// non-event-only potion — the same eligibility the game's own reward
/// roll uses. Rarity is applied at draw time, not by filtering here.
fn oracle_pool(character: CharacterId, rarity: PotionRarity) -> Vec<PotionKind> {
    PotionKind::ALL
        .iter()
        .copied()
        .filter(|k| {
            let spec = sts2_content::potions::all::spec_for(*k);
            !spec.is_event_only
                && spec.rarity == rarity
                && !matches!(spec.character_lock, Some(c) if c != character)
        })
        .collect()
}

/// The game's rarity roll (`create_random_potions_from_pool`): ≤0.10
/// rare, ≤0.35 uncommon, else common.
fn rarity_for(roll: f64) -> PotionRarity {
    if roll <= 0.10 {
        PotionRarity::Rare
    } else if roll <= 0.35 {
        PotionRarity::Uncommon
    } else {
        PotionRarity::Common
    }
}

/// Fill `belt` to `cap` with deterministic draws from the eligible pool.
/// Held potions are kept (an oracle tops up, it does not replace).
pub(crate) fn fill_belt_to_cap(
    belt: &mut Vec<PotionKind>,
    cap: usize,
    character: CharacterId,
    run_seed: impl sts2_core::dotnet_rng::RngSeed,
    total_floor: i32,
) {
    if belt.len() >= cap {
        return;
    }
    let base = run_seed.rng_seed() ^ (u64::from(total_floor as u32) << 32);
    let mut i = 0u64;
    let mut next = || {
        let v = super::splitmix64(base ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        i += 1;
        v
    };
    while belt.len() < cap {
        #[allow(clippy::cast_precision_loss)] // 53-bit mantissa by construction
        let roll = (next() >> 11) as f64 / (1u64 << 53) as f64;
        let pool = oracle_pool(character, rarity_for(roll));
        if pool.is_empty() {
            return;
        }
        belt.push(pool[(next() % pool.len() as u64) as usize]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_maps_the_two_modes_and_leaves_unset_alone() {
        assert_eq!(PotionAblation::resolve(None), None);
        assert_eq!(
            PotionAblation::resolve(Some("no-drinks")),
            Some(PotionAblation::NoDrinks)
        );
        assert_eq!(
            PotionAblation::resolve(Some(" boss-oracle ")),
            Some(PotionAblation::BossOracle)
        );
        assert_eq!(
            PotionAblation::resolve(Some("slots+2")),
            Some(PotionAblation::SlotsPlus2)
        );
    }

    #[test]
    #[should_panic(expected = "unknown mode")]
    fn resolver_panics_on_typos() {
        PotionAblation::resolve(Some("no_drinks"));
    }

    #[test]
    fn oracle_pool_is_character_eligible_and_rarity_pure() {
        for rarity in [
            PotionRarity::Common,
            PotionRarity::Uncommon,
            PotionRarity::Rare,
        ] {
            let pool = oracle_pool(CharacterId::Ironclad, rarity);
            assert!(!pool.is_empty(), "{rarity:?} pool empty");
            for k in &pool {
                let spec = sts2_content::potions::all::spec_for(*k);
                assert!(!spec.is_event_only);
                assert_eq!(spec.rarity, rarity);
                assert!(!matches!(spec.character_lock, Some(c) if c != CharacterId::Ironclad));
            }
        }
    }

    #[test]
    fn rarity_roll_matches_the_games_65_25_10_table() {
        assert_eq!(rarity_for(0.0), PotionRarity::Rare);
        assert_eq!(rarity_for(0.10), PotionRarity::Rare);
        assert_eq!(rarity_for(0.11), PotionRarity::Uncommon);
        assert_eq!(rarity_for(0.35), PotionRarity::Uncommon);
        assert_eq!(rarity_for(0.36), PotionRarity::Common);
        assert_eq!(rarity_for(1.0), PotionRarity::Common);
    }

    /// The empirical mix over many (seed, floor) fills must track the
    /// game's table — the whole point of the rewrite.
    #[test]
    fn empirical_fill_mix_tracks_the_drop_table() {
        let mut counts = [0usize; 3]; // common, uncommon, rare
        for seed in 0..400i32 {
            let mut belt = Vec::new();
            fill_belt_to_cap(&mut belt, 4, CharacterId::Ironclad, seed, seed % 50);
            for k in &belt {
                match sts2_content::potions::all::spec_for(*k).rarity {
                    PotionRarity::Common => counts[0] += 1,
                    PotionRarity::Uncommon => counts[1] += 1,
                    PotionRarity::Rare => counts[2] += 1,
                    other => panic!("oracle drew a {other:?}-rarity potion"),
                }
            }
        }
        let n = counts.iter().sum::<usize>() as f64;
        let share = |i: usize| counts[i] as f64 / n;
        assert!((share(0) - 0.65).abs() < 0.06, "common {:.3}", share(0));
        assert!((share(1) - 0.25).abs() < 0.06, "uncommon {:.3}", share(1));
        assert!((share(2) - 0.10).abs() < 0.05, "rare {:.3}", share(2));
    }

    #[test]
    fn fill_tops_up_to_cap_keeping_held_and_is_deterministic() {
        let mut belt = vec![PotionKind::FirePotion];
        fill_belt_to_cap(&mut belt, 5, CharacterId::Ironclad, 7, 33);
        assert_eq!(belt.len(), 5);
        assert_eq!(belt[0], PotionKind::FirePotion, "held potion kept first");

        let mut again = vec![PotionKind::FirePotion];
        fill_belt_to_cap(&mut again, 5, CharacterId::Ironclad, 7, 33);
        assert_eq!(belt, again, "same (seed, floor) ⇒ same belt");

        let mut other_floor = vec![PotionKind::FirePotion];
        fill_belt_to_cap(&mut other_floor, 5, CharacterId::Ironclad, 7, 34);
        assert_ne!(belt, other_floor, "the draw varies with the floor");
    }

    #[test]
    fn fill_is_a_noop_on_a_full_belt() {
        let mut belt = vec![PotionKind::FirePotion; 5];
        fill_belt_to_cap(&mut belt, 5, CharacterId::Ironclad, 7, 33);
        assert_eq!(belt.len(), 5);
    }
}
