//! `ftl_window_probe` — GH #403 measurement harness: how often does the
//! search play FTL while its draw window is still open?
//!
//! FTL reads `Deal 5/6 damage. If you have played fewer than 3/4 cards
//! this turn, draw 1 card.` (C# `Ftl.cs`, `PlayMax` 3 base / 4 upgraded).
//! At 0 energy the rider is free upside with no downside, so a human plays
//! it early; #403 reports runs where it went late and the draw never fired.
//!
//! This probe plays whole MCTS-driven combats over realistic random decks
//! with FTL shuffled in, and reports what fraction of FTL plays landed
//! inside the window. Unlike the unit fixture in
//! `sts2-mcts/tests/ftl_draw_window.rs` (which pins a single forced
//! position), this samples the *distribution* the run actually sees.
//!
//! ```sh
//! cargo build --release --example ftl_window_probe
//! target/release/examples/ftl_window_probe [fights] [sims] [--upgraded]
//! ```

#[path = "common/mod.rs"]
mod common;

use std::time::Instant;

use sts2_content::cards::spec_index::{DEFECT_SPECS, IRONCLAD_SPECS};
use sts2_content::cards::CardSpec;
use sts2_core::cards::CardId;
use sts2_core::{apply, is_terminal, legal_actions, Action, CombatState};
use sts2_mcts::{Mcts, MctsConfig};

const DECK_SIZE: usize = 24;
const FTL_COPIES: usize = 2;
const MAX_ACTIONS_PER_FIGHT: u32 = 400;
/// Encounters spread across the Act-1 difficulty band so the sample is not
/// dominated by one saturation regime — the search's ability to price a
/// marginal card draw collapses as the leaf eval saturates.
const ENCOUNTERS: [&str; 4] = [
    "FLYCONID_NORMAL",
    "MAWLER_NORMAL",
    "BYRDONIS_ELITE",
    "VANTOM_BOSS",
];

/// A random full-pool deck plus [`FTL_COPIES`] FTL, mirroring
/// `mcts_realistic_profile::random_deck`'s sampling.
fn deck_with_ftl(seed: u64) -> Vec<CardSpec> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let mut rng = sts2_core::dotnet_rng::Rng::new(seed ^ 0xDEC0_DEC0_DEC0_DEC0);
    let mut deck: Vec<CardSpec> = (0..DECK_SIZE - FTL_COPIES)
        .map(|_| {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                clippy::cast_sign_loss
            )]
            let idx = rng.next_int_exclusive(0, IRONCLAD_SPECS.len() as i32) as usize;
            IRONCLAD_SPECS[idx]()
        })
        .collect();
    let ftl = DEFECT_SPECS
        .iter()
        .map(|f| f())
        .find(|s| s.id == CardId::Ftl)
        .expect("FTL is in DEFECT_SPECS");
    for _ in 0..FTL_COPIES {
        deck.push(ftl);
    }
    deck
}

#[derive(Default, Debug)]
struct Tally {
    /// FTL plays whose rider fired (window open AND something to draw).
    fired: u32,
    /// FTL plays whose window had already closed.
    missed: u32,
    /// FTL plays where the window was open but both piles were empty, so
    /// the rider was a no-op regardless of ordering. Not a defect.
    moot: u32,
    /// Fights won, and end-of-fight player HP summed over all fights —
    /// the guard rail that the tie-break is not buying window hits at the
    /// cost of actually playing worse.
    wins: u32,
    hp: i64,
}

impl Tally {
    fn merge(&mut self, o: &Self) {
        self.fired += o.fired;
        self.missed += o.missed;
        self.moot += o.moot;
        self.wins += o.wins;
        self.hp += o.hp;
    }
    fn decided(&self) -> u32 {
        self.fired + self.missed
    }
}

fn play_one(mut s: CombatState, mcts_seed: u64, sims: u32, upgraded: bool) -> Tally {
    let mut tally = Tally::default();
    let play_max = if upgraded { 4 } else { 3 };
    let mut action_count: u32 = 0;
    while !is_terminal(&s) && action_count < MAX_ACTIONS_PER_FIGHT {
        let acts = legal_actions(&s);
        if acts.is_empty() {
            let _ = apply(&mut s, Action::EndTurn);
            action_count += 1;
            continue;
        }
        let cfg = MctsConfig {
            max_simulations: sims,
            seed: mcts_seed
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(u64::from(action_count)),
            ..MctsConfig::default()
        };
        let mut mcts = Mcts::new(s.clone(), cfg);
        let rec = mcts.search();
        if let Action::PlayCard { hand_index, .. } = &rec {
            let is_ftl = s
                .player
                .hand
                .get(*hand_index as usize)
                .and_then(|uid| s.cards.get(uid.0.get() as usize - 1))
                .is_some_and(|c| c.card_id == CardId::Ftl);
            if is_ftl {
                let open = s.count_player_cards_played_this_turn(None) < play_max;
                let drawable = !(s.player.draw.is_empty() && s.player.discard.is_empty());
                if !open {
                    tally.missed += 1;
                } else if drawable {
                    tally.fired += 1;
                } else {
                    tally.moot += 1;
                }
            }
        }
        let _ = apply(&mut s, rec);
        action_count += 1;
    }
    tally.hp = i64::from(s.player.current_hp.max(0));
    tally.wins = u32::from(is_terminal(&s) && s.player.current_hp > 0);
    tally
}

fn main() {
    sts2_content::cards::dispatch::ensure_installed();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let upgraded = args.iter().any(|a| a == "--upgraded");
    let mut pos = args.iter().filter(|a| !a.starts_with("--"));
    let fights: u32 = pos.next().and_then(|s| s.parse().ok()).unwrap_or(24);
    let sims: u32 = pos.next().and_then(|s| s.parse().ok()).unwrap_or(1000);

    eprintln!(
        "[ftl_window_probe] fights={fights} sims={sims} upgraded={upgraded} \
         play_max={}",
        if upgraded { 4 } else { 3 }
    );

    let start = Instant::now();
    let mut total = Tally::default();
    for fight in 0..fights {
        let encounter = ENCOUNTERS[fight as usize % ENCOUNTERS.len()];
        let deck = deck_with_ftl(u64::from(fight).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let engine_seed = u64::from(fight) ^ 0xA5A5_A5A5_A5A5_A5A5;
        let mut s = common::build_ironclad_state_with_deck(encounter, engine_seed, &deck);
        if upgraded {
            for c in &mut s.cards {
                if c.card_id == CardId::Ftl {
                    c.upgraded = true;
                    c.base_damage += 1;
                }
            }
        }
        let t = play_one(s, u64::from(fight).wrapping_add(1), sims, upgraded);
        total.merge(&t);
    }

    let decided = total.decided();
    #[allow(clippy::cast_precision_loss)]
    let pct = if decided == 0 {
        0.0
    } else {
        100.0 * f64::from(total.fired) / f64::from(decided)
    };
    println!(
        "fights={fights} sims={sims} upgraded={upgraded} \
         ftl_plays_in_window={}/{} ({pct:.1}%) moot={} wins={}/{fights} mean_hp={:.1} wall={:.1}s",
        total.fired,
        decided,
        total.moot,
        total.wins,
        f64::from(u32::try_from(total.hp).unwrap_or(u32::MAX)) / f64::from(fights),
        start.elapsed().as_secs_f64(),
    );
}
