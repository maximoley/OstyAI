from human_validation_metrics import summarize

def test_majorities_and_complete_run_denominators():
    m={'runs':[{'run_hash':'a','case_ids':['a1','a2']},{'run_hash':'b','case_ids':['b1']},{'run_hash':'c','case_ids':['c1']}]}
    rows=[]
    for cid,results in [('a1',[1,1,0]),('a2',[1,0,1]),('b1',[1,0,0])]:
        for i,w in enumerate(results):rows.append(dict(case_id=cid,run_hash=cid[0],replicate=i,win=bool(w),terminal=True,truncated=False,initial_hp=50,hp=40 if w else 0,act=1))
    s,cs,rs,ts=summarize(m,rows)
    assert s['runs_won_every_fight']==1 and s['run_win_rate']==1/3
    assert s['wins']==2 and s['win_rate']==.5 and s['unresolved']==1
    assert s['trial_wins']==5 and s['trial_win_rate']==5/12
    assert s['unresolved_trials']==3 and s['unresolved_runs']==1
    assert len(ts)==12
    rows.append(dict(case_id='a1',replicate=0))
    try:summarize(m,rows)
    except ValueError:pass
    else:raise AssertionError('Duplicate trial accepted')

if __name__=='__main__':
    test_majorities_and_complete_run_denominators();print('Human validation aggregation checks passed')
