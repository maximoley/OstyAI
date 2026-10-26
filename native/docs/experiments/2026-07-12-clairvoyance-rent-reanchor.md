# Clairvoyance-removal campaign: G4 re-anchor (2026-07-12)

Bead: `sts2-rs-v0acz`. Plan: `docs/plans/2026-07-12-clairvoyance-removal-plan.md`.

## Summary

Combat MCTS was clairvoyant (draw-pile order materialized in root state;
draws never became chance nodes). The campaign measured the rent, built
three honest modes behind `HiddenInfoMode` (F0 RootScramble — root
blindfold, F1 DrawChance — draws-as-chance, F2 BeliefDraw — lightspeed-style
belief pile), blessed F2 as the honest champion, and flipped the deployed
run-AI default to honest (PR #246). Rollback: `STS2_RUNAI_HONEST=0`.

## Numbers

Setup, unless a row states otherwise: **full-run win rates** (Neow → act-3
boss), **Ironclad, Ascension 0**, paired release-2000 seed panel,
`ranker_v107` run-policy (no value net), eval-cheap combat budget
(1000 sims non-boss / 4000 boss), net-margin escalation ON. The
"combat-level probe" row is a **single-combat** win rate (act-1 encounter
corpus), not a run rate — the two levels are not comparable.

| measurement | binary | result |
|---|---|---|
| Combat-level probe (act-1 random decks, 1000 sims) | probe commit | rent −4.2pp overall, −9pp boss (p=5e-5) |
| Run-level rent, clairvoyant vs honest-F0 (Task 2) | 250655dcf | 807/2000=40.35% vs 273/2000=13.65%, −26.70pp, p=4.0e-12 |
| F1 DrawChance G2 recovery (probe) | Task-4 tip | 5.6% stock → 27.8% retuned — FAIL (<50%); H1 outcome starvation confirmed |
| F2 BeliefDraw G2 recovery (probe, retuned c_puct=2.5, draw_widening_c=2.0) | 7b tip | 44.4%@500 / 41.5%@1000 sims (p=0.016) — still formally FAIL (<50%), best arm |
| Run-level F2-retuned vs F0 (champion selection) | bb07adda9 vs 250655dcf | 348/2000=17.40% vs 273/2000=13.65%, +3.75pp, p=4.7e-5 (~14% run-level recovery) |
| PPO clairvoyant vs honest-F0 | 250655dcf | 1641/2000=82.05% vs 983/2000=49.15%, −32.9pp, flips 718/60, p=3.5e-12 |
| G3 perf (searches/sec vs clairvoyant) | 7b tip | RootScramble ~0% (PASS); DrawChance −24.2%; BeliefDraw −22.1% (over 15% gate; accepted cost) |
| **G4 FINAL re-anchor, clairvoyant vs honest-default** | **968e9e49e (both arms)** | **767/2000=38.35% vs 311/2000=15.55%, −22.80pp, flips 528/72, McNemar p=1.1e-86** |
| **PPO champion (`main.iter_350`), clairvoyant vs honest-default** | **968e9e49e (both arms)** | **1625/2000=81.25% vs 1115/2000=55.75%, −25.50pp, flips 588/78, p=1.0e-97** |
| PPO honest: BeliefDraw vs F0 RootScramble | 968e9e49e vs 250655dcf (cross-binary) | 1115/2000=55.75% vs 983/2000=49.15%, +6.60pp, p=1.7e-7 — F2 helps PPO more than it helps the ranker (+3.75pp) |

G4 per-act loss decomposition (2000 seeds): clairvoyant {act1:23, act2:521,
act3:689}; honest {act1:106, act2:831, act3:752} — the rent grows with act,
matching the combat probe's act-1 near-parity (ratio 0.956).

## New official anchors (2026-07-12, tip engine)

All are **full-run Ironclad-A0 win rates** on the release-2000 panel,
eval-cheap combat:

- **Best honest agent: PPO champion `main.iter_350` under the honest
  BeliefDraw default = 55.75%** (1115/2000). Ranker is deprecated
  (2026-07-12 user decision) — this is the anchor that matters.
- Deployed run-AI (`ranker_v107`, honest BeliefDraw default): **15.55%**
  (311/2000).
- Clairvoyant ablation (`STS2_RUNAI_HONEST=0`): 38.35% (767/2000). Every
  pre-2026-07-12 win-rate anchor in docs/memory (40.35%, 42.0% #126, 84%
  PPO G8) was measured under clairvoyant combat and must not be compared
  against honest numbers.
- Clairvoyant drift 40.35→38.35 across binaries = engine movement (main
  merges #245/#247 + T7c pin fixes), re-confirming the re-anchor-after-
  engine-jump rule.

## Decision + forward path

- Champion = F2/BeliefDraw + baked retune, honest by default at the
  deployment layer (`resolve_hidden_info` in sts2-runai; engine
  `MctsConfig::default()` stays Clairvoyant for fixture stability).
- G2 formally failed (<50% recovery) for all search-side mitigations:
  search fixes alone don't pay back the rent. Next lever = honest-label
  regen + retrain (GH#249, bead `lnjx6`, deferred): lightspeed's honest
  retrain beat their cheat champion.

## Reproduce

The two G4 arms (paired seed panel, McNemar on per-seed `won`):

```
STS2_RUNAI_RANKER=$PWD/data/runai/nets/ranker_v107.json run_arena sweep \
  --panel data/seed-panels/release-2000.json --policy ranker \
  --combat eval-cheap --jobs 0 --no-wall-ms --out <arm>.jsonl
```

Run once with `STS2_RUNAI_HONEST=0` (clairvoyant ablation) and once with
`STS2_RUNAI_HONEST` unset (post-flip default = BeliefDraw honest). Analysis
= paired McNemar on per-seed `won` keyed by seed.
