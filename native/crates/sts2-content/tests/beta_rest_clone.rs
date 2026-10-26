//! Source-backed Pael's Growth clone lifecycle, including deck observers.
use sts2_core::cards::CardId;
use sts2_core::cards::EnchantmentId;
use sts2_core::runstate::{RestSiteActionOutcome, RestSiteOption, RunState};
use sts2_core::RelicId;

#[test]
fn beta_rest_clone_observes_each_snapshot_copy_without_consuming_rng() {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    let mut original =
        sts2_content::cards::spec_index::card_pod_from_id(CardId::DefendNecrobinder, false)
            .unwrap();
    original.set_enchantment(EnchantmentId::Clone, 4);
    let mut rs = RunState {
        relics: vec![RelicId::PaelsGrowth, RelicId::ToxicEgg, RelicId::LuckyFysh],
        master_deck: vec![original],
        ..RunState::default()
    };
    let gold = rs.gold;
    let rng = rs.rng.clone();
    assert_eq!(
        rs.do_rest_site_action(RestSiteOption::Clone),
        RestSiteActionOutcome::Cloned
    );
    assert_eq!(rs.master_deck.len(), 2);
    assert_eq!(rs.master_deck[0], original);
    assert!(rs.master_deck[1].upgraded);
    assert_eq!(rs.master_deck[1].base_block, 8);
    assert!(rs.master_deck[1].has_enchantment(EnchantmentId::Clone));
    assert_eq!(rs.gold, gold + 15);
    assert_eq!(rs.rng, rng);
    assert_eq!(
        rs.do_rest_site_action(RestSiteOption::Clone),
        RestSiteActionOutcome::Cloned
    );
    assert_eq!(rs.master_deck.len(), 4);
    assert_eq!(rs.master_deck[0], original);
    assert!(rs.master_deck[2..]
        .iter()
        .all(|c| c.upgraded && c.has_enchantment(EnchantmentId::Clone)));
    assert_eq!(rs.gold, gold + 45);
    assert_eq!(rs.rng, rng);
}
