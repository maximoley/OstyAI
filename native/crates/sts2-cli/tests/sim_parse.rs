//! Grammar tests for the combat-screen command parser
//! (`sts2_cli::sim::parse`).
//!
//! The inline `#[cfg(test)]` mod in `parse.rs` covers the happy paths of
//! `play` / `potion` / `hint` / `eval`. This file covers the surfaces it
//! doesn't: every `CmpOp` spelling and its comparison semantics, the
//! `ParseError` `Display` strings the REPL echoes back to the user, the
//! whole `assert` predicate family, `info` / `card`, bare-int dispatch,
//! and the argument-rejection paths of each verb.
//!
//! Nothing here encodes game mechanics — this is REPL surface syntax, so
//! the C# decompilation has no bearing on it. Trailing tokens are
//! rejected uniformly across every verb (the fixed-arity verbs
//! `play` / `potion` / `info` / `assert` once silently dropped them
//! while `<action-index>` / `eval` / `hint` were loud — the regression
//! test below pins the now-consistent behavior).

use hegel::generators as gs;
use hegel::TestCase;
use sts2_cli::sim::parse::{parse, Assertion, CmpOp, Command, ParseError};
use sts2_core::Action;

/// Every valid line shape the grammar accepts, used by the
/// whitespace-insignificance property.
const VALID_LINES: &[&str] = &[
    "play 0 1",
    "play 3",
    "potion 1 2",
    "potion 0",
    "hint",
    "hint 500 --fair=250",
    "hint --fair-all",
    "eval 1 2000",
    "assert hp <= 40",
    "assert hp != 0",
    "assert phase Combat",
    "assert monsters_alive 2",
    "info 4",
    "card 0",
    "end",
    "hand",
    "state",
    "actions",
    "help",
    "quit",
    "7",
];

// --- CmpOp ---------------------------------------------------------------

#[test]
fn cmp_op_parses_every_spelling() {
    assert_eq!(CmpOp::parse("<"), Some(CmpOp::Lt));
    assert_eq!(CmpOp::parse("<="), Some(CmpOp::Le));
    assert_eq!(CmpOp::parse("=="), Some(CmpOp::Eq));
    // `=` is an accepted alias for `==`.
    assert_eq!(CmpOp::parse("="), Some(CmpOp::Eq));
    assert_eq!(CmpOp::parse(">="), Some(CmpOp::Ge));
    assert_eq!(CmpOp::parse(">"), Some(CmpOp::Gt));
    assert_eq!(CmpOp::parse("!="), Some(CmpOp::Ne));
}

#[test]
fn cmp_op_rejects_non_operators() {
    for bad in ["", "=>", "=<", "<>", "!", "===", "≤", "eq"] {
        assert_eq!(CmpOp::parse(bad), None, "'{bad}' should not parse");
    }
}

#[test]
fn cmp_op_check_matches_operator_semantics() {
    // (lhs, rhs) triples spanning below / equal / above.
    let cases: [(i32, i32); 3] = [(2, 3), (3, 3), (4, 3)];
    let expected: [(CmpOp, [bool; 3]); 6] = [
        (CmpOp::Lt, [true, false, false]),
        (CmpOp::Le, [true, true, false]),
        (CmpOp::Eq, [false, true, false]),
        (CmpOp::Ge, [false, true, true]),
        (CmpOp::Gt, [false, false, true]),
        (CmpOp::Ne, [true, false, true]),
    ];
    for (op, wants) in expected {
        for (i, (lhs, rhs)) in cases.iter().enumerate() {
            assert_eq!(
                op.check(lhs, rhs),
                wants[i],
                "{op:?}.check({lhs}, {rhs}) mismatch"
            );
        }
    }
}

#[test]
fn cmp_op_check_is_generic_over_ord() {
    // `check` is used on the i32 HP axis today but is generic; strings
    // exercise the non-Copy `PartialOrd` path.
    assert!(CmpOp::Lt.check(&"a".to_string(), &"b".to_string()));
    assert!(CmpOp::Ne.check(&1.5_f64, &2.5_f64));
}

// --- ParseError Display --------------------------------------------------

#[test]
fn parse_error_display_strings() {
    let unknown = ParseError::UnknownCommand("frobnicate".to_string());
    assert_eq!(
        unknown.to_string(),
        "unknown command: 'frobnicate' (try `help`)"
    );
    let bad = ParseError::BadArg {
        cmd: "play",
        detail: "missing hand index".to_string(),
    };
    assert_eq!(bad.to_string(), "bad arg for `play`: missing hand index");
}

