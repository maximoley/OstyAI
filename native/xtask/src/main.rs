//! Repo dev tooling. Not shipped. Run: `cargo run -p xtask -- <cmd> [args]`.
use xtask_lib::{split_cards, split_potions, split_relics};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("split-cards") => {
            let character = args.get(1).expect("usage: split-cards <character>");
            split_cards::run(character);
        }
        Some("split-relics") => {
            split_relics::run();
        }
        Some("split-potions") => {
            split_potions::run();
        }
        Some("colocate-card-data") => {
            let character = args.get(1).expect("usage: colocate-card-data <character>");
            xtask_lib::colocate::run(character);
        }
        other => {
            eprintln!(
                "unknown command: {other:?}; expected `split-cards <character>` | `split-relics` | `split-potions` | `colocate-card-data <character>`"
            );
            std::process::exit(2);
        }
    }
}
