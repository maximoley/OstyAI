//! Combat-screen command grammar.
//!
//! Tokens are whitespace-separated. Lines beginning with `#` are comments
//! (stripped before parsing); blank lines yield [`Command::Blank`].

use sts2_core::Action;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Blank,
    Comment(String),
    Action(Action),
    /// Bare-int dispatch — resolves against the most-recently-rendered
    /// action menu. The REPL renders an indexed action menu after every
    /// state change; `0`/`1`/`2`/… picks an entry from that menu without
    /// re-typing `play <i> <slot>`.
    ActionIndex(u8),
    Hand,
    State,
    Actions,
    Info(u8),
    Help,
    Quit,
    /// `assert <pred>` — supported predicates: `hp <op> <n>`,
    /// `phase <name>`, `monsters_alive <n>`. `<op>` ∈ `< <= == >= > !=`.
    Assert(Assertion),
    /// `hint [SIMS] [--fair[=N] | --fair-all[=N]]` — run sts2-mcts on
    /// the current state.
    ///
    /// - `sims` defaults to whatever `--mcts-hint-default` was passed
    ///   on the CLI (the same budget the auto-hint uses), or 1000 if
    ///   that flag was omitted. Pass an explicit value to override.
    /// - `--fair` (or `--fair=N`) additionally runs an independent
    ///   MCTS for the top-3 root alternatives by visits at N sims each
    ///   — see bead `sts2-rs-60y1`. When `=N` is omitted, the
    ///   per-action sim count inherits whatever the main search used,
    ///   so `hint 20000 --fair` runs both passes at 20000 sims. The
    ///   reported win% reflects each action's own search's root-Q,
    ///   free of the main-tree UCB selection bias.
    /// - `--fair-all` (or `--fair-all=N`) is the same as `--fair`
    ///   except it evaluates *every* legal action, not just the top-3.
    ///   Mutually exclusive with `--fair`.
    ///
    /// Does not advance the game. Bead `sts2-rs-s3tb` (cliext G1) +
    /// `sts2-rs-60y1` (fair-eval).
    Hint {
        /// `None` means "use the caller's default" — typically the
        /// `--mcts-hint-default` budget threaded through `HintConfig`,
        /// falling back to a hard-coded 1000.
        sims: Option<u32>,
        /// Outer `None` means `--fair`/`--fair-all` was not specified.
        /// Inner `None` means the flag was given without `=N` — the
        /// caller inherits the resolved main `sims` for the per-action
        /// budget. Inner `Some(n)` is an explicit `=N`.
        fair_per_action: Option<Option<u32>>,
        fair_all: bool,
    },
    /// `eval <action_idx> [sims]` — run an independent MCTS from the
    /// state that would result from playing the action at menu index
    /// `action_idx` (resolved against the most-recently-rendered action
    /// menu, same as bare-int dispatch). Reports win/tie/loss for that
    /// specific committed action — same idea as `hint --fair` but for
    /// one action you pick rather than top-3 by visits. Sims defaults
    /// to 3000.
    Eval {
        action_idx: u8,
        sims: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assertion {
    PlayerHp(CmpOp, i32),
    Phase(String),
    MonstersAlive(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
    Ne,
}

impl CmpOp {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "<" => Self::Lt,
            "<=" => Self::Le,
            "==" | "=" => Self::Eq,
            ">=" => Self::Ge,
            ">" => Self::Gt,
            "!=" => Self::Ne,
            _ => return None,
        })
    }

    #[must_use]
    pub fn check<T: PartialOrd>(self, lhs: &T, rhs: &T) -> bool {
        match self {
            Self::Lt => lhs < rhs,
            Self::Le => lhs <= rhs,
            Self::Eq => lhs == rhs,
            Self::Ge => lhs >= rhs,
            Self::Gt => lhs > rhs,
            Self::Ne => lhs != rhs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    UnknownCommand(String),
    BadArg { cmd: &'static str, detail: String },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownCommand(c) => write!(f, "unknown command: '{c}' (try `help`)"),
            Self::BadArg { cmd, detail } => write!(f, "bad arg for `{cmd}`: {detail}"),
        }
    }
}

