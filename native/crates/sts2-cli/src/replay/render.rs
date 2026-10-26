//! HTML rendering for `FightRecord`. One self-contained page per
//! fight + a sibling `index.html`. Plain string concat — no
//! templating engine dependency, just `include_str!` of a small
//! CSS shell.

use std::fmt::Write as _;

use sts2_core::monsters::Intent;

use crate::replay::assets::{card_image_path, file_url, monster_image_path};
use crate::replay::record::{
    CardInHand, DecisionRecord, FightRecord, MonsterSnapshot, RootOutcomeRow, StateSnapshot,
};

const STYLE: &str = include_str!("template.css");
const TOP_K_VISIBLE: usize = 8;

#[must_use]
pub fn render_fight_html(record: &FightRecord) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">");
    write!(
        &mut out,
        "<title>{} — fight replay</title>",
        html_escape(&record.fixture_stem)
    )
    .unwrap();
    out.push_str("<style>");
    out.push_str(STYLE);
    out.push_str("</style></head><body>");
    render_header(&mut out, record);
    for decision in &record.decisions {
        render_decision(&mut out, decision);
    }
    render_footer(&mut out, record);
    out.push_str("<p class=\"home\"><a href=\"index.html\">← back to index</a></p>");
    out.push_str("</body></html>");
    out
}

#[must_use]
pub fn render_index_html(records: &[FightRecord]) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">");
    out.push_str("<title>fight replays</title><style>");
    out.push_str(STYLE);
    out.push_str("</style></head><body>");
    out.push_str("<h1>Fight replays</h1>");
    out.push_str("<table class=\"index\"><thead><tr>");
    out.push_str("<th>Encounter</th><th>Floor</th><th>Human</th>");
    out.push_str("<th>MCTS</th><th>Final HP</th><th>Turns</th><th>Decisions</th>");
    out.push_str("</tr></thead><tbody>");
    for r in records {
        let outcome = r.player_outcome_label.as_deref().unwrap_or("?").to_string();
        let mcts = if r.final_won { "WON" } else { "LOST" };
        let class_mcts = if r.final_won { "won" } else { "lost" };
        let encounter_label = r
            .encounter
            .as_deref()
            .unwrap_or(&r.fixture_stem)
            .to_string();
        write!(
            &mut out,
            "<tr><td><a href=\"{stem}.html\">{enc}</a></td>\
             <td>{floor}</td><td class=\"badge {hb}\">{human}</td>\
             <td class=\"badge {class_mcts}\">{mcts}</td>\
             <td>{hp}/{maxhp}</td><td>{round}</td><td>{n}</td></tr>",
            stem = html_escape(&r.fixture_stem),
            enc = html_escape(&encounter_label),
            floor = r.floor.map_or_else(|| "—".into(), |f| f.to_string()),
            hb = badge_class(&outcome),
            human = html_escape(&outcome),
            class_mcts = class_mcts,
            mcts = mcts,
            hp = r.final_player_hp,
            maxhp = r.final_player_max_hp,
            round = r.final_round,
            n = r.decisions.len(),
        )
        .unwrap();
    }
    out.push_str("</tbody></table>");
    out.push_str("</body></html>");
    out
}

fn badge_class(outcome: &str) -> &'static str {
    match outcome {
        "won" => "won",
        "lost" => "lost",
        _ => "neutral",
    }
}

