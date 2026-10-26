# Snapshot provenance and scope

This is a **research package prepared for public release** assembled from the live training project's source and frozen benchmark artifacts. It is not an export of the entire training host project directory.

- `provenance/export.json`: source snapshot time, upstream checkout commit and native binary identities.
- `provenance/native-source-original.json`: source file hashes before portable packaging changes.
- `provenance/upstream-differences.json`: file-level comparison against the pinned upstream Git tree.
- `models/best/metadata.json`: model selection criterion, config, parameter count and checkpoint/actor hashes.
- `provenance/packaging-changes.json`: portable-path and CUDA-discovery changes; production source was not edited.
- `provenance/training-data.json`: verified training-source counts, generation configuration, source hashes and accepted prepared-bank totals; the full training corpus is not bundled.
- `MANIFEST.sha256`: published file contents, excluding the manifest itself and Git metadata.

Historical human and primary validation use different actor-hash conventions: human hashes sorted parameter names plus tensor bytes; primary adds tensor dtype and shape. `scripts/verify_model.py` verifies both against the same exported weights. Distinct hash strings therefore do not imply distinct actors.

No full training corpus, replay buffer, optimizer moments, private SSH credentials, live service configuration, game assembly, extracted art/audio, or decompiled game source is shipped. The simulator includes upstream's small static data and test fixtures needed for builds/tests. Frozen **benchmark** inputs and derived per-trial results are included, as requested.

The bundled human benchmark is retained for reproducible evaluation at the project owner’s direction. It consists of reconstructed states from Spire Codex community runs, identified by run/case hashes. See [Spire Codex API terms](https://github.com/ptrlrd/spire-codex/blob/main/API_TERMS.md). The upstream historical provenance statement records unresolved redistribution questions; inclusion here does not claim that third-party rights have been cleared or assign the underlying records a new license. The trained OstyAI weights have their separate [CC BY-SA 4.0 grant](models/WEIGHTS-LICENSE.md). No authentication material is needed for evaluation. The original upstream [provenance statement](attribution/upstream-PROVENANCE.md) is retained as source material; its export rules do not imply this separate package passed upstream's exporter.

Version differences matter: the human corpus spans recorded game versions, and the benchmark reports contextual reconstruction limitations. Successful smoke tests are not proof of full parity with every game build.

## Sanitized export

Personal host labels and absolute experiment paths have been removed. Paths beginning with `archive/` in historical evidence are redacted labels, not downloadable files. Runtime defaults use repository-relative inputs or explicit environment variables. Historical source/native hashes continue to identify the original experiments; `MANIFEST.sha256` identifies the sanitized distribution. See `provenance/public-sanitization.json`. Model tensor files and human combat fixture shards are unchanged.
