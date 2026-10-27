"""Prepare reproducible extra boss starts from the full eligible source pools."""
import argparse
import ctypes as C
import fcntl
import hashlib
import json
import os
from pathlib import Path
import random
import sys
import time

ROOT = Path(__file__).resolve().parent
SOURCE = Path(os.environ.get("OSTYAI_CORPUS_CONVERTER", str(ROOT / "external-corpus-converter")))
# Keep the conversion module and sts2_rs pinned to the established v31 runtime.
sys.path.insert(0, str(SOURCE))
sys.path.insert(0, str(SOURCE / 'native-lib'))
sys.path.insert(0, str(ROOT))
from boss_augmentation import SCHEMA, DIRECTORY, START_BANK, atomic_bytes, atomic_json, bank_counts, classify, digest, read_json, registry, source_kind, verify_additions


class NativeValidator:
    def __init__(self, root, destination):
        self.root, self.destination = root, destination
        self.lib = C.CDLL(str(root / 'loop_native.so'))
        self.lib.sm_loop_create.argtypes = [C.c_char_p]
        self.lib.sm_loop_create.restype = C.c_void_p
        self.lib.sm_loop_destroy.argtypes = [C.c_void_p]
        self.lib.sm_loop_error.restype = C.c_char_p

    def accept(self, rows, rejects):
        if not rows:
            return []
        probe = self.destination / 'native-import-probe.jsonl'
        atomic_bytes(probe, encode(rows))
        cfg = dict(fixtures=str(probe),schema=str(self.root/'schema'),cases=len(rows),workers=1,
                   batch_size=1,buffers=2,row_capacity=4096,max_actions=500,record_replay=False,
                   reward_shaping=True,simulations=64,simulation_budgets=dict(easy=32,hard=64,elite=96,boss=256))
        pool = self.lib.sm_loop_create(json.dumps(cfg).encode())
        if pool:
            self.lib.sm_loop_destroy(pool)
            return rows
        error = self.lib.sm_loop_error().decode()
        if len(rows) == 1:
            rejects.append(dict(source=rows[0]['source'],stage='native_import',reason=error))
            return []
        split=len(rows)//2
        return self.accept(rows[:split],rejects)+self.accept(rows[split:],rejects)


def encode(rows):
    return ''.join(json.dumps(row,separators=(',',':'))+'\n' for row in rows).encode()


