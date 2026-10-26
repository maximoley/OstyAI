//! `human_features` — reconstruct per-floor run-state context from scraped
//! human `.run` files and emit value-net training rows labeled by the run's
//! TRUE outcome (plan §M6(c): real-outcome value net).
//!
//! The self-play value net plateaus because self-play produces ~0 winning
//! act-3 trajectories, so V never learns what winning states look like
//! (calibration improved but never converted — see
//! `project_combat_budget_not_a_fullrun_lever`). The spire-codex corpus has
//! ~thousands of act-3 WINS. This tool walks the corpus, forward-reconstructs
//! the master deck + relics + HP/gold per floor, encodes each via the SAME
//! `ContextFeatures::from_view` the runtime evaluator uses (so the trained net
//! loads schema-clean), and labels with a win-aware graded target.
//!
//! Output (consumed by `sts2-pycli/python/train_value.py`):
//!   - `features.jsonl`: one `FeatureRow` per floor (context only; no candidates)
//!   - `decisions.jsonl`: one `{seed, act}` per run; `act = acts_reached + win`
//!     so `per_seed_final_act`'s max gives losers {1,2,3} and winners {2,3,4}.
//!     V is used for `argmax E[ΔV]` (rank-invariant), so the absolute scale of
//!     the target is irrelevant — only that winners outrank equal-depth losers.
//!
//! Usage: `human_features <corpus_dir> <out_dir> [--character <name>]
//!         [--ascension-min N] [--ascension-max N] [--all-ascensions]`
//! (defaults: spire-codex Ironclad corpus under `STS2_SCRAPER_ROOT`, A0 only,
//! out `/tmp/human-v`).
//! Necrobinder A1–A10:
//!   `human_features .../necrobinder /tmp/human-necro --character necrobinder
//!    --ascension-min 1 --ascension-max 10`

use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde_json::Value;

use sts2_content::names::{card_id_from_str, potion_kind_from_str, relic_id_from_str};
use sts2_content::starters::starter_deck_for;
use sts2_core::characters::character_starter;
use sts2_core::potions::{CharacterId, PotionKind};
use sts2_core::relics::RelicId;
use sts2_core::run_manager::Phase;
use sts2_runai::eval::ranker_features::{feature_schema_id, ContextFeatures};
use sts2_runai::label::engine_identity;
use sts2_runai::view::{DeckCard, Offers, VisibleRunView};

/// Default corpus location — the `sts2_run_scraper` checkout next to the
/// monorepo — honored only when it exists on disk; `STS2_SCRAPER_ROOT` is the
/// portable way to point at the scraper checkout.
const LEGACY_CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../sts2_run_scraper/data/files-spire-codex/ironclad"
);
const DEFAULT_OUT: &str = "/tmp/human-v";
/// Base potion-belt capacity — uniform across all characters (3). A4
/// `TightBelt` (−1 slot) is not modeled; only the belt COUNT enters the
/// feature and the prior IC-only tool didn't model it either, so a flat 3
/// preserves behavior exactly.
const BASE_POTION_SLOTS: i32 = 3;

/// Strip the `CARD.`/`RELIC.`/`POTION.` namespace prefix the corpus prepends.
fn strip_ns(s: &str) -> &str {
    s.split_once('.').map_or(s, |(_, rest)| rest)
}

/// Parse a scraped card token (`"CARD.IRON_WAVE"`, `"CARD.X+1"`) into a deck
/// card. The `+N` suffix marks an upgrade.
fn parse_card(token: &str) -> Option<DeckCard> {
    let bare = strip_ns(token);
    let (base, upgraded) = bare
        .split_once('+')
        .map_or((bare, false), |(b, _)| (b, true));
    card_id_from_str(base).map(|card_id| DeckCard { card_id, upgraded })
}

/// Pull a card token out of either `"CARD.X"` or `{"id": "CARD.X"}`.
fn token_of(entry: &Value) -> Option<&str> {
    entry
        .as_str()
        .or_else(|| entry.get("id").and_then(Value::as_str))
}

