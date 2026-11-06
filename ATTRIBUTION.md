# Attribution

The Rust simulator is derived from **Thomas Friedel's [spire-mentor](https://github.com/tfriedel/spire-mentor)**, `sts2-rs/`. Its Cargo metadata also retains the historical repository URL `https://github.com/tfriedel/sts2-rs`. The source checkout underlying this snapshot identifies upstream commit `e7a923e90dc731e83683791b6c4d4a4059656149`; it contains subsequent local modifications and is not claimed to be an unmodified upstream commit.

Original engine copyright: **2025–2026 Thomas Friedel**. The full MIT notice is retained in `native/LICENSE`, with the original upstream notices in `attribution/`. The engine, card/enemy/relic implementations, wire reconstruction, original MCTS, and belief-draw machinery are upstream work.

OstyAI-specific work includes the compact numeric observation path, Rust worker/feedback/replay queues, GPU compositional encoder and transformer, neural Gumbel search integration, training and evaluation pipeline, and the documented simulator repairs and persistent search changes. An original source hash manifest and upstream-difference inventory distinguish the derived snapshot from upstream.

The neural search is inspired by Gumbel AlphaZero and AlphaZero policy/value training. It is a custom Rust implementation, not the `mctx` or `mctx-az` library. Prioritized replay uses proportional priorities with importance correction, adapted to fixed combat-pool quotas.

Slay the Spire 2 and its game content belong to **Mega Crit**. No affiliation or endorsement is implied. No game assemblies, art, audio, or decompiled C# source are included. The simulator's original notice excludes verbatim decompiled game quotations from its MIT grant.

The code depends on separately licensed open-source packages, including Rust crates listed in `native/Cargo.lock`, PyTorch, Triton and NumPy. Their copyright and license terms remain with their respective owners.

The separate [`ppo/`](ppo/README.md) full-run trainer is an independent implementation. Its run-continuation curriculum adapts the [published spire-mentor PPO plan in PR #45](https://github.com/tfriedel/spire-mentor/pull/45); it includes neither that project's unpublished trainer nor expert warm-start policy. [AlphaDefect's published model description](https://github.com/hmty-CHEN/alphadefect-evaluation-runs/blob/main/docs/model.md) informed the public/private critic and full-run/curriculum design, and [Ataraxos](https://github.com/AtaraxosAI/stratego) inspired advantage filtering. Their authors are credited for those ideas; the implementation and current recipe differ. The PPO Rust overlay uses the inherited simulator under its existing license and notices.
