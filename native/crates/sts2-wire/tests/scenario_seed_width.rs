//! Public scenario construction must retain the beta's full master seed.
use sts2_wire::scenario::build_scenario_state;

#[test]
fn beta_scenario_setup_preserves_all_64_seed_bits() {
    let deck = [
        "Bodyguard",
        "StrikeNecrobinder",
        "DefendNecrobinder",
        "Soul",
        "BorrowedTime",
    ]
    .map(str::to_owned);
    for seed in [7_898_842_083_224_463_486_u64, u64::MAX] {
        let state = build_scenario_state(
            seed,
            "Necrobinder",
            &deck,
            70,
            80,
            &[],
            &[],
            "SLITHERING_STRANGLER_NORMAL",
            0,
            None,
            None,
        )
        .unwrap();
        assert_eq!(state.run_rng.seed, seed);
        assert_eq!(state.rng.seed(), seed);
        let expected = sts2_core::RunRngSet::new(seed);
        assert_eq!(state.run_rng.shuffle.seed(), expected.shuffle.seed());
        assert_eq!(state.run_rng.niche.seed(), expected.niche.seed());
        let narrowed = build_scenario_state(
            u64::from(seed as u32),
            "Necrobinder",
            &deck,
            70,
            80,
            &[],
            &[],
            "SLITHERING_STRANGLER_NORMAL",
            0,
            None,
            None,
        )
        .unwrap();
        assert_ne!(
            sts2_core::state_hash(&state),
            sts2_core::state_hash(&narrowed)
        );
    }
}
