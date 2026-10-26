"""Select the first 512 fully reconstructable runs in a fixed hash ranking."""
from pathlib import Path
import sys,json,pickle,hashlib,os,time,multiprocessing,ctypes as C
from collections import defaultdict
from concurrent.futures import ProcessPoolExecutor
import build
ROOT=build.ROOT;OUT=build.OUT

def main():
    idx=json.loads((OUT/'index.json').read_text());assert idx['eligible_runs']>512
    jobs=pickle.loads((OUT/'jobs.pkl').read_bytes());groups=defaultdict(list)
    for j in jobs:groups[j['case']['run_hash']].append(j)
    ordered=sorted(idx['runs'],key=lambda r:hashlib.sha256(('human-win-subset:20261025:'+r['run_hash']).encode()).hexdigest())
    candidates=OUT/'candidates';candidates.mkdir(exist_ok=True)
    lib=C.CDLL(str(ROOT/'loop_native.so'));lib.sm_loop_create.argtypes=[C.c_char_p];lib.sm_loop_create.restype=C.c_void_p;lib.sm_loop_destroy.argtypes=[C.c_void_p];lib.sm_loop_error.restype=C.c_char_p
    selected=[];rejected=[];started=time.time()
    with ProcessPoolExecutor(8,mp_context=multiprocessing.get_context('spawn'),initializer=build.init) as pool:
        for run in ordered:
            h=run['run_hash'];items=groups[h];p=candidates/(h+'.jsonl');info=p.with_suffix('.json')
            if info.exists():m=json.loads(info.read_text())
            else:
                rows=[];errors=[]
                if len(items)!=len(run['case_ids']):errors.append(dict(error='missing reconstructed combat'))
                if not errors:
                    for row,error in pool.map(build.convert,[(item,rep) for item in items for rep in range(3)],chunksize=8):
                        if error:errors.append(error)
                        else:rows.append(row)
                if not errors:
                    raw=''.join(json.dumps(row,separators=(',',':'))+'\n' for row in rows).encode();p.write_bytes(raw)
                    assert len({(row['metadata']['case_id'],row['metadata']['replicate']) for row in rows})==len(items)*3
                    for item in items:
                        rs=[row for row in rows if row['metadata']['case_id']==item['case']['case_id']]
                        assert len({row['metadata']['environment_seed'] for row in rs})==3
                    cfg=dict(fixtures=str(p),schema=str(ROOT/'schema'),cases=len(rows),workers=1,batch_size=1,buffers=2,row_capacity=4096,max_actions=1000,simulations=64,record_replay=False,record_actions=False,fixture_potion_costs=True,fixed_potion_cost=1.0,reward_shaping=True)
                    handle=lib.sm_loop_create(json.dumps(cfg).encode())
                    if handle:lib.sm_loop_destroy(handle)
                    else:errors.append(dict(error=lib.sm_loop_error().decode(),stage='native_preflight'))
                m=dict(run_hash=h,accepted=not errors,errors=errors,combats=len(items),trials=len(rows),sha256=hashlib.sha256(p.read_bytes()).hexdigest() if p.exists() else None)
                info.write_text(json.dumps(m,indent=2))
            if m['accepted']:selected.append(run)
            else:rejected.append(m)
            status=dict(stage='selecting_complete_runs',selected=len(selected),rejected=len(rejected),target=512,seconds=time.time()-started)
            (OUT/'build-status.json').write_text(json.dumps(status));print(json.dumps(status),flush=True)
            if len(selected)==512:break
    assert len(selected)==512,'Insufficient complete supported runs'
    selected.sort(key=lambda r:r['run_hash'])
    final=OUT/'frozen';final.mkdir(exist_ok=True);shards=[];pending=[]
    def flush():
        if not pending:return
        p=final/f'part-{len(shards):05d}.jsonl';raw=''.join(pending).encode();p.write_bytes(raw)
        shards.append(dict(file=str(p.relative_to(OUT)),accepted=len(pending),sha256=hashlib.sha256(raw).hexdigest()));pending.clear()
    for run in selected:
        for line in (candidates/(run['run_hash']+'.jsonl')).read_text().splitlines(True):
            pending.append(line)
            if len(pending)==768:flush()
    flush()
    n=sum(len(run['case_ids']) for run in selected)
    manifest=dict(version=2,runs=selected,eligible_runs=512,all_source_eligible_runs=idx['eligible_runs'],expected_combats=n,requested_trials=3*n,supported_trials=3*n,missing_combats=[],conversion_errors=[],compatibility_rejected_runs=rejected,shards=shards,index_sha256=hashlib.sha256((OUT/'index.json').read_bytes()).hexdigest(),seed_scheme='sha256 human-winners-v1:20261025:case_id:replicate; fixed independent 32-bit starting RNG seeds',replicates=3,majority_wins=2,selection='First 512 complete native-compatible runs in SHA256 human-win-subset:20261025:run_hash order. No model outcomes used.',potion_rule='cost 0 for disappearance or explicit single-room use; cost 1 for retained and generated opening potions; final inventory for last combat',training_overlap_allowed=True)
    tmp=OUT/'manifest.tmp';tmp.write_text(json.dumps(manifest,indent=2));os.replace(tmp,OUT/'manifest.json')
    (OUT/'build-status.json').write_text(json.dumps(dict(stage='ready',runs=512,combats=n,trials=n*3,rejected_runs=len(rejected))))
    print('READY',512,n,n*3,flush=True)
if __name__=='__main__':main()
