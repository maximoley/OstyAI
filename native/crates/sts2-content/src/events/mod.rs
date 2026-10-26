// port-pin-skip — module aggregator; per-event files carry the port pins.
//! Events content: static templates + handler-function dispatch for
//! the run-side event surface — bead `sts2-rs-6ynt.4.1` (P7 events
//! framework).
//!
//! Ports the registry side of
//! `sts2-rl-agent@01c094d:sts2_env/events/` (63 events across
//! `shared.py` / `act1.py` / `act2.py` / `act3.py`). Per-event
//! bodies arrive incrementally via the `sts2-rs-6ynt.4.*` sub-beads:
//!
//! - `6ynt.4.1` (this bead): framework + 2 proof-of-shape events
//!   (`LostWisp`, `TrashHeap`)
//! - `6ynt.4.2` (act1 — 4 events)
//! - `6ynt.4.3` (act3 — 5 events)
//! - `6ynt.4.4`/`4.5` (shared — 40 events, two waves)
//! - `6ynt.4.6`/`4.7` (act2 — 19 events, two waves)
//!
//! ## Crate boundary
//!
//! [`EventSpec`] is the *static template* (one per [`EventId`]). It
//! owns the per-event choice-option list ([`EventOptionSpec`]) and
//! the precondition predicate ([`EventAllowedFn`]). Resolution
//! handlers ([`EventChoiceFn`]) take `&mut RunState` and mutate the
//! run state directly (push rewards, gain/lose gold, lose HP, etc.).
//!
//! The split mirrors `RelicSpec`/`Relic`, `PotionSpec`/`Potion`, and
//! `CardSpec`/`Card`: pure-data templates in `sts2-content`, runtime
//! mutation in `sts2-core` via callbacks.
//!
//! ## Dispatcher
//!
//! [`dispatch::spec_for`] resolves an [`EventId`] to its
//! [`EventSpec`]; [`dispatch::present`] returns the (dynamically
//! computed) [`EventOptionInfo`] list for the agent; and
//! [`dispatch::resolve`] applies the chosen option's handler and
//! clears [`RunState::current_event`] on a finished outcome.
//!
//! Multi-page events (`Trial`, `RoundTeaParty`, etc.) are out of
//! scope for this framework bead — see the per-event sub-bead notes.

use sts2_core::{EventId, EventOptionInfo, EventOutcome, EventVarSpec, RunState};

// Zone-named per the C# canonical act classes (`Overgrowth`, `Hive`,
// `Glory`). Python pin uses `act1.py` / `act2.py` / `act3.py`;
// Underdocks (Act 1 alternate) isn't ported yet.
pub mod act1_overgrowth;
pub mod act2_hive;
pub mod act2_hive_wave_b;
pub mod act3_glory;
mod crystal_sphere;
pub mod dispatch;
pub mod neow_blessing;
pub mod shared;
pub mod shared_wave_b;
mod trial;

// ── Handler-function aliases ──────────────────────────────────────────