/// The character's starter deck as `DeckCard`s, plus the A5+ `AscendersBane`
/// deck-tail curse (the scraped per-floor card deltas don't re-state the
/// starting curse). Replaces the former hardcoded `ironclad_starter()`; the
/// deck data now comes from the per-character factories via `starter_deck_for`.
fn starter_deck_dc(character: CharacterId, ascension: u8) -> Vec<DeckCard> {
    let mut deck: Vec<DeckCard> = starter_deck_for(character)
        .into_iter()
        .map(|s| DeckCard {
            card_id: s.id,
            upgraded: false,
        })
        .collect();
    if ascension >= 5 {
        if let Some(card_id) = card_id_from_str("ASCENDERS_BANE") {
            deck.push(DeckCard {
                card_id,
                upgraded: false,
            });
        }
    }
    deck
}

/// Relics held while ARRIVING at `global_floor`: the ever-present starter
/// relic plus every relic acquired on a strictly-earlier floor. The starter
/// relic is emitted exactly once even if the final-relic timeline also lists
/// it (it carries `floor_added_to_deck = 1`, which would otherwise duplicate).
fn arriving_relics(
    starter_relic: RelicId,
    timeline: &[(i64, RelicId)],
    global_floor: i64,
) -> Vec<RelicId> {
    let mut relics = vec![starter_relic];
    relics.extend(
        timeline
            .iter()
            .filter(|(f, rid)| *f < global_floor && *rid != starter_relic)
            .map(|(_, rid)| *rid),
    );
    relics
}

/// Apply this floor's deck mutations (gained/removed/upgraded/transformed) to
/// the running master deck, mirroring the corpus's per-floor delta fields.
fn apply_deck_deltas(deck: &mut Vec<DeckCard>, ps: &Value) {
    if let Some(arr) = ps.get("cards_gained").and_then(Value::as_array) {
        for e in arr {
            if let Some(c) = token_of(e).and_then(parse_card) {
                deck.push(c);
            }
        }
    }
    if let Some(arr) = ps.get("cards_removed").and_then(Value::as_array) {
        for e in arr {
            if let Some(c) = token_of(e).and_then(parse_card) {
                if let Some(pos) = deck.iter().position(|d| d.card_id == c.card_id) {
                    deck.remove(pos);
                }
            }
        }
    }
    if let Some(arr) = ps.get("upgraded_cards").and_then(Value::as_array) {
        for e in arr {
            if let Some(c) = token_of(e).and_then(parse_card) {
                // Upgrade the first not-yet-upgraded copy of that card.
                if let Some(d) = deck
                    .iter_mut()
                    .find(|d| d.card_id == c.card_id && !d.upgraded)
                {
                    d.upgraded = true;
                }
            }
        }
    }
    // Transforms: remove original, append final. Field is a single object or
    // an array of {original_card, final_card}.
    let transforms = ps.get("cards_transformed");
    let apply_one = |deck: &mut Vec<DeckCard>, t: &Value| {
        let orig = t
            .get("original_card")
            .and_then(token_of)
            .and_then(parse_card);
        let fin = t.get("final_card").and_then(token_of).and_then(parse_card);
        if let Some(o) = orig {
            if let Some(pos) = deck.iter().position(|d| d.card_id == o.card_id) {
                deck.remove(pos);
            }
        }
        if let Some(f) = fin {
            deck.push(f);
        }
    };
    match transforms {
        Some(Value::Array(arr)) => arr.iter().for_each(|t| apply_one(deck, t)),
        Some(t @ Value::Object(_)) => apply_one(deck, t),
        _ => {}
    }
}

