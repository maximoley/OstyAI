"""One-time tagging of legacy replay; new records use native search_tier tags."""
import json,re
from pathlib import Path
import numpy as np
from pool_sampling import POOL_CODES


def restore_legacy_pool_tags(replay, root):
    missing = np.flatnonzero(replay.tiers[:replay.count] < 0)
    if not len(missing): return dict(migrated=0)
    registry = {}
    source = Path(root)/'source/sts2-rs/crates/sts2-content/src/encounters'
    if not source.exists():
        source = Path(root).parent/'native/crates/sts2-content/src/encounters'
    for path in source.glob('act*.rs'):
        for name,tier in re.findall(r'id:\s*"([A-Z0-9_]+)"[\s\S]*?tier:\s*EncounterTier::(Weak|Normal|Elite|Boss)', path.read_text()):
            registry[name] = dict(Weak='easy',Normal='hard',Elite='elite',Boss='boss')[tier]
    if not registry: raise ValueError('Native encounter registry unavailable for legacy migration')
    ids = replay.ids[missing]; episodes = ids >> np.uint64(32)
    banks = []
    for episode in np.unique(episodes):
        path = Path(root)/f'prepared/bank-{int(episode)-1:06d}.jsonl'
        labels = []
        with path.open() as source:
            lines = source.readlines()
        for line in lines:
            m=json.loads(line)['metadata']; name=m.get('encounter','').upper().removeprefix('ENCOUNTER.');room=m.get('room_type','')
            tier=registry.get(name)
            if tier is None:
                if room in ('weak','easy'):tier='easy'
                elif room in ('normal','hard','event'):tier='hard'
                elif room in ('elite','boss'):tier=room
                elif room=='monster' and name.endswith('_WEAK'):tier='easy'
                elif room=='monster' and (name.endswith('_NORMAL') or 'EVENT' in name):tier='hard'
                else:raise ValueError(f'Unknown legacy replay pool: {name} / {room}')
            labels.append(POOL_CODES[tier])
        take=np.flatnonzero(episodes==episode)
        replay.tiers[missing[take]]=np.asarray(labels,dtype=np.int8)[(ids[take]&np.uint64(0xffffffff)).astype(np.int64)]
        banks.append(int(episode)-1)
    return dict(migrated=len(missing),banks=banks,retained_by_pool={t:int(np.sum(replay.tiers[:replay.count]==i)) for t,i in POOL_CODES.items()})