#[test]
fn parse_error_display_is_reachable_from_parse() {
    let err = parse("play").unwrap_err();
    assert_eq!(
        err.to_string(),
        "bad arg for `play`: missing hand index",
        "the REPL echoes this string verbatim"
    );
}

// --- bare-int dispatch ---------------------------------------------------

#[test]
fn bare_int_dispatches_to_action_index() {
    assert_eq!(parse("0").unwrap(), Command::ActionIndex(0));
    assert_eq!(parse("7").unwrap(), Command::ActionIndex(7));
    assert_eq!(parse("255").unwrap(), Command::ActionIndex(255));
    assert_eq!(parse("  3  ").unwrap(), Command::ActionIndex(3));
}

#[test]
fn bare_int_rejects_trailing_tokens() {
    let ParseError::BadArg { cmd, detail } = parse("0 foo").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "<action-index>");
    assert!(
        detail.contains("trailing tokens"),
        "unexpected detail: {detail}"
    );
}

#[test]
fn out_of_u8_range_ints_are_unknown_commands() {
    // `256` / `-1` don't parse as u8, so they fall through the bare-int
    // branch into verb dispatch and surface as unknown commands rather
    // than as an out-of-range action index.
    assert_eq!(
        parse("256").unwrap_err(),
        ParseError::UnknownCommand("256".to_string())
    );
    assert_eq!(
        parse("-1").unwrap_err(),
        ParseError::UnknownCommand("-1".to_string())
    );
}

// --- play / potion -------------------------------------------------------

#[test]
fn play_rejects_missing_and_malformed_args() {
    assert!(matches!(
        parse("play"),
        Err(ParseError::BadArg { cmd: "play", .. })
    ));
    let ParseError::BadArg { cmd, detail } = parse("play x").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "play");
    assert!(detail.contains("hand index 'x'"), "detail: {detail}");
    let ParseError::BadArg { cmd, detail } = parse("play 0 z").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "play");
    assert!(detail.contains("target 'z'"), "detail: {detail}");
    // Negative / out-of-u8 indices take the same "not a non-negative
    // integer" path.
    assert!(matches!(
        parse("play -1"),
        Err(ParseError::BadArg { cmd: "play", .. })
    ));
    assert!(matches!(
        parse("play 300"),
        Err(ParseError::BadArg { cmd: "play", .. })
    ));
}

#[test]
fn potion_rejects_missing_and_malformed_args() {
    let ParseError::BadArg { cmd, detail } = parse("potion").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "potion");
    assert!(detail.contains("missing potion slot"), "detail: {detail}");
    let ParseError::BadArg { cmd, detail } = parse("potion q").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "potion");
    assert!(detail.contains("slot 'q'"), "detail: {detail}");
    let ParseError::BadArg { cmd, detail } = parse("potion 1 q").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "potion");
    assert!(detail.contains("target 'q'"), "detail: {detail}");
}

/// Regression: the fixed-arity verbs once stopped reading after their
/// last argument, so a scripted-replay typo like `play 0 1 2` was
/// accepted with an argument silently vanishing — while the bare-int
/// path rejected trailing tokens precisely to be "loud rather than
/// silently dropping" (parse.rs). All verbs are loud now.
#[test]
fn fixed_arity_verbs_reject_trailing_tokens() {
    assert!(matches!(
        parse("play 0 1 nonsense"),
        Err(ParseError::BadArg { cmd: "play", .. })
    ));
    assert!(matches!(
        parse("potion 0 1 nonsense"),
        Err(ParseError::BadArg { cmd: "potion", .. })
    ));
    // A non-numeric token in target position is a target error, not a
    // trailing-token error.
    assert!(matches!(
        parse("potion 0 nonsense"),
        Err(ParseError::BadArg { cmd: "potion", .. })
    ));
    assert!(matches!(
        parse("info 0 nonsense"),
        Err(ParseError::BadArg { cmd: "info", .. })
    ));
    assert!(matches!(
        parse("assert hp <= 40 oops"),
        Err(ParseError::BadArg {
            cmd: "assert hp",
            ..
        })
    ));
    assert!(matches!(
        parse("assert phase Combat oops"),
        Err(ParseError::BadArg {
            cmd: "assert phase",
            ..
        })
    ));
    assert!(matches!(
        parse("assert monsters_alive 1 extra"),
        Err(ParseError::BadArg {
            cmd: "assert monsters_alive",
            ..
        })
    ));
}

// --- info / card ---------------------------------------------------------

