"""Frozen greedy full-run validation under the revised decision/cap contract."""
import random,json,time
import numpy as np
import torch
from run_workers import RunWorkers
from worker import R
from operations import atomic_json,append_json,sha,tensor_hash

def evaluate(model,actor,output,update,cfg,provenance,status,stopping,seeds_override=None):
    seeds=json.loads((R/'validation-seeds.json').read_text()) if seeds_override is None else seeds_override
    assert len(seeds)==len(set(seeds)) and min(seeds)>=2**63
    vc=cfg['validation'];root=output/f'validation-{update:06d}';root.mkdir(exist_ok=True)
    frozen=dict(character=cfg['environment']['character'],update=update,model_tensor_sha256=tensor_hash(model),native_sha256=provenance['native_sha256'],seed_file_sha256=sha(R/'validation-seeds.json'),seeds=seeds,config=vc,weights=cfg['evaluation_weights'],protocol=cfg['protocol'],schema_sha256=provenance['schema_sha256'])
    pp=root/'provenance.json'
    if pp.exists():assert json.loads(pp.read_text())==frozen,'Frozen validation provenance mismatch'
    else:atomic_json(pp,frozen)
    saved=(random.getstate(),np.random.get_state(),torch.get_rng_state(),torch.cuda.get_rng_state());was_training=model.training;begin=time.time();rows=[];cached=0
    try:
        for part,start in enumerate(range(0,len(seeds),vc['slots'])):
            chosen=seeds[start:start+vc['slots']];assert chosen==list(range(chosen[0],chosen[0]+len(chosen)))
            p=root/f'part-{part:03d}.json'
            if p.exists():
                d=json.loads(p.read_text());assert d['actor_hash']==frozen['model_tensor_sha256'] and [r['seed'] for r in d['runs']]==chosen;rows+=d['runs'];cached+=1;continue
            w=RunWorkers(len(chosen),vc['workers'],chosen[0],pinned=True,training=False,character=cfg['environment']['character']);out={};begun=time.perf_counter()
            try:
                for tick in range(vc['max_decisions_per_run']):
                    if stopping():return None
                    w.observe_all();a,_,_=actor.run(w)
                    if stopping():return None
                    active=~w.metadata[:,2].astype(bool);a[~active]=-1;events=w.advance(a)
                    if events[:,4].any():
                        states=w.info()['slots']
                        for i in np.flatnonzero(events[:,4]):
                            r=states[int(i)];assert r['terminal'];out[int(i)]=dict(r,outcome='win' if r['won'] else 'loss')
                    if tick%16==0:status('validation',validation_update=update,part=part,completed_runs=len(rows)+len(out),total_runs=len(seeds),part_tick=tick)
                    if len(out)==w.n:break
                assert len(out)==w.n,'Native 6000-meaningful-decision cap failed'
                part_rows=[out[i] for i in range(w.n)];atomic_json(p,dict(actor_hash=frozen['model_tensor_sha256'],runs=part_rows,seconds=time.perf_counter()-begun));rows+=part_rows
            finally:w.close()
        assert [r['seed'] for r in rows]==seeds
        wins=sum(r['won'] for r in rows);summary=dict(complete=True,character=cfg['environment']['character'],protocol=cfg['protocol'],update=update,weights=cfg['evaluation_weights'],runs=len(rows),wins=wins,losses=len(rows)-wins,unresolved=0,win_rate=wins/len(rows),mean_floor=float(np.mean([r['floor'] for r in rows])),mean_act=float(np.mean([r['act'] for r in rows])),caps=sum(r['cap'] for r in rows),seconds_this_attempt=time.time()-begin,cached_parts_reused=cached,full_fresh_timing=cached==0,actor_hash=frozen['model_tensor_sha256'],simulations=0,selection='legal_argmax',completed_at=time.time())
        atomic_json(root/'runs.json',rows);atomic_json(root/'summary.json',summary)
        history=output/'validation.jsonl';known={json.loads(x)['update'] for x in history.read_text().splitlines()} if history.exists() else set()
        if update not in known:append_json(history,summary)
        return summary
    finally:
        random.setstate(saved[0]);np.random.set_state(saved[1]);torch.set_rng_state(saved[2]);torch.cuda.set_rng_state(saved[3]);model.train(was_training)
        assert tensor_hash(model)==frozen['model_tensor_sha256']
