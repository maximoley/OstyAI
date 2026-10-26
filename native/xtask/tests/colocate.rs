use xtask_lib::colocate::parse_specs;

const SPECS: &str = "\
use sts2_core::cards::{CardId, CardType, TargetType};
use super::types::{CardKeywords, CardRarity, CardSpec};
#[must_use]
pub const fn strike_ironclad() -> CardSpec {
    CardSpec { id: CardId::StrikeIronclad, name: \"STRIKE_IRONCLAD\", base_damage: 6, ..CardSpec::base() }
}
#[must_use]
pub const fn thunderclap() -> CardSpec {
    CardSpec { id: CardId::Thunderclap, name: \"THUNDERCLAP\", base_damage: 4, ..CardSpec::base() }
}
";

#[test]
fn parses_stem_cardid_and_body() {
    let specs = parse_specs(SPECS);
    assert_eq!(specs.len(), 2);
    let tc = specs.iter().find(|s| s.stem == "thunderclap").unwrap();
    assert_eq!(tc.card_id, "CardId::Thunderclap");
    assert!(tc.body.contains("base_damage: 4"));
    assert!(tc.body.trim_start().starts_with('{'));
}

use xtask_lib::colocate::parse_upgrade_arms;

const UPGRADES: &str = "\
use sts2_core::cards::CardId;
use super::types::{CardKeywords, CardUpgradeDelta};
pub fn upgrade_delta(card_id: CardId) -> Option<CardUpgradeDelta> {
    match card_id {
        CardId::Anger => Some(CardUpgradeDelta { cost: None, base_damage: Some(8), base_block: None, keywords: None }),
        CardId::Thunderclap => Some(CardUpgradeDelta { cost: None, base_damage: Some(7), base_block: None, keywords: None }),
        _ => None,
    }
}
";

#[test]
fn parses_upgrade_arms_by_cardid() {
    let arms = parse_upgrade_arms(UPGRADES);
    assert_eq!(
        arms.get("CardId::Thunderclap")
            .map(|e| e.contains("Some(8)")),
        Some(false)
    );
    assert!(arms.get("CardId::Thunderclap").unwrap().contains("Some(7)"));
    assert!(!arms.contains_key("_")); // the wildcard is not an arm
}

use std::collections::BTreeSet;
use xtask_lib::colocate::{build_plan, Plan};

#[test]
fn build_plan_appends_spec_and_upgrade_and_lists_specs() {
    let existing: BTreeSet<String> = ["thunderclap".to_string()].into_iter().collect();
    let plan: Plan = build_plan(SPECS, UPGRADES, &existing, "ironclad", &BTreeSet::new());

    // thunderclap.rs has a handler -> append spec()+upgrade()
    let tc = &plan.appendix["thunderclap"];
    assert!(tc.contains("pub(crate) const fn spec() -> CardSpec"));
    assert!(tc.contains("base_damage: 4"));
    assert!(tc.contains("pub(crate) fn upgrade() -> Option<CardUpgradeDelta>"));
    assert!(tc.contains("Some(7)")); // its upgrade arm

    // strike_ironclad has NO handler -> spec-only file, upgrade() returns None
    let st = &plan.appendix["strike_ironclad"];
    assert!(st.contains("pub(crate) const fn spec() -> CardSpec"));
    assert!(st.contains("fn upgrade() -> Option<CardUpgradeDelta> {\n    None\n}"));
    assert!(plan.spec_only.contains("strike_ironclad"));

    // central slice references per-card spec(), original source order preserved
    assert!(plan
        .specs_slice
        .contains("crate::cards::ironclad::thunderclap::spec"));
    assert!(plan
        .specs_slice
        .contains("crate::cards::ironclad::strike_ironclad::spec"));

    // a delegate_stem gets a `spec()` that delegates to helpers, not the body.
    let plan2: Plan = build_plan(
        SPECS,
        UPGRADES,
        &existing,
        "ironclad",
        &["thunderclap".to_string()].into_iter().collect(),
    );
    let tc2 = &plan2.appendix["thunderclap"];
    assert!(
        tc2.contains("super::helpers::thunderclap()"),
        "delegate stem must delegate: {tc2}"
    );
    assert!(
        !tc2.contains("base_damage: 4"),
        "delegate stem must NOT inline body: {tc2}"
    );

    // delegation arm for the upgraded card
    assert!(plan
        .delegations
        .contains("CardId::Thunderclap => crate::cards::ironclad::thunderclap::upgrade(),"));
    // no delegation for a card with no upgrade arm
    assert!(!plan.delegations.contains("strike_ironclad::upgrade"));
}

use xtask_lib::colocate::disambiguate_tests_globs;

#[test]
fn disambiguate_picks_first_module_of_a_multiline_glob() {
    // colorless regression: a cargo-fmt-wrapped glob puts `use super::{` on its
    // own line; the disambiguator must scan past that empty opener to the first
    // real module (`alchemize`), not bail to the hardcoded `aggression`
    // fallback (which isn't a colorless module -> E0432).
    let src = "\
use super::prelude::*;
use super::{
    alchemize::*, anointed::*, apotheosis::*,
    beacon_of_hope::*,
};
";
    let out = disambiguate_tests_globs(src);
    assert!(
        out.contains("use super::alchemize::{spec, upgrade};"),
        "got: {out}"
    );
    assert!(
        !out.contains("aggression"),
        "must not fall back to aggression: {out}"
    );
}

#[test]
fn disambiguate_still_handles_single_line_glob() {
    let src = "use super::prelude::*;\nuse super::{adaptive_strike::*, ball_lightning::*};\n";
    let out = disambiguate_tests_globs(src);
    assert!(
        out.contains("use super::adaptive_strike::{spec, upgrade};"),
        "got: {out}"
    );
}

use xtask_lib::colocate::fixup_prelude;

#[test]
fn fixup_prelude_adds_cardspec_when_handler_prelude_lacks_it() {
    // defect regression: handler bodies never name CardSpec, so the split
    // prelude lacks it — but relocated spec() bodies need it in scope.
    let pl = "\
pub(crate) use sts2_core::cards::{CardId, VarKey};
pub(crate) use crate::cards::defect_specs_generated as specs;
pub(crate) use super::specs::*;
";
    let out = fixup_prelude(pl, "defect_specs_generated");
    assert!(
        out.contains("pub(crate) use crate::cards::types::CardSpec;"),
        "got: {out}"
    );
    // ironclad-shaped prelude already has CardSpec -> not duplicated
    let with = "pub(crate) use crate::cards::types::CardSpec;\n";
    assert_eq!(
        fixup_prelude(with, "ironclad_specs_generated")
            .matches("CardSpec")
            .count(),
        1
    );
}

#[test]
fn fixup_prelude_adds_cardtype_when_handler_prelude_lacks_it() {
    // regent regression: handlers never name CardType, so the split prelude
    // lacks it — but relocated spec() bodies reference CardType::X.
    let pl = "pub(crate) use sts2_core::cards::CardId;\n";
    let out = fixup_prelude(pl, "regent_specs_generated");
    assert!(
        out.contains("pub(crate) use sts2_core::cards::CardType;"),
        "got: {out}"
    );
    // already-present CardType is not duplicated.
    let with = "pub(crate) use sts2_core::cards::{CardId, CardType, VarKey};\n";
    assert_eq!(
        fixup_prelude(with, "x_specs_generated")
            .matches("CardType")
            .count(),
        1
    );
}

#[test]
fn fixup_prelude_drops_orphaned_cfg_test_attr_with_genmod_line() {
    // defect regression: the `#[cfg(test)]` gating `as specs` sat on its OWN
    // line above the shared `status::{…}` import. Dropping only the `use` line
    // orphaned the attribute onto the status import, gating it behind test cfg.
    let pl = "\
pub(crate) use sts2_core::cards::CardId;
#[cfg(test)]
use crate::cards::defect_specs_generated as specs;
pub(crate) use crate::cards::status::{card_from_spec, spawn_void};
";
    let out = fixup_prelude(pl, "defect_specs_generated");
    assert!(
        !out.contains("#[cfg(test)]"),
        "orphaned cfg-test attr must be dropped: {out}"
    );
    assert!(!out.contains("defect_specs_generated"));
    assert!(out.contains("pub(crate) use crate::cards::status::{card_from_spec, spawn_void};"));
    // the surviving status import must NOT be gated behind cfg(test): the line
    // immediately before it is not an attribute.
    let lines: Vec<&str> = out.lines().collect();
    let idx = lines.iter().position(|l| l.contains("status::{")).unwrap();
    assert!(
        !lines[idx - 1].trim_start().starts_with("#["),
        "status import wrongly cfg-gated: {out}"
    );
}
