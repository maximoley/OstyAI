import sys,json,ctypes as C
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import torch,numpy as np
from model import ActorCritic,Config
from run_workers import RunWorkers
from worker import R
from operations import atomic_json

def main():
    torch.set_num_threads(1);torch.manual_seed(91);m=ActorCritic(Config()).eval();w=RunWorkers(6,2,pinned=False,training=False)
    fixture=w.lib.sm_rv_test_fixture;fixture.argtypes=[C.c_void_p,C.c_size_t,C.c_size_t];fixture.restype=C.c_int
    try:
        for i in range(6):w.check(fixture(w.pool,i,i))
        w.observe_all();t=int(w.lengths.max());p=int(w.private_lengths.max());a=int(w.counts.max());raw=torch.from_numpy(w.raw[:,:t].copy());priv=torch.from_numpy(w.private[:,:p].copy());alt=priv.clone();alt[:,1:]=alt[:,1:].flip(1)
        with torch.no_grad():l,v,mask=m(raw,priv,a);l2,v2,mask2=m(raw,alt,a);actor,legal=m.actor(raw,a)
        assert torch.equal(l,l2) and torch.equal(l,actor) and torch.equal(mask,legal)
        assert not torch.equal(v,v2) and torch.isfinite(v).all() and torch.isfinite(l[mask]).all()
        assert torch.equal(mask.sum(1),torch.from_numpy(w.counts.astype(np.int64)))
        logits,_,_=m(raw,priv,a);m.zero_grad();logits[mask].sum().backward();assert all(p.grad is None for p in m.privileged.parameters())
        # Both value projections are FP32 even when the public trunk autocasts.
        with torch.no_grad():summary=m.encode(raw)[0][:,0]
        with torch.no_grad(),torch.autocast('cpu',dtype=torch.bfloat16):bfv=m.critic(raw,summary,priv)
        assert bfv.dtype==torch.float32
        r=dict(passed=True,actor_logits_bitwise_private_invariant=True,actor_no_privileged_gradient=True,critic_changes_with_private_information=True,actor_api_matches_full_forward=True,value_projection_fp32=True,all_legal_logits_finite=True,parameters=m.describe())
        atomic_json(R/'evidence/model-tests.json',r);print(json.dumps(r))
    finally:w.close()
if __name__=='__main__':main()