pub fn parse(line: &str) -> Result<Command, ParseError> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(Command::Blank);
    }
    if let Some(rest) = trimmed.strip_prefix('#') {
        return Ok(Command::Comment(rest.trim().to_string()));
    }
    let mut toks = trimmed.split_whitespace();
    let head = toks
        .next()
        .expect("non-empty after trim")
        .to_ascii_lowercase();
    // Bare-int dispatch (lightspeed-parity): if the head token parses as
    // a u8 with no extra tokens, dispatch into the last-rendered action
    // menu. Stray tokens after the integer fall through to the
    // unknown-command error so a typo like `0 foo` is loud rather than
    // silently dropping `foo`.
    if let Ok(n) = head.parse::<u8>() {
        if toks.next().is_some() {
            return Err(ParseError::BadArg {
                cmd: "<action-index>",
                detail: format!("trailing tokens after action index '{head}'"),
            });
        }
        return Ok(Command::ActionIndex(n));
    }
    match head.as_str() {
        "play" => parse_play(&mut toks),
        "potion" => parse_potion(&mut toks),
        "end" | "e" => Ok(Command::Action(Action::EndTurn)),
        "hand" => Ok(Command::Hand),
        "state" => Ok(Command::State),
        "actions" => Ok(Command::Actions),
        // `card <i>` is an alias for `info <i>` (lightspeed-parity).
        "info" | "card" => parse_info(&mut toks),
        "help" | "?" => Ok(Command::Help),
        "quit" | "exit" => Ok(Command::Quit),
        "assert" => parse_assert(&mut toks),
        "hint" => parse_hint(&mut toks),
        "eval" => parse_eval_cmd(&mut toks),
        other => Err(ParseError::UnknownCommand(other.to_string())),
    }
}

/// Reject leftover tokens once a fixed-arity verb has consumed its
/// arguments — same rationale as the bare-int path above: a typo like
/// `play 0 1 2` must be loud rather than silently dropping the `2`.
fn reject_trailing<'a>(
    toks: &mut impl Iterator<Item = &'a str>,
    cmd: &'static str,
) -> Result<(), ParseError> {
    match toks.next() {
        Some(tok) => Err(ParseError::BadArg {
            cmd,
            detail: format!("trailing tokens starting at '{tok}'"),
        }),
        None => Ok(()),
    }
}

fn parse_play<'a>(toks: &mut impl Iterator<Item = &'a str>) -> Result<Command, ParseError> {
    let i_str = toks.next().ok_or_else(|| ParseError::BadArg {
        cmd: "play",
        detail: "missing hand index".into(),
    })?;
    let hand_index: u8 = i_str.parse().map_err(|_| ParseError::BadArg {
        cmd: "play",
        detail: format!("hand index '{i_str}' is not a non-negative integer"),
    })?;
    let target = match toks.next() {
        None => None,
        Some(t) => Some(t.parse::<u8>().map_err(|_| ParseError::BadArg {
            cmd: "play",
            detail: format!("target '{t}' is not a non-negative integer"),
        })?),
    };
    reject_trailing(toks, "play")?;
    Ok(Command::Action(Action::PlayCard { hand_index, target }))
}

fn parse_potion<'a>(toks: &mut impl Iterator<Item = &'a str>) -> Result<Command, ParseError> {
    // `potion <slot> [target]` — emits the engine's canonical
    // `Action::DrinkPotion` variant (bead `sts2-rs-13q` (P7-6)
    // renamed the legacy `UsePotion` action to `DrinkPotion` and
    // emits it from `legal_actions`). The bead spec listed
    // `UsePotion` for back-compat phrasing but `DrinkPotion` is the
    // canonical name today.
    let s_str = toks.next().ok_or_else(|| ParseError::BadArg {
        cmd: "potion",
        detail: "missing potion slot".into(),
    })?;
    let potion_slot: u8 = s_str.parse().map_err(|_| ParseError::BadArg {
        cmd: "potion",
        detail: format!("slot '{s_str}' is not a non-negative integer"),
    })?;
    let target = match toks.next() {
        None => None,
        Some(t) => Some(t.parse::<u8>().map_err(|_| ParseError::BadArg {
            cmd: "potion",
            detail: format!("target '{t}' is not a non-negative integer"),
        })?),
    };
    reject_trailing(toks, "potion")?;
    Ok(Command::Action(Action::DrinkPotion {
        potion_slot,
        target,
    }))
}

