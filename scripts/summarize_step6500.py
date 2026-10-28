"""Recompute scheduled and matched step-6500 results from every saved trial."""
from collections import Counter, defaultdict
import gzip
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'ostyai'))
from human_validation_metrics import summarize

BASE = ROOT / 'benchmarks/comparison'
OUT = BASE / 'step6500'
ARMS = ('model6500_boss128', 'model6500_boss256', 'model6500_policy_greedy')

def read(path):
    return json.loads(gzip.decompress(path.read_bytes()) if path.suffix == '.gz' else path.read_bytes())

def write(path, data):
    path.write_text(json.dumps(data, indent=2) + '\n')

def recompute(folder, manifest):
    trials = read(folder / 'trials.json.gz')
    saved = read(folder / 'summary.json')
    summary, combats, runs, _ = summarize(manifest, trials)
    for key, value in summary.items():
        if key in saved:
            assert value == saved[key], (folder.name, key)
    assert combats == read(folder / 'combats.json.gz')
    assert runs == read(folder / 'runs.json.gz')
    assert (summary['runs'], summary['combats'], summary['trials']) == (512, 12099, 36297)
    per_run = defaultdict(list)
    for combat in combats:
        per_run[combat['run_hash']].append(combat['wins'])
    summary['runs_won_every_fight_at_least_one'] = sum(all(w >= 1 for w in wins) for wins in per_run.values())
    summary['run_at_least_one_rate'] = summary['runs_won_every_fight_at_least_one'] / 512
    return summary, {(t['case_id'], t['replicate']): t for t in trials}

def main():
    manifest = read(BASE / 'fixture-manifest.json')
    reference = {(t['case_id'], t['replicate']): t for t in read(BASE / 'model5500_tiered_no_reuse/trials.json.gz')}
    results = []
    for arm in ARMS:
        folder = OUT / arm
        metrics, trials = recompute(folder, manifest)
        assert len(trials) == 36297 and trials.keys() == reference.keys()
        pools = {p: Counter(trials=0, wins=0, losses=0, unresolved=0) for p in ('easy', 'hard', 'elite', 'boss')}
        for key, t in trials.items():
            original = reference[key]
            for field in ('encounter', 'act', 'room_type', 'environment_seed', 'run_hash'):
                assert t[field] == original[field], (arm, key, field)
            pool = original['search_tier']
            if arm != 'model6500_policy_greedy':
                assert t['search_tier'] == pool
            resolved = bool(t.get('terminal') and not t.get('truncated') and not t.get('unsupported'))
            won = resolved and bool(t['win'])
            pools[pool]['trials'] += 1
            pools[pool]['wins'] += won
            pools[pool]['losses'] += resolved and not won
            pools[pool]['unresolved'] += not resolved
        for pool in pools.values():
            pool['win_rate'] = pool['wins'] / pool['trials']
        assert sum(p['wins'] for p in pools.values()) == metrics['trial_wins']
        timing = read(folder / 'timing.json')
        assert timing['exit_code'] == 0 and timing['workers'] == 80
        parts = sorted(folder.glob('part-*.json.gz'))
        assert len(parts) == 48
        for part in parts:
            report = read(part)
            assert report['workers'] == 80 and not report['fatal'] and report['replay_added'] == 0
            if arm == 'model6500_policy_greedy':
                assert all(report[k] == 0 for k in ('simulations', 'search_transitions', 'forced_search_transitions', 'chance_samples', 'forced_actions'))
        metrics.update(configuration=arm, by_pool=pools, wall_seconds=timing['external_wall_seconds'], workers=80,
                       independently_recomputed=True, matched_trial_ids_seeds_and_pools=True)
        write(folder / 'readme-metrics.json', metrics)
        results.append(metrics)
    write(OUT / 'readme-metrics.json', dict(results=results, pool_definition='Join frozen combat pool by case_id and replicate; policy-only mode labels do not redefine combat pools.'))

    # Native zero-search reports label their search mode "uniform". Resolve the
    # encounter pool for the derived failure index without changing raw trials.
    index_path = OUT / 'combat-analysis/failed-trial-fixture-index.jsonl.gz'
    index = [json.loads(line) for line in gzip.decompress(index_path.read_bytes()).splitlines()]
    for row in index:
        row.setdefault('recorded_search_tier', row['pool'])
        row['pool'] = reference[(row['case_id'], row['replicate'])]['search_tier']
    index_path.write_bytes(gzip.compress((''.join(json.dumps(r) + '\n' for r in index)).encode(), mtime=0))
    import hashlib
    hashes_path = OUT / 'source-hashes.json'
    hashes = read(hashes_path)
    entry = hashes[str(index_path.relative_to(ROOT))]
    entry['stored_sha256'] = hashlib.sha256(index_path.read_bytes()).hexdigest()
    entry['transformation'] = 'Personal path prefixes sanitized; pool joined from frozen boss128 reference, original mode label retained as recorded_search_tier.'
    write(hashes_path, hashes)

    scheduled = ROOT / 'benchmarks/human-validation-006500'
    metrics, _ = recompute(scheduled, read(ROOT / 'benchmarks/fixtures/human/manifest.json'))
    metrics.update(update=6500, independently_recomputed=True,
                   timing_note='Scheduled evaluation timer; excludes standalone process startup. Do not substitute for timed comparison.')
    write(ROOT / 'benchmarks/readme-step6500-metrics.json', metrics)
    datafile = ROOT / 'benchmarks/history/human-benchmark-plot-data.json'
    data = read(datafile)
    point = dict(update=6500, trial_win_rate=metrics['trial_win_rate'], win_rate=metrics['win_rate'],
                 runs_won_every_fight=metrics['runs_won_every_fight'],
                 runs_every_combat_at_least_one_win=metrics['runs_won_every_fight_at_least_one'],
                 manifest_sha256=read(scheduled / 'provenance.json')['manifest_sha256'])
    data['points'] = sorted([p for p in data['points'] if p['update'] != 6500] + [point], key=lambda p: p['update'])
    data['current_source'] = 'benchmarks/human-validation-006500/combats.json.gz'
    write(datafile, data)
    print(json.dumps(dict(scheduled=point, matched=[dict(arm=r['configuration'], win_rate=r['trial_win_rate'], pools=r['by_pool']) for r in results]), indent=2))

if __name__ == '__main__':
    main()
