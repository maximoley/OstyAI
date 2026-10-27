# Boss256 and doubled boss collection

Production resumed from step 5,613 after both timed benchmarks completed. Search budgets are 32/64/96/256; learner quotas are 1,170/2,341/3,511/9,362. Original training banks stay immutable, with an equal number of distinct additional boss starts appended separately within human/generated sources. Other settings, native simulator and historical results are preserved.

- [Checkpoint preservation](checkpoint-preservation.json): identical model, actor, optimizer, replay and RNG after loading; original file hashes retained.
- [Smoke](smoke-results.json): all 1,153 combats resolved, native pool counts agreed, exact quotas and simulation accounting passed; production checkpoint/replay unchanged.
- [Resume](resume-verification.json): advancing steps 5,614–5,615, valid checkpoint 5,614, finite state and live processes.
- [Cache verification](cache-resume-verification.json), [unit tests](unit-tests.log), [deployment](deployment.json), and [cycle sample](progress-snapshot.json).
- [Export hashes](export.json): original evidence and source identities before portable packaging.

Paths beginning with archive/ are redacted historical labels. Training datasets, checkpoints, replay and live service files are not included. See [training instructions](../../../docs/TRAINING.md#boss-augmentation-after-step-5613) for the portable implementation.
