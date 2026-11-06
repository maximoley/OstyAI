"""Same rollout contract, without all-slot boundary polling or JSON state dumps."""
import collections,time
import numpy as np
from returns import Row
from collect import values_only

def collect(w,actor,cfg,version,status=lambda *a,**k:None,should_stop=lambda:False):
    assert cfg['rollout_steps']%w.n==0
    rows=[];completed=[];stages=collections.Counter();start=time.perf_counter();last_status=start;weights=np.array([cfg['rewards'][k] for k in ['floor','elite','boss','run_win']],np.float32)
    for tick in range(cfg['rollout_steps']//w.n):
        if should_stop():break
        t=time.perf_counter();w.observe_all();stages['encode_seconds']+=time.perf_counter()-t
        assert (w.counts>=2).all() and not w.metadata[:,2].any(),'Forced/boundary state entered the trainable batch'
        t=time.perf_counter();actions,logp,values=actor.run(w);stages['inference_seconds']+=time.perf_counter()-t
        if should_stop():break
        t=time.perf_counter();part=[]
        for i in range(w.n):
            part.append(Row(w.raw[i,:w.lengths[i]].copy(),w.private[i,:w.private_lengths[i]].copy(),int(actions[i]),float(logp[i]),float(values[i]),0.,int(w.metadata[i,1]),int(w.metadata[i,0]),bool(w.raw[i,0,8]),version))
        stages['pack_seconds']+=time.perf_counter()-t
        if should_stop():break
        t=time.perf_counter();events=w.advance(actions).copy();stages['transition_seconds']+=time.perf_counter()-t
        ended=np.flatnonzero(events[:,4])
        if len(ended):
            t=time.perf_counter();boundary=np.flatnonzero(w.metadata[:,4]);value_map={}
            if len(boundary):
                view=w.observe_subset(boundary);bv=values_only(actor,view);value_map=dict(zip(map(int,boundary),map(float,bv)))
            for i in ended:
                r=part[int(i)];r.segment_end=True;r.terminal=bool(w.metadata[i,3]);r.boundary_value=value_map.get(int(i),0.)
                completed.append(dict(w.completed_outcome(int(i)),version=version))
            stages['boundary_bootstrap_and_logging_seconds']+=time.perf_counter()-t
        rewards=events[:,:4]@weights
        for i,r in enumerate(part):r.reward=float(rewards[i])
        rows.extend(part)
        t=time.perf_counter();w.reset();stages['reset_seconds']+=time.perf_counter()-t
        if time.perf_counter()-last_status>=2.:
            status('collection',rollout_tick=tick,horizon=cfg['rollout_steps']//w.n,elapsed=time.perf_counter()-start);last_status=time.perf_counter()
    complete=len(rows)==cfg['rollout_steps'] and not should_stop()
    if complete:
        t=time.perf_counter();w.observe_all();v=values_only(actor,w);tails={int(w.metadata[i,1]):(float(v[i]),bool(w.raw[i,0,8])) for i in range(w.n)};stages['tail_bootstrap_seconds']+=time.perf_counter()-t
    else:tails={}
    return rows,tails,completed,dict(complete=complete,interrupted=not complete,expected_rows=cfg['rollout_steps'],seconds=time.perf_counter()-start,stages=dict(stages),rows=len(rows),combat_rows=sum(r.combat for r in rows),practice_rows=sum(r.lane==1 for r in rows),reward_sum=float(sum(r.reward for r in rows)))