/// The full relic timeline from the run's FINAL relic list. Each final relic
/// carries `floor_added_to_deck` — a 1-based global floor index that aligns
/// exactly with the flattened `map_point_history` (verified against Neow /
/// treasure / boss acquisitions). This captures EVERY source — Neow
/// (`ancient_choice`), treasure, shop, event, and boss relics — with precise
/// floor attribution, superseding the MVP's `relic_choices`-only accumulation
/// (which silently dropped Neow + boss relics). Relics removed mid-run (rare)
/// don't appear in the final list and are thus undercounted on the floors they
/// were briefly held — an accepted minor edge.
fn relic_timeline(run: &Value) -> Vec<(i64, RelicId)> {
    let mut out = Vec::new();
    let Some(arr) = run
        .get("players")
        .and_then(Value::as_array)
        .and_then(|p| p.first())
        .and_then(|p| p.get("relics"))
        .and_then(Value::as_array)
    else {
        return out;
    };
    for e in arr {
        let floor = e
            .get("floor_added_to_deck")
            .and_then(Value::as_i64)
            .unwrap_or(1);
        if let Some(rid) = e
            .get("id")
            .and_then(Value::as_str)
            .map(strip_ns)
            .and_then(relic_id_from_str)
        {
            out.push((floor, rid));
        }
    }
    out
}

/// Track the potion belt across a floor: consumed potions (`potion_used`) are
/// removed, reward picks (`potion_choices.was_picked`) are added up to the
/// character's slot cap. Only the belt COUNT enters the feature vector (the
/// `potion_count` scalar), so an approximate identity on an unparseable token
/// is harmless — what matters is the running count, which the MVP pinned at 0.
fn apply_potion_deltas(potions: &mut Vec<PotionKind>, ps: &Value, max_slots: i32) {
    if let Some(arr) = ps.get("potion_used").and_then(Value::as_array) {
        for e in arr {
            let kind = token_of(e).map(strip_ns).and_then(potion_kind_from_str);
            match kind.and_then(|k| potions.iter().position(|p| *p == k)) {
                Some(pos) => {
                    potions.remove(pos);
                }
                None => {
                    potions.pop();
                }
            }
        }
    }
    if let Some(arr) = ps.get("potion_choices").and_then(Value::as_array) {
        for e in arr {
            let picked = e
                .get("was_picked")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !picked {
                continue;
            }
            if let Some(kind) = e
                .get("choice")
                .and_then(Value::as_str)
                .map(strip_ns)
                .and_then(potion_kind_from_str)
            {
                if (potions.len() as i32) < max_slots {
                    potions.push(kind);
                }
            }
        }
    }
}

/// One per-floor value-net feature row (JSONL line in `features.jsonl`).
/// Formerly `sts2_runai::feature_export::FeatureRow` — inlined here when the
/// ranker training pipeline was removed (only this value export remains; the
/// `candidates` list is always empty on the per-floor value rows).
#[derive(serde::Serialize)]
struct FeatureRow {
    seed: u64,
    step: u64,
    floor: i32,
    surface: String,
    schema_id: String,
    engine: String,
    context: ContextFeatures,
    candidates: Vec<serde_json::Value>,
}

struct RunResult {
    rows: Vec<FeatureRow>,
    target: i64,
    win: bool,
    dropped_cards: u32,
}

/// Reconstruct one run into feature rows + a per-run target. `seed` is a
/// synthetic unique key (the run's index) used only for the features↔target
/// join and the train/val/test split.
/// Read a run file as JSON text, transparently inflating gzip (`.run.gz`).
/// The spire-codex corpus is stored gzipped; the base-game corpus is plain.
fn read_run_text(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = String::new();
        flate2::read::GzDecoder::new(&bytes[..])
            .read_to_string(&mut out)
            .ok()?;
        Some(out)
    } else {
        String::from_utf8(bytes).ok()
    }
}

fn process_run(
    path: &Path,
    seed: u64,
    character: CharacterId,
    asc_min: u8,
    asc_max: u8,
) -> Option<RunResult> {
    let text = read_run_text(path)?;
    let run: Value = serde_json::from_str(&text).ok()?;
    process_value(&run, seed, character, asc_min, asc_max)
}