/// Per-option builder for the run-dependent half of a localized template's
/// values. See [`EventOptionSpec::vars_fn`].
pub type EventOptionVarsFn = fn(&RunState) -> Vec<(&'static str, sts2_core::EventVar)>;

/// Predicate: is this event allowed to spawn given the current run
/// state? Mirrors Python `EventModel.is_allowed(run_state)` at
/// `sts2-rl-agent@01c094d:sts2_env/run/events.py:48`.
///
/// Defaults to "always allowed" via [`always_allowed`]; per-event
/// specs override (e.g. `TrashHeap` requires `current_hp > 5`).
pub type EventAllowedFn = fn(&RunState) -> bool;

/// Choice handler: apply one event option's effects to the run state
/// and return the post-resolution outcome. Mirrors Python
/// `EventModel.choose(run_state, option_id)` at
/// `sts2-rl-agent@01c094d:sts2_env/run/events.py:62`.
pub type EventChoiceFn = fn(&mut RunState) -> EventOutcome;

/// Dynamic option-list generator. Mirrors Python
/// `EventModel.generate_initial_options(run_state)` at
/// `sts2-rl-agent@01c094d:sts2_env/run/events.py:58`. Receives a
/// shared [`RunState`] (the option text may depend on HP / gold /
/// owned relics / character).
///
/// The default builder ([`default_option_info_from_spec`]) up-converts
/// the static [`EventOptionSpec::default_label`] /
/// [`EventOptionSpec::default_description`] strings; events with
/// dynamic text (e.g. `LostWisp`'s "Gain {gold} gold") override.
pub type EventGenerateOptionsFn = fn(&RunState, &EventSpec) -> Vec<EventOptionInfo>;

/// Default `is_allowed` — always true. Used by events without
/// preconditions.
#[must_use]
pub fn always_allowed(_state: &RunState) -> bool {
    true
}

/// `is_allowed` for events that never spawn from the random pool
/// (Python `def is_allowed(self, run_state): return False`). Used by
/// act3's `TheArchitect` / `Neow` / `WarHistorianRepy` /
/// `DeprecatedEvent` / `DeprecatedAncientEvent` — all triggered only
/// by special game logic, not the room-roll. Bead `sts2-rs-6ynt.4.3`.
#[must_use]
pub fn never_allowed(_state: &RunState) -> bool {
    false
}

// ── C# `EventModel.IsAllowed` predicates (bead `sts2-rs-1hiht`, M5) ──────────
//
// C# `IRunState.CurrentActIndex` is 0-BASED (Act 1 == 0); Rust
// [`RunState::act_index`] is 1-BASED (Act 1 == 1). Each predicate below
// translates the C# expression accordingly. The card-property gates resolve
// rarity via the registered C#-discriminant rank fn and approximate
// `IsRemovable`/`IsTransformable`/per-enchantment `CanEnchant` extras as
// always-true for POD deck cards (no such exception exists in the live
// pools); relic-bag availability (`HasAvailableRelics`) and `IsTradable` are
// approximated by relic-count checks — each documented at its gate.

/// `PotionCourier`/`Symbiote`/`CrystalSphere` base gate: C#
/// `CurrentActIndex > 0` ⇒ Act 2+. Rust `act_index > 1`.
#[must_use]
pub fn allowed_act_2_plus(state: &RunState) -> bool {
    state.act_index > 1
}

/// `BrainLeech`/`RoomFullOfCheese` gate: C# `CurrentActIndex < 2` ⇒ Acts 1–2.
#[must_use]
pub fn allowed_act_1_or_2(state: &RunState) -> bool {
    state.act_index < 3
}

/// `DollRoom` gate: C# `CurrentActIndex == 1` ⇒ Act 2 only.
#[must_use]
pub fn allowed_act_2_only(state: &RunState) -> bool {
    state.act_index == 2
}

/// `CrystalSphere`: C# `Gold >= 100 && CurrentActIndex > 0`.
#[must_use]
pub fn crystal_sphere_allowed(state: &RunState) -> bool {
    state.act_index > 1 && state.gold >= 100
}

/// `TeaMaster`: C# `CurrentActIndex < 2 && Gold >= 150`.
#[must_use]
pub fn tea_master_allowed(state: &RunState) -> bool {
    state.act_index < 3 && state.gold >= 150
}

/// `StoneOfAllTime`: C# `CurrentActIndex == 1 && Potions.Any()` ⇒ Act 2 + ≥1 potion.
#[must_use]
pub fn stone_of_all_time_allowed(state: &RunState) -> bool {
    state.act_index == 2 && !state.potions.is_empty()
}

/// `TheFutureOfPotions`: C# `Potions.Count() >= 2`.
#[must_use]
pub fn future_of_potions_allowed(state: &RunState) -> bool {
    state.potions.len() >= 2
}

/// `TheLegendsWereTrue`: C# `CurrentActIndex == 0 && deck.Count > 0 && CurrentHp >= 10`
/// ⇒ Act 1 + ≥10 HP (the deck is non-empty for the whole run).
#[must_use]
pub fn legends_were_true_allowed(state: &RunState) -> bool {
    state.act_index == 1 && state.current_hp >= 10
}

// These gates are PARITY-LOAD-BEARING beyond the event itself: C#'s
// `RoomSet.EnsureNextEventIsValid` advances the shared `eventsVisited`
// cursor past every rejected candidate, so a missing gate shifts WHICH
// event every later event room sees (run-parity `card_select rs=map`
// family — e.g. Ironclad RP7: Rust offered act-2-gated ThisOrThat at the
// act-1 cursor slot C# skipped through to WoodCarvings).

/// `WoodCarvings`: any Basic-rarity deck card (C# `c.Rarity == Basic &&
/// c.IsRemovable`; `IsRemovable` ≈ true for POD deck cards). Rarity via
/// the registered rank fn (C# discriminant `Basic == 1`); unregistered
/// (raw-core tests) ⇒ allowed.
#[must_use]
pub fn deck_has_basic_card(state: &RunState) -> bool {
    match sts2_core::lookup_card_rarity_rank() {
        Some(f) => state.master_deck.iter().any(|c| f(c.card_id) == 1),
        None => true,
    }
}

/// Generic enchant-pool gate (`GraveOfTheForgotten` /
/// `FieldOfManSizedHoles` / `SpiralingWhirlpool`): any deck card the C#
/// base `EnchantmentModel.CanEnchant` accepts — not Status/Curse-typed,
/// no existing enchantment, concrete cost (`cost >= 0` folds Unplayable
/// and X-cost). Per-enchantment extras (PerfectFit / Spiral) are
/// approximated by the base filter.
#[must_use]
pub fn any_enchantable_deck_card(state: &RunState) -> bool {
    use sts2_core::cards::CardType;
    state.master_deck.iter().any(|c| {
        !matches!(c.card_type, CardType::Status | CardType::Curse)
            && c.enchantments.is_empty()
            && c.cost >= 0
    })
}

/// `Amalgamator`: C# requires ≥2 Strike-tagged AND ≥2 Defend-tagged
/// Basic, removable deck cards (`IsValid(tag, c)` — tag + `Rarity ==
/// Basic` + `IsRemovable`).
#[must_use]
pub fn amalgamator_allowed(state: &RunState) -> bool {
    use sts2_core::cards::CardTag;
    let rank = sts2_core::lookup_card_rarity_rank();
    let is_basic = |c: &sts2_core::cards::Card| rank.map_or(true, |f| f(c.card_id) == 1);
    let strikes = state
        .master_deck
        .iter()
        .filter(|c| c.has_tag(CardTag::Strike) && is_basic(c))
        .count();
    let defends = state
        .master_deck
        .iter()
        .filter(|c| c.has_tag(CardTag::Defend) && is_basic(c))
        .count();
    strikes >= 2 && defends >= 2
}

/// `ByrdonisNest`: C# `!HasEventPet()` — no pet-adding relic and no
/// `ByrdonisEgg` in the deck. The relic half is approximated by the deck
/// check (no pet-relic predicate on the POD; the egg IS the act-1 pet
/// source this event guards against).
#[must_use]
pub fn byrdonis_nest_allowed(state: &RunState) -> bool {
    !state
        .master_deck
        .iter()
        .any(|c| c.card_id == sts2_core::cards::CardId::ByrdonisEgg)
}

/// `RoundTeaParty`: C# `CurrentHp >= 12`.
#[must_use]
pub fn round_tea_party_allowed(state: &RunState) -> bool {
    state.current_hp >= 12
}

/// `PunchOff`: C# `TotalFloor >= 6`. Rust `total_floor` is ONE LESS than
/// C#'s `MapPointHistory` floor (Neow uncounted), hence the `+ 1`.
#[must_use]
pub fn punch_off_allowed(state: &RunState) -> bool {
    state.total_floor + 1 >= 6
}

/// `SlipperyBridge`: C# `TotalFloor > 6 && deck.Any(IsRemovable)` (the
/// deck is never empty of removable cards in practice). Same `+ 1`
/// floor-base translation as [`punch_off_allowed`].
#[must_use]
pub fn slippery_bridge_allowed(state: &RunState) -> bool {
    state.total_floor + 1 > 6 && !state.master_deck.is_empty()
}

/// `EndlessConveyor`: C# `Gold >= 120`.
#[must_use]
pub fn endless_conveyor_allowed(state: &RunState) -> bool {
    state.gold >= 120
}

/// `LuminousChoir`: C# `Gold >= GoldVar(149) && RelicGrabBag.
/// HasAvailableRelics` (the shared bag is never exhausted in practice —
/// approximated true).
#[must_use]
pub fn luminous_choir_allowed(state: &RunState) -> bool {
    state.gold >= 149
}

/// `MorphicGrove`: C# `Gold >= 100 && deck.Count(IsTransformable) >= 2`
/// (every POD deck card counts as transformable).
#[must_use]
pub fn morphic_grove_allowed(state: &RunState) -> bool {
    state.gold >= 100 && state.master_deck.len() >= 2
}

/// `FakeMerchant`: C# act 2+ (`CurrentActIndex < 1 → false`), single
/// player, and `Gold >= 100 || Potions.Any(FoulPotion)`.
#[must_use]
pub fn fake_merchant_allowed(state: &RunState) -> bool {
    state.act_index > 1
        && (state.gold >= 100
            || state
                .potions
                .contains(&sts2_core::potions::PotionKind::FoulPotion))
}

/// `RanwidTheElder`: C# act 2+, ≥1 TRADABLE relic, `Gold >= 100`, and ≥1
/// potion. Ranwid takes a relic off you, so the gate is over
/// [`crate::relics::tradable::relic_is_tradable`] — a run holding only its
/// starter has nothing he will accept.
#[must_use]
pub fn ranwid_the_elder_allowed(state: &RunState) -> bool {
    state.act_index > 1
        && state
            .relics
            .iter()
            .any(|&id| crate::relics::tradable::relic_is_tradable(id))
        && state.gold >= 100
        && !state.potions.is_empty()
}

/// `RelicTrader`: C# `CurrentActIndex != 0 && GetValidRelics(p).Count() >= 5`
/// — act 2+ and at least five relics the trader is allowed to take.
///
/// The count is over TRADABLE relics ([`crate::relics::tradable::
/// relic_is_tradable`]), not the raw inventory: your starter and every Ancient
/// or event relic are untouchable, so a five-relic run built out of Neow
/// blessings can be well short of five valid ones and C# never shows the
/// event. This used to read `state.relics.len()`, which let the trader spawn
/// with nothing legal to put on the table.
#[must_use]
pub fn relic_trader_allowed(state: &RunState) -> bool {
    state.act_index > 1
        && state
            .relics
            .iter()
            .filter(|&&id| crate::relics::tradable::relic_is_tradable(id))
            .count()
            >= 5
}

/// Every localized-template value for one option: the static
/// [`EventOptionSpec::default_vars`], plus whatever
/// [`EventOptionSpec::vars_fn`] computes from the run. Dynamic
/// `generate_options` builders should call this rather than rebuilding the
/// list, so an option cannot lose its values just because its event happens
/// to present itself by hand.
#[must_use]
pub fn option_vars(
    state: &RunState,
    opt: &EventOptionSpec,
) -> Vec<(&'static str, sts2_core::EventVar)> {
    let mut vars: Vec<(&'static str, sts2_core::EventVar)> = opt
        .default_vars
        .iter()
        .map(|&(name, value)| (name, value.to_var()))
        .collect();
    if let Some(f) = opt.vars_fn {
        vars.extend(f(state));
    }
    vars
}

/// The title-only half of an option's values — see
/// [`EventOptionSpec::title_vars_fn`]. Empty for all but one option.
#[must_use]
pub fn option_title_vars(
    state: &RunState,
    opt: &EventOptionSpec,
) -> Vec<(&'static str, sts2_core::EventVar)> {
    opt.title_vars_fn.map(|f| f(state)).unwrap_or_default()
}

/// Default options-builder: takes the static [`EventOptionSpec`]
/// entries from the spec and up-converts each to an
/// [`EventOptionInfo`] using the static label/description text.
///
/// Per-event overrides (e.g. `LostWisp`'s randomized gold) supply
/// their own [`EventGenerateOptionsFn`] and interpolate dynamic
/// values into the description before returning.
#[must_use]
pub fn default_option_info_from_spec(state: &RunState, spec: &EventSpec) -> Vec<EventOptionInfo> {
    spec.options
        .iter()
        .map(|opt| EventOptionInfo {
            option_id: opt.option_id,
            label: opt.default_label.to_string(),
            description: opt.default_description.to_string(),
            enabled: true,
            loc_key: opt.loc_key,
            vars: option_vars(state, opt),
            title_vars: option_title_vars(state, opt),
        })
        .collect()
}

// ── EventOptionSpec / EventSpec ───────────────────────────────────────

/// Static per-choice template. One entry per option_id; the
/// dispatcher matches `option_id` literally on resolve.
///
/// `default_label` / `default_description` are the fallback strings
/// used by [`default_option_info_from_spec`]; events with dynamic
/// presentation supply their own [`EventGenerateOptionsFn`].
#[derive(Clone, Copy, Debug)]
pub struct EventOptionSpec {
    /// Stable per-choice id (matches Python `EventOption.option_id`).
    pub option_id: &'static str,
    /// Localization path for this choice, minus the trailing field —
    /// `"WELLSPRING.pages.INITIAL.options.BOTTLE"`. Copied onto
    /// [`EventOptionInfo::loc_key`], where the full contract lives.
    ///
    /// Empty for choices with no row in `localization_eng/events.json`:
    /// the Ancient / Neow relic offers (text comes from the relic) and
    /// the few whose row is chosen per-run from a set.
    pub loc_key: &'static str,
    /// Default short label. Developer shorthand — a display layer should
    /// prefer `<loc_key>.title`.
    pub default_label: &'static str,
    /// Default long-form description. Developer shorthand — a display
    /// layer should prefer `<loc_key>.description`.
    pub default_description: &'static str,
    /// Values for the `{VarName}` placeholders in the `loc_key` template
    /// that are FIXED for this option — a constant HP cost, the name of
    /// the curse it always adds. Copied onto
    /// [`EventOptionInfo::vars`](sts2_core::EventOptionInfo::vars).
    ///
    /// Empty when the template takes no placeholders, or when the values
    /// depend on the run (a gold roll, which relic was offered) — those
    /// need a dynamic `generate_options`. A template whose placeholders
    /// are not ALL covered cannot be rendered, and the display layer
    /// falls back to `default_description`.
    pub default_vars: &'static [(&'static str, EventVarSpec)],
    /// Values for the `loc_key` template that need the RUN to compute —
    /// a seeded gold roll, a heal scaled off max HP, the name of whichever
    /// relic this slot happens to be offering. Merged over
    /// [`default_vars`](Self::default_vars).
    ///
    /// Every one of these must be a PURE function of the run state. The
    /// seeded rolls already are: each event draws from its own stream,
    /// `DotNetRng::with_name(state.rng.seed, "<EVENT>")` (mirroring C#
    /// `EventModel.Rng = new Rng(seed + hash(Id.Entry))`), rebuilt per call
    /// and consuming nothing shared — so asking at present time returns
    /// exactly what the handler will use at resolve time. A `vars_fn` that
    /// advanced a shared stream would corrupt the run.
    pub vars_fn: Option<EventOptionVarsFn>,
    /// Values that apply to the option's TITLE only, overriding
    /// [`vars_fn`](Self::vars_fn) where the names collide.
    ///
    /// Almost always `None`: a title and its description normally read the
    /// same numbers. The Future of Potions is the exception — `{Rarity}` is
    /// the POTION's rarity in "Insert Rare Potion" and the CARD's in "Obtain
    /// an Upgraded Rare Power", and those differ for Event and Token potions.
    /// C# gets away with one name because it builds two `LocString`s with
    /// their own var bags; this is that second bag.
    pub title_vars_fn: Option<EventOptionVarsFn>,
    /// Handler that applies this choice's effects.
    pub handler: EventChoiceFn,
}

