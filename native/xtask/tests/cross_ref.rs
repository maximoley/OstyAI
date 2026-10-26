use xtask_lib::split_cards::emit_tree_to_string_map;

const SRC: &str = "\
fn play_strike(s: &mut S, t: Option<U>) -> EffectResult { EffectResult::Resolved }
fn play_twin_strike(s: &mut S, t: Option<U>) -> EffectResult { play_strike(s, t); play_strike(s, t) }
pub fn install_pool() { card_effects::register_many(POOL); }
const POOL: &[(CardId, PlayCardFn)] = &[
    (CardId::StrikeIronclad, play_strike as PlayCardFn),
    (CardId::TwinStrike, play_twin_strike as PlayCardFn),
];
";

#[test]
fn cross_card_call_gets_explicit_use() {
    let map = emit_tree_to_string_map(SRC, "ironclad");
    assert!(map["twin_strike.rs"].contains("use super::strike::play_strike;"));
    // and the self file does not import itself
    assert!(!map["strike.rs"].contains("use super::twin_strike"));
}
