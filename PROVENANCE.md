# Snapshot provenance and scope

This is a **private research package** assembled from the live training project's source and frozen benchmark artifacts. It is not an export of the entire Zeus project directory.

- `provenance/export.json`: source snapshot time, upstream checkout commit and native binary identities.
- `provenance/native-source-original.json`: source file hashes before portable packaging changes.
- `provenance/upstream-differences.json`: file-level comparison against the pinned upstream Git tree.
- `models/best/metadata.json`: model selection criterion, config, parameter count and checkpoint/actor hashes.
- `provenance/packaging-changes.json`: portable-path and CUDA-discovery changes; production source was not edited.
- `MANIFEST.sha256`: published file contents, excluding the manifest itself and Git metadata.

Historical human and primary validation use different actor-hash conventions: human hashes sorted parameter names plus tensor bytes; primary adds tensor dtype and shape. `scripts/verify_model.py` verifies both against the same exported weights. Distinct hash strings therefore do not imply distinct actors.

No full training corpus, replay buffer, optimizer moments, private SSH credentials, live service configuration, game assembly, extracted art/audio, or decompiled game source is shipped. The simulator includes upstream's small static data and test fixtures needed for builds/tests. Frozen **benchmark** inputs and derived per-trial results are included, as requested.

Upstream warns that redistribution rights for scraped player runs and models trained on them are unresolved. The human benchmark consists of reconstructed states from such runs; this package stays private and does not assign those artifacts a new open-source license. Original identifiers are run/case hashes; no authentication material is needed for evaluation. The original upstream [provenance statement](attribution/upstream-PROVENANCE.md) is retained as source material; its export rules do not imply this separate package passed upstream's exporter.

Version differences matter: the human corpus spans recorded game versions, and the benchmark reports contextual reconstruction limitations. Successful smoke tests are not proof of full parity with every game build.
