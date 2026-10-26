"""Fixed human-run benchmark: trial, 2-of-3 combat, and every-combat run results."""
from collections import defaultdict
from statistics import mean

def summarize(manifest, results):
    lookup={(r['case_id'],r['replicate']):r for r in results}
    if len(lookup)!=len(results):raise ValueError('duplicate trial results')
    runs=[];combats=[];trials=[]
    for run in manifest['runs']:
        rc=[]
        for cid in run['case_ids']:
            rr=[lookup.get((cid,j),dict(case_id=cid,run_hash=run['run_hash'],replicate=j,terminal=False,truncated=False,unsupported=True,win=False)) for j in range(3)]
            done=[r for r in rr if r.get('terminal') and not r.get('truncated') and not r.get('unsupported')]
            wins=sum(bool(r['win']) for r in done);losses=len(done)-wins
            template=next((r for r in rr if 'act' in r),{})
            row=dict(case_id=cid,run_hash=run['run_hash'],wins=wins,losses=losses,unresolved_trials=3-len(done),majority_win=wins>=2,majority_loss=losses>=2,unresolved=wins<2 and losses<2,trial_win_rate=wins/3,average_hp_lost=mean(r['initial_hp']-r['hp'] for r in done) if done else None,winning_average_hp_lost=mean(r['initial_hp']-r['hp'] for r in done if r['win']) if wins else None,**{k:template.get(k) for k in ['act','encounter','room_type','version']})
            combats.append(row);rc.append(row);trials.extend(rr)
        wins=sum(c['majority_win'] for c in rc);losses=sum(c['majority_loss'] for c in rc)
        runs.append(dict(run_hash=run['run_hash'],combats=len(rc),combat_wins=wins,combat_losses=losses,unresolved_combats=sum(c['unresolved'] for c in rc),won_every_fight=wins==len(rc) and bool(rc),unresolved=losses==0 and wins<len(rc),combat_win_rate=wins/len(rc) if rc else None,trial_win_rate=sum(c['wins'] for c in rc)/(3*len(rc)) if rc else None,average_hp_lost=mean(c['average_hp_lost'] for c in rc if c['average_hp_lost'] is not None) if any(c['average_hp_lost'] is not None for c in rc) else None))
    completed=[r for r in trials if r.get('terminal') and not r.get('truncated') and not r.get('unsupported')]
    def combat_stats(cs):
        return dict(combats=len(cs),wins=sum(c['majority_win'] for c in cs),losses=sum(c['majority_loss'] for c in cs),unresolved=sum(c['unresolved'] for c in cs),win_rate=sum(c['majority_win'] for c in cs)/len(cs),trial_win_rate=sum(c['wins'] for c in cs)/(3*len(cs)),average_hp_lost=mean(c['average_hp_lost'] for c in cs if c['average_hp_lost'] is not None) if any(c['average_hp_lost'] is not None for c in cs) else None)
    by={}
    for field in ['act','encounter','room_type','version']:
        groups=defaultdict(list)
        for c in combats:groups[str(c[field])].append(c)
        by[field]={key:combat_stats(cs) for key,cs in sorted(groups.items())}
    by['replicate']={}
    for rep in range(3):
        rr=[r for r in trials if r['replicate']==rep];dd=[r for r in rr if r.get('terminal') and not r.get('truncated') and not r.get('unsupported')]
        by['replicate'][str(rep)]=dict(trials=len(rr),wins=sum(r['win'] for r in dd),win_rate=sum(r['win'] for r in dd)/len(rr),unresolved=len(rr)-len(dd),average_hp_lost=mean(r['initial_hp']-r['hp'] for r in dd) if dd else None)
    summary=dict(runs=len(runs),runs_won_every_fight=sum(r['won_every_fight'] for r in runs),run_win_rate=sum(r['won_every_fight'] for r in runs)/len(runs),unresolved_runs=sum(r['unresolved'] for r in runs),**combat_stats(combats),trials=len(trials),completed_trials=len(completed),trial_wins=sum(r['win'] for r in completed),unresolved_trials=len(trials)-len(completed),completed_trial_win_rate=sum(r['win'] for r in completed)/len(completed) if completed else None,trial_average_hp_lost=mean(r['initial_hp']-r['hp'] for r in completed) if completed else None,winning_trial_average_hp_lost=mean(r['initial_hp']-r['hp'] for r in completed if r['win']) if any(r['win'] for r in completed) else None,by=by,definition='Combat win = at least 2 of 3 RNG trials won; run win = every recorded combat wins. Primary win-rate denominators include unresolved cases. HP loss is initial minus final HP, allowing healing; completed trials only. Independent historical starts, not a linked run rollout.')
    return summary,combats,runs,trials
