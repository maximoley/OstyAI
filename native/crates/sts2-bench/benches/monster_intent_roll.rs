//! P5-8 (sts2-rs-2nx) monster intent-roll perf bench.
//!
//! Phase-5 target: ≤100 ns / intent roll across the full ~120-monster
//! `MonsterAi` DSL ctor set (plan v2.1 §Phase-5 perf target).
//!
//! What's measured: one full `MonsterAi::roll_move(seed)` call per
//! iteration. The roll advances the state machine through any
//! intervening `RandomBranch` / `ConditionalBranch` nodes to the next
//! `MoveState`, returning the intents vector. Pre-roll setup (ctor +
//! `on_move_performed`) lives outside `b.iter` so we measure just the
//! roll cost.
//!
//! Two views are reported:
//!   * `monster_intent_roll/full_set` — sum-loop across the full
//!     registry; headline `time / iter / N_ctors` is the ns/intent number.
//!   * `monster_intent_roll/single/<name>` — spot-check on 5 representative
//!     ctors so individual regressions surface.
//!
//! Run with: `cargo bench -p sts2-bench --bench monster_intent_roll`.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use sts2_core::CombatState;

use sts2_content::monsters::act1_overgrowth::{self, InkletSlot, KinFollowerSlot};
use sts2_content::monsters::act1_underdocks::{
    self, PhantasmalGardenerSlot, ToadpoleSlot, TwoTailedRatSlot,
};
use sts2_content::monsters::act1_weak;
use sts2_content::monsters::act2_hive::{self, ExoskeletonSlot, InfestedPrismSlot};
use sts2_content::monsters::act3_glory;
use sts2_content::monsters::shared::{self, DenseVegetationWrigglerSlot};
use sts2_core::monster_ai::MonsterAi;