fn parse_eval_cmd<'a>(toks: &mut impl Iterator<Item = &'a str>) -> Result<Command, ParseError> {
    let idx_str = toks.next().ok_or(ParseError::BadArg {
        cmd: "eval",
        detail: "missing action index (use the menu-index shown by the auto-prompt)".into(),
    })?;
    let action_idx: u8 = idx_str.parse().map_err(|_| ParseError::BadArg {
        cmd: "eval",
        detail: format!("action index '{idx_str}' is not a non-negative integer"),
    })?;
    let sims = match toks.next() {
        None => 3000_u32,
        Some(s) => s.parse::<u32>().map_err(|_| ParseError::BadArg {
            cmd: "eval",
            detail: format!("sims '{s}' is not a positive integer"),
        })?,
    };
    if sims == 0 {
        return Err(ParseError::BadArg {
            cmd: "eval",
            detail: "sims must be > 0".into(),
        });
    }
    if toks.next().is_some() {
        return Err(ParseError::BadArg {
            cmd: "eval",
            detail: "trailing tokens after sims".into(),
        });
    }
    Ok(Command::Eval { action_idx, sims })
}

fn parse_hint<'a>(toks: &mut impl Iterator<Item = &'a str>) -> Result<Command, ParseError> {
    let mut sims: Option<u32> = None;
    let mut fair_per_action: Option<Option<u32>> = None;
    let mut fair_all = false;
    for tok in toks {
        // Match the longer prefix first so `--fair-all` isn't parsed as
        // `--fair` with a trailing `-all` body.
        let fair_match = if let Some(rest) = tok.strip_prefix("--fair-all") {
            Some(("--fair-all", rest, true))
        } else {
            tok.strip_prefix("--fair")
                .map(|rest| ("--fair", rest, false))
        };
        if let Some((flag, rest, is_all)) = fair_match {
            // `rest` is "" for bare `--fair`/`--fair-all` (inherit main
            // sims), "=N" for explicit, anything else is unknown.
            let per_action: Option<u32> = if rest.is_empty() {
                None
            } else if let Some(n_str) = rest.strip_prefix('=') {
                let n = n_str.parse::<u32>().map_err(|_| ParseError::BadArg {
                    cmd: "hint",
                    detail: format!("{flag} value '{n_str}' is not a positive integer"),
                })?;
                if n == 0 {
                    return Err(ParseError::BadArg {
                        cmd: "hint",
                        detail: format!("{flag} value must be > 0"),
                    });
                }
                Some(n)
            } else {
                return Err(ParseError::BadArg {
                    cmd: "hint",
                    detail: format!(
                        "unknown flag '{tok}' (did you mean --fair, --fair=N, --fair-all, \
                         or --fair-all=N?)"
                    ),
                });
            };
            if fair_per_action.is_some() {
                return Err(ParseError::BadArg {
                    cmd: "hint",
                    detail: "--fair / --fair-all specified more than once (mutually exclusive)"
                        .into(),
                });
            }
            fair_per_action = Some(per_action);
            fair_all = is_all;
        } else if tok.starts_with("--") {
            return Err(ParseError::BadArg {
                cmd: "hint",
                detail: format!("unknown flag '{tok}'"),
            });
        } else if sims.is_some() {
            return Err(ParseError::BadArg {
                cmd: "hint",
                detail: "trailing tokens after sims".into(),
            });
        } else {
            let n = tok.parse::<u32>().map_err(|_| ParseError::BadArg {
                cmd: "hint",
                detail: format!("sims '{tok}' is not a positive integer"),
            })?;
            if n == 0 {
                return Err(ParseError::BadArg {
                    cmd: "hint",
                    detail: "sims must be > 0".into(),
                });
            }
            sims = Some(n);
        }
    }
    Ok(Command::Hint {
        sims,
        fair_per_action,
        fair_all,
    })
}

