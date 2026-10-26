# Pool-proportional learner sampling — 2026-10-25

Production paused cleanly after step 4176. Model, actor, optimizer moments/step, replay observations/policy/value targets/priorities, replay RNG and torch RNG are preserved. The diagnostic GPU update is discarded; production resumes from the unchanged paused checkpoint.

The full 16384-example optimizer update has exact largest-remainder quotas proportional to simulation budgets (sum 304): easy 862, hard 3449, elite 5174, boss 6899. Both policy CE and value MSE use the same mixture. Combat generation, FIFO replay retention, search budgets, rewards, forced action handling, and both validation schedules are unchanged.

For each pool b with N_b retained decision samples:
P(i|b) = 0.95 * max(priority_i, 0.001) / sum_j_in_b max(priority_j,0.001) + 0.05 / N_b.
Draw the exact quota q_b * 16384 with replacement. Importance weight w_i = 1 / (N_b * P(i|b)); beta remains 1. The pool fraction cancels between target and proposal; do not multiply by global uniform replay probabilities. In expectation, the loss is sum_b q_b * mean(loss_i for i in b). Quota rounding is deterministic, totals exactly 16384, and changes proportions by less than one example from the ideal ratio.

After selecting all examples, shuffle tie order and sort by token capacity. Pack consecutive groups of 256 using the maximum token capacity in that microbatch. No sample or importance weight is changed, dropped, or duplicated by packing. All 64 backward passes precede one optimizer update. Pool proportions apply to the full update, not each microbatch. A missing pool is an explicit error, never a silent redistribution.

New samples are tagged using the native outcome search_tier and (episode << 32) | combat_id. Existing replay is tagged once using its original prepared banks and native encounter registry, with the same fallback rules as native search-budget assignment. Tags are checkpointed with pool_schema=1. New priorities initialize from the maximum within their own pool (minimum 1). Existing priorities are preserved; subsequent updates still use absolute value error.

Logging: learner.replay_sampling.by_pool contains quota, sampled count, retained count, unique examples, weight statistics/effective sample size, sampled policy CE/value MSE, and each pool's weighted contribution to the full policy/value loss. Contributions are checked against aggregate logged losses. Native search_by_tier remains available separately.

Tests: exact quota and padded packing, conditional importance expectation, no cross-pool priority influence, exact RNG replay, empty-bin rejection, and legacy-tag migration preserving IDs/priorities/RNG. GPU smoke loads real checkpoint/replay, checks arrays unchanged by migration, tests tag checkpoint round-trip and native ingestion, then performs one isolated full 16384-example optimizer update and checks pool counts, finite weights/gradients and loss contributions. Evidence is in tests.log, smoke-results.json, pause-verification.json and resume-verification.json.