/// Build the full ~120-monster DSL ctor registry. Module paths follow
/// the file layout in `crates/sts2-content/src/monsters/` (the file
/// names — `act1/act1_weak/act2/act3/act4/shared` — are the
/// authoritative module names; STS-style act numbering on the
/// monsters themselves is informational).
fn full_registry() -> Vec<(&'static str, MonsterAi)> {
    vec![
        // ── act1.rs (22 ctors) ──────────────────────────────────────────
        (
            "cubex_construct",
            act1_overgrowth::create_cubex_construct_ai(),
        ),
        ("flyconid", act1_overgrowth::create_flyconid_ai()),
        (
            "eye_with_teeth",
            act1_overgrowth::create_eye_with_teeth_ai(),
        ),
        ("fogmog", act1_overgrowth::create_fogmog_ai()),
        (
            "inklet_first",
            act1_overgrowth::create_inklet_ai(InkletSlot::First),
        ),
        (
            "inklet_second",
            act1_overgrowth::create_inklet_ai(InkletSlot::Second),
        ),
        (
            "inklet_other",
            act1_overgrowth::create_inklet_ai(InkletSlot::Other),
        ),
        ("mawler", act1_overgrowth::create_mawler_ai()),
        ("vine_shambler", act1_overgrowth::create_vine_shambler_ai()),
        (
            "slithering_strangler",
            act1_overgrowth::create_slithering_strangler_ai(),
        ),
        (
            "snapping_jaxfruit",
            act1_overgrowth::create_snapping_jaxfruit_ai(),
        ),
        (
            "assassin_ruby_raider",
            act1_overgrowth::create_assassin_ruby_raider_ai(),
        ),
        (
            "axe_ruby_raider",
            act1_overgrowth::create_axe_ruby_raider_ai(),
        ),
        (
            "brute_ruby_raider",
            act1_overgrowth::create_brute_ruby_raider_ai(),
        ),
        (
            "crossbow_ruby_raider",
            act1_overgrowth::create_crossbow_ruby_raider_ai(),
        ),
        (
            "tracker_ruby_raider",
            act1_overgrowth::create_tracker_ruby_raider_ai(),
        ),
        ("bygone_effigy", act1_overgrowth::create_bygone_effigy_ai()),
        ("byrdonis", act1_overgrowth::create_byrdonis_ai()),
        (
            "phrog_parasite",
            act1_overgrowth::create_phrog_parasite_ai(),
        ),
        ("parafright", act1_overgrowth::create_parafright_ai()),
        ("vantom", act1_overgrowth::create_vantom_ai()),
        (
            "ceremonial_beast",
            act1_overgrowth::create_ceremonial_beast_ai(),
        ),
        ("kin_priest", act1_overgrowth::create_kin_priest_ai()),
        (
            "kin_follower_first",
            act1_overgrowth::create_kin_follower_ai(KinFollowerSlot::First),
        ),
        (
            "kin_follower_second",
            act1_overgrowth::create_kin_follower_ai(KinFollowerSlot::Second),
        ),
        (
            "kin_follower_third",
            act1_overgrowth::create_kin_follower_ai(KinFollowerSlot::Third),
        ),
        // ── act1_weak.rs (8 instantiations of 7 ctors) ─────────────────
        ("shrinker_beetle", act1_weak::create_shrinker_beetle_ai()),
        (
            "fuzzy_wurm_crawler",
            act1_weak::create_fuzzy_wurm_crawler_ai(),
        ),
        (
            "nibbit_alone_front",
            act1_weak::create_nibbit_ai(true, true),
        ),
        (
            "nibbit_alone_back",
            act1_weak::create_nibbit_ai(true, false),
        ),
        (
            "nibbit_pack_front",
            act1_weak::create_nibbit_ai(false, true),
        ),
        (
            "nibbit_pack_back",
            act1_weak::create_nibbit_ai(false, false),
        ),
        ("leaf_slime_s", act1_weak::create_leaf_slime_s_ai()),
        ("twig_slime_s", act1_weak::create_twig_slime_s_ai()),
        ("leaf_slime_m", act1_weak::create_leaf_slime_m_ai()),
        ("twig_slime_m", act1_weak::create_twig_slime_m_ai()),
        // ── act2.rs (27 ctors) ──────────────────────────────────────────
        ("thieving_hopper", act2_hive::create_thieving_hopper_ai()),
        ("tunneler", act2_hive::create_tunneler_ai()),
        ("bowlbug_egg", act2_hive::create_bowlbug_egg_ai()),
        ("bowlbug_nectar", act2_hive::create_bowlbug_nectar_ai()),
        ("bowlbug_rock", act2_hive::create_bowlbug_rock_ai()),
        ("bowlbug_silk", act2_hive::create_bowlbug_silk_ai()),
        (
            "exoskeleton_first",
            act2_hive::create_exoskeleton_ai(ExoskeletonSlot::First),
        ),
        (
            "exoskeleton_second",
            act2_hive::create_exoskeleton_ai(ExoskeletonSlot::Second),
        ),
        (
            "exoskeleton_third",
            act2_hive::create_exoskeleton_ai(ExoskeletonSlot::Third),
        ),
        (
            "exoskeleton_fourth",
            act2_hive::create_exoskeleton_ai(ExoskeletonSlot::Fourth),
        ),
        ("chomper_scream", act2_hive::create_chomper_ai(true)),
        ("chomper_attack", act2_hive::create_chomper_ai(false)),
        ("hunter_killer", act2_hive::create_hunter_killer_ai()),
        ("wriggler", act2_hive::create_wriggler_ai()),
        ("louse_progenitor", act2_hive::create_louse_progenitor_ai()),
        (
            "myte",
            act2_hive::create_myte_ai(act2_hive::MyteSlot::First),
        ),
        ("tough_egg", act2_hive::create_tough_egg_ai()),
        ("ovicopter", act2_hive::create_ovicopter_ai()),
        (
            "slumbering_beetle",
            act2_hive::create_slumbering_beetle_ai(),
        ),
        ("spiny_toad", act2_hive::create_spiny_toad_ai()),
        ("the_obscura", act2_hive::create_the_obscura_ai()),
        (
            "decimillipede_segment_0",
            act2_hive::create_decimillipede_segment_ai(0),
        ),
        (
            "decimillipede_segment_1",
            act2_hive::create_decimillipede_segment_ai(1),
        ),
        (
            "decimillipede_segment_front_0",
            act2_hive::create_decimillipede_segment_front_ai(0),
        ),
        (
            "decimillipede_segment_middle_0",
            act2_hive::create_decimillipede_segment_middle_ai(0),
        ),
        (
            "decimillipede_segment_back_0",
            act2_hive::create_decimillipede_segment_back_ai(0),
        ),
        ("entomancer", act2_hive::create_entomancer_ai()),
        (
            "infested_prism_first",
            act2_hive::create_infested_prism_ai(InfestedPrismSlot::First),
        ),
        (
            "infested_prism_second",
            act2_hive::create_infested_prism_ai(InfestedPrismSlot::Second),
        ),
        (
            "infested_prism_other",
            act2_hive::create_infested_prism_ai(InfestedPrismSlot::Other),
        ),
        ("the_insatiable", act2_hive::create_the_insatiable_ai()),
        ("knowledge_demon", act2_hive::create_knowledge_demon_ai()),
        ("crusher", act2_hive::create_crusher_ai()),
        ("rocket", act2_hive::create_rocket_ai()),
        // ── act3.rs (28 ctors) ──────────────────────────────────────────
        ("devoted_sculptor", act3_glory::create_devoted_sculptor_ai()),
        (
            "scroll_of_biting",
            act3_glory::create_scroll_of_biting_ai(0),
        ),
        ("turret_operator", act3_glory::create_turret_operator_ai()),
        ("axebot_bootup", act3_glory::create_axebot_ai(true)),
        ("axebot_nobootup", act3_glory::create_axebot_ai(false)),
        ("zapbot", act3_glory::create_zapbot_ai()),
        ("stabbot", act3_glory::create_stabbot_ai()),
        ("guardbot", act3_glory::create_guardbot_ai()),
        ("noisebot", act3_glory::create_noisebot_ai()),
        ("fabricator", act3_glory::create_fabricator_ai()),
        ("frog_knight", act3_glory::create_frog_knight_ai()),
        ("globe_head", act3_glory::create_globe_head_ai()),
        ("owl_magistrate", act3_glory::create_owl_magistrate_ai()),
        ("slimed_berserker", act3_glory::create_slimed_berserker_ai()),
        ("the_lost", act3_glory::create_the_lost_ai()),
        ("the_forgotten", act3_glory::create_the_forgotten_ai()),
        ("flail_knight", act3_glory::create_flail_knight_ai()),
        (
            "act3_mysterious_knight",
            act3_glory::create_mysterious_knight_ai(),
        ),
        ("living_shield", act3_glory::create_living_shield_ai()),
        ("magi_knight", act3_glory::create_magi_knight_ai()),
        ("spectral_knight", act3_glory::create_spectral_knight_ai()),
        ("mecha_knight", act3_glory::create_mecha_knight_ai()),
        ("osty", act3_glory::create_osty_ai()),
        ("soul_nexus", act3_glory::create_soul_nexus_ai()),
        // Aeonglass replaced Doormaker in v0.107.1.
        ("aeonglass", act3_glory::create_aeonglass_ai()),
        ("queen", act3_glory::create_queen_ai()),
        ("test_subject", act3_glory::create_test_subject_ai()),
        // ── act4.rs (22 ctors) ──────────────────────────────────────────
        ("corpse_slug_0", act1_underdocks::create_corpse_slug_ai(0)),
        ("corpse_slug_1", act1_underdocks::create_corpse_slug_ai(1)),
        ("seapunk", act1_underdocks::create_seapunk_ai()),
        (
            "sludge_spinner",
            act1_underdocks::create_sludge_spinner_ai(),
        ),
        (
            "toadpole_first",
            act1_underdocks::create_toadpole_ai(ToadpoleSlot::First),
        ),
        (
            "toadpole_second",
            act1_underdocks::create_toadpole_ai(ToadpoleSlot::Second),
        ),
        (
            "toadpole_other",
            act1_underdocks::create_toadpole_ai(ToadpoleSlot::Other),
        ),
        (
            "calcified_cultist",
            act1_underdocks::create_calcified_cultist_ai(),
        ),
        ("damp_cultist", act1_underdocks::create_damp_cultist_ai()),
        (
            "fossil_stalker",
            act1_underdocks::create_fossil_stalker_ai(),
        ),
        (
            "sneaky_gremlin",
            act1_underdocks::create_sneaky_gremlin_ai(),
        ),
        ("fat_gremlin", act1_underdocks::create_fat_gremlin_ai()),
        ("gremlin_merc", act1_underdocks::create_gremlin_merc_ai()),
        ("haunted_ship", act1_underdocks::create_haunted_ship_ai()),
        ("gas_bomb", act1_underdocks::create_gas_bomb_ai()),
        ("living_fog", act1_underdocks::create_living_fog_ai()),
        (
            "punch_construct_strong",
            act1_underdocks::create_punch_construct_ai(true),
        ),
        (
            "punch_construct_weak",
            act1_underdocks::create_punch_construct_ai(false),
        ),
        ("sewer_clam", act1_underdocks::create_sewer_clam_ai()),
        (
            "two_tailed_rat_first",
            act1_underdocks::create_two_tailed_rat_ai(TwoTailedRatSlot::First),
        ),
        (
            "two_tailed_rat_second",
            act1_underdocks::create_two_tailed_rat_ai(TwoTailedRatSlot::Second),
        ),
        (
            "two_tailed_rat_other",
            act1_underdocks::create_two_tailed_rat_ai(TwoTailedRatSlot::Other),
        ),
        (
            "phantasmal_gardener",
            act1_underdocks::create_phantasmal_gardener_ai(PhantasmalGardenerSlot::First),
        ),
        (
            "skulking_colony",
            act1_underdocks::create_skulking_colony_ai(),
        ),
        ("terror_eel", act1_underdocks::create_terror_eel_ai()),
        (
            "waterfall_giant",
            act1_underdocks::create_waterfall_giant_ai(),
        ),
        ("soul_fysh", act1_underdocks::create_soul_fysh_ai()),
        (
            "lagavulin_matriarch",
            act1_underdocks::create_lagavulin_matriarch_ai(),
        ),
        // ── shared.rs (18 ctors) ────────────────────────────────────────
        ("architect", shared::create_architect_ai()),
        ("byrdpip", shared::create_byrdpip_ai()),
        ("paels_legion", shared::create_paels_legion_ai()),
        (
            "fake_merchant_monster",
            shared::create_fake_merchant_monster_ai(),
        ),
        (
            "shared_mysterious_knight",
            shared::create_mysterious_knight_ai(),
        ),
        (
            "dvw_first",
            shared::create_dense_vegetation_wriggler_ai(DenseVegetationWrigglerSlot::First),
        ),
        (
            "dvw_second",
            shared::create_dense_vegetation_wriggler_ai(DenseVegetationWrigglerSlot::Second),
        ),
        (
            "dvw_third",
            shared::create_dense_vegetation_wriggler_ai(DenseVegetationWrigglerSlot::Third),
        ),
        (
            "dvw_fourth",
            shared::create_dense_vegetation_wriggler_ai(DenseVegetationWrigglerSlot::Fourth),
        ),
        ("torch_head_amalgam", shared::create_torch_head_amalgam_ai()),
        (
            "the_adversary_mk_one",
            shared::create_the_adversary_mk_one_ai(),
        ),
        (
            "the_adversary_mk_two",
            shared::create_the_adversary_mk_two_ai(),
        ),
        (
            "the_adversary_mk_three",
            shared::create_the_adversary_mk_three_ai(),
        ),
        ("battle_friend_v1", shared::create_battle_friend_v1_ai()),
        ("battle_friend_v2", shared::create_battle_friend_v2_ai()),
        ("battle_friend_v3", shared::create_battle_friend_v3_ai()),
        ("big_dummy", shared::create_big_dummy_ai()),
        ("one_hp_monster", shared::create_one_hp_monster_ai()),
        ("ten_hp_monster", shared::create_ten_hp_monster_ai()),
        (
            "single_attack_move_monster",
            shared::create_single_attack_move_monster_ai(),
        ),
        (
            "multi_attack_move_monster",
            shared::create_multi_attack_move_monster_ai(),
        ),
    ]
}