fn render_header(out: &mut String, record: &FightRecord) {
    let mcts_outcome = if record.final_won { "WON" } else { "LOST" };
    let mcts_class = if record.final_won { "won" } else { "lost" };
    write!(
        out,
        "<header class=\"fight-header\">\
         <h1>{stem}</h1>\
         <p class=\"meta\">encounter: <code>{enc}</code> · floor: {floor} · human outcome: \
         <span class=\"badge {hb}\">{human}</span> · MCTS outcome: \
         <span class=\"badge {mc}\">{mo}</span> at turn {round}, HP {hp}/{maxhp}</p>\
         <p class=\"meta\">MCTS config: sims={sims}, rollout_depth={depth}, c_puct={cpuct}, \
         seed={seed}, tree_reuse={reuse}</p>",
        stem = html_escape(&record.fixture_stem),
        enc = html_escape(record.encounter.as_deref().unwrap_or(&record.fixture_stem)),
        floor = record.floor.map_or_else(|| "—".into(), |f| f.to_string()),
        hb = badge_class(record.player_outcome_label.as_deref().unwrap_or("?")),
        human = html_escape(record.player_outcome_label.as_deref().unwrap_or("?")),
        mc = mcts_class,
        mo = mcts_outcome,
        round = record.final_round,
        hp = record.final_player_hp,
        maxhp = record.final_player_max_hp,
        sims = record.config.sims,
        depth = record.config.rollout_depth,
        cpuct = record.config.c_puct,
        seed = record.config.seed,
        reuse = record.config.tree_reuse,
    )
    .unwrap();
    if let Some(reason) = &record.aborted_reason {
        write!(
            out,
            "<p class=\"meta abort\">aborted: {}</p>",
            html_escape(reason)
        )
        .unwrap();
    }
    out.push_str("</header>");
}

fn render_footer(out: &mut String, record: &FightRecord) {
    let mcts_outcome = if record.final_won { "WON" } else { "LOST" };
    let mcts_class = if record.final_won { "won" } else { "lost" };
    write!(
        out,
        "<section class=\"terminal\"><h2>Terminal state</h2>\
         <p>MCTS outcome: <span class=\"badge {mc}\">{mo}</span> · turn {round} · \
         player HP {hp}/{maxhp} · {n} decisions recorded.</p></section>",
        mc = mcts_class,
        mo = mcts_outcome,
        round = record.final_round,
        hp = record.final_player_hp,
        maxhp = record.final_player_max_hp,
        n = record.decisions.len(),
    )
    .unwrap();
}

fn render_decision(out: &mut String, d: &DecisionRecord) {
    write!(
        out,
        "<section class=\"decision\"><h2>Decision #{i} · turn {t}</h2>",
        i = d.decision_index,
        t = d.pre.turn,
    )
    .unwrap();
    render_state(out, &d.pre);
    render_search(out, d);
    render_delta(out, d);
    out.push_str("</section>");
}

fn render_state(out: &mut String, s: &StateSnapshot) {
    out.push_str("<div class=\"state\">");
    // Player
    write!(
        out,
        "<div class=\"player\"><h3>Player</h3>\
         <p>HP <strong>{hp}/{max}</strong> · block <strong>{block}</strong> · \
         energy <strong>{e}/{me}</strong> · phase <code>{phase:?}</code></p>\
         <p class=\"piles\">draw {d} · discard {disc} · exhaust {ex}</p>",
        hp = s.player_hp,
        max = s.player_max_hp,
        block = s.player_block,
        e = s.player_energy,
        me = s.player_max_energy,
        phase = s.phase,
        d = s.draw_n,
        disc = s.discard_n,
        ex = s.exhaust_n,
    )
    .unwrap();
    if !s.player_powers.is_empty() {
        out.push_str("<p class=\"powers\">powers: ");
        for (kind, amount) in &s.player_powers {
            write!(out, "<span class=\"pow\">{kind:?} ({amount})</span> ").unwrap();
        }
        out.push_str("</p>");
    }
    render_hand(out, &s.hand);
    out.push_str("</div>");
    // Monsters
    out.push_str("<div class=\"monsters\"><h3>Monsters</h3>");
    for m in &s.monsters {
        render_monster(out, m);
    }
    out.push_str("</div></div>");
}

fn render_hand(out: &mut String, hand: &[CardInHand]) {
    out.push_str("<div class=\"hand\"><h4>Hand</h4><div class=\"hand-row\">");
    for c in hand {
        let img = card_image_path(c.card_id).map(|p| file_url(&p));
        let label = format!("{:?}{}", c.card_id, if c.upgraded { "+" } else { "" });
        out.push_str("<div class=\"card\">");
        if let Some(url) = img {
            write!(
                out,
                "<img src=\"{}\" alt=\"{}\" loading=\"lazy\">",
                html_escape(&url),
                html_escape(&label)
            )
            .unwrap();
        } else {
            write!(
                out,
                "<div class=\"card-text\">{}</div>",
                html_escape(&label)
            )
            .unwrap();
        }
        write!(
            out,
            "<div class=\"card-meta\">h{} · cost {}</div>\
             <div class=\"card-id\">{}</div></div>",
            c.hand_index,
            c.cost,
            html_escape(&label),
        )
        .unwrap();
    }
    out.push_str("</div></div>");
}

