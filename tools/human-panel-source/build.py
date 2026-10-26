"""Freeze three independently reconstructed RNG starts for every indexed human combat."""
from pathlib import Path
import sys,json,pickle,hashlib,multiprocessing,time,os
from concurrent.futures import ProcessPoolExecutor
ROOT=Path('/public/maxim/Codex/spire-mentor-az16384-fresh-20261024')
OUT=ROOT/'human-win-validation'
SOURCE=Path('/public/maxim/Codex/spire-mentor-v31-full-policy-ce-fresh-20261023')
def init():
 sys.path[:0]=[str(SOURCE/'native-lib'),str(SOURCE),'/public/maxim/Codex/spire-mentor-structured-tokenizer-20261019']
 from corpus_conversion import setup
 setup()
def convert(job):
 item,rep=job;c=item['case'];cid=c['case_id']
 seed=int.from_bytes(hashlib.sha256(f'human-winners-v1:20261025:{cid}:{rep}'.encode()).digest()[:4],'big')
 try:
  from sts2_env.human_combat_eval.simulation import make_manager
  from fixture_import import fixture
  manager=make_manager(c,seed)
  manager._current_encounter_pool=c['room_type'];manager._current_encounter_id=c['encounter']
  source=dict(index=0,checksum=hashlib.sha256(cid.encode()).hexdigest(),state_file='human:'+cid,act=c['act'],total_floor=c['floor'],enemies=c['monster_ids'],encounter_pool=c['room_type'])
  row,_,_=fixture(manager,source)
  import re
  norm=lambda p:re.sub('[^A-Z0-9]','',str(p).upper())
  available={}
  for potion,cost in zip([p for p in c['potions'] if p],item['potion_costs']):
   available.setdefault(norm(potion),[]).append(cost)
  costs=[]
  for potion in row['wire']['player'].get('potions',[]):
   if potion is None:continue
   matches=available.get(norm(potion),[])
   costs.append(matches.pop(0) if matches else 1.0)
  row['metadata']=dict(case_id=cid,run_hash=c['run_hash'],replicate=rep,environment_seed=seed,act=c['act'],floor=c['floor'],room_index=c['room_index'],room_type=c['room_type'],encounter=c['encounter'],version=c['version'],human_start_hp=c['human_start_hp'],human_end_hp=c['human_end_hp'],reconstruction_notes=c.get('notes',[]))
  row['evaluation']=dict(potion_costs=costs,potion_evidence=item['potion_evidence'])
  return row,None
 except Exception as e:return None,dict(case_id=cid,run_hash=c['run_hash'],replicate=rep,environment_seed=seed,error=repr(e))
def main():
 index=json.loads((OUT/'index.json').read_text());jobs=pickle.loads((OUT/'jobs.pkl').read_bytes())
 out=OUT/'shards';out.mkdir(exist_ok=True)
 expanded=[(item,rep) for item in jobs for rep in range(3)]
 shards=[];errors=[];started=time.time()
 with ProcessPoolExecutor(8,mp_context=multiprocessing.get_context('spawn'),initializer=init) as pool:
  for start in range(0,len(expanded),768):
   j=start//768;p=out/f'part-{j:05d}.jsonl';info=p.with_suffix('.json')
   if info.exists():m=json.loads(info.read_text());assert hashlib.sha256(p.read_bytes()).hexdigest()==m['sha256']
   else:
    rows=[];bad=[]
    for row,error in pool.map(convert,expanded[start:start+768],chunksize=8):
     if error:bad.append(error)
     else:rows.append(row)
    raw=''.join(json.dumps(r,separators=(',',':'))+'\n' for r in rows).encode()
    tmp=p.with_suffix('.tmp');tmp.write_bytes(raw);os.replace(tmp,p)
    m=dict(file=str(p.relative_to(OUT)),attempted=min(768,len(expanded)-start),accepted=len(rows),errors=bad,sha256=hashlib.sha256(raw).hexdigest())
    info.write_text(json.dumps(m,indent=2))
   shards.append(m);errors.extend(m['errors'])
   status=dict(stage='building',shards=len(shards),total_shards=(len(expanded)+767)//768,accepted=sum(x['accepted'] for x in shards),errors=len(errors),seconds=time.time()-started)
   (OUT/'build-status.json').write_text(json.dumps(status));print(json.dumps(status),flush=True)
 manifest=dict(version=1,runs=index['runs'],eligible_runs=index['eligible_runs'],expected_combats=index['expected_combats'],requested_trials=index['expected_combats']*3,supported_trials=sum(x['accepted'] for x in shards),missing_combats=index['missing_combats'],conversion_errors=errors,shards=shards,index_sha256=hashlib.sha256((OUT/'index.json').read_bytes()).hexdigest(),seed_scheme='sha256 human-winners-v1:20261025:case_id:replicate, independent 32-bit seeds frozen in fixtures',replicates=3,majority_wins=2,potion_rule='disappearance plus explicit single-room use; final inventory for last combat; new potions cost 1',training_overlap_allowed=True)
 (OUT/'manifest.json').write_text(json.dumps(manifest,indent=2));(OUT/'build-status.json').write_text(json.dumps(dict(stage='ready',supported_trials=manifest['supported_trials'],errors=len(errors))))
 print('READY',manifest['supported_trials'],len(errors),flush=True)
if __name__=='__main__':main()