/// Reconstruct one already-parsed run into feature rows. Split out of
/// `process_run` so the reconstruction is unit-testable without file IO.
fn process_value(
    run: &Value,
    seed: u64,
    character: CharacterId,
    asc_min: u8,
    asc_max: u8,
) -> Option<RunResult> {
    let build = run.get("build_id").and_then(Value::as_str).unwrap_or("");
    // Branch version/0.107.1: train against the v0.107.x human corpus (~20k IC
    // runs). v0.107.0 and v0.107.1 differ only in the RNG generator swap
    // (System.Random -> Xoshiro256**), which is irrelevant here: human_features
    // reconstructs per-floor state from the recorded run log, it does not
    // re-simulate with RNG. Same schema_version (9) and field layout as v0.103.
    if !build.starts_with("v0.107") {
        return None;
    }
    if run.get("game_mode").and_then(Value::as_str) != Some("standard") {
        return None;
    }
    if run
        .get("players")
        .and_then(Value::as_array)
        .map_or(1, Vec::len)
        != 1
    {
        return None;
    }
    let ascension = run.get("ascension").and_then(Value::as_u64).unwrap_or(0) as u8;
    if ascension < asc_min || ascension > asc_max {
        return None;
    }
    let win = run.get("win").and_then(Value::as_bool).unwrap_or(false);
    let mph = run.get("map_point_history").and_then(Value::as_array)?;

    let starter = character_starter(character);
    let mut deck = starter_deck_dc(character, ascension);
    let relic_timeline = relic_timeline(run);
    let schema_id = feature_schema_id().to_string();
    let engine = engine_identity().to_string();

    let mut potions: Vec<PotionKind> = Vec::new();
    let mut rows = Vec::new();
    let mut acts_reached = 0u8;
    let mut dropped_cards = 0u32;
    let mut step = 0u64;
    // 1-based global floor index; aligns with `floor_added_to_deck`.
    let mut global_floor = 0i64;

    for (act_idx, act) in mph.iter().enumerate() {
        let Some(floors) = act.as_array() else {
            continue;
        };
        if floors.is_empty() {
            continue;
        }
        acts_reached = (act_idx as u8) + 1;
        for (fl_i, floor) in floors.iter().enumerate() {
            global_floor += 1;
            let Some(ps) = floor
                .get("player_stats")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
            else {
                continue;
            };
            // Clamp to sane ranges: a minority of corpus floors carry sentinel
            // values (current_hp = 1e9, current_gold ~ 3e7 — the 999999999-class
            // sentinel) which otherwise blow up the scalar mean/std and crush
            // the real hp/gold signal to a dead constant after normalization
            // (harmless for the rank-robust value net, fatal for the ranker).
            let max_hp = (ps
                .get("max_hp")
                .and_then(Value::as_i64)
                .unwrap_or(i64::from(starter.hp)) as i32)
                .clamp(1, 999);
            let hp =
                (ps.get("current_hp").and_then(Value::as_i64).unwrap_or(0) as i32).clamp(0, max_hp);
            let gold =
                (ps.get("current_gold").and_then(Value::as_i64).unwrap_or(0) as i32).clamp(0, 9999);

            // Context = state ARRIVING at this floor (deck / relics / potions
            // accumulated from prior floors), then apply this floor's deltas
            // afterwards. Relics arriving = acquired strictly before this global
            // floor, plus the ever-present starter relic.
            let relics = arriving_relics(starter.starter_relic, &relic_timeline, global_floor);
            let view = VisibleRunView {
                character,
                ascension,
                act_index: act_idx as u8,
                act_floor: (fl_i as i32) + 1,
                hp,
                max_hp,
                gold,
                deck: deck.clone(),
                relics,
                potions: potions.clone(),
                max_potion_slots: BASE_POTION_SLOTS,
                boss: None,
                map: None,
                seen_events: BTreeSet::new(),
                current_event: None,
                phase: Phase::MapChoice,
                offers: Offers::default(),
            };
            let context = ContextFeatures::from_view(&view);
            rows.push(FeatureRow {
                seed,
                step,
                floor: (fl_i as i32) + 1,
                surface: "human".to_string(),
                schema_id: schema_id.clone(),
                engine: engine.clone(),
                context,
                candidates: Vec::new(),
            });
            step += 1;

            let before = deck.len();
            apply_deck_deltas(&mut deck, ps);
            // Track gained cards that failed to parse (removed/renamed content).
            if let Some(g) = ps.get("cards_gained").and_then(Value::as_array) {
                let parsed = g
                    .iter()
                    .filter(|e| token_of(e).and_then(parse_card).is_some())
                    .count();
                dropped_cards += (g.len() - parsed) as u32;
            }
            let _ = before;
            apply_potion_deltas(&mut potions, ps, BASE_POTION_SLOTS);
        }
    }

    if rows.is_empty() {
        return None;
    }
    // Win-aware graded target: deeper + winning runs score higher.
    let target = i64::from(acts_reached) + i64::from(win);
    Some(RunResult {
        rows,
        target,
        win,
        dropped_cards,
    })
}