fn render_monster(out: &mut String, m: &MonsterSnapshot) {
    let img = m.kind.and_then(monster_image_path).map(|p| file_url(&p));
    let intent_text = m
        .intent
        .as_ref()
        .map_or_else(|| "—".to_string(), format_intent);
    out.push_str("<div class=\"monster\">");
    if let Some(url) = img {
        write!(
            out,
            "<img src=\"{}\" alt=\"{}\" loading=\"lazy\">",
            html_escape(&url),
            html_escape(&m.display_name),
        )
        .unwrap();
    } else {
        write!(
            out,
            "<div class=\"monster-text\">{}</div>",
            html_escape(&m.display_name)
        )
        .unwrap();
    }
    let alive_class = if m.alive { "" } else { " dead" };
    write!(
        out,
        "<div class=\"monster-meta{alive}\"><div class=\"name\">slot {s} · {n}</div>\
         <div>HP {hp}/{max} · block {b}</div>\
         <div class=\"intent\">intent: {it}</div>",
        alive = alive_class,
        s = m.slot,
        n = html_escape(&m.display_name),
        hp = m.hp,
        max = m.max_hp,
        b = m.block,
        it = html_escape(&intent_text),
    )
    .unwrap();
    if !m.powers.is_empty() {
        out.push_str("<div class=\"powers\">");
        for (kind, amount) in &m.powers {
            write!(out, "<span class=\"pow\">{kind:?} ({amount})</span> ").unwrap();
        }
        out.push_str("</div>");
    }
    out.push_str("</div></div>");
}

fn format_intent(intent: &Intent) -> String {
    match intent {
        Intent::Attack { damage } => format!("Attack {damage}"),
        Intent::MultiAttack { damage, hits } => format!("Attack {damage} × {hits}"),
        Intent::Defend { block } => format!("Defend {block}"),
        Intent::Buff { kind, amount } => format!("Buff {kind:?} {amount}"),
        Intent::Debuff { kind, amount } => format!("Debuff {kind:?} {amount}"),
        Intent::DebuffStrong => "Strong debuff".to_string(),
        Intent::Sleep => "Sleep".to_string(),
        Intent::Stun => "Stun".to_string(),
        Intent::StatusCard => "Status card".to_string(),
        Intent::Unknown => "Unknown".to_string(),
        Intent::Summon => "Summon".to_string(),
        Intent::Heal => "Heal".to_string(),
        other => format!("{other:?}"),
    }
}

fn render_search(out: &mut String, d: &DecisionRecord) {
    out.push_str("<div class=\"search\"><h3>MCTS</h3>");
    let q_best = best_q_idx(&d.root_outcomes);
    write!(
        out,
        "<p>recommended: <strong>{}</strong> · total visits: {}</p>",
        html_escape(&d.recommended_label),
        d.total_visits,
    )
    .unwrap();
    out.push_str(
        "<table class=\"outcomes\"><thead><tr><th>rank</th><th>action</th>\
                  <th>visits</th><th>share</th><th>Q</th><th>W/L/T</th></tr></thead><tbody>",
    );
    let total = d.total_visits.max(1) as f32;
    for (rank, row) in d.root_outcomes.iter().enumerate().take(TOP_K_VISIBLE) {
        write_row(out, rank, row, total, d.recommended_idx, q_best);
    }
    out.push_str("</tbody></table>");
    if d.root_outcomes.len() > TOP_K_VISIBLE {
        out.push_str("<details><summary>show remaining actions</summary>");
        out.push_str(
            "<table class=\"outcomes\"><thead><tr><th>rank</th><th>action</th>\
                      <th>visits</th><th>share</th><th>Q</th><th>W/L/T</th></tr></thead><tbody>",
        );
        for (rank, row) in d.root_outcomes.iter().enumerate().skip(TOP_K_VISIBLE) {
            write_row(out, rank, row, total, d.recommended_idx, q_best);
        }
        out.push_str("</tbody></table></details>");
    }
    if !d.pv.is_empty() {
        out.push_str("<p class=\"pv\">PV: ");
        for (i, step) in d.pv.iter().enumerate() {
            if i > 0 {
                out.push_str(" → ");
            }
            write!(out, "<code>{}</code>", html_escape(step)).unwrap();
        }
        out.push_str("</p>");
    }
    out.push_str("</div>");
}

