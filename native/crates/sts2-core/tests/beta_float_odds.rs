//! Exact source-float boundaries with valid serialized Xoshiro256** states.
use sts2_core::card_rewards::{PotionRewardOdds, RewardRarity};
use sts2_core::dotnet_rng::{Rng, RngState};
use sts2_core::shop::{ShopPotionRarity, ShopRelicRarity};
use sts2_core::RelicRarity;

fn rng_at(value: f32) -> Rng {
    let raw = ((f64::from(value) * 9_007_199_254_740_992.0) as u64) << 11;
    // Invert xoshiro256** output rotl(s1*5,7)*9; remaining state words
    // are nonzero so this is a valid resumed generator.
    let s1 = raw
        .wrapping_mul(0x8e38_e38e_38e3_8e39)
        .rotate_right(7)
        .wrapping_mul(0xcccc_cccc_cccc_cccd);
    let mut rng = Rng::new(0_u64);
    rng.load_state(RngState {
        counter: 0,
        s0: 1,
        s1,
        s2: 2,
        s3: 3,
    });
    let mut probe = rng.clone();
    assert_eq!((probe.next_float(1.0) as f32).to_bits(), value.to_bits());
    rng
}

#[test]
fn beta_relic_factories_use_strict_source_float_thresholds() {
    let boundary = 0.83_f32;
    for (value, expected) in [
        (
            f32::from_bits(boundary.to_bits() - 1),
            RelicRarity::Uncommon,
        ),
        (boundary, RelicRarity::Rare),
    ] {
        let mut rng = rng_at(value);
        assert_eq!(sts2_core::runstate::roll_relic_rarity(&mut rng), expected);
        assert_eq!(rng.counter(), 1);
    }
    assert_eq!(
        sts2_core::shop::roll_relic_rarity(&mut rng_at(boundary)),
        ShopRelicRarity::Rare
    );
    let mut run = sts2_core::RunState::default();
    run.rng.treasure_room = rng_at(boundary);
    assert_eq!(run.roll_treasure_relic_rarity(), RewardRarity::Rare);
}

#[test]
fn beta_shop_potion_rarity_includes_source_float_endpoints() {
    assert_eq!(
        sts2_core::shop::roll_potion_rarity(&mut rng_at(0.1_f32)),
        ShopPotionRarity::Rare
    );
    assert_eq!(
        sts2_core::shop::roll_potion_rarity(&mut rng_at(f32::from_bits(0.1_f32.to_bits() + 1))),
        ShopPotionRarity::Uncommon
    );
    assert_eq!(
        sts2_core::shop::roll_potion_rarity(&mut rng_at(0.35_f32)),
        ShopPotionRarity::Uncommon
    );
    assert_eq!(
        sts2_core::shop::roll_potion_rarity(&mut rng_at(f32::from_bits(0.35_f32.to_bits() + 1))),
        ShopPotionRarity::Common
    );
}

#[test]
fn beta_adaptive_potion_elite_threshold_and_updates_round_as_float() {
    let mut odds = PotionRewardOdds::new();
    let mut rng = rng_at(0.525_f32);
    assert!(
        !odds.roll(&mut rng, true, false),
        "source comparison is strict at float .525"
    );
    assert_eq!(odds.current_value.to_bits(), f64::from(0.5_f32).to_bits());
    assert_eq!(rng.counter(), 1);
    let mut odds = PotionRewardOdds::new();
    let mut rng = rng_at(f32::from_bits(0.525_f32.to_bits() - 1));
    assert!(odds.roll(&mut rng, true, false));
    assert_eq!(odds.current_value.to_bits(), f64::from(0.3_f32).to_bits());
    assert_eq!(rng.counter(), 1);
    let before = odds;
    let state = rng.state();
    assert!(odds.roll(&mut rng, true, true));
    assert_eq!(odds, before);
    assert_eq!(rng.state(), state);
}
