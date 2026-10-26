//! Format the engine card play-log into workbench event-log lines.
//!
//! Port of `sts2-pycli/python/workbench/playlog.py`.
//!
//! The Rust engine records the ordered sequence of cards that resolved during
//! one action — the manually played card FIRST, then any engine auto-plays
//! (Cascade / Havoc / Stampede / Mayhem) it spawned. This renders that
//! parent-first sequence into event-log lines. Pure: takes the list of entry
//! dicts (sliced to one action's tail by the caller) plus an id->display-name
//! resolver.

use serde_json::Value;

use crate::render::py;

/// Two-space indent + downwards-arrow-with-tip-rightwards.
const AUTO: &str = "  ↳ ";

/// Return ordered event-log lines for one action's play-log tail.
///
/// * `entries`: play-log entry dicts from `RunManager.card_play_log()`,
///   already sliced to this action's plays.
/// * `name_of`: maps a card id (SCREAMING_SNAKE) to a display title; pass
///   `|i| i.to_string()` for the identity the Python defaults to.
#[must_use]
pub fn format_play_log(entries: &[Value], name_of: &dyn Fn(&str) -> String) -> Vec<String> {
    let mut lines = Vec::with_capacity(entries.len());
    for e in entries {
        // `str(e.get("id", "?"))` — a present-but-null id prints as "None".
        let id = py::get_raw(e, "id").map_or_else(|| "?".to_string(), py::to_str);
        let mut name = name_of(&id);
        if py::truthy_opt(py::get_raw(e, "upgraded")) {
            name.push('+');
        }
        if py::truthy_opt(py::get_raw(e, "auto_played")) {
            lines.push(format!("{AUTO}{name} (auto-played)"));
            continue;
        }
        // Only X-cost triggers carry a stamped energy_spent at record time; a
        // normal card logs 0, so a positive value IS the X to surface.
        let suffix = py::get_raw(e, "energy_spent")
            .filter(|s| py::truthy(s))
            .map_or_else(String::new, |s| format!(" (X={})", py::to_str(s)));
        lines.push(format!("Play {name}{suffix}"));
    }
    lines
}

#[cfg(test)]
mod tests {
    //! Port of `tests/workbench/test_playlog.py` (every case, same names).

    use super::*;
    use serde_json::json;

    fn e(id: &str, upgraded: bool, energy_spent: i64, auto_played: bool) -> Value {
        json!({
            "id": id,
            "upgraded": upgraded,
            "energy_spent": energy_spent,
            "auto_played": auto_played,
            "target": null,
        })
    }

    fn identity(i: &str) -> String {
        i.to_string()
    }

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn test_empty_log_is_no_lines() {
        assert!(format_play_log(&[], &identity).is_empty());
    }

    #[test]
    fn test_manual_xcost_shows_x_value() {
        assert_eq!(
            format_play_log(&[e("Cascade", false, 3, false)], &identity),
            lines(&["Play Cascade (X=3)"])
        );
    }

    #[test]
    fn test_non_xcost_manual_has_no_x_suffix() {
        assert_eq!(
            format_play_log(&[e("Strike", false, 0, false)], &identity),
            lines(&["Play Strike"])
        );
    }

    #[test]
    fn test_upgraded_marked_with_plus() {
        assert_eq!(
            format_play_log(&[e("Strike", true, 0, false)], &identity),
            lines(&["Play Strike+"])
        );
    }

    #[test]
    fn test_auto_plays_indented_under_trigger() {
        let log = [
            e("Cascade", false, 3, false),
            e("Strike", false, 0, true),
            e("Strike", false, 0, true),
            e("Defend", false, 0, true),
        ];
        assert_eq!(
            format_play_log(&log, &identity),
            lines(&[
                "Play Cascade (X=3)",
                "  ↳ Strike (auto-played)",
                "  ↳ Strike (auto-played)",
                "  ↳ Defend (auto-played)",
            ])
        );
    }

    #[test]
    fn test_name_resolver_is_applied() {
        let name_of = |i: &str| {
            if i == "CASCADE" {
                "Cascade".to_string()
            } else {
                i.to_string()
            }
        };
        let out = format_play_log(&[e("CASCADE", false, 2, false)], &name_of);
        assert_eq!(out, lines(&["Play Cascade (X=2)"]));
    }

    #[test]
    fn missing_id_and_odd_energy_render_like_python() {
        let out = format_play_log(
            &[json!({}), json!({"id": null, "energy_spent": 2.0})],
            &identity,
        );
        assert_eq!(out, lines(&["Play ?", "Play None (X=2.0)"]));
    }
}