fn write_row(
    out: &mut String,
    rank: usize,
    row: &RootOutcomeRow,
    total: f32,
    recommended_idx: Option<usize>,
    q_best: Option<usize>,
) {
    let is_rec = recommended_idx == Some(rank);
    let is_q_best = q_best == Some(rank);
    let mut classes = String::new();
    if is_rec {
        classes.push_str(" recommended");
    }
    if is_q_best && !is_rec {
        classes.push_str(" q-best");
    }
    write!(
        out,
        "<tr class=\"{cls}\"><td>{rank}</td><td>{action}</td>\
         <td>{visits}</td><td>{share:.1}%</td><td>{q:+.3}</td>\
         <td>{w}/{l}/{t}</td></tr>",
        cls = classes.trim(),
        rank = rank,
        action = html_escape(&row.action_label),
        visits = row.visits,
        share = 100.0 * row.visits as f32 / total,
        q = row.mean_value,
        w = row.wins,
        l = row.losses,
        t = row.ties,
    )
    .unwrap();
}

fn best_q_idx(rows: &[RootOutcomeRow]) -> Option<usize> {
    rows.iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            a.mean_value
                .partial_cmp(&b.mean_value)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
}

fn render_delta(out: &mut String, d: &DecisionRecord) {
    out.push_str("<div class=\"delta\"><h3>Δ</h3><ul>");
    let dp = d.player_hp_after - d.pre.player_hp;
    let cls = delta_class(dp);
    let after = d.player_hp_after;
    write!(
        out,
        "<li>player HP: <span class=\"{cls}\">{dp:+}</span> (now {after})</li>",
    )
    .unwrap();
    for (slot, hp_after) in &d.monster_hp_after {
        let dm = d
            .pre
            .monsters
            .iter()
            .find(|m| m.slot == *slot)
            .map_or(0, |pre| hp_after - pre.hp);
        let cls = delta_class(dm);
        write!(
            out,
            "<li>monster[{slot}] HP: <span class=\"{cls}\">{dm:+}</span> (now {hp_after})</li>",
        )
        .unwrap();
    }
    write!(
        out,
        "<li>engine result: <code>{}</code></li>",
        html_escape(&d.step_result)
    )
    .unwrap();
    out.push_str("</ul></div>");
}

