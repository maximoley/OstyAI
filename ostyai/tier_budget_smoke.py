"""Mixed-pool native search and both validation-path deployment checks."""
import json,hashlib,collections
from pathlib import Path
import torch
from production_train import ROOT,config,collect,SEARCH_BUDGETS
from loop import Native,GPU
from secondary_validation import evaluate
from corpus_stream import atomic_json

def category(row):
 m=row['metadata'];name=m.get('encounter','').upper();room=m.get('room_type','')
 if name.endswith('_BOSS') or room=='boss':return 'boss'
 if name.endswith('_ELITE') or room=='elite':return 'elite'
 if name.endswith('_WEAK') or room in ('weak','easy'):return 'easy'
 return 'hard'

def pick(file,count=4):
 buckets=collections.defaultdict(list)
 for line in Path(file).open():
  row=json.loads(line);tier=category(row)
  if len(buckets[tier])<count:buckets[tier].append(row)
  if all(len(buckets[t])==count for t in SEARCH_BUDGETS):break
 assert all(len(buckets[t])==count for t in SEARCH_BUDGETS)
 return [r for t in SEARCH_BUDGETS for r in buckets[t]]

def main():
 torch.set_num_threads(1);torch.set_grad_enabled(False);torch.set_float32_matmul_precision('high')
 out=ROOT/'changes-20261025-tier-budgets';native=Native();gpu=GPU()
 ck=torch.load(out/'paused.pt',map_location='cpu',weights_only=False);gpu.model.load_state_dict(ck['actor'])
 results={}
 for mode,source in [('training',ROOT/'prepared/bank-004107.jsonl'),('primary',ROOT/'validation/fixtures.jsonl')]:
  rows=pick(source);path=out/(mode+'-fixtures.jsonl');path.write_text(''.join(json.dumps(r)+'\n'for r in rows))
  cfg=config(path,len(rows),4108,4107,mode!='training')
  rep=collect(native,gpu,cfg);assert rep['completed']==len(rows)
  assert set(rep['search_by_tier'])==set(SEARCH_BUDGETS)
  for t,v in rep['search_by_tier'].items():assert v['simulations']==SEARCH_BUDGETS[t]*v['searched_decisions'] and v['searched_decisions']>0
  atomic_json(out/(mode+'-report.json'),rep);results[mode]=dict(search_by_tier=rep['search_by_tier'],forced_search_transitions=rep['forced_search_transitions'],seconds=rep['total_seconds'])
  print(mode,json.dumps(results[mode]),flush=True)
 # Exercise the actual secondary evaluator, potion overrides and resumable cache.
 source=ROOT/'human-win-validation';manifest=json.loads((source/'manifest.json').read_text());run=manifest['runs'][0]
 rows=[]
 for shard in manifest['shards']:
  for line in (source/shard['file']).open():
   row=json.loads(line)
   if row['metadata']['run_hash']==run['run_hash']:rows.append(row)
  if len(rows)==len(run['case_ids'])*3:break
 assert len(rows)==len(run['case_ids'])*3
 fixtures=out/'secondary-fixtures';fixtures.mkdir(exist_ok=True);path=fixtures/'cases.jsonl';path.write_text(''.join(json.dumps(r)+'\n'for r in rows))
 m=dict(runs=[run],requested_trials=len(rows),expected_combats=len(run['case_ids']),conversion_errors=[],shards=[dict(file='cases.jsonl',sha256=hashlib.sha256(path.read_bytes()).hexdigest())])
 atomic_json(fixtures/'manifest.json',m)
 result=evaluate(native,gpu,out,4107,collect,config,fixture_root=fixtures)
 assert result['simulations']==SEARCH_BUDGETS and result['trials']==len(rows)
 repeated=evaluate(native,gpu,out,4107,lambda *a,**kw:(_ for _ in ()).throw(AssertionError('cache missed')),config,fixture_root=fixtures)
 assert repeated==result
 results['secondary']=dict(trials=result['trials'],cache_resume_verified=True,budgets=result['simulations'])
 results['native_sha256']=hashlib.sha256((ROOT/'loop_native.so').read_bytes()).hexdigest()
 atomic_json(out/'smoke-results.json',results);print('ALL_TIER_BUDGET_SMOKE_CHECKS_PASSED',flush=True)
if __name__=='__main__':main()