/// Static event template. One [`EventSpec`] exists per [`EventId`].
///
/// Field selection mirrors Python's `EventModel` subclass surface
/// (`is_allowed`, `generate_initial_options`, `choose`); the registry
/// (`dispatch::spec_for`) maps [`EventId`] to its [`EventSpec`].
#[derive(Clone, Copy, Debug)]
pub struct EventSpec {
    /// Discriminant.
    pub id: EventId,
    /// Python class name (matches [`EventId::python_name`]).
    pub python_name: &'static str,
    /// Precondition for this event to be allowed to spawn.
    pub is_allowed: EventAllowedFn,
    /// Builder for the per-presentation option list.
    pub generate_options: EventGenerateOptionsFn,
    /// Static option template list (the dispatcher uses this for
    /// `option_id`-based lookup on resolve).
    pub options: &'static [EventOptionSpec],
}

/// Queue the source-verified one-card deck-enchantment selector. The game
/// automatically takes the only eligible card; interactive choices use the
/// same callback gate when RunManager enumerates their legal actions.
pub(crate) fn enchant_deck_card(
    state: &mut sts2_core::RunState,
    enchantment: sts2_core::cards::EnchantmentId,
    amount: i32,
) {
    let eligible: Vec<usize> = state
        .master_deck
        .iter()
        .enumerate()
        .filter_map(|(i, card)| {
            crate::enchantments::can_enchant_deck_card(card, enchantment).then_some(i)
        })
        .collect();
    if state.interactive_card_transform && eligible.len() > 1 {
        state
            .pending_deck_enchants
            .push(sts2_core::PendingDeckEnchant {
                enchantment,
                amount,
                type_restriction: None,
                secondary_type: None,
                post_selection_rng_read: None,
            });
    } else if let Some(&index) = eligible.first() {
        let _ = crate::enchantments::apply_static_enchantment(
            &mut state.master_deck[index],
            enchantment,
            amount,
        );
    }
}

