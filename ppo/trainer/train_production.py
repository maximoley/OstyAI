"""Fresh run-continuation PPO; strict resumability and durable 2-hour validation."""
import argparse,collections,fcntl,json,os,random,signal,subprocess,threading,time,traceback
import numpy as np
import torch
from worker import R
from model import ActorCritic,Config
from actor import Actor
from fast_workers import FastWorkers as RunWorkers
from returns import Returns
from learner import Learner
from collect_fast import collect
from validation import evaluate
from gc_budget import GCBudget
from training_schedule import deadline_reached,next_rollout_size,timestamp
from operations import atomic_json,append_json,append_json_many,sha,tensor_hash,foreign_gpu

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--output',default='production');ap.add_argument('--resume');ap.add_argument('--updates',type=int);ap.add_argument('--checkpoint-every',type=int);ap.add_argument('--start-depth',type=int);ap.add_argument('--preflight-validation',type=int,default=0);a=ap.parse_args()
    cfg=json.loads((R/'config.json').read_text());out=R/a.output;out.mkdir(exist_ok=True);lock=(out/'trainer.lock').open('w');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    stop=threading.Event();monitor_stop=threading.Event();foreign=threading.Event();state={};target=a.updates or cfg['total_policy_updates'];every=a.checkpoint_every or cfg['checkpoint_every']
    signal.signal(signal.SIGTERM,lambda *args:stop.set());signal.signal(signal.SIGINT,lambda *args:stop.set())
    def stopping():return stop.is_set() or (R/'STOP').exists() or (out/'STOP').exists()
    assert not stopping(),'STOP is present'
    assert not deadline_reached(cfg),'Wall deadline has passed'
    assert cfg['max_collected_decisions']%cfg['environment']['slots']==0
    assert not foreign_gpu([os.getpid()]),'Idle GPU required'
    def status(stage,**extra):
        state.update(stage=stage,pid=os.getpid(),heartbeat=time.time(),**extra);atomic_json(out/'status.json',state)
    def monitor():
        while not monitor_stop.is_set():
            try:
                others=foreign_gpu([os.getpid()]);g=subprocess.run(['nvidia-smi','--query-gpu=utilization.gpu,memory.used,power.draw','--format=csv,noheader,nounits'],capture_output=True,text=True,timeout=15)
                append_json(out/'gpu.jsonl',dict(time=time.time(),stage=state.get('stage'),foreign=others,gpu=g.stdout.strip()))
                if others or g.returncode:foreign.set()
            except Exception as e:append_json(out/'gpu.jsonl',dict(time=time.time(),error=str(e)));foreign.set()
            monitor_stop.wait(10)
    seed=cfg['initialization_seed'];torch.set_num_threads(1);random.seed(seed);np.random.seed(seed);torch.manual_seed(seed);torch.cuda.manual_seed_all(seed);rng=np.random.default_rng(seed)
    status('setup',update=0,workers=80,slots=512,target_policy_updates=target,initialization='resume' if a.resume else 'fresh',character=cfg['environment']['character'])
    model=ActorCritic(Config(**cfg['architecture'])).cuda();learner=Learner(model,cfg);actor=Actor(model);evaluation_model=ActorCritic(Config(**cfg['architecture'])).cuda();greedy=Actor(evaluation_model,greedy=True)
    e=cfg['environment'];w=RunWorkers(e['slots'],e['workers'],e['seed'],e['observation_capacity'],e['private_capacity'],character=e['character']);returns=Returns(cfg)
    provenance=dict(character=e['character'],protocol=cfg['protocol'],reference=cfg['reference'],native_sha256=sha(R/'loop_native.so'),architecture=model.describe(),schema_sha256={p.name:sha(p) for p in (R/'schema').glob('*.json')},validation_seed_sha256=sha(R/'validation-seeds.json'),code_sha256={p.name:sha(p) for p in R.glob('*.py')},torch_version=torch.__version__)
    runtime=dict(started_at=time.time(),next_validation_at=time.time()+cfg['validation']['first_after_seconds'],pending_validation=None,validated_update=None,collected_decisions=0,trained_example_uses=0,elapsed_active_seconds=0.,full_runs_completed=0,full_run_wins=0,practice_completed=0,practice_terminals=0,practice_bootstraps=0,caps=0)
    pending_collection=None
    if a.resume:
        cp=torch.load(a.resume,map_location='cuda',weights_only=False);assert cp['config']==cfg and cp['provenance']==provenance,'Config/source/native provenance mismatch'
        assert cp.get('pending_collection') is None,'Partial deadline rollout is preserved; an explicit migration is required before extending this run'
        model.load_state_dict(cp['model']);learner.optimizer.load_state_dict(cp['optimizer']);learner.load_state_dict(cp['learner']);returns.load_state_dict(cp['returns']);w.load_journal(cp['native_journal']);runtime=cp['runtime'];rng.bit_generator.state=cp['numpy_rng'];random.setstate(cp['python_rng']);torch.set_rng_state(cp['torch_rng'].cpu());torch.cuda.set_rng_state(cp['cuda_rng'].cpu())
        assert tensor_hash(model)==cp['model_tensor_sha256'] and w.save_journal()==cp['native_journal'],'Resume parity failed'
        atomic_json(out/f'resume-{learner.updates:06d}-{int(time.time())}.json',dict(passed=True,update=learner.updates,model_optimizer_ema_returns_journal_rng_loaded=True,checkpoint_sha256=sha(a.resume)))
    else:
        assert not (out/'latest.pt').exists(),'Refusing to overwrite an existing run'
        atomic_json(out/'initialization.json',dict(fresh=True,seed=seed,model_tensor_sha256=tensor_hash(model),old_checkpoint_loaded=False,parameters=model.describe()))
    atomic_json(out/'config.json',cfg);atomic_json(out/'provenance.json',provenance)
    def save(reason):
        status('checkpoint',update=learner.updates,optimizer_steps=learner.optimizer_steps,checkpoint_reason=reason);torch.cuda.synchronize();begun=time.perf_counter()
        cp=dict(native_character_audit=w.audit_character(),pending_collection=pending_collection,config=cfg,provenance=provenance,model=model.state_dict(),model_tensor_sha256=tensor_hash(model),optimizer=learner.optimizer.state_dict(),learner=learner.state_dict(),returns=returns.state_dict(),native_journal=w.save_journal(),runtime=runtime,numpy_rng=rng.bit_generator.state,python_rng=random.getstate(),torch_rng=torch.get_rng_state(),cuda_rng=torch.cuda.get_rng_state())
        tmp=out/'latest.tmp'
        with tmp.open('wb') as f:torch.save(cp,f);f.flush();os.fsync(f.fileno())
        if (out/'latest.pt').exists():
            previous=out/'previous.pt';previous.unlink(missing_ok=True);os.link(out/'latest.pt',previous)
        os.replace(tmp,out/'latest.pt');r=dict(time=time.time(),update=learner.updates,optimizer_steps=learner.optimizer_steps,reason=reason,sha256=sha(out/'latest.pt'),bytes=(out/'latest.pt').stat().st_size,seconds=time.perf_counter()-begun)
        atomic_json(out/'checkpoint.json',r);append_json(out/'checkpoints.jsonl',r)
        if reason.startswith('validation') or learner.updates%1000==0:
            p=out/f'checkpoint-{learner.updates:06d}.pt'
            if not p.exists():os.link(out/'latest.pt',p)
    thread=threading.Thread(target=monitor,daemon=True);thread.start();gc_policy=GCBudget(10);gc_policy.start()
    try:
        status('prepare_starts');t=time.perf_counter();w.refill(a.start_depth or cfg['pre_generated_starts']);atomic_json(out/'start-bank.json',dict(character=w.character,seconds=time.perf_counter()-t,ready_starts=w.summary()['ready_starts']));atomic_json(out/'start-bank-character-audit.json',w.audit_character())
        if not a.resume:save('initialized')
        def validation(seeds=None):
            evaluation_model.load_state_dict(learner.ema['0.996']);return evaluate(evaluation_model,greedy,out,learner.updates,cfg,provenance,status,lambda:stopping() or foreign.is_set() or deadline_reached(cfg),seeds_override=seeds)
        if a.preflight_validation:
            result=validation(json.loads((R/'validation-seeds.json').read_text())[:a.preflight_validation]);assert result is not None;atomic_json(out/'validation-preflight.json',result)
        def finish_reason():
            if deadline_reached(cfg):return 'wall_deadline'
            if runtime['collected_decisions']>=cfg['max_collected_decisions']:return 'decision_cap'
            if learner.updates>=target:return 'policy_update_limit'
            return None
        def record_outcomes(completed,update,unfitted=False):
            full=[r for r in completed if r['lane']==0];practice=[r for r in completed if r['lane']==1]
            runtime['full_runs_completed']+=len(full);runtime['full_run_wins']+=sum(r['won'] for r in full);runtime['practice_completed']+=len(practice);runtime['practice_bootstraps']+=sum(r['bootstrap'] for r in practice);runtime['practice_terminals']+=sum(r['terminal'] for r in practice);runtime['caps']+=sum(r['cap'] for r in completed)
            append_json_many(out/'full-run-results.jsonl',(dict(row,update=update,unfitted_partial_rollout=unfitted) for row in full));append_json_many(out/'practice-results.jsonl',(dict(row,update=update,unfitted_partial_rollout=unfitted) for row in practice))
            return full,practice
        while not finish_reason() and not stopping():
            if foreign.is_set():save('foreign_gpu_pause');status('waiting_for_gpu',stopped=True,completed=False);return
            if runtime['pending_validation'] is not None or time.time()>=runtime['next_validation_at']:
                runtime['pending_validation']=learner.updates;save('validation_pending');result=validation()
                if result is None:break
                runtime.update(pending_validation=None,validated_update=learner.updates,next_validation_at=time.time()+cfg['validation']['every_seconds']);save('validation_complete')
            if stopping() or finish_reason():break
            update=learner.updates;start=time.perf_counter();before_graphs=int(torch._dynamo.utils.counters['stats']['unique_graphs']);before_prepare=actor.preparation_seconds
            status('collection',update=update,optimizer_steps=learner.optimizer_steps,next_validation_at=runtime['next_validation_at']);w.refill(a.start_depth or cfg['pre_generated_starts'])
            cycle_cfg=dict(cfg,rollout_steps=next_rollout_size(cfg,runtime['collected_decisions']))
            rows,tails,completed,col=collect(w,actor,cycle_cfg,update,status,should_stop=lambda:deadline_reached(cfg))
            runtime['collected_decisions']+=len(rows)
            if not col['complete'] or deadline_reached(cfg):
                pending_collection=dict(rows=rows,tails=tails,completed=completed,collection=col,source_update=update,reason='wall_deadline',model_tensor_sha256=tensor_hash(model),unfitted=True)
                runtime['elapsed_active_seconds']+=time.perf_counter()-start
                record_outcomes(completed,update,unfitted=True)
                atomic_json(out/'partial-rollout.json',dict(rows=len(rows),expected_rows=cycle_cfg['rollout_steps'],source_update=update,reason='wall_deadline',checkpoint_key='pending_collection',unfitted=True))
                break
            returns.ingest(rows)
            t=time.perf_counter();held=returns.held_rows(update);learner.refresh(held);data,ret=returns.finish(rows,tails,update);refresh_seconds=time.perf_counter()-t
            status('learning',eligible_rows=data.steps,held_ooc=ret['ooc_held']);train=learner.train(data,rng,len(rows));gc_report=gc_policy.tick(learner.updates);seconds=time.perf_counter()-start
            runtime['trained_example_uses']+=train['trained_decisions'];runtime['elapsed_active_seconds']+=seconds
            full,practice=record_outcomes(completed,learner.updates)
            stats=w.summary();wall_seconds=time.perf_counter()-start;rec=dict(time=time.time(),collector_implementation='compact-native-v1',loop_wall_seconds=wall_seconds,garbage_collection=gc_report,protocol=cfg['protocol'],character=w.character,update=learner.updates,optimizer_steps=learner.optimizer_steps,seconds=seconds,collection=col,held_prepass_and_returns_seconds=refresh_seconds,returns=ret,learner=train,metrics=train['metrics'],runtime=dict(runtime),full_run_completions=len(full),practice_completions=len(practice),full_run_mean_floor=float(np.mean([r['floor'] for r in full])) if full else None,practice_admitted=stats['admitted'],practice_fallbacks=stats['fallbacks'],practice_bank_sizes=stats['bank_sizes'],ready_starts=stats['ready_starts'],new_compiled_graphs=int(torch._dynamo.utils.counters['stats']['unique_graphs'])-before_graphs,new_actor_preparation_seconds=actor.preparation_seconds-before_prepare,decisions_per_second=len(rows)/seconds,trained_positions_per_second=train['trained_decisions']/seconds,foreign_gpu_overlap=foreign.is_set(),native_sha256=provenance['native_sha256'],stop_at=cfg['schedule']['stop_at'],max_collected_decisions=cfg['max_collected_decisions'])
            append_json(out/'progress.jsonl',rec);append_json(out/'metrics.jsonl',dict(time=rec['time'],update=learner.updates,seconds=seconds,collected_decisions=runtime['collected_decisions'],**train['metrics'],kl_exact=train['kl_exact_categorical'],full_run_mean_floor=rec['full_run_mean_floor']))
            if learner.updates%every==0:save('periodic')
            status('update_complete',update=learner.updates,optimizer_steps=learner.optimizer_steps,latest_seconds=seconds,metrics=train['metrics'],total_collected_decisions=runtime['collected_decisions'],stopped=False)
            print(json.dumps(dict(update=learner.updates,seconds=seconds,collection=col['seconds'],learning=train['seconds'],retained=train['trained_decisions'],kl=train['kl_exact_categorical'],bank=stats['bank_sizes'],practice=col['practice_rows'])),flush=True)
        reason=finish_reason();complete=reason is not None
        runtime['stop_reason']=reason or 'user_pause'
        save(reason or 'paused');status('complete' if complete else 'waiting_for_gpu' if foreign.is_set() and not stopping() else 'paused',stopped=True,completed=complete,stop_reason=runtime['stop_reason'],update=learner.updates,optimizer_steps=learner.optimizer_steps,total_collected_decisions=runtime['collected_decisions'],max_collected_decisions=cfg['max_collected_decisions'],stop_at=cfg['schedule']['stop_at'],pending_collection_rows=len(pending_collection['rows']) if pending_collection else 0)
    except BaseException as e:
        atomic_json(out/'failure.json',dict(time=time.time(),error=repr(e),traceback=traceback.format_exc(),committed=json.loads((out/'checkpoint.json').read_text()).get('update') if (out/'checkpoint.json').exists() else None,in_memory_update=learner.updates));status('failed',error=str(e));raise
    finally:gc_policy.close();monitor_stop.set();thread.join(timeout=20);w.close()
if __name__=='__main__':main()
