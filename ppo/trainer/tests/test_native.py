import ctypes as C,json,sys,time
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from run_workers import RunWorkers
from operations import atomic_json
from worker import R

def equal(a,b):
    assert np.array_equal(a.lengths,b.lengths) and np.array_equal(a.counts,b.counts) and np.array_equal(a.private_lengths,b.private_lengths) and np.array_equal(a.metadata,b.metadata)
    for i in range(a.n):
        np.testing.assert_array_equal(a.raw[i,:a.lengths[i]],b.raw[i,:b.lengths[i]])
        np.testing.assert_array_equal(a.private[i,:a.private_lengths[i]],b.private[i,:b.private_lengths[i]])

def api(w,name,args):
    f=getattr(w.lib,'sm_rv_test_'+name);f.argtypes=[C.c_void_p]+args;f.restype=C.c_int
    return lambda *x:w.check(f(w.pool,*x))

def main():
    start=time.perf_counter();a=RunWorkers(32,1,pinned=False);b=RunWorkers(32,80,pinned=False);rng=np.random.default_rng(37);frames=0
    try:
        for w in [a,b]:w.refill(2048);w.observe_all()
        for _ in range(256):
            equal(a,b);actions=np.array([rng.integers(c) for c in a.counts],np.int32)
            for w in [a,b]:w.advance(actions)
            np.testing.assert_array_equal(a.events,b.events)
            for w in [a,b]:w.reset();w.observe_all()
            frames+=a.n
        j=a.save_journal();before=a.raw.copy();private=a.private.copy();a.load_journal(j);a.observe_all();assert j==a.save_journal();equal(a,b)
        trace=dict(frames=frames,one_vs_eighty_workers_arrays_and_ordered_classes=True,journal_exact=True,counts_min=int(a.counts.min()))
    finally:a.close();b.close()
    w=RunWorkers(1,1,pinned=False,training=False);fixture=api(w,'fixture',[C.c_size_t,C.c_size_t]);resolve=api(w,'resolve',[C.c_size_t,C.c_bool]);cap=api(w,'cap',[C.c_size_t,C.c_uint]);intervene=api(w,'intervention',[C.c_size_t]);dedup=api(w,'dedup',[C.c_size_t,C.c_bool])
    try:
        cells=[]
        for cell in range(6):
            fixture(0,cell);w.observe_all();s=w.info()['slots'][0];assert s['lane']==1 and s['phase']==2 and not s['segment_done'];resolve(0,True);w.observe_all();s=w.info()['slots'][0]
            if cell==5:
                assert not s['segment_done'] and s['phase']==2,'Double boss stopped after boss1'
                resolve(0,True);w.observe_all();s=w.info()['slots'][0];assert s['terminal'] and s['won'] and not s['bootstrap']
            else:assert s['segment_done'] and s['bootstrap'] and not s['terminal'] and s['phase']!=2
            cells.append(dict(cell=cell,post_win=s))
            fixture(0,cell);resolve(0,False);s=w.info()['slots'][0];assert s['terminal'] and not s['won'] and not s['bootstrap']
        for kind in range(3):
            fixture(0,0);cap(0,kind);s=w.info()['slots'][0];assert s['terminal'] and s['cap'] and not s['bootstrap'] and not s['won']
        fixture(0,0);w.observe_all();public=w.raw[0,:w.lengths[0]].copy();private=w.private[0,:w.private_lengths[0]].copy();intervene(0);w.observe_all();np.testing.assert_array_equal(public,w.raw[0,:w.lengths[0]]);assert not np.array_equal(private,w.private[0,:w.private_lengths[0]])
        fixture(0,0);dedup(0,False);w.observe_all();same=int(w.counts[0]);dedup(0,True);w.observe_all();different=int(w.counts[0]);assert different>same,'Per-copy modifications were incorrectly merged'
        result=dict(passed=True,synthetic_witnesses_not_training_rows=True,parity=trace,six_cells=cells,loss_and_three_cap_kinds_zero_bootstrap=True,public_private_intervention=True,card_dedup=dict(same=same,different=different),seconds=time.perf_counter()-start)
        atomic_json(R/'evidence/native-tests.json',result);print(json.dumps(dict(passed=True,seconds=result['seconds'],parity=trace,cells=6,dedup=result['card_dedup'])))
    finally:w.close()
if __name__=='__main__':main()