fn collect_run_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_run_files(&p, out);
            continue;
        }
        let is_run = p.extension().is_some_and(|e| e == "run");
        // `.run.gz` has extension "gz", so match the full suffix for the
        // gzipped spire-codex corpus.
        let is_run_gz = p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".run.gz"));
        if is_run || is_run_gz {
            out.push(p);
        }
    }
}

/// The feature-layout descriptor the trainer reads to size its dense input
/// (`schema.json`). Built from the SAME `ranker_features` constants the rows
/// are emitted with, so schema ↔ features can never drift apart.
fn feature_schema_descriptor() -> Value {
    use sts2_runai::eval::ranker_features::{
        context_dim, feature_schema_id, n_cards, n_relics, n_scalars, N_CAND_FEAT,
    };
    serde_json::json!({
        "schema_id": feature_schema_id(),
        "engine": engine_identity(),
        "context_dim": context_dim(),
        "n_card": n_cards(),
        "n_relic": n_relics(),
        "n_scalar": n_scalars(),
        "n_cand_feat": N_CAND_FEAT,
    })
}

/// Map a `--character` token to a `CharacterId` (case-insensitive).
fn parse_character(s: &str) -> Option<CharacterId> {
    match s.to_ascii_lowercase().as_str() {
        "ironclad" => Some(CharacterId::Ironclad),
        "silent" => Some(CharacterId::Silent),
        "defect" => Some(CharacterId::Defect),
        "necrobinder" | "necro" => Some(CharacterId::Necrobinder),
        "regent" => Some(CharacterId::Regent),
        _ => None,
    }
}

/// Default corpus dir when no positional path is given: the Ironclad
/// spire-codex tree inside the external `sts2_run_scraper` checkout, located
/// via `STS2_SCRAPER_ROOT`, falling back to the legacy hardcoded location
/// only when that still exists on disk.
fn default_corpus() -> String {
    if let Ok(root) = std::env::var("STS2_SCRAPER_ROOT") {
        return Path::new(&root)
            .join("data/files-spire-codex/ironclad")
            .to_string_lossy()
            .into_owned();
    }
    if Path::new(LEGACY_CORPUS).is_dir() {
        return LEGACY_CORPUS.to_string();
    }
    eprintln!(
        "human_features: no <corpus_dir> given and STS2_SCRAPER_ROOT is unset \
         (legacy default {LEGACY_CORPUS} not found); pass a corpus dir or set \
         STS2_SCRAPER_ROOT to an sts2_run_scraper checkout"
    );
    std::process::exit(2);
}

