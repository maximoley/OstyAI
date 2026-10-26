# Easy pool budget 32 and proportional learner mixture

Requested 2026-10-26. Clean pause at optimizer step 4598. Increase the easy/normal pool previously using 16 simulations to 32. Hard stays 64, elite 96, boss 128. The shared POOL_BUDGETS mapping drives native collection, primary validation, human validation provenance/config, learner quotas and checkpoint settings.

Total budget mass is 320. The 16384-example optimizer update has largest-remainder quotas easy=1638, hard=3277, elite=4915, boss=6554 (10%,20%,30%,40% before integer rounding). Within-pool prioritized sampling and beta=1 importance correction, 5% uniform mixing, FIFO retention and 64 microbatches of 256 remain unchanged.

Checkpoint/actor/model/optimizer/replay priorities/pool tags/RNG are retained without migration. The first resumed update checkpoints the settings change through existing budget_changed handling. The native library is unchanged, including the dead legacy enemy import repair. Historical results and frozen fixture bytes are retained. New validation provenance records the new complete budget mapping.

Evidence: pause-verification.json, tests.log (existing sampling tests), smoke-results.json (native training and both validation paths, including secondary cache reuse), and resume-verification.json (actual new search counts, learner quotas, checkpoint and process progress).