#[test]
fn info_and_card_are_aliases() {
    assert_eq!(parse("info 0").unwrap(), Command::Info(0));
    assert_eq!(parse("card 4").unwrap(), Command::Info(4));
    assert_eq!(parse("CARD 4").unwrap(), Command::Info(4));
}

#[test]
fn info_rejects_missing_and_malformed_index() {
    let ParseError::BadArg { cmd, detail } = parse("info").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "info");
    assert!(detail.contains("missing hand index"), "detail: {detail}");
    // The `card` alias reports the canonical `info` cmd name.
    let ParseError::BadArg { cmd, detail } = parse("card nope").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "info");
    assert!(detail.contains("hand index 'nope'"), "detail: {detail}");
}

// --- assert --------------------------------------------------------------

#[test]
fn assert_hp_accepts_every_operator() {
    let cases = [
        ("assert hp < 10", CmpOp::Lt),
        ("assert hp <= 10", CmpOp::Le),
        ("assert hp == 10", CmpOp::Eq),
        ("assert hp = 10", CmpOp::Eq),
        ("assert hp >= 10", CmpOp::Ge),
        ("assert hp > 10", CmpOp::Gt),
        ("assert hp != 10", CmpOp::Ne),
    ];
    for (line, op) in cases {
        assert_eq!(
            parse(line).unwrap(),
            Command::Assert(Assertion::PlayerHp(op, 10)),
            "line: {line}"
        );
    }
}

#[test]
fn assert_hp_rhs_may_be_negative() {
    // HP is an i32 axis; a negative rhs is a legal (if odd) bound.
    assert_eq!(
        parse("assert hp > -5").unwrap(),
        Command::Assert(Assertion::PlayerHp(CmpOp::Gt, -5))
    );
}

#[test]
fn assert_hp_rejects_missing_and_malformed_parts() {
    for (line, cmd) in [
        ("assert hp", "assert hp"),
        ("assert hp <=", "assert hp"),
        ("assert hp ~ 3", "assert hp"),
        ("assert hp <= x", "assert hp"),
    ] {
        let ParseError::BadArg { cmd: got, .. } = parse(line).unwrap_err() else {
            panic!("expected a BadArg error for '{line}'");
        };
        assert_eq!(got, cmd, "line: {line}");
    }
    let ParseError::BadArg { detail, .. } = parse("assert hp ~ 3").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert!(detail.contains("unknown op '~'"), "detail: {detail}");
}

#[test]
fn assert_phase_keeps_the_name_verbatim() {
    assert_eq!(
        parse("assert phase Combat").unwrap(),
        Command::Assert(Assertion::Phase("Combat".to_string()))
    );
    // Case is preserved — the predicate name is compared downstream, not
    // normalized here.
    assert_eq!(
        parse("assert phase cOmBaT").unwrap(),
        Command::Assert(Assertion::Phase("cOmBaT".to_string()))
    );
    let ParseError::BadArg { cmd, detail } = parse("assert phase").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "assert phase");
    assert!(detail.contains("missing phase name"), "detail: {detail}");
}

#[test]
fn assert_monsters_alive_parses_and_rejects() {
    assert_eq!(
        parse("assert monsters_alive 0").unwrap(),
        Command::Assert(Assertion::MonstersAlive(0))
    );
    assert_eq!(
        parse("assert monsters_alive 3").unwrap(),
        Command::Assert(Assertion::MonstersAlive(3))
    );
    let ParseError::BadArg { cmd, detail } = parse("assert monsters_alive").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "assert monsters_alive");
    assert!(detail.contains("missing count"), "detail: {detail}");
    // usize, so a negative count is malformed rather than clamped.
    let ParseError::BadArg { cmd, detail } = parse("assert monsters_alive -1").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "assert monsters_alive");
    assert!(detail.contains("count '-1'"), "detail: {detail}");
}

#[test]
fn assert_rejects_missing_and_unknown_predicates() {
    let ParseError::BadArg { cmd, detail } = parse("assert").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "assert");
    assert!(detail.contains("hp / phase / monsters_alive"), "{detail}");
    let ParseError::BadArg { cmd, detail } = parse("assert energy == 3").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "assert");
    assert!(detail.contains("unknown predicate 'energy'"), "{detail}");
}

#[test]
fn assert_predicate_kind_is_case_sensitive() {
    // Only the head verb is lowercased (`parse` calls
    // `to_ascii_lowercase` on it); predicate kinds are matched raw, so
    // `HP` is an unknown predicate even though `ASSERT` is fine.
    assert!(matches!(parse("ASSERT hp < 1"), Ok(Command::Assert(_))));
    let ParseError::BadArg { cmd, detail } = parse("assert HP < 1").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "assert");
    assert!(detail.contains("unknown predicate 'HP'"), "{detail}");
}

