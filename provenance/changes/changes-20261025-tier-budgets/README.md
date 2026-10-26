# Per-combat search budgets — 2026-10-25

Training paused cleanly after optimizer step 4107 (last primary validation 4100). The first new-budget cycle is 4108. Preserve the existing target of 10000 updates, model/optimizer/replay/RNG state, per-potion reward, forced action bypass, and both validation schedules.

Budgets per genuine decision: easy 16, hard 64, elite 96, boss 128. Resolve the native encounter registry first: Weak -> easy, Normal -> hard, Elite -> elite, Boss -> boss. For generated fixtures without encounter IDs, use their explicit weak/normal/elite/boss room labels. Unclassified event encounters use hard 64. Ambiguous unknown monster encounters are errors rather than silently assigned a pool.

The budget is stored once on the native combat Record and persists through every decision. Search algorithm, considered-action count (8), belief draws, and forced-chain collapse are unchanged. Factual forced actions use zero simulations. Both primary validation and human validation call the same production configuration. Historical validation files are unchanged; comparisons after this change use a different search allocation.

Each outcome records search_tier, simulation_budget, searched_decisions, forced_actions and simulations. Reports and production/progress.jsonl include search_by_tier. collect() asserts per combat:
- decisions = searched_decisions + forced_actions
- simulations = simulation_budget * searched_decisions
- total simulations equals the sum over combats

Validation summaries and secondary cache provenance store the whole budget mapping. Primary cached reports reject a different mapping. Old replay examples retain their original targets and priorities.

Evidence: pause-verification.json and paused.pt; tests.log (17 native tests); build.log; training-report.json and primary-report.json; smoke-results.json; resume-verification.json. The small secondary smoke exercises the actual secondary evaluator with fixed potion costs and verifies cached resume.