/// Select both cards before revealing any transform. Auto-selection follows
/// source deck order when the eligible pool does not exceed the requested two.
pub(super) fn transform_two_cards_from_event(
    state: &mut RunState,
    stream: &'static str,
    skip: u32,
) {
    let removable = sts2_core::lookup_card_removable();
    let selected: Vec<_> = state
        .master_deck
        .iter()
        .enumerate()
        .filter(|(_, card)| {
            removable.map_or(true, |predicate| predicate(card))
                && crate::cards::spec_index::rarity_for_card_id(card.card_id)
                    != Some(crate::cards::CardRarity::Quest)
        })
        .map(|(index, _)| index)
        .collect();
    if state.interactive_card_transform && selected.len() > 2 {
        state
            .pending_deck_transforms
            .push(sts2_core::PendingDeckTransform {
                batch_count: 2,
                event_stream_roll: Some((stream, skip)),
                ..Default::default()
            });
        return;
    }
    for (offset, original_index) in selected.into_iter().take(2).enumerate() {
        let index = original_index - offset;
        let original = state.master_deck[index].card_id;
        if let Some(replacement) =
            crate::cards::spec_index::roll_transform_replacement_event_stream(
                state,
                state.character_id,
                original,
                stream,
                skip + offset as u32,
            )
            .and_then(|id| crate::cards::spec_index::card_pod_from_id(id, false))
        {
            state.master_deck.remove(index);
            state.add_card_to_deck(replacement, sts2_core::relic_effects::AddSource::EventGain);
        }
    }
}

// CardModel.IsTransformable excludes unremovable cards and Eternal keywords.
fn first_transformable_deck_index(state: &RunState) -> Option<usize> {
    let removable = sts2_core::lookup_card_removable();
    state.master_deck.iter().position(|card| {
        card.card_type != sts2_core::cards::CardType::Quest
            && removable.map_or(true, |check| check(card))
    })
}

/// Prepare source event-owned state at arrival, before any event choice.
pub fn initialize_event(state: &mut RunState) {
    if state.current_event == Some(sts2_core::EventId::SunkenTreasury) {
        shared_wave_b::prepare_sunken_treasury(state);
    }
    if state.current_event == Some(sts2_core::EventId::CrystalSphere) {
        crystal_sphere::prepare(state);
    }
    if state.current_event == Some(sts2_core::EventId::StoneOfAllTime) {
        act2_hive_wave_b::prepare_stone(state);
    }
    if state.current_event == Some(sts2_core::EventId::PunchOff) {
        shared::prepare_punch_off(state);
    }
}

