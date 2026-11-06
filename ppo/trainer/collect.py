"""Collect exactly 32768 meaningful choices; forced transitions stay in Rust."""
from types import SimpleNamespace
import collections,time
import numpy as np
import torch
from returns import Row

@torch.inference_mode()
def values_only(actor,w,ids=None):
    saved=torch.cuda.get_rng_state()
    try:
        if ids is None:_,_,v=actor.run(w);return v
        ids=np.asarray(ids,np.int64)
        sub=SimpleNamespace(n=len(ids),raw=w.raw[ids],private=w.private[ids],lengths=w.lengths[ids],counts=w.counts[ids],private_lengths=w.private_lengths[ids])
        return actor.run(sub)[2]
    finally:torch.cuda.set_rng_state(saved)

def collect(w,actor,cfg,version,status=lambda *a,**k:None):
    assert cfg['rollout_steps']%w.n==0
    rows=[];completed=[];stages=collections.Counter();start=time.perf_counter();weights=np.array([cfg['rewards'][k] for k in ['floor','elite','boss','run_win']],np.float32)
    for tick in range(cfg['rollout_steps']//w.n):
        t=time.perf_counter();w.observe_all();stages['encode_seconds']+=time.perf_counter()-t
        assert (w.counts>=2).all() and not w.metadata[:,2].any(),'Forced/boundary state entered the trainable batch'
        t=time.perf_counter();actions,logp,values=actor.run(w);stages['inference_seconds']+=time.perf_counter()-t
        t=time.perf_counter();part=[]
        for i in range(w.n):
            part.append(Row(w.raw[i,:w.lengths[i]].copy(),w.private[i,:w.private_lengths[i]].copy(),int(actions[i]),float(logp[i]),float(values[i]),0.,int(w.metadata[i,1]),int(w.metadata[i,0]),bool(w.raw[i,0,8]),version))
        stages['pack_seconds']+=time.perf_counter()-t
        t=time.perf_counter();events=w.advance(actions).copy();stages['transition_seconds']+=time.perf_counter()-t
        ended=np.flatnonzero(events[:,4]);meta=None
        if len(ended):
            t=time.perf_counter();w.observe_all();meta=w.metadata.copy();boundary=np.flatnonzero(meta[:,4]);bv=values_only(actor,w,boundary) if len(boundary) else []
            value_map=dict(zip(map(int,boundary),map(float,bv)));info=w.info()
            for i in ended:
                r=part[int(i)];r.segment_end=True;r.terminal=bool(meta[i,3]);r.boundary_value=value_map.get(int(i),0.)
                completed.append(dict(info['slots'][int(i)],version=version))
            stages['boundary_bootstrap_and_logging_seconds']+=time.perf_counter()-t
        rewards=events[:,:4]@weights
        for i,r in enumerate(part):r.reward=float(rewards[i])
        rows.extend(part)
        t=time.perf_counter();w.reset();stages['reset_seconds']+=time.perf_counter()-t
        if tick%8==0:status('collection',rollout_tick=tick,horizon=cfg['rollout_steps']//w.n,elapsed=time.perf_counter()-start)
    t=time.perf_counter();w.observe_all();v=values_only(actor,w);tails={int(w.metadata[i,1]):(float(v[i]),bool(w.raw[i,0,8])) for i in range(w.n)};stages['tail_bootstrap_seconds']+=time.perf_counter()-t
    assert len(rows)==cfg['rollout_steps']
    return rows,tails,completed,dict(seconds=time.perf_counter()-start,stages=dict(stages),rows=len(rows),combat_rows=sum(r.combat for r in rows),practice_rows=sum(r.lane==1 for r in rows),reward_sum=float(sum(r.reward for r in rows)))
