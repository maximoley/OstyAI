# Fixed boss128 budget for scheduled human benchmarks

After a clean pause at step5628, pin the human benchmark run every500 updates to easy32/hard64/elite96/boss128. The next scheduled human benchmark is step6000. These are simulations per non-forced decision, not a search depth limit.

Training and primary search validation retain32/64/96/256, doubled boss collection, learner quotas1170/2341/3511/9362 and80 Rust workers. Policy-only validation stays zero-search. Persistent search reuse, forced-action compression, potion costs, frozen fixtures/seeds, schedules and all native repairs remain unchanged. Consequently matching budgets alone does not remove the historical reuse/simulator differences from older checkpoints.

`production_train.human_validation_config` makes a fresh copy of the fixed mapping and is used only by the scheduled human callback. `human_validation_search_simulations` records the mapping in settings/checkpoints. Both human provenance and actual collection consume this same callback. Incomplete caches must still match provenance; existing completed historical results are never overwritten or relabeled.

Regression tests cover independence from current/future training budgets, primary/training preservation, per-call isolation and unchanged evaluation settings. See tests.log, pause-verification.json, deployment.json and resume-verification.json. Checkpoint/replay bytes were verified unchanged immediately before resume; no migration of model, actor, optimizer, replay or RNG was needed.

Recovery must preserve both separate mappings. Resume via run.sh/supervise_run.py; augmentation feeder resumes from its first missing bank with the existing v31 converter/runtime. Respect later STOP requests. This change does not authorize reverting training to boss128 or changing benchmark reuse.
