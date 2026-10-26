//! `sts2-cli` argument parsing — subcommand-shaped surface.
//!
//! The binary takes one subcommand:
//!
//! * `combat` — drive a single combat (REPL or random agent). Carries
//!   every flag that used to live on the bare `Cli` struct verbatim.
//! * `list <kind>` — inspect static content (encounters, cards, relics,
//!   potions, monsters). Filters STS1-name Phase-4 scaffolding stubs by
//!   default; `--all` opts back into them.

pub mod combat;
pub mod fuzz;
pub mod list;
pub mod seedfind;

use clap::{Parser, Subcommand};

pub use combat::CombatArgs;
pub use fuzz::FuzzArgs;
pub use list::ListArgs;
pub use seedfind::SeedfindArgs;

#[derive(Debug, Parser)]
#[command(
    name = "sts2-cli",
    version,
    about = "Interactive REPL + scripted-replay driver for sts2-core."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Drive a single combat (REPL or random agent).
    Combat(CombatArgs),
    /// Inspect static content surfaces (encounters, cards, relics,
    /// potions, monsters).
    List(ListArgs),
    /// Seeded full-run crash/livelock sweep (the AutoSlay-style fuzzer).
    Fuzz(FuzzArgs),
    /// Search seeds for a specific run-content combination (GH #297).
    Seedfind(SeedfindArgs),
}
