//! `sts2-cli combat` argument surface.
//!
//! Every flag here is verbatim the set that used to live on the bare
//! `Cli` struct before the A2 subcommand restructure. Wave B/C/D beads
//! will append further flags to this struct as the flexible-combat
//! exploration surface grows.

use clap::{Args, ValueEnum};

use crate::sim::PlayerClass;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum AgentKind {
    Human,
    Random,
    /// Headless MCTS auto-player. Uses `--mcts-hint-default` /
    /// `--mcts-rollout-depth` / `--mcts-c-puct` /
    /// `--mcts-cheat-draw-order` to drive every decision via MCTS.
    /// Prints one `RESULT …` line to stdout when combat ends so
    /// outer harnesses (e.g. the `mcts-sweep` subcommand) can parse
    /// outcome + final HP without scraping the banner.
    Mcts,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliPlayerClass {
    Ironclad,
    Silent,
}

impl From<CliPlayerClass> for PlayerClass {
    fn from(c: CliPlayerClass) -> Self {
        match c {
            CliPlayerClass::Ironclad => Self::Ironclad,
            CliPlayerClass::Silent => Self::Silent,
        }
    }
}

#[derive(Debug, Args)]
pub struct CombatArgs {
    #[arg(long, default_value_t = 0)]
    pub seed: u64,

    /// Encounter alias (see `sts2-cli list encounters` for the full
    /// table). Case- and separator-insensitive. Bead `sts2-rs-2o0k`
    /// (B2) replaced the previous 6-variant `CliEncounter` enum with a
    /// free-form alias string so multi-monster groups
    /// (`two-sentries`, `three-louses`, `slimes-2x-acid`) become
    /// addressable.
    #[arg(long, default_value = "nibbit")]
    pub encounter: String,

    #[arg(long, value_enum, default_value_t = CliPlayerClass::Ironclad)]
    pub player: CliPlayerClass,

    #[arg(long)]
    pub hp: Option<u32>,

    /// Override the player's `max_hp` independently from `--hp`. When
    /// unset, defaults to `--hp` if provided, else the class default
    /// (Ironclad 80 / Silent 70). Plumbed for `sts2log` fixture
    /// loaders that need to start a wounded player at the run's true
    /// `current_hp / max_hp` ratio (bead `sts2-rs-f3g0`).
    #[arg(long)]
    pub max_hp: Option<u32>,

    #[arg(long, value_enum, default_value_t = AgentKind::Human)]
    pub agent: AgentKind,

    #[arg(long, default_value_t = 0)]
    pub steps: u32,

    #[arg(long)]
    pub jsonl: Option<std::path::PathBuf>,

    /// Echo each input line to stdout before processing (useful for
    /// scripted-replay golden tests where stdin and stdout interleave).
    #[arg(long)]
    pub echo_input: bool,

    /// Suppress the `> ` prompt before each input read.
    #[arg(long)]
    pub no_prompt: bool,

    // ── Relic input (mutually exclusive — bead sts2-rs-q3tv / C3) ──
    /// Inline comma-separated relic list, e.g. `--relic BurningBlood,Anchor`.
    /// Accepts both `PascalCase` and human display form ("Burning Blood").
    #[arg(long, value_delimiter = ',', group = "relic_input")]
    pub relic: Vec<String>,

    /// Symbolic relic preset: `none` / `ironclad-starter` /
    /// `silent-starter` / `kitchen-sink`.
    #[arg(long, group = "relic_input")]
    pub relic_preset: Option<String>,

    /// Path to a newline-delimited relic list (one `RelicId` per line;
    /// `#` comments and blank lines are skipped).
    #[arg(long, group = "relic_input")]
    pub relic_file: Option<std::path::PathBuf>,

    // ── Deck input (mutually exclusive — bead sts2-rs-63bi / C2) ──
    /// Inline deck: comma-separated card names. Append `+` for upgraded
    /// (e.g. `Strike,Strike,Defend,Bash+`).
    #[arg(long, group = "deck_input")]
    pub deck: Option<String>,

    /// Path to a deck file. Either newline-delimited (one card per
    /// line, `#` comments) or TOML (`cards = ["Strike", "Bash+"]`).
    #[arg(long, group = "deck_input")]
    pub deck_file: Option<std::path::PathBuf>,

    /// Named preset (`ironclad-basic`, `silent-basic`,
    /// `ironclad-attack-demo`, `silent-stealth-demo`).
    #[arg(long, group = "deck_input")]
    pub deck_preset: Option<String>,

    // ── Potion input (mutually exclusive — bead sts2-rs-bzgs / C4) ──
    /// Inline comma-separated potion list, e.g. `--potion FirePotion,BloodPotion`.
    /// Accepts `PascalCase`, `"Title Case"`, and `UPPER_SNAKE_CASE`.
    #[arg(long, value_delimiter = ',', group = "potion_input")]
    pub potion: Vec<String>,

    /// Symbolic potion preset: `none` (default) or `random-3`
    /// (3 deterministic non-event-only draws keyed off `--seed`).
    #[arg(long = "potion-preset", group = "potion_input")]
    pub potion_preset: Option<String>,

    /// Path to a newline-delimited potion list (one `PotionKind` per
    /// line; `#` comments tolerated).
    #[arg(long = "potion-file", group = "potion_input")]
    pub potion_file: Option<std::path::PathBuf>,

    /// Path to a TOML scenario file (bead `sts2-rs-g2ex` / cliext D1)
    /// bundling player class + deck + relics + potions + monsters in
    /// one declarative file. Precedence: CLI flags > scenario file >
    /// defaults. File must declare `schema = 1`.
    #[arg(long)]
    pub scenario: Option<std::path::PathBuf>,

    /// Dump the resolved scenario (monsters + deck + relics + potions
    /// + player class/hp) as a `schema = 1` TOML block to stdout and
    /// exit 0 WITHOUT running combat. The dumped block is a strict
    /// subset of the loader format so the output round-trips through
    /// `--scenario <file>`. Bead `sts2-rs-92sq` (cliext E1).
    #[arg(long)]
    pub print_scenario: bool,

    /// Shorthand: randomize deck / relics / potions / encounter
    /// using `--seed`. Per-input explicit flags WIN over `--random-all`
    /// (precedence rule, bead `sts2-rs-c9s5` / cliext F3). Each random
    /// draw consumes an independent fork of the scenario RNG so the
    /// streams stay decorrelated and `--seed` reproduces the same
    /// scenario across runs.
    #[arg(long)]
    pub random_all: bool,

    /// Lock `--deck-preset random` (and `--random-all`'s random-deck
    /// draw) to a single character pool. Supported tags: `ironclad`,
    /// `silent`. When omitted, the class is drawn from `--seed` via
    /// `random_class()` (bead `sts2-rs-9yoh` / cliext F4) so the
    /// composite scenario stays single-seed reproducible.
    #[arg(long, value_name = "CHARACTER")]
    pub random_deck_character: Option<String>,

    /// Number of cards drawn by `--deck-preset random` (and
    /// `--random-all`'s deck draw). Default 10; bounded 1..=200 by
    /// the resolver. Bead `sts2-rs-9yoh` / cliext F4.
    #[arg(long, value_name = "N")]
    pub random_deck_size: Option<usize>,

    /// Ad-hoc encounter composition: comma-separated monster tags
    /// (e.g. `--monsters cultist,sentry,green-louse`). Lets you build
    /// any combination of the 6 implemented monster kinds without
    /// writing a scenario file or going through the alias table.
    /// Mutually exclusive with `--encounter` when both are explicit.
    /// Bead `sts2-rs-45xt` / cliext H2.
    #[arg(long, value_name = "TAGS")]
    pub monsters: Option<String>,

    /// Auto-run an MCTS hint at the top of every interactive player
    /// decision, using `<SIMS>` simulations. When omitted, the `hint
    /// [SIMS]` REPL verb still works on demand. Bead `sts2-rs-s3tb`
    /// / cliext G1.
    #[arg(long, value_name = "SIMS")]
    pub mcts_hint_default: Option<u32>,

    /// UCT exploration constant for `--mcts-hint-default` and the
    /// `hint` REPL verb. Defaults to `MctsConfig`'s `c_puct` (sqrt(2)).
    #[arg(long, value_name = "F")]
    pub mcts_c_puct: Option<f32>,

    /// Hard rollout-depth cap for `--mcts-hint-default` and the `hint`
    /// REPL verb. Defaults to `MctsConfig`'s `max_rollout_depth` (10).
    #[arg(long, value_name = "N")]
    pub mcts_rollout_depth: Option<u32>,

    /// Number of rayon workers for root-parallel MCTS. `0` means "use
    /// all available cores" (resolved via `rayon::current_num_threads()`);
    /// `1` is the single-threaded path. The total simulation budget is
    /// split evenly across workers, so wall time scales roughly with
    /// `1 / threads` until rollout-side memory bandwidth saturates.
    /// Note: changing this value also changes the per-worker seed split,
    /// so output differs across thread counts even at the same sim
    /// budget.
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub mcts_threads: u32,

    /// (For `--agent mcts`) Preserve the search tree across decisions
    /// by re-rooting at the chosen action's subtree instead of
    /// rebuilding from scratch. Accumulates visit / value stats across
    /// the combat — net effect is more total search per decision after
    /// the first. PIMC caveat: surviving subtree statistics are
    /// approximately-but-not-exactly conditioned on the new root state
    /// (see `sts2_mcts::Mcts::reroot` doc-comment). Default off.
    #[arg(long, default_value_t = false)]
    pub mcts_tree_reuse: bool,

    /// Force the pre-`dfv1` uniform-random rollout policy instead of the
    /// default expert-heuristic softmax. Used for A/B comparisons against
    /// the heuristic rollout (bead `sts2-rs-dfv1`).
    #[arg(long, default_value_t = false)]
    pub mcts_uniform_rollout: bool,
}
