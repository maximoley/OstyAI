"""Inject expiry after one real native batch; exercise the production save path."""
import sys,json
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import train_production as tp
from operations import atomic_json,tensor_hash
expired=[False];original_collect=tp.collect
def collect_one(w,actor,cfg,version,status,should_stop):
 original=w.advance
 def advance(actions):
  events=original(actions);expired[0]=True;return events
 w.advance=advance
 try:return original_collect(w,actor,cfg,version,status,should_stop=should_stop)
 finally:w.advance=original
tp.deadline_reached=lambda cfg:expired[0]
tp.collect=collect_one
sys.argv=['train_production.py','--output','deadline-smoke']
tp.main()
p=tp.R/'deadline-smoke';c=tp.torch.load(p/'latest.pt',weights_only=False,map_location='cpu');initial=json.loads((p/'initialization.json').read_text())
r=dict(test_only_injected_deadline_after_first_native_batch=True,update=c['learner']['updates'],optimizer_steps=c['learner']['optimizer_steps'],pending_rows=len(c['pending_collection']['rows']),collected=c['runtime']['collected_decisions'],learner_collected=c['learner']['decisions'],stop_reason=c['runtime']['stop_reason'],model_unchanged=c['model_tensor_sha256']==initial['model_tensor_sha256'],optimizer_untouched=len(c['optimizer']['state'])==0,pending_unfitted=c['pending_collection']['unfitted'],native_journal_present=bool(c['native_journal']),rng_present=all(k in c for k in ['numpy_rng','python_rng','torch_rng','cuda_rng']))
assert r['update']==r['optimizer_steps']==r['learner_collected']==0 and r['pending_rows']==r['collected']==512
assert all(r[k] for k in ['model_unchanged','optimizer_untouched','pending_unfitted','native_journal_present','rng_present']) and r['stop_reason']=='wall_deadline'
r['passed']=True;atomic_json(tp.R/'evidence/deadline-integration.json',r);print(json.dumps(r))
