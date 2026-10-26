"""Verify completed comparison artifacts, write tables, and verify trainer advancement."""
import argparse,csv,hashlib,json,os,subprocess,sys,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'ostyai'))
from human_validation_metrics import summarize

def save(p,obj):
    t=p.with_suffix(p.suffix+'.tmp');t.write_text(json.dumps(obj,indent=2)+'\n');t.replace(p)
def main():
    ap=argparse.ArgumentParser();ap.add_argument('--root',type=Path,required=True);ap.add_argument('--production',type=Path,required=True);a=ap.parse_args()
    p=a.root;manifest=json.loads((p/'compatible-fixtures/manifest.json').read_text())
    while True:
        if (p/'STOP').exists():raise SystemExit('Comparison STOP exists; no automatic finalization')
        if (p/'comparison.json').exists():
            comparison=json.loads((p/'comparison.json').read_text())
            if comparison['complete']:break
        runner=int((p/'runner.pid').read_text())
        if not Path(f'/proc/{runner}').exists():raise RuntimeError('Benchmark runner exited before completion; inspect runner.log')
        time.sleep(30)
    assert [r['configuration'] for r in comparison['results']]==['mentor_default','model5500_tiered_no_reuse','model5500_uniform32_no_reuse','model5500_policy_greedy']
    trial_sets=[];checks=[]
    for row in comparison['results']:
        sp=Path(row['summary']);d=sp.parent;s=json.loads(sp.read_text());trials=json.loads((d/'trials.json').read_text())
        recalculated,_,_,_=summarize(manifest,trials)
        for k in ['runs','combats','trials','trial_wins','wins','runs_won_every_fight','unresolved_trials','trial_average_hp_lost','average_hp_lost']:
            assert recalculated[k]==s[k],(row['configuration'],k)
        assert s['runs']==512 and s['trials']==36297 and not s['partial_smoke']
        trial_sets.append({(r['case_id'],r['replicate'])for r in trials});assert len(trial_sets[-1])==36297
        if row['configuration'].startswith('model'):
            assert s['model_sha256']=='a04bd9f4964954ad3c6bc11e8de7e809e01075bfb7b45fe4939dcd2580d84376'
            assert s['search_reuse']['enabled'] is False
            policy_only=row['configuration']=='model5500_policy_greedy'
            if policy_only:
                assert s['policy_only'] and s['action_selection']=='legal_argmax' and s['simulations']==0 and s['workers']==80
            budgets=dict(easy=32,hard=32,elite=32,boss=32)if'uniform32'in row['configuration']else dict(easy=32,hard=64,elite=96,boss=128)
            if not policy_only:assert s['simulations']==budgets
            parts=list(d.glob('part-*.json'));assert len(parts)==48
            for part in parts:
                r=json.loads(part.read_text());assert r['workers']==80 and r['replay_added']==0 and not r['fatal']
                if policy_only:
                    assert all(r[k]==0 for k in ['simulations','search_transitions','forced_search_transitions','chance_samples','replay_added','forced_actions'])
                    continue
                assert r['simulations']==sum(budgets[o['search_tier']]*o['searched_decisions']for o in r['outcomes'])
        else:
            for r in trials:
                assert r['requested_simulations']in(1000*r['searches'],4000*r['searches'])and r['rollout_depth']==200
        checks.append(dict(configuration=row['configuration'],metrics_recomputed=True,full_trial_set=True,settings_verified=True))
    assert all(x==trial_sets[0]for x in trial_sets)
    fields=['configuration','trial_wins','trials','trial_win_rate','wins','combats','win_rate','runs_won_every_fight','runs','run_win_rate','unresolved_trials','trial_average_hp_lost','average_hp_lost','wall_seconds','workers']
    with (p/'comparison.csv').open('w')as f:
        writer=csv.DictWriter(f,fieldnames=fields,extrasaction='ignore');writer.writeheader();writer.writerows(comparison['results'])
    lines=['# Full human benchmark comparison','', '512 runs; 12,099 combats; three frozen RNG starts each. All configurations used 80 workers sequentially with training paused.','', '| Configuration | Trial win rate | Majority combat win rate | Every-combat run win rate | Mean HP lost per completed trial | Unresolved trials | Full wall time |','|---|---:|---:|---:|---:|---:|---:|']
    for r in comparison['results']:
        sec=r['wall_seconds'];h=int(sec//3600);m=int(sec%3600//60);ss=sec%60
        lines.append(f"| {r['configuration']} | {r['trial_wins']}/{r['trials']} ({100*r['trial_win_rate']:.3f}%) | {r['wins']}/{r['combats']} ({100*r['win_rate']:.3f}%) | {r['runs_won_every_fight']}/512 ({100*r['run_win_rate']:.3f}%) | {r['trial_average_hp_lost']:.3f} | {r['unresolved_trials']} | {h}h {m}m {ss:.1f}s |")
    lines+=['','Unresolved trials count as nonwins. HP means exclude unresolved trials. Combats are independent historical starts, not linked model-played runs. Human fixtures may overlap training.','', 'Timing includes process startup, loading, compilation and result writing; offline compatibility preparation/preflight are excluded. All four runs use identical compatibility-prepared states. Original frozen files remain unchanged. Mentor retains its upstream evaluator; model runs use the fixed 0/1 potion-cost labels.']
    (p/'comparison.md').write_text('\n'.join(lines)+'\n')
    save(p/'results-verification.json',dict(complete=True,checks=checks,trial_sets_identical=True,time=time.time()))
    while not (p/'resume-started.json').exists():
        if not Path('/proc/'+(p/'runner.pid').read_text().strip()).exists():raise RuntimeError('Results complete but trainer resume did not start')
        time.sleep(10)
    paused=json.loads((p/'pause-verification.json').read_text());deadline=time.time()+1800
    while time.time()<deadline:
        sp=a.production/'production/status.json';status=json.loads(sp.read_text());service=json.loads((a.production/'production/service.json').read_text())
        if status.get('updates',0)>paused['step']:
            import torch
            torch.set_num_threads(1)
            checkpoint=torch.load(a.production/'production/latest.pt',map_location='cpu',weights_only=False)
            if checkpoint['updates']>paused['step']:
                assert checkpoint['settings']==paused['settings']
                for name in ['trainer_pid','supervisor_pid']:
                    cmd=Path(f"/proc/{service[name]}/cmdline").read_bytes().replace(b'\0',b' ').decode();assert str(a.production)in cmd
                assert all(torch.isfinite(t).all()for t in checkpoint['model'].values())
                save(p/'resume-verification.json',dict(paused_step=paused['step'],checkpoint_step=checkpoint['updates'],settings_unchanged=True,model_finite=True,trainer_pid=service['trainer_pid'],supervisor_pid=service['supervisor_pid'],time=time.time()))
                save(p/'status.json',dict(stage='complete',all_benchmarks_verified=True,training_resumed=True,checkpoint_step=checkpoint['updates'],time=time.time()));return
        time.sleep(30)
    raise RuntimeError('Training resume has not produced an advancing checkpoint within 30 minutes')
if __name__=='__main__':main()