fn delta_class(d: i32) -> &'static str {
    use std::cmp::Ordering;
    match d.cmp(&0) {
        Ordering::Less => "neg",
        Ordering::Greater => "pos",
        Ordering::Equal => "",
    }
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::record::RecorderConfig;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use sts2_core::cards::character_of;
    use sts2_core::cards::{CardId, CardType};
    use sts2_core::monsters::MonsterKind;
    use sts2_core::{Action, CombatPhase, PowerKind};

    static CTR: AtomicUsize = AtomicUsize::new(0);

    /// Temp `STS2_WIKI` root carrying real sprite files for one card and
    /// one monster, so the `<img>` branches in `render_hand` /
    /// `render_monster` fire while the unsprited entries hit the text
    /// fallback. Per-process env isolation comes from nextest.
    fn wiki_with_sprites(card: CardId, monster: MonsterKind) -> PathBuf {
        let n = CTR.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("sts2render_{}_{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let atlas = root
            .join("site/public/images/card_atlas")
            .join(character_of(card));
        fs::create_dir_all(&atlas).unwrap();
        let stem_raw = card.to_python_name().to_ascii_lowercase();
        let stem = stem_raw
            .strip_suffix("_card")
            .map_or(stem_raw.clone(), str::to_string);
        fs::write(atlas.join(format!("{stem}.png")), b"png").unwrap();

        let mon_dir = root.join("site/public/images/monsters");
        fs::create_dir_all(&mon_dir).unwrap();
        // `python_name` -> lower_snake matches assets::pascal_to_snake.
        let mut snake = String::new();
        for (i, ch) in monster.python_name().chars().enumerate() {
            if ch.is_ascii_uppercase() && i > 0 {
                snake.push('_');
            }
            snake.extend(ch.to_lowercase());
        }
        fs::write(mon_dir.join(format!("{snake}.png")), b"png").unwrap();
        std::env::set_var("STS2_WIKI", &root);
        root
    }

    fn sample_state() -> StateSnapshot {
        StateSnapshot {
            turn: 2,
            phase: CombatPhase::PlayerPlayPhase,
            player_hp: 55,
            player_max_hp: 80,
            player_block: 5,
            player_energy: 3,
            player_max_energy: 3,
            draw_n: 4,
            discard_n: 1,
            exhaust_n: 0,
            hand: vec![
                // Sprited card -> <img> branch.
                CardInHand {
                    hand_index: 0,
                    card_id: CardId::StrikeIronclad,
                    upgraded: true,
                    cost: 1,
                    card_type: CardType::Attack,
                },
                // Unsprited card -> text-badge branch.
                CardInHand {
                    hand_index: 1,
                    card_id: CardId::Inflame,
                    upgraded: false,
                    cost: 1,
                    card_type: CardType::Power,
                },
            ],
            monsters: vec![
                // Sprited, alive, with powers + an Attack intent.
                MonsterSnapshot {
                    slot: 0,
                    kind: Some(MonsterKind::Nibbit),
                    display_name: "Nibbit".to_string(),
                    hp: 30,
                    max_hp: 48,
                    block: 0,
                    alive: true,
                    intent: Some(sts2_core::monsters::Intent::Attack { damage: 7 }),
                    powers: vec![(PowerKind::Strength, 2)],
                },
                // Unsprited (kind None), dead, no powers, no intent.
                MonsterSnapshot {
                    slot: 1,
                    kind: None,
                    display_name: "Mystery".to_string(),
                    hp: 0,
                    max_hp: 20,
                    block: 0,
                    alive: false,
                    intent: None,
                    powers: vec![],
                },
            ],
            player_powers: vec![(PowerKind::Strength, 1)],
            potions_held: 1,
        }
    }

    /// A decision with 10 root outcomes (> TOP_K_VISIBLE) so both the
    /// visible table and the `<details>` overflow table render, a PV
    /// chain, and a monster-hp-after entry for a slot with no matching
    /// pre-monster (exercising the `map_or(0, …)` fallback).
    fn sample_decision() -> DecisionRecord {
        let rows: Vec<RootOutcomeRow> = (0..10)
            .map(|i| RootOutcomeRow {
                action: Action::EndTurn,
                action_label: format!("act {i}"),
                visits: 100 - i as u32,
                wins: i as u32,
                losses: 1,
                ties: 0,
                // Make rank 1 the Q-best so the `q-best` class (distinct
                // from `recommended`) is emitted.
                mean_value: if i == 1 { 0.9 } else { 0.1 },
            })
            .collect();
        DecisionRecord {
            decision_index: 0,
            pre: sample_state(),
            root_outcomes: rows,
            total_visits: 1000,
            recommended_idx: Some(0),
            recommended_label: "End turn".to_string(),
            pv: vec!["Play Strike (h0)".to_string(), "End turn".to_string()],
            player_hp_after: 50,
            monster_hp_after: vec![(0, 18), (5, 0)],
            step_result: "Advanced".to_string(),
        }
    }

    fn sample_record(stem: &str, won: bool, with_meta: bool) -> FightRecord {
        FightRecord {
            fixture_path: PathBuf::from(format!("/tmp/{stem}.toml")),
            fixture_stem: stem.to_string(),
            encounter: with_meta.then(|| "Nibbit & co <test>".to_string()),
            floor: with_meta.then_some(7),
            player_outcome_label: with_meta.then(|| "won".to_string()),
            config: RecorderConfig::default(),
            decisions: vec![sample_decision()],
            final_won: won,
            final_round: 4,
            final_player_hp: 50,
            final_player_max_hp: 80,
            aborted_reason: Some("engine paused in phase Foo".to_string()),
        }
    }

    #[test]
    fn render_fight_html_includes_state_search_and_delta() {
        let _root = wiki_with_sprites(CardId::StrikeIronclad, MonsterKind::Nibbit);
        let rec = sample_record("nibbit_fight", true, true);
        let html = render_fight_html(&rec);

        assert!(html.starts_with("<!doctype html>"));
        assert!(html.ends_with("</body></html>"));
        // Header reflects MCTS outcome + config + abort reason.
        assert!(html.contains("MCTS outcome"));
        assert!(html.contains("sims=50000"));
        assert!(html.contains("aborted:"));
        // The `<` in the encounter label must be escaped, never raw.
        assert!(html.contains("Nibbit &amp; co &lt;test&gt;"));
        assert!(!html.contains("co <test>"));
        // Sprited card + monster produce <img>; unsprited produce text.
        assert!(html.contains("<img src="));
        assert!(html.contains("Mystery")); // kind:None text fallback
                                           // Overflow table appears because 10 > TOP_K_VISIBLE.
        assert!(html.contains("show remaining actions"));
        // Recommended + q-best row classes both rendered.
        assert!(html.contains("class=\"recommended\""));
        assert!(html.contains("q-best"));
        // PV chain joined with arrows.
        assert!(html.contains("PV:") && html.contains(" → "));
        // Delta section: player lost 5 HP -> neg class, monster delta too.
        assert!(html.contains("player HP"));
        assert!(html.contains("class=\"neg\""));
    }

    #[test]
    fn render_index_html_rows_cover_won_lost_and_missing_meta() {
        let won = sample_record("won_fight", true, true);
        let lost = sample_record("lost_fight", false, false);
        let html = render_index_html(&[won, lost]);

        assert!(html.contains("<h1>Fight replays</h1>"));
        assert!(html.contains(">WON<"));
        assert!(html.contains(">LOST<"));
        // Won row links to its page and shows floor 7.
        assert!(html.contains("href=\"won_fight.html\""));
        assert!(html.contains("<td>7</td>"));
        // Missing-meta row falls back to em-dash floor + fixture stem as
        // the encounter label + "?" human outcome.
        assert!(html.contains("<td>—</td>"));
        assert!(html.contains("lost_fight"));
        assert!(html.contains(">?<"));
    }

    #[test]
    fn badge_class_maps_known_outcomes() {
        assert_eq!(badge_class("won"), "won");
        assert_eq!(badge_class("lost"), "lost");
        assert_eq!(badge_class("?"), "neutral");
    }

    #[test]
    fn delta_class_signs() {
        assert_eq!(delta_class(-3), "neg");
        assert_eq!(delta_class(4), "pos");
        assert_eq!(delta_class(0), "");
    }

    #[test]
    fn html_escape_replaces_all_special_chars() {
        assert_eq!(
            html_escape("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&#39;f"
        );
        assert_eq!(html_escape("plain"), "plain");
    }

    #[test]
    fn format_intent_renders_each_shape() {
        use sts2_core::monsters::Intent;
        assert_eq!(format_intent(&Intent::Attack { damage: 6 }), "Attack 6");
        assert_eq!(
            format_intent(&Intent::MultiAttack { damage: 3, hits: 4 }),
            "Attack 3 × 4"
        );
        assert_eq!(format_intent(&Intent::Defend { block: 5 }), "Defend 5");
        assert!(format_intent(&Intent::Buff {
            kind: PowerKind::Strength,
            amount: 2
        })
        .starts_with("Buff"));
        assert!(format_intent(&Intent::Debuff {
            kind: PowerKind::Weak,
            amount: 1
        })
        .starts_with("Debuff"));
        assert_eq!(format_intent(&Intent::Sleep), "Sleep");
        assert_eq!(format_intent(&Intent::Unknown), "Unknown");
    }

    #[test]
    fn best_q_idx_picks_max_mean_value() {
        let mk = |q: f32| RootOutcomeRow {
            action: Action::EndTurn,
            action_label: String::new(),
            visits: 1,
            wins: 0,
            losses: 0,
            ties: 0,
            mean_value: q,
        };
        let rows = vec![mk(0.1), mk(0.7), mk(0.3)];
        assert_eq!(best_q_idx(&rows), Some(1));
        assert_eq!(best_q_idx(&[]), None);
    }
}