// --- hint / eval flag errors --------------------------------------------

#[test]
fn hint_rejects_malformed_sims_and_flags() {
    let ParseError::BadArg { cmd, detail } = parse("hint abc").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "hint");
    assert!(detail.contains("sims 'abc'"), "detail: {detail}");
    let ParseError::BadArg { cmd, detail } = parse("hint --fair=abc").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "hint");
    assert!(detail.contains("--fair value 'abc'"), "detail: {detail}");
    let ParseError::BadArg { detail, .. } = parse("hint --fair-all=abc").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert!(detail.contains("--fair-all value 'abc'"), "{detail}");
    assert!(matches!(
        parse("hint --fair-all=0"),
        Err(ParseError::BadArg { cmd: "hint", .. })
    ));
}

#[test]
fn hint_rejects_garbage_suffix_after_fair() {
    // `--fairy` strips the `--fair` prefix but leaves a non-`=` body,
    // which is reported with the four accepted spellings.
    let ParseError::BadArg { cmd, detail } = parse("hint --fairy").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "hint");
    assert!(detail.contains("unknown flag '--fairy'"), "{detail}");
    assert!(detail.contains("--fair-all=N"), "{detail}");
    let ParseError::BadArg { detail, .. } = parse("hint --fair-allx").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert!(detail.contains("unknown flag '--fair-allx'"), "{detail}");
}

#[test]
fn hint_fair_all_wins_the_prefix_race() {
    // `--fair-all=7` must not be read as `--fair` with a `-all=7` body.
    assert_eq!(
        parse("hint --fair-all=7").unwrap(),
        Command::Hint {
            sims: None,
            fair_per_action: Some(Some(7)),
            fair_all: true,
        }
    );
}

#[test]
fn eval_rejects_malformed_args() {
    let ParseError::BadArg { cmd, detail } = parse("eval x").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "eval");
    assert!(detail.contains("action index 'x'"), "detail: {detail}");
    let ParseError::BadArg { cmd, detail } = parse("eval 0 x").unwrap_err() else {
        panic!("expected a BadArg error");
    };
    assert_eq!(cmd, "eval");
    assert!(detail.contains("sims 'x'"), "detail: {detail}");
}

// --- head-token dispatch -------------------------------------------------

#[test]
fn head_verb_is_case_insensitive_and_aliased() {
    assert_eq!(parse("END").unwrap(), Command::Action(Action::EndTurn));
    assert_eq!(parse("E").unwrap(), Command::Action(Action::EndTurn));
    assert_eq!(parse("Hand").unwrap(), Command::Hand);
    assert_eq!(parse("STATE").unwrap(), Command::State);
    assert_eq!(parse("Actions").unwrap(), Command::Actions);
    assert_eq!(parse("HELP").unwrap(), Command::Help);
    assert_eq!(parse("?").unwrap(), Command::Help);
    assert_eq!(parse("Quit").unwrap(), Command::Quit);
    assert_eq!(parse("EXIT").unwrap(), Command::Quit);
    assert_eq!(
        parse("PLAY 1 2").unwrap(),
        Command::Action(Action::PlayCard {
            hand_index: 1,
            target: Some(2),
        })
    );
}

#[test]
fn unknown_verb_reports_the_lowercased_head() {
    assert_eq!(
        parse("FROBNICATE now").unwrap_err(),
        ParseError::UnknownCommand("frobnicate".to_string())
    );
}

#[test]
fn comments_and_blanks() {
    assert_eq!(parse("#").unwrap(), Command::Comment(String::new()));
    assert_eq!(
        parse("   #   spaced   ").unwrap(),
        Command::Comment("spaced".to_string())
    );
    // `#` only introduces a comment at the start of the (trimmed) line.
    assert!(parse("play # 0").is_err());
    assert_eq!(parse("\t\n ").unwrap(), Command::Blank);
}

// --- properties ----------------------------------------------------------

