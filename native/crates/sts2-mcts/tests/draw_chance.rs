//! F1 draws-as-chance-events black-box regression gates (bead
//! `sts2-rs-i8bgu`, plan `docs/plans/2026-07-12-clairvoyance-removal-plan.md`
//! §3.2/G0). Complements the deeper white-box unit tests in
//! `crates/sts2-mcts/src/lib.rs`'s own `mod tests` (which have access to
//! `Mcts`'s private fields/methods — `chance_widen_or_sample`,
//! `realize_chance_outcome`, the exact detector condition, etc.) by
//! exercising `HiddenInfoMode::DrawChance` purely through the PUBLIC
//! `Mcts` surface (`Mcts::new`/`search`/`search_n`, `tree.arena`/`root`,
//! `EdgeAction`'s public variants, `NodeKind`), on the same starter-deck
//! fixture `tests/root_blindfold.rs` uses for `RootScramble` — the goal is
//! confidence that F1 behaves sanely on a REALISTIC deck, not just the
//! hand-crafted single-card fixtures used for precise detector control.
//!
//! Byte-stability note: `HiddenInfoMode::Clairvoyant` is untouched by this
//! bead — every pre-existing `sts2-mcts` test (in particular
//! `playable_strike_in_hand_search_value_beats_unplayable_curse_in_hand`,
//! `vs_random`, `strike_bash_ordering::never_plays_strike_before_bash`, and
//! `root_blindfold`'s own suite) passes unchanged suite-wide and pins that
//! contract — see the task report for the full list.

mod common;

use common::build_vertical_slice;
use sts2_mcts::{EdgeAction, HiddenInfoMode, Mcts, MctsConfig, NodeKind};

fn draw_chance_config(seed: u64, sims: u32) -> MctsConfig {
    MctsConfig {
        max_simulations: sims,
        seed,
        hidden_info: HiddenInfoMode::DrawChance,
        ..MctsConfig::default()
    }
}

/// `DrawChance` must still produce a usable search on a realistic
/// (starter-deck) fixture — the deeper unit tests use minimal hand-crafted
/// states to pin exact detector/dedup behavior, but a wiring mistake could
/// still leave the search unable to complete or panicking on a real deck.
#[test]
fn draw_chance_search_completes_on_starter_deck() {
    let state = build_vertical_slice(21);
    let mut mcts = Mcts::new(state, draw_chance_config(0x1CE, 300));
    let action = mcts.search();
    // A concrete action came back and is one of the root's own outcomes —
    // the same self-consistency check `root_blindfold.rs`-style tests rely
    // on implicitly via `root_outcomes()`.
    assert!(
        mcts.root_outcomes().iter().any(|o| o.action == action),
        "chosen action must be present in the root outcomes vec"
    );
}

/// Determinism: the same state + config searched twice under `DrawChance`
/// (fresh `Mcts` instances) must pick the same action — mirrors
/// `root_scramble_is_deterministic_for_fixed_seed` in `root_blindfold.rs`
/// for the F1 mode, on a realistic deck rather than the minimal fixture
/// the internal unit test uses.
#[test]
fn draw_chance_search_is_deterministic_for_fixed_seed_on_starter_deck() {
    let cfg = draw_chance_config(0xBADC_0FFE, 300);
    let action_1 = Mcts::new(build_vertical_slice(11), cfg).search();
    let action_2 = Mcts::new(build_vertical_slice(11), cfg).search();
    assert_eq!(
        format!("{action_1:?}"),
        format!("{action_2:?}"),
        "same state + config must reproduce the same chosen action under DrawChance"
    );
}

/// `Mcts::reroot`'s hard assert (bead `sts2-rs-vqckz`) already covers
/// `DrawChance` via `== Clairvoyant` — this pins that it fires for the
/// NEW variant too, not just `RootScramble` (a regression here would
/// silently re-leak hidden information through tree reuse).
#[test]
#[should_panic(expected = "reroot is unsupported under DrawChance")]
fn reroot_panics_under_draw_chance() {
    let state = build_vertical_slice(3);
    let cfg = draw_chance_config(0xDEAD_BEEF, 50);
    let mut mcts = Mcts::new(state.clone(), cfg);
    let action = mcts.search();
    let _ = mcts.reroot(&action, state);
}

/// A search whose root has no affordable card (drawing never fires from
/// the root) has no `PlayCard` edges to chance-node in the first place —
/// a light sanity check that `DrawChance` doesn't crash or misbehave when
/// the root's only legal action happens not to touch the draw pile.
#[test]
fn draw_chance_search_handles_end_turn_only_root() {
    let mut state = build_vertical_slice(5);
    // Drain energy to 0 so no card in hand is affordable — the only
    // legal root action is EndTurn.
    state.player.energy = 0;
    let mut mcts = Mcts::new(state, draw_chance_config(0xE0D0, 50));
    let action = mcts.search();
    assert_eq!(
        format!("{action:?}"),
        format!("{:?}", sts2_core::Action::EndTurn),
        "with 0 energy the only legal root action is EndTurn"
    );
    // Sanity: no PlayCard edge exists at the root under these conditions,
    // and the tree walk above didn't panic.
    let root = mcts.tree.root;
    assert!(
        !mcts.tree.arena[root.0 as usize]
            .children
            .iter()
            .any(|(edge, _)| matches!(edge, EdgeAction::PlayCard { .. })),
        "0 energy must leave no affordable PlayCard edge at the root"
    );
    // The EndTurn edge resolves the enemy turn (RNG-consuming) — it must
    // still have chance-noded normally under DrawChance.
    let end_turn_kind = mcts.tree.arena[root.0 as usize]
        .children
        .iter()
        .find_map(|(edge, cid)| matches!(edge, EdgeAction::Other(_)).then_some(*cid))
        .map(|cid| mcts.tree.arena[cid.0 as usize].kind);
    assert_eq!(
        end_turn_kind,
        Some(NodeKind::Chance),
        "EndTurn's enemy-turn roll must still chance-node under DrawChance"
    );
}