/// Deterministic visible-information policy for the untrained Crystal board.
#[must_use]
pub fn crystal_sphere_policy_option(state: &RunState) -> Option<usize> {
    crystal_sphere::policy_option(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_spec_is_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<EventSpec>();
        assert_copy::<EventOptionSpec>();
    }

    #[test]
    fn always_allowed_default_is_true() {
        let rs = RunState::new();
        assert!(always_allowed(&rs));
    }

    #[test]
    fn default_option_builder_preserves_label_and_description() {
        fn noop(_s: &mut RunState) -> EventOutcome {
            EventOutcome::finished("noop")
        }
        const OPTS: &[EventOptionSpec] = &[EventOptionSpec {
            option_id: "x",
            loc_key: "",
            default_label: "Label",
            default_description: "Desc",
            default_vars: &[],
            vars_fn: None,
            title_vars_fn: None,
            handler: noop,
        }];
        let spec = EventSpec {
            id: EventId::LostWisp,
            python_name: "LostWisp",
            is_allowed: always_allowed,
            generate_options: default_option_info_from_spec,
            options: OPTS,
        };
        let rs = RunState::new();
        let opts = default_option_info_from_spec(&rs, &spec);
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].option_id, "x");
        assert_eq!(opts[0].label, "Label");
        assert_eq!(opts[0].description, "Desc");
        assert!(opts[0].enabled);
    }

    // ── C# `IsAllowed` predicate contract pins (mutation coverage) ──────
    //
    // Each predicate below straddles its boundary/operator: a TRUE case and
    // a FALSE case on either side of every comparison, so the `*_allowed`
    // gates can't silently collapse to `true`/`false` or drift an operator.
    // Fixtures build a bare [`RunState`] and poke only the field(s) each
    // gate reads, mirroring the per-field fixture idiom in `dispatch.rs`.

    use sts2_core::cards::{Card, CardId, CardTag, CardType};

    /// Build a `Card` POD with a given id, type, tags, cost, and
    /// (optionally) an enchantment, for deck-content gates.
    fn deck_card(card_id: CardId, card_type: CardType, tags: sts2_core::cards::CardTags) -> Card {
        Card {
            card_id,
            card_type,
            cost: 1,
            tags,
            ..Card::pod_defaults()
        }
    }

    // --- act-index base gates ---

    #[test]
    fn allowed_act_2_plus_straddles_act_boundary() {
        let mut rs = RunState::new();
        rs.act_index = 1;
        assert!(!allowed_act_2_plus(&rs)); // act 1: below
        rs.act_index = 2;
        assert!(allowed_act_2_plus(&rs)); // act 2: above (kills `>`→`>=`/`==`/`<`)
        rs.act_index = 3;
        assert!(allowed_act_2_plus(&rs));
    }

    #[test]
    fn allowed_act_1_or_2_straddles_act_3() {
        let mut rs = RunState::new();
        rs.act_index = 2;
        assert!(allowed_act_1_or_2(&rs)); // act 2: below 3
        rs.act_index = 3;
        assert!(!allowed_act_1_or_2(&rs)); // act 3: NOT (kills `<`→`<=`, `->bool true`)
    }

    #[test]
    fn allowed_act_2_only_is_exactly_act_2() {
        let mut rs = RunState::new();
        rs.act_index = 1;
        assert!(!allowed_act_2_only(&rs));
        rs.act_index = 2;
        assert!(allowed_act_2_only(&rs)); // kills `==`→`!=`, `->bool false`
        rs.act_index = 3;
        assert!(!allowed_act_2_only(&rs));
    }

    // --- gold + act compound gates ---

    #[test]
    fn crystal_sphere_needs_act_2_plus_and_100_gold() {
        let mut rs = RunState::new();
        rs.act_index = 2;
        rs.gold = 100;
        assert!(crystal_sphere_allowed(&rs)); // both conjuncts true
        rs.gold = 99;
        assert!(!crystal_sphere_allowed(&rs)); // gold short (kills `>=`→`<`, `&&`→`||`)
        rs.act_index = 1;
        rs.gold = 100;
        assert!(!crystal_sphere_allowed(&rs)); // act short (kills `>`→`==`/`<`/`>=`, `&&`→`||`)
    }

    #[test]
    fn tea_master_needs_act_1_or_2_and_150_gold() {
        let mut rs = RunState::new();
        rs.act_index = 2;
        rs.gold = 150;
        assert!(tea_master_allowed(&rs)); // both true
        rs.gold = 149;
        assert!(!tea_master_allowed(&rs)); // gold short (kills `>=`→`<`, `&&`→`||`)
        rs.act_index = 3;
        rs.gold = 150;
        assert!(!tea_master_allowed(&rs)); // act short (kills `<`→`==`/`>`/`<=`, `&&`→`||`)
    }

    #[test]
    fn stone_of_all_time_needs_act_2_and_a_potion() {
        let mut rs = RunState::new();
        rs.act_index = 2;
        rs.potions.push(sts2_core::potions::PotionKind::FoulPotion);
        assert!(stone_of_all_time_allowed(&rs)); // both true
        rs.potions.clear();
        assert!(!stone_of_all_time_allowed(&rs)); // no potion (kills `delete !`, `&&`→`||`)
        rs.potions.push(sts2_core::potions::PotionKind::FoulPotion);
        rs.act_index = 1;
        assert!(!stone_of_all_time_allowed(&rs)); // wrong act (kills `==`→`!=`, `&&`→`||`)
    }

    #[test]
    fn future_of_potions_needs_two_potions() {
        let mut rs = RunState::new();
        rs.potions.push(sts2_core::potions::PotionKind::FoulPotion);
        assert!(!future_of_potions_allowed(&rs)); // 1: below
        rs.potions.push(sts2_core::potions::PotionKind::FoulPotion);
        assert!(future_of_potions_allowed(&rs)); // 2: at boundary (kills `>=`→`<`)
    }

    #[test]
    fn legends_were_true_needs_act_1_and_10_hp() {
        let mut rs = RunState::new();
        rs.act_index = 1;
        rs.current_hp = 10;
        assert!(legends_were_true_allowed(&rs)); // both true at boundary
        rs.current_hp = 9;
        assert!(!legends_were_true_allowed(&rs)); // hp short (kills `>=`→`<`, `&&`→`||`)
        rs.current_hp = 10;
        rs.act_index = 2;
        assert!(!legends_were_true_allowed(&rs)); // wrong act (kills `==`→`!=`, `&&`→`||`)
    }

    // --- deck-content gates ---
    //
    // `deck_has_basic_card` / `amalgamator_allowed` resolve rarity via the
    // registered C#-discriminant rank fn. With the registry INSTALLED a
    // basic-rarity card ranks 1; a Common ranks 2. The `None` (unregistered)
    // branch of `deck_has_basic_card` is pinned by its own process-isolated
    // test below (nextest runs each test in a fresh process, so installing
    // the registry in one test does not leak into the other).

    #[test]
    fn deck_has_basic_card_distinguishes_basic_from_common() {
        crate::cards::dispatch::install_default_registry();
        let mut rs = RunState::new();
        // A Common card (Anger, rank 2) is NOT basic.
        rs.master_deck.push(deck_card(
            CardId::Anger,
            CardType::Attack,
            sts2_core::cards::CardTags::NONE,
        ));
        assert!(!deck_has_basic_card(&rs)); // kills `->bool true`, `==`→`!=`
                                            // A Basic Strike (rank 1) IS basic.
        rs.master_deck.push(deck_card(
            CardId::StrikeIronclad,
            CardType::Attack,
            sts2_core::cards::CardTags::NONE,
        ));
        assert!(deck_has_basic_card(&rs)); // kills `->bool false`
    }

    #[test]
    fn deck_has_basic_card_unregistered_is_allowed() {
        // No `install_default_registry()` in this process → `None` branch.
        let rs = RunState::new();
        assert!(deck_has_basic_card(&rs)); // empty deck still allowed via None arm
    }

    #[test]
    fn any_enchantable_deck_card_filters_type_enchant_and_cost() {
        let mut rs = RunState::new();
        // Plain attack, no enchant, cost 1 → enchantable.
        rs.master_deck.push(deck_card(
            CardId::StrikeIronclad,
            CardType::Attack,
            sts2_core::cards::CardTags::NONE,
        ));
        assert!(any_enchantable_deck_card(&rs)); // kills `->bool false`

        // Now make the ONLY card fail each conjunct in turn.
        rs.master_deck.clear();
        // Status type is rejected (kills `delete !`).
        rs.master_deck.push(deck_card(
            CardId::StrikeIronclad,
            CardType::Status,
            sts2_core::cards::CardTags::NONE,
        ));
        assert!(!any_enchantable_deck_card(&rs));

        // Curse type is rejected too.
        rs.master_deck.clear();
        rs.master_deck.push(deck_card(
            CardId::StrikeIronclad,
            CardType::Curse,
            sts2_core::cards::CardTags::NONE,
        ));
        assert!(!any_enchantable_deck_card(&rs));

        // Already enchanted is rejected (kills first `&&`→`||`).
        rs.master_deck.clear();
        let mut enchanted = deck_card(
            CardId::StrikeIronclad,
            CardType::Attack,
            sts2_core::cards::CardTags::NONE,
        );
        enchanted
            .enchantments
            .push((sts2_core::cards::EnchantmentId::Sharp, 1));
        rs.master_deck.push(enchanted);
        assert!(!any_enchantable_deck_card(&rs));

        // Negative cost (Unplayable/X) is rejected (kills second `&&`→`||`, `>=`→`<`).
        rs.master_deck.clear();
        let mut unplayable = deck_card(
            CardId::StrikeIronclad,
            CardType::Attack,
            sts2_core::cards::CardTags::NONE,
        );
        unplayable.cost = -1;
        rs.master_deck.push(unplayable);
        assert!(!any_enchantable_deck_card(&rs));

        // cost == 0 is the boundary case and IS enchantable.
        rs.master_deck.clear();
        let mut zero_cost = deck_card(
            CardId::StrikeIronclad,
            CardType::Attack,
            sts2_core::cards::CardTags::NONE,
        );
        zero_cost.cost = 0;
        rs.master_deck.push(zero_cost);
        assert!(any_enchantable_deck_card(&rs));
    }

    #[test]
    fn amalgamator_needs_two_strikes_and_two_defends() {
        crate::cards::dispatch::install_default_registry();
        let strike = || {
            deck_card(
                CardId::StrikeIronclad,
                CardType::Attack,
                sts2_core::cards::CardTags::one(CardTag::Strike),
            )
        };
        let defend = || {
            deck_card(
                CardId::DefendIronclad,
                CardType::Skill,
                sts2_core::cards::CardTags::one(CardTag::Defend),
            )
        };
        let mut rs = RunState::new();
        rs.master_deck = vec![strike(), strike(), defend(), defend()];
        assert!(amalgamator_allowed(&rs)); // 2 + 2 at boundary (kills both `>=`→`<`)

        // One strike short → false (kills strike `&&`).
        rs.master_deck = vec![strike(), defend(), defend()];
        assert!(!amalgamator_allowed(&rs));

        // One defend short → false (kills defend `&&` / final `&&`).
        rs.master_deck = vec![strike(), strike(), defend()];
        assert!(!amalgamator_allowed(&rs));

        // Two strikes + two defends but the strikes are Common (Anger, rank 2),
        // so `is_basic` rejects them: kills `== 1`→`!= 1` in `is_basic`.
        let common_strike = || {
            deck_card(
                CardId::Anger,
                CardType::Attack,
                sts2_core::cards::CardTags::one(CardTag::Strike),
            )
        };
        rs.master_deck = vec![common_strike(), common_strike(), defend(), defend()];
        assert!(!amalgamator_allowed(&rs));
    }

    #[test]
    fn byrdonis_nest_blocked_only_by_the_egg() {
        let mut rs = RunState::new();
        assert!(byrdonis_nest_allowed(&rs)); // empty deck → allowed (kills `->bool false`)
        rs.master_deck.push(deck_card(
            CardId::StrikeIronclad,
            CardType::Attack,
            sts2_core::cards::CardTags::NONE,
        ));
        assert!(byrdonis_nest_allowed(&rs)); // non-egg card → still allowed
        rs.master_deck.push(deck_card(
            CardId::ByrdonisEgg,
            CardType::Skill,
            sts2_core::cards::CardTags::NONE,
        ));
        assert!(!byrdonis_nest_allowed(&rs)); // egg present (kills `delete !`, `==`→`!=`, `->bool true`)
    }

    // --- HP / floor / gold scalar gates ---

    #[test]
    fn round_tea_party_needs_12_hp() {
        let mut rs = RunState::new();
        rs.current_hp = 11;
        assert!(!round_tea_party_allowed(&rs));
        rs.current_hp = 12;
        assert!(round_tea_party_allowed(&rs)); // boundary (kills `>=`→`<`)
    }

    #[test]
    fn punch_off_needs_total_floor_5() {
        // gate is `total_floor + 1 >= 6`, i.e. total_floor >= 5.
        let mut rs = RunState::new();
        rs.total_floor = 4;
        assert!(!punch_off_allowed(&rs)); // 4+1=5 < 6
        rs.total_floor = 5;
        assert!(punch_off_allowed(&rs)); // 5+1=6 boundary (kills `>=`→`<`, `+`→`-`/`*`)
    }

    #[test]
    fn slippery_bridge_needs_floor_over_6_and_nonempty_deck() {
        // gate is `total_floor + 1 > 6` (total_floor >= 6) AND deck non-empty.
        let mut rs = RunState::new();
        rs.master_deck.push(deck_card(
            CardId::StrikeIronclad,
            CardType::Attack,
            sts2_core::cards::CardTags::NONE,
        ));
        rs.total_floor = 5;
        assert!(!slippery_bridge_allowed(&rs)); // 5+1=6 not > 6 (kills `>`→`>=`/`==`, `+`→`-`/`*`)
        rs.total_floor = 6;
        assert!(slippery_bridge_allowed(&rs)); // 6+1=7 > 6 (kills `>`→`<`)
        rs.master_deck.clear();
        assert!(!slippery_bridge_allowed(&rs)); // empty deck (kills `delete !`, `&&`→`||`)
    }

    #[test]
    fn endless_conveyor_needs_120_gold() {
        let mut rs = RunState::new();
        rs.gold = 119;
        assert!(!endless_conveyor_allowed(&rs));
        rs.gold = 120;
        assert!(endless_conveyor_allowed(&rs)); // boundary (kills `>=`→`<`)
    }

    #[test]
    fn luminous_choir_needs_149_gold() {
        let mut rs = RunState::new();
        rs.gold = 148;
        assert!(!luminous_choir_allowed(&rs));
        rs.gold = 149;
        assert!(luminous_choir_allowed(&rs)); // boundary (kills `>=`→`<`)
    }

    #[test]
    fn morphic_grove_needs_100_gold_and_two_cards() {
        let mut rs = RunState::new();
        rs.gold = 100;
        rs.master_deck = vec![
            deck_card(
                CardId::StrikeIronclad,
                CardType::Attack,
                sts2_core::cards::CardTags::NONE,
            ),
            deck_card(
                CardId::DefendIronclad,
                CardType::Skill,
                sts2_core::cards::CardTags::NONE,
            ),
        ];
        assert!(morphic_grove_allowed(&rs)); // 100 gold + 2 cards (boundaries)
        rs.gold = 99;
        assert!(!morphic_grove_allowed(&rs)); // gold short (kills first `>=`→`<`, `&&`→`||`)
        rs.gold = 100;
        rs.master_deck.pop();
        assert!(!morphic_grove_allowed(&rs)); // 1 card (kills second `>=`→`<`, `&&`→`||`)
    }

    #[test]
    fn fake_merchant_act_2_plus_and_gold_or_foul_potion() {
        let mut rs = RunState::new();
        rs.act_index = 2;
        rs.gold = 100;
        assert!(fake_merchant_allowed(&rs)); // act 2 + gold (boundary)

        // Act 1 is never allowed even with gold (kills `>`→`==`/`>=`/`<`, `&&`→`||`).
        rs.act_index = 1;
        assert!(!fake_merchant_allowed(&rs));

        // Act 2, gold short, no foul potion → false (kills gold `>=`→`<`).
        rs.act_index = 2;
        rs.gold = 99;
        assert!(!fake_merchant_allowed(&rs));

        // Act 2, gold short, BUT a foul potion → true (kills `||`→`&&`).
        rs.potions.push(sts2_core::potions::PotionKind::FoulPotion);
        assert!(fake_merchant_allowed(&rs));
    }

    #[test]
    fn ranwid_the_elder_act_2_plus_relic_gold_and_potion() {
        let mut rs = RunState::new();
        rs.act_index = 2;
        // A TRADABLE relic — Ranwid takes one, and the run's starter
        // (BurningBlood) is not something he will accept.
        rs.relics.push(sts2_core::RelicId::Anchor);
        rs.gold = 100;
        rs.potions.push(sts2_core::potions::PotionKind::FoulPotion);
        assert!(ranwid_the_elder_allowed(&rs)); // all four true (boundaries)

        // A starter-only shelf → false: non-empty, but nothing tradable.
        rs.relics = vec![sts2_core::RelicId::BurningBlood];
        assert!(!ranwid_the_elder_allowed(&rs));
        rs.relics = vec![sts2_core::RelicId::Anchor];

        // Drop the act → false (kills `>`→`==`/`>=`/`<`, first `&&`).
        rs.act_index = 1;
        assert!(!ranwid_the_elder_allowed(&rs));
        rs.act_index = 2;

        // No relic → false (kills `delete !` on relics, second `&&`).
        rs.relics.clear();
        assert!(!ranwid_the_elder_allowed(&rs));
        rs.relics.push(sts2_core::RelicId::Anchor);

        // Gold short → false (kills `>=`→`<`, third `&&`).
        rs.gold = 99;
        assert!(!ranwid_the_elder_allowed(&rs));
        rs.gold = 100;

        // No potion → false (kills `delete !` on potions, fourth `&&`).
        rs.potions.clear();
        assert!(!ranwid_the_elder_allowed(&rs));
    }

    #[test]
    fn relic_trader_act_2_plus_and_five_relics() {
        use sts2_core::RelicId;
        let mut rs = RunState::new();
        rs.act_index = 2;
        // Five TRADABLE relics — the gate counts `GetValidRelics`, so the
        // starter relic every run carries does not pay into it.
        rs.relics = vec![
            RelicId::Anchor,
            RelicId::BloodVial,
            RelicId::HappyFlower,
            RelicId::Lantern,
            RelicId::Pendulum,
        ];
        assert!(relic_trader_allowed(&rs)); // act 2 + 5 tradable (boundary)

        // Four relics → false (kills `>=`→`<`, `&&`→`||`).
        rs.relics.pop();
        assert!(!relic_trader_allowed(&rs));

        // Topping back up with the STARTER does not help — it is untradable.
        rs.relics.push(RelicId::BurningBlood);
        assert!(!relic_trader_allowed(&rs));

        // Five tradable but act 1 → false (kills `>`→`==`/`>=`/`<`, `&&`→`||`).
        rs.relics.push(RelicId::Pendulum);
        rs.act_index = 1;
        assert!(!relic_trader_allowed(&rs));
    }
}

