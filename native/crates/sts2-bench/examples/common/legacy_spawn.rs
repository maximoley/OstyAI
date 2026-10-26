//! The `sts2-cli` scenario builder's monsters as they were before GH #762, for
//! the benchmark harnesses' `--legacy-spawn` flag. Included with
//! `#[path = "common/legacy_spawn.rs"] mod legacy_spawn;`.
//!
//! The 2026-10-14 site numbers (`docs/perf/2026-10-14-site-performance.md`)
//! were measured on that builder, and the site and the bench scripts reproduce
//! them with `--legacy-spawn`. The builder now spawns monsters the way a run
//! does (`sts2_content::run_bridge::spawn_roster`), so this module rebuilds the
//! old start state on top of it. Never use it for anything but reproducing
//! those numbers: every fight it builds is unfaithful.

use sts2_cli::sim::scenario::{MonsterSpec, Scenario};
use sts2_content::monster_dispatch::{build_ai_with_ascension, MonsterSpawnArgs, SpawnPosition};
use sts2_core::run_rng_set::RunRngSet;
use sts2_core::{CombatState, MonsterKind};

/// Turn a freshly built (not yet started) scenario state for `specs` and
/// `seed` back into the pre-#762 builder's:
/// * every monster's AI built at `SpawnPosition.index` 0, front unless the
///   spec's `ai_args` say otherwise, with no spawn args except Nibbit's;
/// * HP rolled per monster, in order and without uniqueness, off
///   `Rng::new(seed)`, which also becomes the legacy combat RNG at the cursor
///   those rolls leave it at;
/// * the `niche` stream unread.
///
/// The rest is already as it was. Act 1 has no monster that debuffs the
/// player on spawn, and the player's mirror creature, now wired by the
/// builder rather than by `start_combat`, gets the same id and the same
/// values either way.
pub fn respawn_legacy(state: &mut CombatState, specs: &[MonsterSpec], seed: u64) {
    assert_eq!(state.enemy_uids.len(), specs.len(), "one creature per spec");
    // The positional `ai_args` the builder read, as `Scenario::from_monsters`
    // fixes them up.
    let specs = Scenario::from_monsters(specs.to_vec()).monsters;
    let mut rng = sts2_core::dotnet_rng::Rng::new(seed);
    for (i, spec) in specs.iter().enumerate() {
        let hp = spec.hp_override.map_or_else(
            || spec.kind.starting_hp(&mut rng, state.ascension),
            |h| i32::try_from(h).expect("hp fits i32"),
        );
        let args = match spec.kind {
            MonsterKind::Nibbit => MonsterSpawnArgs::Nibbit {
                is_alone: spec.ai_args.is_alone,
                is_front: spec.ai_args.is_front,
            },
            _ => MonsterSpawnArgs::None,
        };
        let position = SpawnPosition {
            alone: spec.ai_args.is_alone,
            front: spec.ai_args.is_front,
            back: !spec.ai_args.is_front,
            index: 0,
        };
        let ai = build_ai_with_ascension(spec.kind, args, position, state.ascension);
        let uid = state.enemy_uids[i];
        let creature = state
            .creatures
            .iter_mut()
            .find(|c| c.id == uid.0)
            .expect("enemy uid in the creature store");
        assert_eq!(
            creature.species,
            Some(spec.kind),
            "creature {i} is spec {i}"
        );
        creature.current_hp = hp;
        creature.max_hp = hp;
        creature.ai = Some(ai);
        // The encounter slot lives on the AI.
        if let Some(slot) = spec.slot {
            state.set_enemy_slot(uid, slot);
        }
    }
    state.rng = rng;
    state.run_rng.niche = RunRngSet::new(seed).niche;
}
