"""Immutable boss-only additions to original collection banks; no learner RNG use."""
from collections import Counter
import hashlib
import json
from pathlib import Path
import re

SCHEMA = 'double-boss-starts-v1'
DIRECTORY = 'boss-augmentation-v1'
START_BANK = 5613
POOLS = ('easy', 'hard', 'elite', 'boss')


def digest(payload):
    return hashlib.sha256(payload).hexdigest()


def read_json(path):
    return json.loads(Path(path).read_text())


def atomic_bytes(path, payload):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + '.tmp')
    temporary.write_bytes(payload)
    temporary.replace(path)


def atomic_json(path, data):
    atomic_bytes(path, (json.dumps(data, indent=2) + '\n').encode())


def registry(root):
    result = {}
    for path in sorted((Path(root).parent / 'native/crates/sts2-content/src/encounters').glob('act*.rs')):
        for name, tier in re.findall(r'id:\s*"([A-Z0-9_]+)"[\s\S]*?tier:\s*EncounterTier::(Weak|Normal|Elite|Boss)', path.read_text()):
            result[name] = dict(Weak='easy', Normal='hard', Elite='elite', Boss='boss')[tier]
    if not result:
        raise ValueError('Native encounter registry is unavailable')
    return result


def classify(row, native_registry):
    metadata = row['metadata']
    name = metadata.get('encounter', '').upper().removeprefix('ENCOUNTER.')
    room = metadata.get('room_type', '')
    if name in native_registry:
        return native_registry[name]
    fallback = dict(weak='easy', easy='easy', normal='hard', hard='hard', elite='elite', boss='boss', event='hard')
    if room in fallback:
        return fallback[room]
    if room == 'monster':
        if name.endswith('_WEAK'):
            return 'easy'
        if name.endswith('_NORMAL') or 'EVENT' in name:
            return 'hard'
    raise ValueError(f'Cannot classify encounter {name!r}, room {room!r}')


def source_kind(row):
    kind = row['source'].split(':', 1)[0]
    if kind not in ('human', 'generated'):
        raise ValueError(f'Unknown source kind {kind}')
    return kind


def bank_counts(rows, native_registry):
    pool = Counter({name: 0 for name in POOLS})
    source = Counter(human=0, generated=0)
    boss_source = Counter(human=0, generated=0)
    for row in rows:
        tier = classify(row, native_registry)
        kind = source_kind(row)
        pool[tier] += 1
        source[kind] += 1
        if tier == 'boss':
            boss_source[kind] += 1
    return dict(by_pool=dict(pool), by_source=dict(source), bosses_by_source=dict(boss_source))


def verify_additions(base_rows, extra_rows, native_registry, excluded):
    identities = [row['source'] for row in base_rows + extra_rows]
    if len(identities) != len(set(identities)):
        raise ValueError('Duplicate source identity within augmented bank')
    if any(row['metadata'].get('run_hash') in excluded for row in base_rows + extra_rows):
        raise ValueError('Held-out human run in collection bank')
    if any(classify(row, native_registry) != 'boss' for row in extra_rows):
        raise ValueError('Only boss starts may be appended')
    before = bank_counts(base_rows, native_registry)
    added = bank_counts(extra_rows, native_registry)
    final = bank_counts(base_rows + extra_rows, native_registry)
    if added['bosses_by_source'] != before['bosses_by_source']:
        raise ValueError('Must add exactly one additional boss per original boss in each source stratum')
    assert final['by_pool']['boss'] == 2 * before['by_pool']['boss']
    assert all(final['by_pool'][tier] == before['by_pool'][tier] for tier in POOLS if tier != 'boss')
    return dict(base=before, added=added, final=final)


def materialize(root, output, index):
    """Assemble one temporary active bank; original and extra banks stay immutable."""
    root, output = Path(root), Path(output)
    original = root / f'prepared/bank-{index:06d}.jsonl'
    base_meta = read_json(original.with_suffix('.json'))
    base = original.read_bytes()
    if digest(base) != base_meta['sha256']:
        raise ValueError('Original bank checksum mismatch')
    extra_path = root / DIRECTORY / f'bank-{index:06d}.jsonl'
    extra_meta = read_json(extra_path.with_suffix('.json'))
    extra = extra_path.read_bytes()
    if extra_meta['schema'] != SCHEMA or extra_meta['bank'] != index:
        raise ValueError('Augmentation schema/index mismatch')
    if extra_meta['base_sha256'] != digest(base) or extra_meta['sha256'] != digest(extra):
        raise ValueError('Augmentation checksum mismatch')
    catalog = read_json(root / DIRECTORY / 'manifest.json')
    if extra_meta['catalog_sha256'] != digest((root / DIRECTORY / 'manifest.json').read_bytes()):
        raise ValueError('Augmentation catalog mismatch')
    if catalog['native_sha256'] != digest((root / 'loop_native.so').read_bytes()):
        raise ValueError('Augmentation needs revalidation for changed native library')
    rows = [json.loads(line) for line in base.splitlines()]
    additions = [json.loads(line) for line in extra.splitlines()]
    excluded = set(read_json(root / 'corpus/manifest.json')['excluded_human_training_runs'])
    counts = verify_additions(rows, additions, registry(root), excluded)
    if counts != extra_meta['counts'] or len(rows) != base_meta['accepted']:
        raise ValueError('Augmented bank counts mismatch')
    # Existing record indices and bytes remain the unchanged prefix.
    if base and not base.endswith(b'\n'):
        raise ValueError('Original bank must end in a newline')
    merged = base + extra
    if digest(merged) != extra_meta['merged_sha256']:
        raise ValueError('Merged bank checksum mismatch')
    path = output / 'active-boss-bank.jsonl'
    atomic_bytes(path, merged)
    metadata = dict(base_meta, accepted=len(rows) + len(additions), sha256=digest(merged),
                    collection_sampling=dict(schema=SCHEMA, bank=index, **counts,
                      base_sha256=digest(base), added_sha256=digest(extra),
                      catalog_sha256=extra_meta['catalog_sha256'],
                      native_sha256=catalog['native_sha256'], added_sources=[r['source'] for r in additions]))
    return path, metadata