#[cfg(test)]
mod beta_enchant_event_tests {
    use super::*;
    use sts2_core::cards::{CardId, EnchantmentId};

    fn card(id: CardId) -> sts2_core::Card {
        crate::cards::spec_index::card_pod_from_id(id, false).unwrap()
    }

    #[test]
    fn disabled_stone_push_cannot_be_selected_or_enumerated() {
        crate::cards::dispatch::ensure_installed();
        let mut rm =
            sts2_core::run_manager::RunManager::new(1, sts2_core::CharacterId::Necrobinder)
                .with_content_callbacks(crate::run_callbacks::ironclad_content_callbacks())
                .with_neow_start();
        rm.run_state.master_deck = vec![card(CardId::DefendNecrobinder)];
        rm.run_state
            .enter_event_room(sts2_core::EventId::StoneOfAllTime);
        let before = rm.run_state.current_hp;
        assert!(!rm
            .get_available_actions()
            .iter()
            .any(|a| matches!(a, sts2_core::run_manager::RunAction::EventChoice { idx: 1 })));
        assert_eq!(
            rm.take_action(sts2_core::run_manager::RunAction::EventChoice { idx: 1 }),
            sts2_core::run_manager::ActionResult::InvalidForPhase
        );
        assert!(dispatch::resolve(&mut rm.run_state, "push").is_none());
        assert_eq!(rm.run_state.current_hp, before);
    }

