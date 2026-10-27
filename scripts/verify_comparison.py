"""Recompute every published matched comparison from its complete saved trials."""
from collections import defaultdict
import gzip
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'ostyai'))
from human_validation_metrics import summarize


def main():
    directory = ROOT / 'benchmarks/comparison'
    read = lambda p: json.loads(p.read_text())
    manifest = read(directory / 'fixture-manifest.json')
    comparison = read(directory / 'comparison.json')
    reference = None
    results = []
    for entry in comparison['results']:
        folder = directory / entry['configuration']
        with gzip.open(folder / 'trials.json.gz', 'rt') as handle:
            trials = json.load(handle)
        summary, combats, runs, ordered = summarize(manifest, trials)
        saved = read(folder / 'summary.json')
        assert all(saved[k] == value for k, value in summary.items())
        assert (summary['runs'], summary['combats'], summary['trials']) == (512, 12099, 36297)
        assert ordered == trials
        for name, rows in [('combats', combats), ('runs', runs)]:
            with gzip.open(folder / (name + '.json.gz'), 'rt') as handle:
                assert json.load(handle) == rows
        keys = {(row['case_id'], row['replicate']) for row in trials}
        if reference is None:
            reference = keys
        assert len(keys) == 36297 and keys == reference
        groups = defaultdict(list)
        for combat in combats:
            groups[combat['run_hash']].append(combat['wins'])
        metrics = read(folder / 'readme-metrics.json')
        assert metrics['runs_every_fight_at_least_one'] == sum(all(w >= 1 for w in wins) for wins in groups.values())
        for key in ('trial_win_rate', 'win_rate', 'run_win_rate', 'unresolved_trials'):
            assert metrics[key] == summary[key]
        assert entry['trial_wins'] == summary['trial_wins']
        assert entry['wall_seconds'] == metrics['full_process_wall_seconds']
        results.append(dict(configuration=entry['configuration'], trial_wins=summary['trial_wins'], trials=36297))
    for relative, hashes in read(directory / 'source-hashes.json').items():
        assert hashlib.sha256((directory / relative).read_bytes()).hexdigest() == hashes['stored_file_sha256'], relative
    print(json.dumps(dict(verified=True, results=results), indent=2))


if __name__ == '__main__':
    main()