/// Headline bench — sum-loop across the entire registry. The criterion
/// report's per-iter time, divided by `N_ctors`, is the ns/intent number
/// the Phase-5 plan target measures (≤100 ns/intent).
fn bench_full_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("monster_intent_roll");
    let registry = full_registry();
    let n = registry.len();
    group.throughput(Throughput::Elements(n as u64));

    // Pre-perform the first move on every AI so `roll_move` actually
    // advances state (otherwise the first call short-circuits to the
    // initial state). This matches the "in-combat per-turn cadence"
    // the bench is meant to characterize.
    let mut warmed: Vec<MonsterAi> = registry
        .into_iter()
        .map(|(_, mut ai)| {
            ai.on_move_performed();
            ai
        })
        .collect();

    // `roll_move_with_combat` is the universal entry point — both
    // RandomBranch and ConditionalBranch resolution paths go through it
    // (the no-combat variant panics on ConditionalBranch states; using
    // the combat variant unconditionally keeps the bench scope to the
    // full registry). A default `CombatState` is enough for the bench
    // because all conditional branches we ship at this pin guard on
    // `combat.creatures[...]` indexes that resolve to no-op false on
    // empty creature lists.
    let combat = CombatState::default();

    // The headline `full_set` bench measures *one roll per ctor* per
    // iteration. We do NOT call `on_move_performed` inside the timed
    // section — that bookkeeping is part of the combat-loop driver
    // (`enemy_turn_exec`), not the intent-roll cost itself, and would
    // inflate the per-iter measurement by a per-monster `state_log`
    // push on every iteration. The Phase-5 ≤100 ns/intent target is
    // for the roll dispatch only.
    //
    // Side effect: because we never advance the state machine inside
    // `b.iter`, every iteration re-rolls from the same (already-warmed)
    // first-non-initial move. That's the cold path through
    // `get_next_state_with_combat` for each ctor — the right thing to
    // characterize per-ctor dispatch cost.
    group.bench_function("full_set", |b| {
        b.iter(|| {
            for ai in &mut warmed {
                let mv = ai.roll_move_with_combat_seeded(
                    black_box(0x1234_5678_9abc_def0u64),
                    black_box(&combat),
                );
                black_box(&mv.intents);
            }
        });
    });

    // Per-monster spot checks (first 5 in registry) so a single-ctor
    // regression surfaces independently of the headline number.
    for (name, mut ai) in full_registry().into_iter().take(5) {
        ai.on_move_performed();
        group.bench_function(format!("single/{name}"), |b| {
            b.iter(|| {
                let mv = ai.roll_move_with_combat_seeded(
                    black_box(0x1234_5678_9abc_def0u64),
                    black_box(&combat),
                );
                black_box(&mv.intents);
                ai.on_move_performed();
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_full_set);
criterion_main!(benches);
