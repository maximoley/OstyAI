//! Source PotionFactory float endpoints, exercised through real pool generation.
use sts2_content::potions::all::{create_random_potions_out_of_combat, spec_for};
use sts2_core::dotnet_rng::{Rng, RngState};
use sts2_core::{CharacterId, PotionRarity};

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
fn beta_potion_factory_includes_exact_source_float_endpoints() {
    for (value, expected) in [
        (0.1_f32, PotionRarity::Rare),
        (
            f32::from_bits(0.1_f32.to_bits() + 1),
            PotionRarity::Uncommon,
        ),
        (0.35_f32, PotionRarity::Uncommon),
        (f32::from_bits(0.35_f32.to_bits() + 1), PotionRarity::Common),
    ] {
        let mut rng = rng_at(value);
        let potions = create_random_potions_out_of_combat(&mut rng, CharacterId::Necrobinder, 1);
        assert_eq!(potions.len(), 1);
        assert_eq!(spec_for(potions[0]).rarity, expected);
        assert_eq!(rng.counter(), 2, "rarity draw followed by pool index draw");
    }
}
