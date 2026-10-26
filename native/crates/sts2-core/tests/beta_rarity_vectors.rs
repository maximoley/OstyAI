//! Results from unmodified v0.111.0 CardRarityOdds.Roll in the verified beta DLL.
use serde::Deserialize;
use sts2_core::card_rewards::{CardRarityOdds, RarityContext, RewardRarity};
use sts2_core::dotnet_rng::Rng;

#[derive(Deserialize)]
struct Fixture {
    assembly_sha256: String,
    sequences: Vec<Sequence>,
}
#[derive(Deserialize)]
struct Sequence {
    ascension: u8,
    seed: u64,
    initial_bits: i32,
    steps: Vec<Step>,
}
#[derive(Deserialize)]
struct Step {
    kind: String,
    rarity: String,
    before_bits: i32,
    after_bits: i32,
    counter: u32,
}

#[test]
fn beta_rarity_outcomes_and_every_pity_float_bit_match_actual_dll() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/beta_rarity_vectors.json")).unwrap();
    assert_eq!(
        fixture.assembly_sha256,
        "9cb4f1ad8c9f284aa8fec3122ffd6d780bbf543d875c817abdd12ff63fbf12b4"
    );
    assert_eq!(fixture.sequences.len(), 24);
    for sequence in fixture.sequences {
        let mut odds = CardRarityOdds::new(sequence.ascension);
        odds.current_value = f64::from(f32::from_bits(sequence.initial_bits as u32));
        let mut rng = Rng::new(sequence.seed);
        for (index, step) in sequence.steps.into_iter().enumerate() {
            let context = match step.kind.as_str() {
                "RegularEncounter" => RarityContext::Regular,
                "EliteEncounter" => RarityContext::Elite,
                "Shop" => RarityContext::Shop,
                "Uniform" => RarityContext::Uniform,
                "BossEncounter" => RarityContext::Boss,
                other => panic!("unexpected source context {other}"),
            };
            let expected = match step.rarity.as_str() {
                "Common" => RewardRarity::Common,
                "Uncommon" => RewardRarity::Uncommon,
                "Rare" => RewardRarity::Rare,
                other => panic!("unexpected source rarity {other}"),
            };
            assert_eq!(
                (odds.current_value as f32).to_bits(),
                step.before_bits as u32
            );
            assert_eq!(
                odds.roll_with_dotnet(&mut rng, context),
                expected,
                "A{} seed{} roll{}",
                sequence.ascension,
                sequence.seed,
                index
            );
            assert_eq!(
                (odds.current_value as f32).to_bits(),
                step.after_bits as u32,
                "A{} seed{} roll{} pity",
                sequence.ascension,
                sequence.seed,
                index
            );
            assert_eq!(rng.counter(), step.counter);
        }
    }
}

#[test]
fn beta_base_roll_includes_the_previously_missing_uncommon_band() {
    // Seed 47's first source float is 0.38535043597221375. Old base rolls
    // incorrectly classified it Common by comparing against 0.37 alone.
    let normal = CardRarityOdds::new(0);
    let before = normal.current_value;
    for context in [RarityContext::Regular, RarityContext::Shop] {
        let mut rng = Rng::new(47_u64);
        assert_eq!(
            normal.roll_with_base_odds_with_dotnet(&mut rng, context),
            RewardRarity::Uncommon
        );
        assert_eq!(rng.counter(), 1);
        assert_eq!(normal.current_value.to_bits(), before.to_bits());
    }
    let scarcity = CardRarityOdds::new(7);
    assert_eq!(
        scarcity.roll_with_base_odds_with_dotnet(&mut Rng::new(47_u64), RarityContext::Regular),
        RewardRarity::Common
    );
}