    #[test]
    fn grave_enchants_exhaust_card_and_spiral_only_offers_basic_strikes_or_defends() {
        crate::cards::dispatch::ensure_installed();
        let mut rm =
            sts2_core::run_manager::RunManager::new(1, sts2_core::CharacterId::Necrobinder)
                .with_content_callbacks(crate::run_callbacks::ironclad_content_callbacks());
        rm.run_state.interactive_card_transform = true;
        rm.run_state.master_deck = vec![
            card(CardId::Impervious),
            card(CardId::StrikeNecrobinder),
            card(CardId::DefendNecrobinder),
        ];
        rm.run_state
            .enter_event_room(sts2_core::EventId::GraveOfTheForgotten);
        assert!(dispatch::resolve_by_index(&mut rm.run_state, 0).is_some());
        assert_eq!(
            rm.run_state.master_deck[0].enchantment(EnchantmentId::SoulsPower),
            1
        );
        assert_eq!(
            rm.run_state.master_deck.last().unwrap().card_id,
            CardId::Decay
        );
        rm.run_state
            .enter_event_room(sts2_core::EventId::SpiralingWhirlpool);
        assert!(dispatch::resolve_by_index(&mut rm.run_state, 0).is_some());
        assert_eq!(
            rm.run_state.pending_deck_enchants[0].enchantment,
            EnchantmentId::Spiral
        );
        assert_eq!(rm.enchant_eligible_deck_indices(), vec![1, 2]);
        assert!(rm.run_state.pending_rewards.is_empty());
    }

