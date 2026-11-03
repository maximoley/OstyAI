"""Recompute the network-enabled Mentor benchmark from its saved trial outcomes."""
from collections import Counter, defaultdict
import gzip
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'ostyai'))
from human_validation_metrics import summarize

BASE = ROOT / 'benchmarks/comparison'
DIRECTORY = BASE / 'mentor_net250_1000'

def read(path):
    data = path.read_bytes()
    return json.loads(gzip.decompress(data) if path.suffix == '.gz' else data)

def main():
    trials = read(DIRECTORY / 'trials.json.gz')
    saved = read(DIRECTORY / 'summary.json')
    metrics = read(DIRECTORY / 'readme-metrics.json')
    summary, combats, runs, ordered = summarize(read(BASE / 'fixture-manifest.json'), trials)
    assert (summary['runs'], summary['combats'], summary['trials']) == (512, 12099, 36297)
    assert all(saved[k] == v for k, v in summary.items())
    assert ordered == trials
    assert combats == read(DIRECTORY / 'combats.json.gz')
    assert runs == read(DIRECTORY / 'runs.json.gz')
    key = lambda row: (row['case_id'], row['replicate'])
    reference = {key(t): t for t in read(BASE / 'model5500_tiered_no_reuse/trials.json.gz')}
    old = {key(t): t for t in read(BASE / 'mentor_rl_training/trials.json.gz')}
    indexed = {key(t): t for t in trials}
    assert len(indexed) == len(trials) == 36297 and indexed.keys() == reference.keys() == old.keys()
    pools = {pool: Counter(trials=0, wins=0, old_heuristic_wins=0, unresolved=0) for pool in ('easy', 'hard', 'elite', 'boss')}
    for k, t in indexed.items():
        assert t['rollout_policy'] == 'Net' and t['rollout_network_loaded']
        assert t['configured_simulations_per_decision'] == (1000 if t['room_type'] == 'boss' else 250)
        assert t['requested_simulations'] == t['searches'] * t['configured_simulations_per_decision']
        assert t['rollout_depth'] == 200
        for field in ('fixture_sha256', 'environment_seed', 'run_hash', 'potion_costs'):
            assert t[field] == old[k][field], (k, field)
        pool = pools[reference[k]['search_tier']]
        pool['trials'] += 1
        pool['wins'] += bool(t['win'] and t['terminal'] and not t['truncated'])
        pool['old_heuristic_wins'] += bool(old[k]['win'] and old[k]['terminal'] and not old[k]['truncated'])
        pool['unresolved'] += not t['terminal'] or t['truncated']
    groups = defaultdict(list)
    for combat in combats: groups[combat['run_hash']].append(combat['wins'])
    assert metrics['runs_1of3'] == sum(all(w >= 1 for w in wins) for wins in groups.values()) == 330
    assert metrics['runs_2of3'] == summary['runs_won_every_fight'] == 212
    assert metrics['combat_majority_wins'] == summary['wins'] == 11514
    assert metrics['trial_wins'] == summary['trial_wins'] == 34209
    assert metrics['unresolved_trials'] == summary['unresolved_trials'] == 0
    for pool, values in pools.items():
        saved_pool = read(DIRECTORY / 'pool-win-rates.json')[pool]
        assert all(saved_pool[k] == v for k, v in values.items())
        assert saved_pool['win_rate'] == values['wins'] / values['trials']
    provenance = read(DIRECTORY / 'provenance.json')
    assert provenance['workers'] == 80 and provenance['rollout_policy'] == 'Net'
    assert provenance['rollout_network']['weights_sha256'] == '50a71a00d64efc63e1042e54dd81e45bc6f7a9435fb6846b60d3a9c7393b6d67'
    assert sum(t['rollout_net_action_selections'] for t in trials) == read(DIRECTORY / 'verification.json')['net_action_selections'] > 0
    timing = read(DIRECTORY / 'timing.json')
    assert timing['workers'] == 80 and timing['exit_code'] == 0 and timing['fresh_process']
    assert timing['external_wall_seconds'] == metrics['external_wall_seconds']
    for path, hashes in read(DIRECTORY / 'source-hashes.json').items():
        assert hashlib.sha256((DIRECTORY / path).read_bytes()).hexdigest() == hashes['stored_sha256'], path
    print(json.dumps(dict(verified=True, trials=36297, wins=34209, combats=12099, runs=512, unresolved=0, matched_frozen_trials=True), indent=2))

if __name__ == '__main__': main()
