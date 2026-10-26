# sts2-cli scripted-replay fixtures

Each `<name>.txt` is fed through the `sts2-cli` binary's stdin and its
stdout (whitespace-normalized) is diffed against the matching
`<name>.golden`. Set `BLESS_GOLDEN=1` to overwrite the golden in place.

| Fixture | Regeneration command |
|---------|----------------------|
| `basic_strike_defend` | `BLESS_GOLDEN=1 cargo test -p sts2-cli --test scripted_replay human_basic_strike_defend_seed0` |
| `random_seed42` | `BLESS_GOLDEN=1 cargo test -p sts2-cli --test scripted_replay random_seed42_30steps_nibbit` |
| `nibbit_first_turn` | `BLESS_GOLDEN=1 cargo test -p sts2-cli --test scripted_replay human_nibbit_first_turn_bare_int_and_card_alias` |

Re-bless any single fixture above; `cargo test -p sts2-cli --test
scripted_replay` (no env var) is the read-only verification.

## Long-trajectory determinism digest

`random_seed0_steps1000.sha256` pins the SHA-256 of the JSONL produced by a
1000-step autoplay on seed 0. The test lives in `tests/determinism.rs`.

Regenerate:

```bash
cargo build -p sts2-cli --release
./target/release/sts2-cli combat --agent random --seed 0 --steps 1000 \
    --jsonl /tmp/seed0.jsonl
sha256sum /tmp/seed0.jsonl | awk '{print $1}' \
    > crates/sts2-cli/tests/fixtures/random_seed0_steps1000.sha256
```

Re-generation is required whenever the JSONL schema, starter-deck content,
encounter default, or RandomAgent's selection logic intentionally changes.