/// Value of a `--flag <value>` pair, if present.
fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Positionals = args that are neither a `--flag` nor the value consumed by
    // a value-taking flag (else `necrobinder`/`1` would masquerade as the
    // corpus/out path).
    const VALUE_FLAGS: [&str; 3] = ["--character", "--ascension-min", "--ascension-max"];
    let mut positional: Vec<&String> = Vec::new();
    let mut skip = false;
    for a in &args[1..] {
        if skip {
            skip = false;
            continue;
        }
        if a.starts_with("--") {
            skip = VALUE_FLAGS.contains(&a.as_str());
            continue;
        }
        positional.push(a);
    }

    let corpus = positional
        .first()
        .map_or_else(default_corpus, |s| (*s).clone());
    let out_dir = positional.get(1).map_or(DEFAULT_OUT, |s| s.as_str());

    let character = match flag_value(&args, "--character") {
        Some(c) => parse_character(c).unwrap_or_else(|| {
            eprintln!("human_features: unknown --character '{c}'");
            std::process::exit(2);
        }),
        None => CharacterId::Ironclad,
    };
    // Ascension band: `--all-ascensions` = [0,255]; explicit `--ascension-min/
    // -max` set the bounds; no flag preserves the historical A0-only default.
    let (asc_min, asc_max) = if args.iter().any(|a| a == "--all-ascensions") {
        (0u8, 255u8)
    } else {
        let lo = flag_value(&args, "--ascension-min").and_then(|s| s.parse().ok());
        let hi = flag_value(&args, "--ascension-max").and_then(|s| s.parse().ok());
        match (lo, hi) {
            (None, None) => (0, 0),
            (l, h) => (l.unwrap_or(0), h.unwrap_or(255)),
        }
    };

    eprintln!(
        "human_features: scanning {corpus} (character={character:?}, \
         ascension=[{asc_min},{asc_max}])"
    );
    let mut files = Vec::new();
    collect_run_files(Path::new(&corpus), &mut files);
    files.sort(); // deterministic seed assignment
    eprintln!("human_features: {} .run files found", files.len());

    let results: Vec<RunResult> = files
        .par_iter()
        .enumerate()
        .filter_map(|(i, path)| process_run(path, i as u64, character, asc_min, asc_max))
        .collect();

    fs::create_dir_all(out_dir).expect("create out dir");
    let feat_path = format!("{out_dir}/features.jsonl");
    let dec_path = format!("{out_dir}/decisions.jsonl");
    let mut feat = std::io::BufWriter::new(fs::File::create(&feat_path).expect("features.jsonl"));
    let mut dec = std::io::BufWriter::new(fs::File::create(&dec_path).expect("decisions.jsonl"));

    let mut n_runs = 0u64;
    let mut n_wins = 0u64;
    let mut n_rows = 0u64;
    let mut dropped = 0u64;
    for r in &results {
        n_runs += 1;
        n_wins += u64::from(r.win);
        dropped += u64::from(r.dropped_cards);
        let seed = r.rows[0].seed;
        for row in &r.rows {
            serde_json::to_writer(&mut feat, row).expect("write feature");
            feat.write_all(b"\n").expect("nl");
            n_rows += 1;
        }
        serde_json::to_writer(
            &mut dec,
            &serde_json::json!({"seed": seed, "act": r.target, "win": i32::from(r.win)}),
        )
        .expect("write decision");
        dec.write_all(b"\n").expect("nl");
    }
    feat.flush().ok();
    dec.flush().ok();

    // Co-emit the schema so the trainer densifies against the exact layout
    // these rows were written with (schema_id embedded in every row).
    let schema_path = format!("{out_dir}/schema.json");
    fs::write(
        &schema_path,
        serde_json::to_string_pretty(&feature_schema_descriptor()).expect("schema json"),
    )
    .expect("write schema.json");

    eprintln!(
        "human_features: {n_runs} runs ({n_wins} wins, {:.1}%), {n_rows} feature rows, \
         {dropped} unparseable gained-cards dropped",
        if n_runs > 0 {
            100.0 * n_wins as f64 / n_runs as f64
        } else {
            0.0
        }
    );
    eprintln!("  features -> {feat_path}");
    eprintln!("  decisions -> {dec_path}");
    eprintln!("  schema   -> {schema_path}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(deck: &[DeckCard], name: &str) -> bool {
        let id = card_id_from_str(name).expect("card id");
        deck.iter().any(|d| d.card_id == id)
    }

    #[test]
    fn necro_starter_deck_is_necrobinder_not_ironclad() {
        let deck = starter_deck_dc(CharacterId::Necrobinder, 1);
        assert!(
            has(&deck, "STRIKE_NECROBINDER"),
            "must contain StrikeNecrobinder"
        );
        assert!(
            has(&deck, "BODYGUARD") && has(&deck, "UNLEASH"),
            "necro signature starters"
        );
        assert!(!has(&deck, "BASH"), "Bash is an IC-hardcoding leak");
        assert!(
            !has(&deck, "STRIKE_IRONCLAD"),
            "IC strike is a hardcoding leak"
        );
        assert_eq!(deck.len(), 10, "necro starter is 10 cards below A5");
    }

    #[test]
    fn ironclad_starter_deck_regression() {
        // The parametrized helper must reproduce the old hardcoded IC starter.
        let deck = starter_deck_dc(CharacterId::Ironclad, 1);
        assert!(has(&deck, "STRIKE_IRONCLAD") && has(&deck, "BASH"));
        assert_eq!(deck.len(), 10);
    }

    #[test]
    fn ascenders_bane_appended_at_a5_only() {
        assert!(!has(
            &starter_deck_dc(CharacterId::Necrobinder, 4),
            "ASCENDERS_BANE"
        ));
        assert!(has(
            &starter_deck_dc(CharacterId::Necrobinder, 5),
            "ASCENDERS_BANE"
        ));
    }

    #[test]
    fn arriving_relics_includes_starter_once_and_filters_by_floor() {
        let bp = relic_id_from_str("BOUND_PHYLACTERY").expect("BoundPhylactery");
        let other = relic_id_from_str("BURNING_BLOOD").expect("other relic");
        // starter always present, even with an empty timeline
        assert_eq!(arriving_relics(bp, &[], 1), vec![bp]);
        // starter never double-counted, even if the timeline re-lists it
        assert_eq!(arriving_relics(bp, &[(0, bp)], 5), vec![bp]);
        // prior-floor acquisitions included; strictly-future ones excluded
        assert_eq!(arriving_relics(bp, &[(2, other)], 5), vec![bp, other]);
        assert_eq!(arriving_relics(bp, &[(2, other)], 1), vec![bp]);
    }

    fn necro_run_json(ascension: u64) -> Value {
        serde_json::json!({
            "build_id": "v0.107.1",
            "game_mode": "standard",
            "ascension": ascension,
            "win": true,
            "players": [{
                "character": "CHARACTER.NECROBINDER",
                "deck": [],
                "relics": [{"floor_added_to_deck": 1, "id": "RELIC.BOUND_PHYLACTERY"}]
            }],
            "map_point_history": [[{
                "map_point_type": "monster",
                "player_stats": [{"current_hp": 50, "max_hp": 66, "current_gold": 99}],
                "rooms": []
            }]]
        })
    }

    #[test]
    fn process_value_respects_ascension_band() {
        // A3 is inside [1,10] -> reconstructed; A0 and A11 are outside -> dropped.
        assert!(process_value(&necro_run_json(3), 0, CharacterId::Necrobinder, 1, 10).is_some());
        assert!(process_value(&necro_run_json(0), 0, CharacterId::Necrobinder, 1, 10).is_none());
        assert!(process_value(&necro_run_json(11), 0, CharacterId::Necrobinder, 1, 10).is_none());
    }

    #[test]
    fn process_value_reconstructs_a_necro_run() {
        let r = process_value(&necro_run_json(3), 7, CharacterId::Necrobinder, 1, 10)
            .expect("in-band necro run reconstructs");
        assert!(!r.rows.is_empty(), "at least one per-floor feature row");
        assert!(r.win);
        assert_eq!(r.rows[0].seed, 7);
    }

    #[test]
    fn schema_descriptor_matches_the_live_feature_layout() {
        use sts2_runai::eval::ranker_features::{context_dim, feature_schema_id, N_CAND_FEAT};
        let s = feature_schema_descriptor();
        // The trainer densifies against these; if the descriptor drifts from
        // the layout the rows were emitted with, the join is silently wrong.
        assert_eq!(s["schema_id"].as_str(), Some(feature_schema_id()));
        assert_eq!(s["context_dim"].as_u64(), Some(context_dim() as u64));
        assert_eq!(s["n_cand_feat"].as_u64(), Some(N_CAND_FEAT as u64));
        assert!(s["n_card"].as_u64().is_some());
        assert!(s["n_relic"].as_u64().is_some());
        assert!(s["n_scalar"].as_u64().is_some());
    }
}
