//! `sts2-cli list <kind>` — static content inspection.
//!
//! Wave B2 (bead `sts2-rs-2o0k`) wired the `encounters` arm to the
//! `ENCOUNTERS` alias table; wave E1 (bead `sts2-rs-92sq`) fills in
//! `cards` / `relics` / `potions` against the same registry iterators
//! C1's `name_lookup` consumes (`CardId::all()` / `RelicId::all()` /
//! `PotionKind::ALL`).
//!
//! Output shape: one row per id as `<PascalCase>  "<display-string>"`,
//! two columns padded so the display column lines up. `PascalCase` is the
//! canonical CLI form (cliext plan §3.3); the display string is the
//! `pascal_display`-rendered human form (e.g. `BurningBlood` →
//! `"Burning Blood"`).

use std::io::{self, Write};

use clap::{Args, ValueEnum};

use sts2_content::encounters::{
    act1_overgrowth::ACT1_ENCOUNTERS, act1_underdocks::ACT1_UNDERDOCKS_ENCOUNTERS, EncounterTier,
};
use sts2_content::names::{deck_card_spec_pool, pascal_display};
use sts2_core::cards::CardId;
use sts2_core::relics::RelicId;
use sts2_core::PotionKind;

use crate::sim::scenario::encounter::SELECTABLE_MONSTERS;
use crate::sim::scenario::relics::relic_id_to_kind;
use crate::sim::scenario::{summarize_monsters, ENCOUNTERS};

