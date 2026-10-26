# Archived human-panel construction tools

These tools reproduce source preparation when the external original corpus and conversion projects are available. The frozen benchmark included in this repository does not require them.

Set the following paths explicitly before using the applicable script:

| Variable | Input |
|---|---|
| `OSTYAI_SOURCE_RUN` | Experiment workspace; output is `human-win-validation/` within it |
| `OSTYAI_HUMAN_SOURCE` | Directory of original compressed human-run records (`index.py`) |
| `OSTYAI_HUMAN_CACHE` | Reconstruction-cache pickle (`index.py`) |
| `OSTYAI_CONVERSION_SOURCE` | Conversion project containing `native-lib` and conversion modules (`build.py`) |
| `OSTYAI_TOKENIZER_SOURCE` | Tokenizer project (`build.py`) |

Only load trusted pickle inputs. The full training/source corpus and those external project environments are not included. See [benchmark documentation](../../benchmarks/README.md) to use the already frozen fixtures.