/// Arbitrary input must never panic, and the parse result must be a pure
/// function of the input. The two structural guarantees stated in the
/// module doc — blank-iff-empty and `#`-prefix-iff-comment — hold for
/// every input, not just the well-formed ones.
#[hegel::test(test_cases = 400)]
fn parse_never_panics_on_arbitrary_input(tc: TestCase) {
    let line: String = tc.draw(gs::text().max_size(32));
    let first = parse(&line);
    let second = parse(&line);
    assert_eq!(first, second, "parse is not a pure function of its input");

    let trimmed = line.trim();
    if trimmed.is_empty() {
        assert_eq!(first, Ok(Command::Blank));
    } else if let Some(rest) = trimmed.strip_prefix('#') {
        assert_eq!(first, Ok(Command::Comment(rest.trim().to_string())));
    } else {
        // Any other input either resolves to a command or reports a
        // structured error; it must never resolve to Blank/Comment.
        assert!(!matches!(first, Ok(Command::Blank | Command::Comment(_))));
    }
}

/// Tokens are whitespace-separated (`split_whitespace`), so the amount
/// and flavor of inter-token whitespace — and any leading/trailing
/// padding — must not change the parse.
#[hegel::test(test_cases = 300)]
fn whitespace_between_tokens_is_insignificant(tc: TestCase) {
    let line: String = tc.draw(gs::sampled_from(VALID_LINES)).to_string();
    let pad = |tc: &TestCase| -> String {
        let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(4));
        let ws = tc.draw(gs::sampled_from(&[" ", "\t", " \t", "\u{c}"]));
        ws.repeat(n as usize)
    };
    let mut spaced = pad(&tc);
    for (i, tok) in line.split_whitespace().enumerate() {
        if i > 0 {
            spaced.push_str(&pad(&tc));
        }
        spaced.push_str(tok);
    }
    spaced.push_str(&pad(&tc));

    assert_eq!(
        parse(&spaced),
        parse(&line),
        "re-spaced '{spaced:?}' parsed differently from '{line}'"
    );
}

/// Numeric arguments round-trip through the grammar for the whole `u8`
/// domain: formatting a command and re-parsing it yields the same
/// indices, with no clamping or off-by-one at the 0/255 ends.
#[hegel::test(test_cases = 300)]
fn numeric_args_round_trip(tc: TestCase) {
    let i = tc.draw(gs::integers::<u32>().min_value(0).max_value(255)) as u8;
    let t = tc.draw(gs::integers::<u32>().min_value(0).max_value(255)) as u8;
    let sims = tc.draw(gs::integers::<u32>().min_value(1).max_value(u32::MAX));

    assert_eq!(
        parse(&format!("play {i} {t}")).unwrap(),
        Command::Action(Action::PlayCard {
            hand_index: i,
            target: Some(t),
        })
    );
    assert_eq!(
        parse(&format!("play {i}")).unwrap(),
        Command::Action(Action::PlayCard {
            hand_index: i,
            target: None,
        })
    );
    assert_eq!(
        parse(&format!("potion {i} {t}")).unwrap(),
        Command::Action(Action::DrinkPotion {
            potion_slot: i,
            target: Some(t),
        })
    );
    assert_eq!(parse(&format!("info {i}")).unwrap(), Command::Info(i));
    assert_eq!(parse(&format!("{i}")).unwrap(), Command::ActionIndex(i));
    assert_eq!(
        parse(&format!("eval {i} {sims}")).unwrap(),
        Command::Eval {
            action_idx: i,
            sims,
        }
    );
    assert_eq!(
        parse(&format!("hint {sims} --fair={sims}")).unwrap(),
        Command::Hint {
            sims: Some(sims),
            fair_per_action: Some(Some(sims)),
            fair_all: false,
        }
    );
}

/// `assert hp <op> <n>` round-trips the operator and an arbitrary i32
/// bound, and the parsed operator agrees with the reference comparison.
#[hegel::test(test_cases = 300)]
fn assert_hp_round_trips_and_agrees_with_check(tc: TestCase) {
    let spellings = ["<", "<=", "==", "=", ">=", ">", "!="];
    let spelling = tc.draw(gs::sampled_from(&spellings));
    let rhs = tc.draw(gs::integers::<i32>());
    let hp = tc.draw(gs::integers::<i32>());

    let cmd = parse(&format!("assert hp {spelling} {rhs}")).unwrap();
    let Command::Assert(Assertion::PlayerHp(op, got_rhs)) = cmd else {
        panic!("expected an hp assertion, got {cmd:?}");
    };
    assert_eq!(got_rhs, rhs);
    let expected = match spelling {
        "<" => hp < rhs,
        "<=" => hp <= rhs,
        "==" | "=" => hp == rhs,
        ">=" => hp >= rhs,
        ">" => hp > rhs,
        _ => hp != rhs,
    };
    assert_eq!(op.check(&hp, &rhs), expected, "{hp} {spelling} {rhs}");
}