    #[test]
    fn stone_push_exhausts_no_card_and_enchants_an_attack_with_vigorous_eight() {
        crate::cards::dispatch::ensure_installed();
        let mut rs = RunState::new();
        rs.master_deck = vec![
            card(CardId::DefendNecrobinder),
            card(CardId::StrikeNecrobinder),
        ];
        let hp = rs.current_hp;
        rs.enter_event_room(sts2_core::EventId::StoneOfAllTime);
        assert!(dispatch::resolve_by_index(&mut rs, 1).is_some());
        assert_eq!(rs.current_hp, hp - 6);
        assert_eq!(rs.master_deck[0].enchantment(EnchantmentId::Vigorous), 0);
        assert_eq!(rs.master_deck[1].enchantment(EnchantmentId::Vigorous), 8);
        assert!(rs.pending_rewards.is_empty());
    }
}

#[cfg(test)]
mod beta_event_transform_eligibility_tests {
    use super::*;
    #[test]
    fn transform_auto_skips_eternal_and_unremovable_cards() {
        crate::cards::dispatch::ensure_installed();
        let mut state = RunState::new();
        let mut strike = crate::cards::spec_index::card_pod_from_id(
            sts2_core::cards::CardId::StrikeNecrobinder,
            false,
        )
        .unwrap();
        strike.set_enchantment(sts2_core::cards::EnchantmentId::TezcatarasEmber, 1);
        state.master_deck = vec![
            strike,
            crate::cards::spec_index::card_pod_from_id(
                sts2_core::cards::CardId::DefendNecrobinder,
                false,
            )
            .unwrap(),
        ];
        assert_eq!(first_transformable_deck_index(&state), Some(1));
        let option = shared::AROMA_OF_CHAOS
            .options
            .iter()
            .find(|option| option.option_id == "let_go")
            .unwrap();
        (option.handler)(&mut state);
        assert_eq!(state.master_deck[0], strike);
        assert_eq!(state.master_deck.len(), 2);
    }
}