def main():
    if not (SOURCE / "full_corpus.py").exists():
        raise SystemExit("Set OSTYAI_CORPUS_CONVERTER to the full corpus conversion checkout; see docs/TRAINING.md")
    from full_corpus import FullCorpus, convert
    ap=argparse.ArgumentParser()
    ap.add_argument('--start',type=int,default=START_BANK)
    ap.add_argument('--target',type=int,default=10000)
    ap.add_argument('--workers',type=int,default=8)
    ap.add_argument('--allow-paused',action='store_true',help='Explicit bounded preparation while production STOP remains set')
    a=ap.parse_args()
    assert START_BANK<=a.start<a.target<=10000
    if a.allow_paused:assert a.target-a.start<=10,'Paused preparation must be bounded'
    directory=ROOT/DIRECTORY;directory.mkdir(exist_ok=True)
    lock=(directory/'producer.lock').open('w');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    source=FullCorpus(ROOT,seed=20261023,workers=a.workers,size=1024)
    native_registry=registry(ROOT)
    excluded=set(read_json(ROOT/'corpus/manifest.json')['excluded_human_training_runs'])
    human_ids=[i for i,c in enumerate(source.human) if c.get('room_type')=='boss' or native_registry.get(c.get('encounter','').upper().removeprefix('ENCOUNTER.'))=='boss']
    assert human_ids
    specification=dict(schema=SCHEMA,seed=20261026,first_bank=START_BANK,
      selection='Append one new boss per original boss within each source stratum; uniform without replacement from full eligible boss source pools, independently per bank; generated bosses use rejection sampling from the complete generated catalog.',
      full_corpus_manifest_sha256=digest((ROOT/'full-corpus/manifest.json').read_bytes()),
      holdout_manifest_sha256=digest((ROOT/'corpus/manifest.json').read_bytes()),
      native_sha256=digest((ROOT/'loop_native.so').read_bytes()),
      human_candidates=len(source.human),human_boss_candidates=len(human_ids),generated_candidates=source.total,
      conversion_python=sys.executable,conversion_module=str(SOURCE/'corpus_conversion.py'),
      conversion_module_sha256=digest((SOURCE/'corpus_conversion.py').read_bytes()))
    manifest=directory/'manifest.json'
    if manifest.exists():assert read_json(manifest)==specification,'Augmentation source/configuration changed'
    else:atomic_json(manifest,specification)
    catalog_hash=digest(manifest.read_bytes())
    validator=NativeValidator(ROOT,directory)
    atomic_json(directory/'producer-status.json',dict(stage='preparing',pid=os.getpid(),start=a.start,target=a.target))
    last_ready=a.start-1
    try:
        for index in range(a.start,a.target):
            if (directory/'STOP').exists() or ((ROOT/'STOP').exists() and not a.allow_paused):break
            started=time.perf_counter()
            original=ROOT/f'prepared/bank-{index:06d}.jsonl';base=original.read_bytes();base_meta=read_json(original.with_suffix('.json'))
            assert digest(base)==base_meta['sha256']
            base_rows=[json.loads(line) for line in base.splitlines()]
            path=directory/f'bank-{index:06d}.jsonl';info=path.with_suffix('.json')
            if info.exists():
                old=read_json(info);raw=path.read_bytes()
                assert old['schema']==SCHEMA and old['catalog_sha256']==catalog_hash and old['base_sha256']==digest(base) and old['sha256']==digest(raw)
                assert old['merged_sha256']==digest(base+raw)
                assert verify_additions(base_rows,[json.loads(l) for l in raw.splitlines()],native_registry,excluded)==old['counts']
                last_ready=index
                atomic_json(directory/'producer-status.json',dict(stage='ready',last_ready=index,target=a.target,pid=os.getpid(),cached=True))
                continue
            counts=bank_counts(base_rows,native_registry);existing={r['source'] for r in base_rows};additions=[];rejects=[];sampling={}
            for kind in ('human','generated'):
                required=counts['bosses_by_source'][kind]
                rng=random.Random(f'{SCHEMA}:20261026:{index}:{kind}')
                population=human_ids if kind=='human' else range(source.total)
                chosen=set();accepted=[];attempted=nonboss=duplicates=0
                while len(accepted)<required:
                    if (directory/'STOP').exists() or ((ROOT/'STOP').exists() and not a.allow_paused):
                        atomic_json(directory/'producer-status.json',dict(stage='stopped',last_ready=last_ready,target=a.target,pid=os.getpid()))
                        return
                    jobs=[];indices=[]
                    for _ in range(min(32, max(1,(required-len(accepted))*(1 if kind=='human' else 5)))):
                        if len(chosen)==len(population):break
                        i=population[rng.randrange(len(population))]
                        while i in chosen:i=population[rng.randrange(len(population))]
                        chosen.add(i)
                        source_id='human:'+source.human[i]['case_id'] if kind=='human' else 'generated:'+str(i)
                        if source_id in existing:duplicates+=1;continue
                        indices.append(i);jobs.append((kind,source.human[i] if kind=='human' else i))
                    if not jobs:
                        if len(chosen)==len(population):raise RuntimeError('Insufficient distinct supported boss starts: '+kind)
                        continue
                    candidates=[]
                    for i,(row,error) in zip(indices,source.pool.map(convert,jobs,chunksize=1)):
                        attempted+=1
                        if error:rejects.append(dict(stage='conversion',source_kind=kind,index=i,error=error));continue
                        if classify(row,native_registry)!='boss':nonboss+=1;continue
                        if len(row['wire']['player'].get('potions',[]))>5:
                            rejects.append(dict(stage='capacity',source=row['source'],reason='native potion capacity is five'));continue
                        assert source_kind(row)==kind and row['source'] not in existing
                        assert row['metadata'].get('run_hash') not in excluded
                        row['global_template_id']=i+source.total if kind=='human' else i
                        candidates.append(row)
                    accepted.extend(validator.accept(candidates,rejects)[:required-len(accepted)])
                    atomic_json(directory/'producer-status.json',dict(stage='converting',bank=index,source=kind,accepted=len(accepted),requested=required,attempted=attempted,nonboss=nonboss,pid=os.getpid(),seconds=time.perf_counter()-started,target=a.target))
                additions.extend(accepted)
                sampling[kind]=dict(requested=required,accepted=len(accepted),attempted=attempted,nonboss_rejections=nonboss,existing_source_rejections=duplicates,seed=f'{SCHEMA}:20261026:{index}:{kind}')
            counts=verify_additions(base_rows,additions,native_registry,excluded)
            payload=encode(additions);atomic_bytes(path,payload)
            atomic_json(info,dict(schema=SCHEMA,bank=index,catalog_sha256=catalog_hash,base_sha256=digest(base),sha256=digest(payload),merged_sha256=digest(base+payload),accepted=len(additions),counts=counts,sampling=sampling,rejected=rejects,added_sources=[r['source'] for r in additions],seconds=time.perf_counter()-started))
            last_ready=index
            atomic_json(directory/'producer-status.json',dict(stage='ready',last_ready=index,target=a.target,pid=os.getpid(),counts=counts,seconds=time.perf_counter()-started))
            print(json.dumps(dict(bank=index,counts=counts,seconds=time.perf_counter()-started)),flush=True)
        atomic_json(directory/'producer-status.json',dict(stage='complete' if last_ready==a.target-1 else 'stopped',last_ready=last_ready,target=a.target,pid=os.getpid()))
    finally:source.close()

if __name__=='__main__':main()