fn parse_info<'a>(toks: &mut impl Iterator<Item = &'a str>) -> Result<Command, ParseError> {
    let i_str = toks.next().ok_or_else(|| ParseError::BadArg {
        cmd: "info",
        detail: "missing hand index".into(),
    })?;
    let i: u8 = i_str.parse().map_err(|_| ParseError::BadArg {
        cmd: "info",
        detail: format!("hand index '{i_str}' is not a non-negative integer"),
    })?;
    reject_trailing(toks, "info")?;
    Ok(Command::Info(i))
}

fn parse_assert<'a>(toks: &mut impl Iterator<Item = &'a str>) -> Result<Command, ParseError> {
    let kind = toks.next().ok_or_else(|| ParseError::BadArg {
        cmd: "assert",
        detail: "missing predicate kind (hp / phase / monsters_alive)".into(),
    })?;
    match kind {
        "hp" => {
            let op_str = toks.next().ok_or_else(|| ParseError::BadArg {
                cmd: "assert hp",
                detail: "missing comparison op".into(),
            })?;
            let op = CmpOp::parse(op_str).ok_or_else(|| ParseError::BadArg {
                cmd: "assert hp",
                detail: format!("unknown op '{op_str}'"),
            })?;
            let n_str = toks.next().ok_or_else(|| ParseError::BadArg {
                cmd: "assert hp",
                detail: "missing rhs".into(),
            })?;
            let n: i32 = n_str.parse().map_err(|_| ParseError::BadArg {
                cmd: "assert hp",
                detail: format!("rhs '{n_str}' is not an integer"),
            })?;
            reject_trailing(toks, "assert hp")?;
            Ok(Command::Assert(Assertion::PlayerHp(op, n)))
        }
        "phase" => {
            let name = toks.next().ok_or_else(|| ParseError::BadArg {
                cmd: "assert phase",
                detail: "missing phase name".into(),
            })?;
            reject_trailing(toks, "assert phase")?;
            Ok(Command::Assert(Assertion::Phase(name.to_string())))
        }
        "monsters_alive" => {
            let n_str = toks.next().ok_or_else(|| ParseError::BadArg {
                cmd: "assert monsters_alive",
                detail: "missing count".into(),
            })?;
            let n: usize = n_str.parse().map_err(|_| ParseError::BadArg {
                cmd: "assert monsters_alive",
                detail: format!("count '{n_str}' is not a non-negative integer"),
            })?;
            reject_trailing(toks, "assert monsters_alive")?;
            Ok(Command::Assert(Assertion::MonstersAlive(n)))
        }
        other => Err(ParseError::BadArg {
            cmd: "assert",
            detail: format!("unknown predicate '{other}'"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_play_with_target() {
        assert_eq!(
            parse("play 0 1").unwrap(),
            Command::Action(Action::PlayCard {
                hand_index: 0,
                target: Some(1),
            })
        );
    }

    #[test]
    fn parses_play_without_target() {
        assert_eq!(
            parse("play 2").unwrap(),
            Command::Action(Action::PlayCard {
                hand_index: 2,
                target: None,
            })
        );
    }

    #[test]
    fn parses_end_and_quit() {
        assert_eq!(parse("end").unwrap(), Command::Action(Action::EndTurn));
        assert_eq!(parse("quit").unwrap(), Command::Quit);
        assert_eq!(parse("exit").unwrap(), Command::Quit);
    }

    #[test]
    fn parses_assert_hp() {
        assert_eq!(
            parse("assert hp <= 40").unwrap(),
            Command::Assert(Assertion::PlayerHp(CmpOp::Le, 40))
        );
    }

    #[test]
    fn parses_hint_default_sims() {
        // Parser leaves `sims = None` so the caller (HumanAgent) can
        // resolve the default against `--mcts-hint-default`.
        assert_eq!(
            parse("hint").unwrap(),
            Command::Hint {
                sims: None,
                fair_per_action: None,
                fair_all: false,
            }
        );
    }

    #[test]
    fn parses_hint_with_sims() {
        assert_eq!(
            parse("hint 250").unwrap(),
            Command::Hint {
                sims: Some(250),
                fair_per_action: None,
                fair_all: false,
            }
        );
    }

    #[test]
    fn rejects_hint_zero_and_trailing() {
        assert!(matches!(
            parse("hint 0"),
            Err(ParseError::BadArg { cmd: "hint", .. })
        ));
        assert!(matches!(
            parse("hint 100 extra"),
            Err(ParseError::BadArg { cmd: "hint", .. })
        ));
    }

    #[test]
    fn parses_hint_fair_flag() {
        assert_eq!(
            parse("hint --fair").unwrap(),
            Command::Hint {
                sims: None,
                // Bare `--fair` defers the per-action sim count to the
                // caller (inherits main `sims`).
                fair_per_action: Some(None),
                fair_all: false,
            }
        );
        assert_eq!(
            parse("hint 5000 --fair").unwrap(),
            Command::Hint {
                sims: Some(5000),
                fair_per_action: Some(None),
                fair_all: false,
            }
        );
        assert_eq!(
            parse("hint 5000 --fair=500").unwrap(),
            Command::Hint {
                sims: Some(5000),
                fair_per_action: Some(Some(500)),
                fair_all: false,
            }
        );
        // Flag order shouldn't matter.
        assert_eq!(
            parse("hint --fair=2000 8000").unwrap(),
            Command::Hint {
                sims: Some(8000),
                fair_per_action: Some(Some(2000)),
                fair_all: false,
            }
        );
    }

    #[test]
    fn parses_hint_fair_all_flag() {
        assert_eq!(
            parse("hint --fair-all").unwrap(),
            Command::Hint {
                sims: None,
                fair_per_action: Some(None),
                fair_all: true,
            }
        );
        assert_eq!(
            parse("hint 5000 --fair-all=500").unwrap(),
            Command::Hint {
                sims: Some(5000),
                fair_per_action: Some(Some(500)),
                fair_all: true,
            }
        );
        // Mutually exclusive with --fair.
        assert!(matches!(
            parse("hint --fair --fair-all"),
            Err(ParseError::BadArg { cmd: "hint", .. })
        ));
    }

    #[test]
    fn parses_eval_verb() {
        assert_eq!(
            parse("eval 2").unwrap(),
            Command::Eval {
                action_idx: 2,
                sims: 3000,
            }
        );
        assert_eq!(
            parse("eval 0 5000").unwrap(),
            Command::Eval {
                action_idx: 0,
                sims: 5000,
            }
        );
        assert!(matches!(
            parse("eval"),
            Err(ParseError::BadArg { cmd: "eval", .. })
        ));
        assert!(matches!(
            parse("eval 0 0"),
            Err(ParseError::BadArg { cmd: "eval", .. })
        ));
        assert!(matches!(
            parse("eval 0 100 extra"),
            Err(ParseError::BadArg { cmd: "eval", .. })
        ));
    }

    #[test]
    fn rejects_unknown_hint_flag() {
        assert!(matches!(
            parse("hint --bogus"),
            Err(ParseError::BadArg { cmd: "hint", .. })
        ));
        assert!(matches!(
            parse("hint --fair=0"),
            Err(ParseError::BadArg { cmd: "hint", .. })
        ));
    }

    #[test]
    fn comment_lines_round_trip() {
        match parse("# beat the cultist").unwrap() {
            Command::Comment(c) => assert_eq!(c, "beat the cultist"),
            other => panic!("expected Comment, got {other:?}"),
        }
    }

    #[test]
    fn blank_lines_blank() {
        assert_eq!(parse("").unwrap(), Command::Blank);
        assert_eq!(parse("   ").unwrap(), Command::Blank);
    }

    #[test]
    fn parses_potion_with_target() {
        assert_eq!(
            parse("potion 0 1").unwrap(),
            Command::Action(Action::DrinkPotion {
                potion_slot: 0,
                target: Some(1),
            })
        );
    }

    #[test]
    fn parses_potion_without_target() {
        assert_eq!(
            parse("potion 2").unwrap(),
            Command::Action(Action::DrinkPotion {
                potion_slot: 2,
                target: None,
            })
        );
    }

    #[test]
    fn unknown_command_errors() {
        assert!(matches!(
            parse("frobnicate 1"),
            Err(ParseError::UnknownCommand(_))
        ));
    }
}
