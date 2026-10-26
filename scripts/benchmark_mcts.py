"""Run upstream deployed MCTS over the complete frozen human panel."""
import argparse,concurrent.futures,gzip,hashlib,importlib.util,json,os,sys,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'ostyai'))
from human_validation_metrics import summarize

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def save(p,obj):
    tmp=p.with_suffix(p.suffix+'.tmp');tmp.write_text(json.dumps(obj,indent=2)+'\n');tmp.replace(p)

def main():
    start=time.perf_counter();wall_start=time.time()
    ap=argparse.ArgumentParser();ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--prepared',type=Path,required=True);ap.add_argument('--workers',type=int,default=80);ap.add_argument('--simulations',type=int)
    ap.add_argument('--rollout-depth',type=int,default=200);ap.add_argument('--limit',type=int)
    a=ap.parse_args();assert a.workers==80,'Comparison requires 80 workers'
    assert a.rollout_depth>0 and (a.simulations is None or a.simulations>0)
    a.output.mkdir(parents=True,exist_ok=True)
    fixture_root=a.prepared;mp=fixture_root/'manifest.json';manifest=json.loads(mp.read_text());prepared_manifest=json.loads((fixture_root/'prepared-manifest.json').read_text())
    lib=ROOT/'ostyai/loop_native.so';spec=importlib.util.spec_from_file_location('sts2_rs',lib);native=importlib.util.module_from_spec(spec);spec.loader.exec_module(native)
    encoder=native.AdversarialEncoder((ROOT/'ostyai/schema/registry.json').read_text(),(ROOT/'ostyai/schema/metadata.json').read_text())
    provenance=dict(mode='mentor_eval_cheap' if a.simulations is None else 'mentor_fixed_budget',workers=a.workers,simulations_per_decision=a.simulations,
        default_tier_budgets=dict(common=1000,elite=1000,boss=4000),max_rollout_depth=a.rollout_depth,max_actions=1000,
        hidden_info='BeliefDraw',leaf='TexelWide',terminal_eval='RichBoth',rollout_policy='Heuristic',root_guards=True,
        objective='Unmodified upstream evaluator; frozen potion costs are recorded but are not an OstyAI reward override',
        model=None,manifest_sha256=sha(mp),prepared_manifest_sha256=sha(fixture_root/'prepared-manifest.json'),native_sha256=sha(lib),search_seed_source='frozen fixture wire.rng.seed; splitmix64 per decision',limit=a.limit)
    pp=a.output/'provenance.json'
    if pp.exists():assert json.loads(pp.read_text())==provenance,'Provenance mismatch'
    else:save(pp,provenance)
    if (a.output/'summary.json').exists():raise SystemExit('Completed benchmark exists; choose a new output')
    journal=a.output/'trials.jsonl';results=[]
    if journal.exists():
        for line in journal.read_text().splitlines():results.append(json.loads(line))
    done={(r['case_id'],r['replicate'])for r in results};assert len(done)==len(results)
    def fixtures():
        count=0
        for part in manifest['shards']:
            data=(fixture_root/part['file']).read_bytes();assert hashlib.sha256(data).hexdigest()==prepared_manifest[part['file']]
            for raw in data.splitlines():
                if a.limit is not None and count>=a.limit:return
                count+=1;row=json.loads(raw);key=(row['metadata']['case_id'],row['metadata']['replicate'])
                if key not in done:yield raw.decode(),row
    def play(item):
        raw,fixture=item
        batch=native.AdversarialCombatBatch(encoder,[native.state_from_dict(fixture['wire'])],[json.dumps(fixture['context'])],max_actions=1000,workers=1)
        batch.check_invariants()
        r=json.loads(batch.play_default_mcts(0,int(fixture['wire']['rng']['seed']),max_rollout_depth=a.rollout_depth,simulations_per_decision=a.simulations))
        assert r['rollout_depth']==a.rollout_depth and r['simulation_cap'] is None
        if a.simulations is not None:assert r['requested_simulations']==a.simulations*r['searches']
        r.update(fixture['metadata']);r['potion_costs']=fixture['evaluation']['potion_costs']
        r['fixture_sha256']=hashlib.sha256(raw.encode()).hexdigest()
        return r
    iterator=iter(fixtures());last=time.perf_counter();requested=a.limit or manifest['requested_trials']
    with journal.open('a',buffering=1)as stream,concurrent.futures.ThreadPoolExecutor(max_workers=a.workers)as pool:
        pending={}
        def submit():
            try:item=next(iterator)
            except StopIteration:return False
            pending[pool.submit(play,item)]=(item[1]['metadata'],time.time());return True
        for _ in range(a.workers*2):
            if not submit():break
        while pending:
            finished,_=concurrent.futures.wait(pending,timeout=5,return_when=concurrent.futures.FIRST_COMPLETED)
            for future in finished:
                identity,begun=pending.pop(future)
                try:r=future.result()
                except BaseException as e:
                    save(a.output/'failure.json',dict(metadata=identity,error=repr(e),time=time.time()));raise
                stream.write(json.dumps(r)+'\n');results.append(r);submit()
            if time.perf_counter()-last>=10 or not pending:
                stream.flush();os.fsync(stream.fileno());last=time.perf_counter()
                save(a.output/'status.json',dict(stage='mcts',completed_trials=len(results),requested_trials=requested,elapsed_seconds=last-start,workers=a.workers,
                    longest_submitted_age_seconds=max([time.time()-t for _,t in pending.values()]or[0]),time=time.time()))
                print(json.dumps(dict(completed=len(results),requested=requested,seconds=last-start)),flush=True)
    assert len(results)==requested
    summary,combats,runs,trials=summarize(manifest,results)
    summary.update(provenance,partial_smoke=a.limit is not None,wall_seconds=time.perf_counter()-start,started_at_unix=wall_start,finished_at_unix=time.time(),
        summed_combat_seconds=sum(r['seconds']for r in results),fresh_process=len(done)==0,resumed_trials=len(done),executed_trials=len(results))
    for name,value in [('combats',combats),('runs',runs),('trials',trials),('summary',summary)]:save(a.output/(name+'.json'),value)
    save(a.output/'status.json',dict(stage='complete',completed_trials=len(results),time=time.time()))
    print(json.dumps({k:summary[k]for k in ['trial_win_rate','win_rate','run_win_rate','wall_seconds','unresolved_trials']}),flush=True)
if __name__=='__main__':main()
