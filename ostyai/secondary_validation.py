"""Resumable human-only validation at every 500 optimizer updates."""
from pathlib import Path
import hashlib,json,time,os
import torch
from corpus_stream import atomic_json
from human_validation_metrics import summarize
ROOT=Path(__file__).resolve().parent

def evaluate(native,gpu,output,update,collect,config,stopping=lambda:False,fixture_root=None,part_limit=None):
    fixtures=Path(fixture_root or ROOT/'human-win-validation')
    while not (fixtures/'manifest.json').exists():
        if stopping():return None
        atomic_json(output/'status.json',dict(stage='waiting_for_human_validation_fixtures',update=update,pid=os.getpid()))
        time.sleep(5)
    raw=(fixtures/'manifest.json').read_bytes();manifest=json.loads(raw)
    directory=output/f'human-validation-{update:06d}';directory.mkdir(exist_ok=True)
    h=hashlib.sha256()
    for name,t in sorted(gpu.model.state_dict().items()):h.update(name.encode());h.update(t.detach().cpu().contiguous().numpy().tobytes())
    provenance=dict(update=update,manifest_sha256=hashlib.sha256(raw).hexdigest(),model_sha256=h.hexdigest(),native_sha256=hashlib.sha256((ROOT/'loop_native.so').read_bytes()).hexdigest(),simulations=config(fixtures/manifest["shards"][0]["file"],1,2000000,update,True)["simulation_budgets"],skip_forced_actions=True,replicates=3,part_limit=part_limit)
    # Completed historical benchmarks remain tied to their original native/search
    # configuration. A later deployment may reuse the completed result, never
    # unfinished parts from a different simulator. Model/fixtures must still match.
    provenance['search_reuse']=dict(enabled=config(fixtures/manifest['shards'][0]['file'],1,2000000,update,True).get('persistent_search',False),capacity_per_combat=config(fixtures/manifest['shards'][0]['file'],1,2000000,update,True).get('search_cache_nodes',512))
    pp=directory/'provenance.json'
    completed=directory/'summary.json'
    if completed.exists():
        old=json.loads(pp.read_text()); summary=json.loads(completed.read_text())
        for key in ['update','manifest_sha256','model_sha256','replicates','part_limit']:
            assert old[key]==provenance[key], 'Completed human validation identity mismatch'
        assert all(summary.get(k)==v for k,v in old.items()), 'Human summary provenance mismatch'
        if part_limit is None:
            assert summary['trials']==manifest['requested_trials']
        if part_limit is None and fixture_root is None:
            assert summary['runs']==512 and not summary['partial_smoke']
        return summary
    if pp.exists():assert json.loads(pp.read_text())==provenance,'Human validation cache provenance mismatch'
    else:atomic_json(pp,provenance)
    if (directory/'summary.json').exists():return json.loads((directory/'summary.json').read_text())
    all_rows=[];started=time.perf_counter();collection_seconds=0.0
    parts=manifest['shards'][:part_limit] if part_limit else manifest['shards']
    for number,part in enumerate(parts):
        if stopping():return None
        path=fixtures/part['file'];payload=path.read_bytes()
        assert hashlib.sha256(payload).hexdigest()==part['sha256']
        source=[json.loads(l) for l in payload.splitlines()];result=directory/f'part-{number:05d}.json'
        if result.exists():report=json.loads(result.read_text())
        elif source:
            cfg=config(path,len(source),2000000+number,update,True)
            cfg.update(fixture_potion_costs=True,fixed_potion_cost=1.0,record_replay=False)
            report=collect(native,gpu,cfg)
            assert report['completed']==len(source) and report['fixture_potion_costs']
            for row in report['outcomes']:
                original=source[row['id']]
                row.update(original['metadata'])
                row['potion_costs']=original['evaluation']['potion_costs']
                assert [p['cost'] for p in row['potions'][:len(row['potion_costs'])]]==row['potion_costs']
            atomic_json(result,report)
        else:report=dict(outcomes=[],completed=0)
        collection_seconds+=report.get('total_seconds',0.0)
        all_rows.extend(report['outcomes'])
        atomic_json(output/'status.json',dict(stage='human_validation',update=update,parts_completed=number+1,parts_total=len(parts),completed_trials=len(all_rows),requested_trials=manifest['requested_trials'],pid=os.getpid()))
        print(json.dumps(dict(human_validation_update=update,part=number+1,total_parts=len(parts),completed_trials=len(all_rows))),flush=True)
    summary,combats,runs,trials=summarize(manifest,all_rows)
    summary.update(provenance,seconds=time.perf_counter()-started,collection_seconds=collection_seconds,fixture_conversion_failures=len(manifest['conversion_errors']),partial_smoke=part_limit is not None or fixture_root is not None)
    for name,data in [('combats',combats),('runs',runs),('trials',trials),('summary',summary)]:atomic_json(directory/(name+'.json'),data)
    history=output/'human-validation.jsonl'
    existing=[json.loads(l) for l in history.read_text().splitlines()] if history.exists() else []
    if not any(e['update']==update for e in existing):
        with history.open('a') as f:f.write(json.dumps(summary)+'\n')
    print(json.dumps(dict(human_validation=summary)),flush=True)
    return summary
