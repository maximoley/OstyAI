"""Verify the safe actor export against both historical fingerprint formats."""
import hashlib,json,sys
from pathlib import Path
import torch
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'ostyai'))
from model import Config,CombatTransformer
p=ROOT/'models/best/model.pt';m=json.loads(p.with_name('metadata.json').read_text())
assert hashlib.sha256(p.read_bytes()).hexdigest()==m['model_file_sha256']
c=torch.load(p,map_location='cpu',weights_only=True)
model=CombatTransformer(Config(**c['config']));model.load_state_dict(c['state_dict'],strict=True)
a=hashlib.sha256();b=hashlib.sha256()
for k,v in sorted(c['state_dict'].items()):
    assert torch.isfinite(v).all(),k
    a.update(k.encode());a.update(v.numpy().tobytes())
    b.update(f"{k}:{v.dtype}:{tuple(v.shape)}".encode());b.update(v.view(torch.uint8).numpy().tobytes())
assert a.hexdigest()==m['actor_sha256']
if 'actor_sha256' in c: assert c['actor_sha256']==a.hexdigest()
pv=json.loads((ROOT/f"benchmarks/validation-{m['step']:06d}/provenance.json").read_text())
assert b.hexdigest()==pv['model_sha256']==m['primary_actor_sha256']
for name in ['model6500_boss128','model6500_boss256','model6500_policy_greedy']:
    human=json.loads((ROOT/'benchmarks/comparison/step6500'/name/'provenance.json').read_text())
    assert a.hexdigest()==human['model_sha256'] and human['update']==m['step']
assert sum(v.numel()for v in model.parameters())==m['parameters']==1593346
assert c['step']==m['step']==6500 and c['config']==m['config']
print(json.dumps(dict(step=c['step'],parameters=m['parameters'],human_actor_hash=a.hexdigest(),primary_actor_hash=b.hexdigest(),finite=True,strict_load=True),indent=2))
