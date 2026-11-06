import sys,json,hashlib
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import torch,numpy as np
from model import ActorCritic,Config
from learner import Learner
from returns import Returns
from worker import R
from operations import atomic_json,sha

def eq(a,b):
    if torch.is_tensor(a):return torch.equal(a.cpu(),b.cpu())
    if isinstance(a,np.ndarray):return np.array_equal(a,b)
    if isinstance(a,dict):return a.keys()==b.keys() and all(eq(a[k],b[k]) for k in a)
    if isinstance(a,(list,tuple)):return len(a)==len(b) and all(eq(x,y) for x,y in zip(a,b))
    if hasattr(a,'__dict__'):return eq(vars(a),vars(b))
    return a==b

def finite(a):
    if torch.is_tensor(a):return not a.is_floating_point() or bool(torch.isfinite(a).all())
    if isinstance(a,np.ndarray):return bool(np.isfinite(a).all())
    if isinstance(a,dict):return all(finite(v) for v in a.values())
    if isinstance(a,(list,tuple)):return all(map(finite,a))
    if hasattr(a,'__dict__'):return finite(vars(a))
    return True

def main():
    path=Path(sys.argv[1]);c=torch.load(path,map_location='cpu',weights_only=False);torch.set_num_threads(1)
    model=ActorCritic(Config(**c['config']['architecture']));model.load_state_dict(c['model']);l=Learner(model,c['config'],device='cpu');l.optimizer.load_state_dict(c['optimizer']);l.load_state_dict(c['learner']);ret=Returns(c['config']);ret.load_state_dict(c['returns'])
    result=dict(model_exact=eq(model.state_dict(),c['model']),optimizer_exact=eq(l.optimizer.state_dict(),c['optimizer']),learner_ema_moments_lr_exact=eq(l.state_dict(),c['learner']),held_ooc_exact=eq(ret.state_dict(),c['returns']),model_optimizer_ema_returns_finite=finite([c['model'],c['optimizer'],c['learner'],c['returns']]),all_rng_fields_present=all(k in c for k in ['numpy_rng','python_rng','torch_rng','cuda_rng']),config_matches=json.loads((R/'config.json').read_text())==c['config'],native_matches=sha(R/'loop_native.so')==c['provenance']['native_sha256'])
    assert all(result.values()),result
    result.update(passed=True,checkpoint=str(path),sha256=sha(path),updates=c['learner']['updates'],optimizer_steps=c['learner']['optimizer_steps'],held_ooc=sum(map(len,ret.pending.values())))
    atomic_json(R/'evidence/checkpoint-verification.json',result);print(json.dumps(result))
if __name__=='__main__':main()
