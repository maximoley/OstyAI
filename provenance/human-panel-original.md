# Human winning-run benchmark

Runs every 500 optimizer updates, alongside every fifth original validation. First scheduled checkpoint: 4000. The original 8192-combat validation remains every 100 updates.

Select 512 solo standard A10 Necrobinder human winning runs in fixed SHA256 rank order (human-win-subset:20261025:run_hash), excluding unrecognized/modded acquired cards or relics and histories that cannot reconstruct every combat in all three RNG variants or fail the native import/observation preflight. No neural outcomes are used for selection. Source audit covers all uploaded A10 files and records whole-run rejections. This is a training-overlapping diagnostic benchmark, not a held-out generalization claim.

Each combat is reconstructed independently from its recorded historical HP, deck, relics and inventory. Three distinct per-combat environment seeds are derived once from SHA256 human-winners-v1:20261025:case_id:replicate. Frozen fixture files contain the resulting full wire RNG state, opening state and context. Every later evaluation reuses the same files and verifies SHA256 hashes. These are contextual reconstructions, not exact recorded action snapshots.

Potion cost is 0 for starting potion instances missing from the next combat inventory or explicitly recorded as used on a single-room combat floor; otherwise 1. Last combat uses final run inventory and explicit floor use records. Duplicate potion kinds match in starting-slot order. Potions generated at combat opening or later receive cost 1. The native reward ledger sets per-instance costs before numeric observation construction, and cloned search states retain those costs. The original random per-potion training objective remains unchanged.

Step 4000 used 64 simulations for genuine choices. After training step 4107, evaluation uses easy 16, hard 64, elite 96 and boss 128 simulations for genuine choices, automatic forced actions in play and belief search, 80 workers, no replay ingestion, and the existing 1000-played-action validation cap. Capped trials are unresolved and remain in primary win-rate denominators. The model is frozen throughout a validation. Cached shard results are reusable only with matching model, native-library, and fixture hashes.

Metrics:
- Raw trial win rate: successful RNG trials divided by all requested trials.
- Combat win: at least 2 of its 3 RNG trials won.
- Run win: every recorded combat satisfies that majority rule; this is not a linked rollout carrying model HP from fight to fight.
- Net HP lost: starting HP minus ending HP, including healing as negative loss. Report completed-trial, combat-average, and winning-trial HP means, with unresolved counts.
- Per-combat and per-run results plus breakdowns by act, encounter, room type, game version, and RNG replicate.

Artifacts: manifest.json, frozen/*.jsonl, candidates/*.json (compatibility evidence), index.json and all-eligible-index.json (source audit). production/human-validation-XXXXXX/ stores per-shard outcomes, provenance.json, trials.json, combats.json, runs.json, summary.json. production/human-validation.jsonl is the history. Secondary results are separate from the original validation.

During implementation a repeated forced-chain stall was traced to Queen AI retaining a false historical companion-death counter despite a dead TorchHeadAmalgam. The predicate now also checks the actual dead companion. This repairs the AI invariant without a search cap; 9 Queen tests and 15 native search/replay tests passed. End-to-end smoke evaluated 81 trials, checked 0/1 potion ledgers and cached resume; 80 completed, one hit the existing action cap.
