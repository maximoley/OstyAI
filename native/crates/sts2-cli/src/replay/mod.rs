//! HTML fight-replay tooling. See `crates/sts2-cli/src/bin/
//! fight_replay.rs` for the user-facing CLI.

pub mod assets;
pub mod audit;
pub mod motif;
pub mod record;
pub mod render;

pub use audit::{audit_fight, format_summary, AuditSummary, RedFlag, RedFlagKind};
pub use record::{record_from_fixture, FightRecord, RecorderConfig};
pub use render::{render_fight_html, render_index_html};
