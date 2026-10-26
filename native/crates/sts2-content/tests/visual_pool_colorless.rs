//! Regression: `is_visual_pool_colorless` mirrors C#
//! `CardModel.VisualCardPool.IsColorless` — true for the Colorless, Token and
//! Event pools, false for character pools, Status and Curse.
//!
//! HeirloomHammer (HeirloomHammer.cs:33) filters hand cards by this property
//! before cloning one. The old `is_colorless_card_id` list omitted Token cards
//! like SOVEREIGN_BLADE, so HeirloomHammer cloned a different colorless hand
//! card than C# (Regent fuzz seeds 42 / 366 / 406). Bead `sts2-rs-1rv1q`.

use sts2_content::cards::colorless::is_visual_pool_colorless;
use sts2_core::cards::CardId;

#[test]
fn token_cards_are_visual_pool_colorless() {
    // SOVEREIGN_BLADE is a Token (CardRarity::Token) — TokenCardPool.IsColorless.
    assert!(
        is_visual_pool_colorless(CardId::SovereignBlade),
        "SOVEREIGN_BLADE (Token) must count as colorless for HeirloomHammer",
    );
}

#[test]
fn colorless_proper_cards_are_visual_pool_colorless() {
    // Both EQUILIBRIUM and SECRET_WEAPON are colorless-proper.
    assert!(is_visual_pool_colorless(CardId::Equilibrium));
    assert!(is_visual_pool_colorless(CardId::SecretWeapon));
    assert!(is_visual_pool_colorless(CardId::Alchemize));
}

#[test]
fn character_cards_are_not_visual_pool_colorless() {
    // Character-pool cards (RegentCardPool / IroncladCardPool) are NOT colorless.
    assert!(
        !is_visual_pool_colorless(CardId::StrikeIronclad),
        "an Ironclad card must not count as colorless",
    );
    assert!(
        !is_visual_pool_colorless(CardId::HeirloomHammer),
        "a Regent Rare must not count as colorless",
    );
}

#[test]
fn status_cards_are_not_visual_pool_colorless() {
    // StatusCardPool.IsColorless => false — must stay excluded even though the
    // sts2-core character map labels status cards "colorless".
    assert!(
        !is_visual_pool_colorless(CardId::Slimed),
        "SLIMED (Status) must not count as colorless",
    );
    assert!(
        !is_visual_pool_colorless(CardId::Dazed),
        "DAZED (Status) must not count as colorless",
    );
}
