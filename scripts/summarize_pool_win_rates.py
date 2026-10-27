"""Recompute matched benchmark trial win rates using identical combat-pool labels."""
from collections import Counter
import gzip
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DIRECTORY = ROOT / 'benchmarks/comparison'
POOLS = ('easy', 'hard', 'elite', 'boss')
ORDER = ('mentor_rl_training', 'mentor250_boss500', 'model5500_tiered_no_reuse',
         'model5500_boss256_no_reuse', 'model5500_policy_greedy')
LABELS = ('Mentor MCTS 250/1,000', 'Mentor MCTS 250/500', 'OstyAI 5500, boss128',
          'OstyAI 5500, boss256', 'OstyAI 5500, greedy')


def read_trials(configuration):
    path = DIRECTORY / configuration / 'trials.json.gz'
    with gzip.open(path, 'rt') as handle:
        trials = json.load(handle)
    keyed = {(row['case_id'], row['replicate']): row for row in trials}
    assert len(keyed) == len(trials) == 36297
    return keyed


def summarize():
    reference = read_trials('model5500_tiered_no_reuse')
    labels = {key: row['search_tier'] for key, row in reference.items()}
    assert set(labels.values()) == set(POOLS)
    # A combat must retain its pool across its three frozen RNG trials.
    by_case = {}
    for (case, replicate), pool in labels.items():
        assert replicate in (0, 1, 2)
        assert by_case.setdefault(case, pool) == pool
    assert len(by_case) == 12099
    counts = Counter(labels.values())
    results = []
    hashes = {}
    for configuration in ORDER:
        trials = read_trials(configuration)
        assert trials.keys() == reference.keys()
        stats = {pool: Counter(trials=0, wins=0, losses=0, unresolved=0) for pool in POOLS}
        for key, trial in trials.items():
            original = reference[key]
            for field in ('encounter', 'room_type', 'act', 'environment_seed', 'run_hash'):
                assert trial[field] == original[field], (configuration, key, field)
            pool = labels[key]
            if configuration in ('model5500_tiered_no_reuse', 'model5500_boss256_no_reuse'):
                assert trial['search_tier'] == pool
            resolved = bool(trial.get('terminal') and not trial.get('truncated')
                            and not trial.get('unsupported'))
            won = resolved and bool(trial['win'])
            stats[pool]['trials'] += 1
            stats[pool]['wins'] += won
            stats[pool]['losses'] += resolved and not won
            stats[pool]['unresolved'] += not resolved
        source = json.loads((DIRECTORY / configuration / 'summary.json').read_text())
        assert sum(x['wins'] for x in stats.values()) == source['trial_wins']
        assert sum(x['unresolved'] for x in stats.values()) == source['unresolved_trials']
        for pool, values in stats.items():
            assert values['trials'] == counts[pool]
            assert values['wins'] + values['losses'] + values['unresolved'] == values['trials']
            values['win_rate'] = values['wins'] / values['trials']
        results.append(dict(configuration=configuration,by_pool=stats))
        path = DIRECTORY / configuration / 'trials.json.gz'
        hashes[str(path.relative_to(DIRECTORY))] = hashlib.sha256(path.read_bytes()).hexdigest()
    report = dict(
        definition='Wins / all three-seed trials in each pool; unresolved trials count as nonwins.',
        pool_assignment='Recorded native search_tier from tiered boss128 trials, verified against boss256 trials; joined to every agent by (case_id, replicate). Policy-only uniform labels describe evaluation mode, not combat pool.',
        trials_by_pool=dict(counts), combats_by_pool=dict(Counter(by_case.values())),
        matched_trial_ids=True, source_sha256=hashes, results=results)
    (DIRECTORY / 'pool-win-rates.json').write_text(json.dumps(report, indent=2) + '\n')
    table = ['| Combat pool | ' + ' | '.join(LABELS) + ' |', '|' + '---|' * (len(LABELS)+1)]
    for pool in POOLS:
        table.append('| ' + pool.capitalize() + ' | ' + ' | '.join(
            f"**{r['by_pool'][pool]['win_rate'] * 100:.2f}%**" for r in results) + ' |')
    print('\n'.join(table))
    return report


if __name__ == '__main__':
    summarize()
