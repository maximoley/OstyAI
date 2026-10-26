# Matched policy-only validation

User requested a raw-policy benchmark alongside each normal validation.
Production was cleanly paused at optimizer step 5278. Only production_train.py
changes; no native library, model architecture, objective, optimizer, replay,
search budgets, learner quotas, corpus, or fixtures are changed.

Every 100 optimizer updates, evaluate the search actor on the existing primary
8192 combat trials, then evaluate that identical actor with simulations=0 and
simulation_budgets=None. Both modes use the same 4096 fixture records twice,
episodes 1000000 and 1000001, independent frozen per-potion costs, 1000-action
cap, and 80 workers. These passes are sequential
within the same validation event, with no training between them. The policy-only
path executes legal-action argmax directly, including a sole legal action. The
native immediate-forced-action fast path is search-specific; in zero-search
mode a sole legal action still passes through the evaluator and its forced
counter stays zero. This does not change the chosen action. The unused value-head output does
not influence action selection. No replay examples are collected. Training and
the every-500-update human benchmark retain their existing search budgets.

New files:
- production/policy-validation-XXXXXX/{provenance,replicate-0,replicate-1,summary}.json
- production/policy-validation.jsonl
- production/validation-comparison-XXXXXX.json

Existing primary validation paths remain unchanged. The comparison uses
wins/8192, counting unresolved/capped trials as nonwins, and reports search gain
in percentage points. Each mode also retains the previous resolved-only win
rate, reward, potion usage, and unresolved count. Initial potion cost signatures
must match the established production/validation-cost-signatures.json.

New caches pin actor tensor hash, fixture hash, native hash, episode seeds,
action cap, forced-action handling, and search configuration. A mismatch fails
loudly. Completed parts are reusable after interruption; history writes are
idempotent. validated_update is committed only after both passes succeed.
Unversioned historical primary caches are not silently relabeled or reused.
A legacy checkpoint already marked validated at a 100-step boundary can run
the missing policy-only pass without overwriting its historical primary result.
Validation wraps torch RNG state so graph preparation cannot perturb subsequent
learner sampling/RNG; model weights and replay are unchanged by evaluation.

Recovery: use run.sh and supervise_run.py as usual. Respect STOP. The stage
policy_validation is expected GPU evaluation, not a training stall. Final step
10000 requires primary, policy-only, and human validation to finish. The corpus
feeder has completed all 10000 banks and should stay exited.

Evidence:
- before-production_train.py: original source.
- before-latest.pt and before-replay-005278.pt: clean checkpoint and replay backup.
- pause-verification.json: original settings, checkpoint hash, finite state,
  positive priorities/valid pool tags, and exited trainer/supervisor.
- test_and_benchmark.py: full matched test plus model/RNG/checkpoint preservation,
  cached restart, idempotence, and wrong-model-cache rejection.
- matched-initial/: off-schedule step-5278 comparison; not substituted into
  historical scheduled validation results.
- smoke-results.json: completed test evidence and counters.
- deployment.json and resume-verification.json: written after testing/restart.

Native SHA256 remains ce7318db3469e64a0f4f93b3989f76e4f96c8c9c3c55462f4b90a716e40fe566.

Full matched step5278 test completed: search 7514/8192 (91.7236%), raw policy 6958/8192 (84.9365%), gain 6.7871 percentage points. Unresolved/capped trials are counted as nonwins: 1 search, 64 policy-only. Total validation wall time was 274.97s search and 24.04s policy-only, excluding common initial graph prewarm. Policy counters verified zero simulations, search transitions, forced-search transitions and replay additions. Model, training RNG and checkpoint remained unchanged; cache resume and wrong-model rejection passed.

Resume verified at step5279 with a fresh valid checkpoint: finite model/actor/optimizer/replay, positive replay priorities, valid pool tags, unchanged training settings, correct search accounting and learner quotas, active trainer/supervisor, and GPU activity. The only new setting enables matched policy-only validation. Hourly recovery instructions now include all three final validation requirements. See changes-20261026-policy-validation/resume-verification.json.