#[derive(Debug, Clone, Copy, ValueEnum)]
#[non_exhaustive]
pub enum ListKind {
    Encounters,
    /// Monster roster — every selectable `MonsterKind`. Cliext Phase 4
    /// (`sts2-rs-90n5`); accepts `--filter act1` to scope to Act-1.
    Monsters,
    Cards,
    Relics,
    Potions,
    /// Cross-cutting content inventory: implemented vs total counts
    /// for every kind. Cliext H1 (bead `sts2-rs-ef4x`).
    Content,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ListFilter {
    /// No filter — print every row.
    All,
    /// Restrict `encounters` / `monsters` to Act-1.
    /// Cliext Phase 4 (`sts2-rs-90n5`).
    Act1,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Which content surface to list.
    pub kind: ListKind,

    /// Show every enum token (including unimplemented ones). Without
    /// this flag, `cards` / `relics` / `potions` filter to entries
    /// that actually have a runtime implementation — i.e. things you
    /// can pick on `--deck` / `--relic` / `--potion` without hitting
    /// an "unimplemented" error. Bead `sts2-rs-ef4x` / cliext H1.
    #[arg(long)]
    pub all: bool,

    /// Scope the listing to a specific act / tag. Currently supports
    /// `act1` for `encounters` + `monsters`. Cliext Phase 4
    /// (`sts2-rs-90n5`).
    #[arg(long, default_value = "all")]
    pub filter: ListFilter,
}

/// Entry point for `sts2-cli list <kind>`.
pub fn run<W: Write>(args: ListArgs, out: &mut W) -> io::Result<()> {
    match args.kind {
        ListKind::Encounters => list_encounters(out, args.filter),
        ListKind::Monsters => list_monsters(out, args.filter),
        ListKind::Cards => list_cards(out, args.all),
        ListKind::Relics => list_relics(out, args.all),
        ListKind::Potions => list_potions(out, args.all),
        ListKind::Content => list_content(out),
    }
}

/// Convert the canonical Python parity name (`UPPER_SNAKE_CASE`) to
/// `PascalCase`. Same logic as `sts2_content::names::snake_to_pascal`
/// but that helper is private; we re-derive it here for the four-line
/// loop to avoid widening the content-crate public surface for one
/// formatting concern.
fn snake_to_pascal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut next_upper = true;
    for ch in s.chars() {
        if ch == '_' {
            next_upper = true;
        } else if next_upper {
            out.extend(ch.to_uppercase());
            next_upper = false;
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

/// Shared two-column writer. `rows` is `(pascal, display)` pairs in the
/// emission order; column width is sized to the widest `PascalCase` form.
fn write_two_column<W: Write>(out: &mut W, rows: &[(String, String)]) -> io::Result<()> {
    let width = rows.iter().map(|(p, _)| p.len()).max().unwrap_or(0);
    for (pascal, display) in rows {
        writeln!(out, "{pascal:<width$}  \"{display}\"")?;
    }
    Ok(())
}

/// Render the encounter table — one row per entry, padded to a
/// consistent alias column so the monster summaries line up. Without
/// `--filter act1` this prints the legacy `ENCOUNTERS` alias list plus
/// the Act-1 registry; with `--filter act1` it prints only the
/// registry, grouped by tier.
fn list_encounters<W: Write>(out: &mut W, filter: ListFilter) -> io::Result<()> {
    let show_legacy = matches!(filter, ListFilter::All);
    let id_width = if show_legacy {
        ENCOUNTERS
            .iter()
            .map(|(a, _)| a.len())
            .chain(ACT1_ENCOUNTERS.iter().map(|e| e.id.len()))
            .chain(ACT1_UNDERDOCKS_ENCOUNTERS.iter().map(|e| e.id.len()))
            .max()
            .unwrap_or(0)
    } else {
        ACT1_ENCOUNTERS
            .iter()
            .chain(ACT1_UNDERDOCKS_ENCOUNTERS)
            .map(|e| e.id.len())
            .max()
            .unwrap_or(0)
    };

    if show_legacy {
        // Use a fixed seed so the printed summary is deterministic for
        // the (currently all-deterministic) v1 alias set. Future
        // randomized aliases will still render their seed=0
        // composition here, which is what `--seed 0 --encounter
        // <alias>` would produce.
        let mut rng = sts2_core::dotnet_rng::Rng::new(0_i32);
        writeln!(out, "# legacy aliases ({} entries)", ENCOUNTERS.len())?;
        for (alias, builder) in ENCOUNTERS {
            let specs = builder(&mut rng);
            writeln!(
                out,
                "{:<width$}  {}",
                alias,
                summarize_monsters(&specs),
                width = id_width
            )?;
        }
        writeln!(out)?;
    }

    // Act 1 ships TWO act models (GH #544) and a run walks exactly one, so
    // they print as separate sections rather than one merged act-1 list.
    for (title, table) in [
        ("act1 overgrowth registry", ACT1_ENCOUNTERS),
        ("act1 underdocks registry", ACT1_UNDERDOCKS_ENCOUNTERS),
    ] {
        let n = |t: EncounterTier| table.iter().filter(|e| e.tier == t).count();
        writeln!(
            out,
            "# {title} ({} entries — {} weak / {} normal / {} elite / {} boss)",
            table.len(),
            n(EncounterTier::Weak),
            n(EncounterTier::Normal),
            n(EncounterTier::Elite),
            n(EncounterTier::Boss),
        )?;
        for tier in [
            EncounterTier::Weak,
            EncounterTier::Normal,
            EncounterTier::Elite,
            EncounterTier::Boss,
        ] {
            for enc in table.iter().filter(|e| e.tier == tier) {
                writeln!(
                    out,
                    "{:<width$}  \"{}\"",
                    enc.id,
                    enc.display,
                    width = id_width
                )?;
            }
        }
        writeln!(out)?;
    }
    Ok(())
}

/// Render the monster roster — every entry in `SELECTABLE_MONSTERS`.
/// `--filter act1` filters to the 29 Act-1 monsters (7 `act1_weak`
/// including Nibbit + 22 act1, covering minions and raider variants
/// that may not appear in standalone `EncounterDef` rows). Default
/// `--filter all` includes the 5 STS1-name Phase-4 stubs as well.
/// Cliext Phase 4 (`sts2-rs-90n5`).
fn list_monsters<W: Write>(out: &mut W, filter: ListFilter) -> io::Result<()> {
    let rows: Vec<(&str, sts2_core::MonsterKind, bool)> = SELECTABLE_MONSTERS
        .iter()
        .filter(|(_, kind, sts2)| match filter {
            ListFilter::All => true,
            // act1: every STS2-native Act-1 monster (6 act1_weak + 22
            // act1, including minions and raider variants that may not
            // appear in standalone EncounterDef rows yet).
            ListFilter::Act1 => *sts2 && is_act1_kind(*kind),
        })
        .copied()
        .collect();

    let tag_width = rows.iter().map(|(t, _, _)| t.len()).max().unwrap_or(0);
    writeln!(out, "# monsters: {} entries", rows.len())?;
    for (tag, kind, sts2) in &rows {
        let provenance = if *sts2 { "sts2" } else { "sts1-stub" };
        writeln!(out, "{tag:<tag_width$}  {kind:?}  ({provenance})")?;
    }
    Ok(())
}

/// `true` when `kind` is any of the 28 Act-1 monsters (7 `act1_weak`
/// including Nibbit + 22 act1 — registry-listed and standalone-only
/// alike). The set mirrors `sts2_env/monsters/act1*.py` factories.
fn is_act1_kind(kind: sts2_core::MonsterKind) -> bool {
    use sts2_core::MonsterKind as M;
    matches!(
        kind,
        // act1_weak (7 — Nibbit + 6 others)
        M::Nibbit
            | M::ShrinkerBeetle
            | M::FuzzyWurmCrawler
            | M::LeafSlimeS
            | M::TwigSlimeS
            | M::LeafSlimeM
            | M::TwigSlimeM
            // act1 (22)
            | M::CubexConstruct
            | M::Flyconid
            | M::EyeWithTeeth
            | M::Fogmog
            | M::Inklet
            | M::Mawler
            | M::VineShambler
            | M::SlitheringStrangler
            | M::SnappingJaxfruit
            | M::AssassinRubyRaider
            | M::AxeRubyRaider
            | M::BruteRubyRaider
            | M::CrossbowRubyRaider
            | M::TrackerRubyRaider
            | M::BygoneEffigy
            | M::Byrdonis
            | M::PhrogParasite
            | M::Parafright
            | M::Vantom
            | M::CeremonialBeast
            | M::KinPriest
            | M::KinFollower
    )
}

fn list_cards<W: Write>(out: &mut W, all: bool) -> io::Result<()> {
    let total = CardId::all().len();
    let buildable: std::collections::HashSet<CardId> =
        deck_card_spec_pool().iter().map(|s| s.id).collect();
    let rows: Vec<(String, String)> = CardId::all()
        .iter()
        .filter(|id| all || buildable.contains(id))
        .map(|id| {
            let pascal = snake_to_pascal(id.to_python_name());
            let display = pascal_display(&pascal);
            (pascal, display)
        })
        .collect();
    writeln!(
        out,
        "# cards: {n} of {total} deck-buildable{tail}",
        n = if all { total } else { buildable.len() },
        tail = if all {
            " (--all dump: includes unimplemented enum tokens)"
        } else {
            " — some have conservative-marker handlers that no-op when \
             played (see docs/reality-checks/2026-05-12-gap-report.md §G3); \
             use --all to dump every enum token"
        },
    )?;
    write_two_column(out, &rows)
}

fn list_relics<W: Write>(out: &mut W, all: bool) -> io::Result<()> {
    let total = RelicId::all().len();
    let seedable_count = RelicId::all()
        .iter()
        .filter(|id| relic_id_to_kind(**id).is_some())
        .count();
    let rows: Vec<(String, String)> = RelicId::all()
        .iter()
        .filter(|id| all || relic_id_to_kind(**id).is_some())
        .map(|id| {
            let pascal = snake_to_pascal(id.to_python_name());
            let display = pascal_display(&pascal);
            (pascal, display)
        })
        .collect();
    writeln!(
        out,
        "# relics: {n} of {total} implemented{tail}",
        n = if all { total } else { seedable_count },
        tail = if all {
            " (--all dump: includes unseedable enum tokens)"
        } else {
            " — use --all to dump every enum token. P7 surfaces in flight"
        },
    )?;
    write_two_column(out, &rows)
}

fn list_potions<W: Write>(out: &mut W, all: bool) -> io::Result<()> {
    let total = PotionKind::ALL.len();
    let usable_count = PotionKind::ALL
        .iter()
        .filter(|p| !p.is_event_only())
        .count();
    let rows: Vec<(String, String)> = PotionKind::ALL
        .iter()
        .filter(|p| all || !p.is_event_only())
        .map(|id| {
            let pascal = snake_to_pascal(id.python_name());
            let display = pascal_display(&pascal);
            (pascal, display)
        })
        .collect();
    writeln!(
        out,
        "# potions: {n} of {total} in normal potion pools{tail}",
        n = if all { total } else { usable_count },
        tail = if all {
            " (--all includes event and token potions)"
        } else {
            " — use --all to include event and token potions"
        },
    )?;
    write_two_column(out, &rows)
}

/// Cross-cutting content-inventory summary for the `list content`
/// subcommand. Bead `sts2-rs-ef4x` / cliext H1.
fn list_content<W: Write>(out: &mut W) -> io::Result<()> {
    use crate::sim::scenario::encounter::SELECTABLE_MONSTERS;
    let card_total = CardId::all().len();
    let card_built = deck_card_spec_pool().len();
    let relic_total = RelicId::all().len();
    let relic_seedable = RelicId::all()
        .iter()
        .filter(|id| relic_id_to_kind(**id).is_some())
        .count();
    let potion_total = PotionKind::ALL.len();
    let potion_usable = PotionKind::ALL
        .iter()
        .filter(|p| !p.is_event_only())
        .count();
    let monster_count = SELECTABLE_MONSTERS.len();

    writeln!(
        out,
        "sts2-rs content inventory (implemented / total enum tokens):"
    )?;
    writeln!(
        out,
        "  monsters:   {monster_count} kinds   — 5 STS1-name Phase-4 stubs + \
         act1_weak STS2 set (H3 sts2-rs-yioi); ~117 DSL factories unreachable \
         (rc-wiring sts2-rs-78ks)"
    )?;
    writeln!(
        out,
        "  cards:      {card_built} of {card_total}   — every generated \
         CardSpec factory registered (H4 sts2-rs-pjln); some play_* handlers \
         are conservative-marker stubs that no-op when played",
    )?;
    writeln!(
        out,
        "  relics:     {relic_seedable} of {relic_total}   — P7 surfaces in flight \
         (bead 01o umbrella)",
    )?;
    writeln!(
        out,
        "  potions:    {potion_usable} of {potion_total}   — event-only potions are not seedable",
    )?;
    writeln!(
        out,
        "  encounters: {} alias groups + ad-hoc via --monsters",
        ENCOUNTERS.len()
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "Pick from the implemented surface with: `list cards`, `list relics`, `list potions`. \
         Pass --all to dump the full enum (most rows will be unselectable).",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_encounters_default_prints_legacy_aliases_plus_act1_registry() {
        let mut buf = Vec::new();
        list_encounters(&mut buf, ListFilter::All).unwrap();
        let text = String::from_utf8(buf).unwrap();
        // Legacy aliases. Bead `sts2-rs-ar9u` removed the STS1
        // scaffolding aliases (cultist / jaw-worm / two-sentries /
        // three-louses / slimes-2x-acid); `nibbit` is the lone
        // STS2-native legacy alias still present.
        assert!(text.contains("nibbit"));
        assert!(text.contains("legacy aliases"));
        assert!(!text.contains("cultist"));
        assert!(!text.contains("two-sentries"));
        // Act-1 registry entries (sts2-rs-k923).
        assert!(text.contains("MAWLER_NORMAL"));
        assert!(text.contains("THE_KIN_BOSS"));
        assert!(text.contains("VANTOM_BOSS"));
        assert!(text.contains("act1 overgrowth registry"));
        // Both Act-1 models list, under separate headings — a run walks one
        // or the other, so a merged list would misrepresent the pool
        // (GH #544).
        assert!(text.contains("act1 underdocks registry"));
        assert!(text.contains("TOADPOLES_WEAK"));
        assert!(text.contains("LAGAVULIN_MATRIARCH_BOSS"));
    }

    #[test]
    fn list_encounters_act1_filter_hides_legacy_aliases() {
        let mut buf = Vec::new();
        list_encounters(&mut buf, ListFilter::Act1).unwrap();
        let text = String::from_utf8(buf).unwrap();
        // Act-1 registry shown.
        assert!(text.contains("MAWLER_NORMAL"));
        // Legacy aliases header NOT shown.
        assert!(!text.contains("legacy aliases"));
    }

    #[test]
    fn list_monsters_act1_excludes_sts1_stubs() {
        let mut buf = Vec::new();
        list_monsters(&mut buf, ListFilter::Act1).unwrap();
        let text = String::from_utf8(buf).unwrap();
        // Act-1 monsters (22 + 6 act1_weak = 28).
        assert!(text.contains("mawler"));
        assert!(text.contains("vantom"));
        assert!(text.contains("ceremonial-beast"));
        assert!(text.contains("shrinker-beetle"));
        // STS1 stubs not shown.
        assert!(!text.contains("cultist"));
        assert!(!text.contains("jaw-worm"));
    }

    #[test]
    fn list_monsters_all_includes_everything() {
        // Bead `sts2-rs-ar9u`: STS1 stubs are gone from
        // `SELECTABLE_MONSTERS`; `list monsters --all` now enumerates
        // only the STS2-native roster.
        let mut buf = Vec::new();
        list_monsters(&mut buf, ListFilter::All).unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("nibbit"));
        assert!(text.contains("mawler"));
        assert!(text.contains("vantom"));
        assert!(!text.contains("cultist"));
    }

    #[test]
    fn list_cards_all_dumps_every_card_id() {
        let mut buf = Vec::new();
        list_cards(&mut buf, true).unwrap();
        let text = String::from_utf8(buf).unwrap();
        // First line is the inventory header (`# cards: ...`); the
        // remaining lines are the enum rows.
        let rows = text.lines().filter(|l| !l.starts_with('#')).count();
        assert_eq!(rows, CardId::all().len());
        assert!(text.starts_with("# cards:"));
    }

    #[test]
    fn list_cards_default_filters_to_buildable() {
        let mut buf = Vec::new();
        list_cards(&mut buf, false).unwrap();
        let text = String::from_utf8(buf).unwrap();
        let rows = text.lines().filter(|l| !l.starts_with('#')).count();
        // Filtered count matches the buildable pool size.
        assert_eq!(rows, deck_card_spec_pool().len());
        assert!(
            rows < CardId::all().len(),
            "filter should be strictly smaller"
        );
        assert!(text.contains("StrikeIronclad"));
        assert!(text.contains("PommelStrike"));
    }

    #[test]
    fn list_relics_all_dumps_every_relic_id() {
        let mut buf = Vec::new();
        list_relics(&mut buf, true).unwrap();
        let text = String::from_utf8(buf).unwrap();
        let rows = text.lines().filter(|l| !l.starts_with('#')).count();
        assert_eq!(rows, RelicId::all().len());
        assert!(text.contains("BurningBlood"));
    }

    #[test]
    fn list_relics_default_filters_to_seedable() {
        let mut buf = Vec::new();
        list_relics(&mut buf, false).unwrap();
        let text = String::from_utf8(buf).unwrap();
        let rows = text.lines().filter(|l| !l.starts_with('#')).count();
        let seedable = RelicId::all()
            .iter()
            .filter(|id| relic_id_to_kind(**id).is_some())
            .count();
        assert_eq!(rows, seedable);
        // Since the 2026-07-17 soft-drop census wave 3, the
        // `RelicId → RelicKind` projection is total (the last W9 Neow
        // deferrals gained marker kinds), so the seedable filter now
        // covers the full roster. If a future RelicId deliberately
        // ships without a RelicKind, this becomes `<` again — amend
        // alongside `to_relic_kind_is_total` in sts2-core.
        assert_eq!(rows, RelicId::all().len());
    }

    #[test]
    fn normal_potion_listing_includes_fairy_fruit_and_regen() {
        let mut buf = Vec::new();
        list_potions(&mut buf, false).unwrap();
        let text = String::from_utf8(buf).unwrap();
        for name in ["FairyInABottle", "FruitJuice", "RegenPotion"] {
            assert!(text.contains(name));
        }
        for name in [
            "Ambergris",
            "FoulPotion",
            "GlowwaterPotion",
            "PotionShapedRock",
        ] {
            assert!(!text.contains(name));
        }
        assert_eq!(
            text.lines().filter(|line| !line.starts_with('#')).count(),
            60
        );
    }

    #[test]
    fn list_potions_all_dumps_every_potion_kind() {
        let mut buf = Vec::new();
        list_potions(&mut buf, true).unwrap();
        let text = String::from_utf8(buf).unwrap();
        let rows = text.lines().filter(|l| !l.starts_with('#')).count();
        assert_eq!(rows, PotionKind::ALL.len());
        assert!(text.contains("FirePotion"));
    }

    #[test]
    fn list_content_summary_mentions_every_kind() {
        let mut buf = Vec::new();
        list_content(&mut buf).unwrap();
        let text = String::from_utf8(buf).unwrap();
        for needle in ["monsters:", "cards:", "relics:", "potions:", "encounters:"] {
            assert!(text.contains(needle), "missing '{needle}' in: {text}");
        }
    }
}
