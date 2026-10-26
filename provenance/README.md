# Provenance and audit guide

[Top-level provenance](../PROVENANCE.md) · [Attribution](../ATTRIBUTION.md)

**On this page:**

- [Source and package identity](#source-and-package-identity)
- [Dataset construction](#dataset-construction)
- [Simulator and training changes](#simulator-and-training-changes)

## Source and package identity

- [Upstream differences](upstream-differences.json): inventory relative to the recorded upstream checkout.
- [Package verification](package-verification.json): checks performed for the exported package, distinct from earlier deployment tests.
- [Historical pre-reuse search source](pre-reuse/): files needed to reconstruct the search implementation used for the published step-5500 result.
- [Root file manifest](../MANIFEST.sha256): hashes of tracked package contents, excluding the manifest itself.

## Dataset construction

[Training-data audit](training-data.json) records the current human/generated source pools, 100,000-run generation settings and source hashes, prepared-bank totals, and the separate learner quotas. See [training data and generation](../docs/TRAINING.md#training-data-and-generation) for their interpretation. Full training datasets are not bundled.

[Human-panel notes](human-panel-original.md) describe fixed hash-based run selection, exclusions, frozen RNG states and potion inference. The [frozen manifest](../benchmarks/fixtures/human/manifest.json) identifies the actual evaluation panel. [Construction tools](../tools/human-panel-source/) are archival and require external raw data and conversion dependencies.

## Simulator and training changes

[changes/](changes/) retains dated implementation, regression, smoke-test, deployment and checkpoint-preservation evidence. Use [simulator changes](../docs/SIMULATOR_CHANGES.md) and [training documentation](../docs/TRAINING.md) to navigate it.

Historical evidence is not rewritten to claim that a later native library produced earlier results. Model, fixture, native and evaluation identities must be read together. A local rebuild can have a different binary hash because of compiler paths and build metadata.
