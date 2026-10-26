# Gumbel search and belief draws

[Documentation guide](README.md) · [Project overview](../README.md)

The policy scores legal actions and the value network estimates shaped terminal utility. Search uses those predictions to compare actions before playing one in the real combat. Gumbel sequential halving considers up to eight root actions, clipped by the legal-action count and simulation budget. A simulation budget counts search iterations, not a fixed number of moves into the future.

## Chance outcomes compete with search depth

At a stochastic action, such as playing a draw card, search may sample a new belief outcome or revisit an already sampled outcome. A new draw outcome creates a leaf for neural evaluation. Revisiting an existing outcome preserves its sampled hand and permits further action search. It does not redraw that cached hand on every visit. Forced-action chains are collapsed into their parent edge while retaining their chance effects.

With a small budget, sampling many hands can leave few simulations to explore how to use each hand. Too few sampled hands can instead overfit a lucky draw. An action can therefore remain strongly dependent on the value network's estimate even after search.

## Recorded Soul example

| Property | Recorded value |
|---|---|
| Checkpoint | 5460, frozen for this diagnostic |
| Encounter | `SHRINKER_BEETLE_WEAK`, easy pool |
| Initial hand | Defend, Defend, Soul, Soul, Strike |
| Player / enemy HP | 52/66 / 42 |
| Search budget | 32 simulations |
| Chosen action | Soul, hand slot 2 / action index 3, zero-based |
| Simulations allocated to chosen Soul | 13 |
| New sampled outcomes for that Soul | 8 |
| Cached-outcome revisits for that Soul | 5 |
| Maximum explored depth | 2 actions |
| Final chosen-action Q | 1.475011230, shaped return rather than win probability |

Eight visits sampled new draw outcomes and evaluated their values immediately. Five reused an existing outcome and explored one more action. This illustrates why shallow search may not uncover a delayed combo when its value estimate is poor; it does not establish the typical depth of all easy-pool decisions.

## Inspect every search step

- [Interactive walkthrough](search-trace/soul-search-walkthrough.html): download and open locally; GitHub displays HTML source rather than running the viewer.
- [Full search tree SVG](search-trace/full-search-tree.svg): expanded nodes and sampled outcome edges.
- [Per-simulation CSV](search-trace/simulations.csv): root actions, leaf values, backed-up Q values and visits.
- [Complete trace JSON](search-trace/soul-search-full.json): all 160 events and 33 full node states.
- [Verification](search-trace/verification.json): reconstructed backup sums, edge visits, parent structure and viewer checks.

The trace used one state per inference batch and is not a replay of a historical production batch. Its original diagnostic verified full-combat parity with the uninstrumented native library on 24 seeded easy cases. The packaged step-5500 actor is a different checkpoint. Source paths in the trace's provenance refer to the original diagnostic environment.
