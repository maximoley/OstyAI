//! Cross-build engine state-trace parity harness (perf-branch merge gate).
//!
//! Drives seeded uniform-random-agent combats across several encounters and
//! loadouts (bare, kitchen-sink relics + random-3 potions) and folds an
//! FNV-1a hash over `(action, StepResult, canonical parity-bridge state,
//! RNG cursors)` after EVERY transition. Two builds that print the same
//! final hash executed byte-identical engine behavior on this coverage —
//! run it on the baseline commit and on the candidate branch and diff the
//! one line.
//!
//! ```sh
//! cargo build --release --example state_trace_parity -p sts2-bench
//! target/release/examples/state_trace_parity [seeds_per_config]   # default 40
//! ```

use std::collections::BTreeMap;

use sts2_cli::sim::scenario::encounter::resolve_encounter;
use sts2_cli::sim::scenario::potions::{resolve_potions, PotionInput};
use sts2_cli::sim::scenario::random::random_deck;
use sts2_cli::sim::scenario::relics::{resolve_relics, seed_relics_with_counters, RelicInput};
use sts2_cli::sim::{build_starter_state_from_monsters, PlayerClass};
use sts2_core::parity::combat_state_to_bridge_state;
use sts2_core::{apply, legal_actions, CombatState};

const ENCOUNTERS: &[&str] = &[
    "MAWLER_NORMAL",
    "SLIMES_NORMAL",
    "INKLETS_NORMAL",
    "PHROG_PARASITE_ELITE",
    "VANTOM_BOSS",
];

fn fnv1a(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// Tiny deterministic PRNG for the random agent (host-independent).
struct Sm64(u64);
impl Sm64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

fn build_state(encounter: &str, seed: u64, with_loadout: bool) -> CombatState {
    let deck = random_deck(seed, 25, sts2_content::names::PlayerClassHint::Ironclad);
    let mut enc_rng = sts2_core::dotnet_rng::Rng::new(seed);
    let monsters = resolve_encounter(encounter, &mut enc_rng).expect("encounter resolves");
    let mut state = build_starter_state_from_monsters(
        seed,
        monsters,
        PlayerClass::Ironclad,
        None,
        None,
        Some(deck),
    );
    if with_loadout {
        let mut potion_rng = sts2_core::dotnet_rng::Rng::new(seed);
        let potions = resolve_potions(&PotionInput::Preset("random-3".into()), &mut potion_rng)
            .expect("potions resolve");
        for kind in &potions {
            state.player.potions.push(*kind);
        }
        state.player.potion_slots = 3;
        let relics =
            resolve_relics(&RelicInput::Preset("kitchen-sink".into())).expect("relics resolve");
        seed_relics_with_counters(&mut state, &relics, &BTreeMap::default()).expect("relics seed");
    }
    let _ = sts2_core::combat_loop::start_combat(&mut state);
    let _ = sts2_core::combat_loop::start_player_turn(&mut state);
    state
}

fn main() {
    sts2_content::cards::dispatch::install_default_registry();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();

    let seeds_per_config: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(40);

    let mut hash = 0xCBF2_9CE4_8422_2325_u64; // FNV offset basis
    let mut combats = 0u32;
    let mut transitions = 0u64;
    for &with_loadout in &[false, true] {
        for encounter in ENCOUNTERS {
            for seed in 1..=seeds_per_config {
                let mut state = build_state(encounter, seed, with_loadout);
                let mut agent = Sm64(seed ^ 0xA5A5_A5A5_A5A5_A5A5);
                let mut steps = 0u32;
                while !state.is_over && steps < 600 {
                    let acts = legal_actions(&state);
                    if acts.is_empty() {
                        break;
                    }
                    let action = acts[(agent.next() % acts.len() as u64) as usize].clone();
                    let result = apply(&mut state, action.clone());
                    // Canonical pointer-free state view: the parity bridge
                    // export (the same projection the Python-parity harness
                    // diffs), plus the RNG cursors it omits. A whole-state
                    // `Debug` render is NOT usable here — `MonsterAi`
                    // condition fn-pointers Debug-print ASLR-randomized
                    // addresses.
                    let bridge = serde_json::to_string(&combat_state_to_bridge_state(&state))
                        .expect("bridge state serializes");
                    let cursors = format!(
                        "|rng={}:{}|fp={:?}",
                        state.rng.seed(),
                        state.rng.counter(),
                        state.run_rng.combat_counter_fingerprint()
                    );
                    hash = fnv1a(hash, format!("{action:?}|{result:?}|").as_bytes());
                    hash = fnv1a(hash, bridge.as_bytes());
                    hash = fnv1a(hash, cursors.as_bytes());
                    steps += 1;
                    transitions += 1;
                }
                combats += 1;
            }
        }
    }
    println!("combats={combats} transitions={transitions} trace_hash={hash:#018x}");
}
