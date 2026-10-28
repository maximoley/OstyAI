"""Rebuild primary validation history with all original and continuation markers."""
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from matplotlib.ticker import PercentFormatter
R=Path(__file__).resolve().parents[1]
O=R/'docs/assets'
rows=[json.loads(l) for l in (R/'benchmarks/history/validation.jsonl').read_text().splitlines() if l.strip()]
rows=sorted({r['update']:r for r in rows}.values(),key=lambda r:r['update'])
x=[r['update'] for r in rows];y=[100*r['wins']/r['requested'] for r in rows]
policy_rows=[json.loads(l) for l in (R/'benchmarks/history/policy-validation.jsonl').read_text().splitlines() if l.strip()]
policy_rows=sorted({r['update']:r for r in policy_rows}.values(),key=lambda r:r['update'])
search_by_step={r['update']:r for r in rows}
for r in policy_rows:
 assert r['requested']==8192 and r['simulations']==0 and r['action_selection']=='legal_argmax'
 matched=search_by_step[r['update']]
 for key in ['model_sha256','fixture_sha256','native_sha256']:assert r[key]==matched[key]
px=[r['update'] for r in policy_rows];py=[100*r['wins']/r['requested'] for r in policy_rows]

events=[
(400,'Prioritized replay + importance correction','training'),
(3770,'Search budget: 16 → 64','training'),
(3935,'Forced actions bypass search; forced chains collapsed','training'),
(4107,'Search by pool: 16 / 64 / 96 / 128','training'),
(4176,'Learner pool quotas proportional to search budgets','training'),
(4242,'Enemy-AI import fix + affected replay quarantine','simulator'),
(4598,'Easy pool: 16 → 32; learner quotas rebalanced','training'),
(4880,'Autoplay / turn-continuation repair (resume point)','simulator'),
(5020,'Combat-end pending-choice repair (resume point)','simulator'),
(5500,'Persistent search reuse enabled after this checkpoint','training'),
(5613,'Boss: 128 → 256; double boss starts + rebalance quotas','training'),
(5645,'Persistent search reuse disabled','training')]
plt.rcParams.update({'font.family':'DejaVu Sans','font.size':11,'axes.spines.top':False,'axes.spines.right':False})
fig=plt.figure(figsize=(14,9.2));ax=fig.add_axes([.08,.38,.87,.46])
ax.plot(x,y,color='#156e91',lw=2.6,zorder=5,label='Gumbel search')
ax.scatter(x,y,s=10,color='#156e91',zorder=6)
ax.plot(px,py,color='#c58b25',lw=2.2,ls='--',marker='s',ms=3.8,zorder=6,label='Policy only · greedy, no search')
ax.legend(loc='upper left',frameon=False,fontsize=10)
ax.annotate(f'{py[-1]:.2f}%',(px[-1],py[-1]),xytext=(0,10),textcoords='offset points',ha='center',fontweight='bold',color='#a56e0c')
ax.set_xlim(0,max(x)+230);ax.set_ylim(35,100);ax.yaxis.set_major_formatter(PercentFormatter(100));ax.grid(axis='y',alpha=.18);ax.set_axisbelow(True)
ax.set_xlabel('Optimizer step',labelpad=10);ax.set_ylabel('Validation win rate')
colors={'training':'#775a98','simulator':'#bf7637'}
for i,(step,label,kind) in enumerate(events,1):
 if step>max(x):continue
 c=colors[kind];ax.axvline(step,color=c,lw=.8,alpha=.33,zorder=1)
 level=38.8+((i-1)%3)*6.2
 ax.text(step,level,str(i),ha='center',va='center',fontsize=9,color='white',bbox=dict(boxstyle='circle,pad=.28',fc=c,ec='white',lw=.7),zorder=7)
ax.annotate(f'{y[-1]:.2f}%',(x[-1],y[-1]),xytext=(0,15),textcoords='offset points',ha='center',fontweight='bold',color='#156e91')
fig.text(.08,.94,'Validation win rate over training',fontsize=22,fontweight='bold')
fig.text(.08,.89,f'Search and policy-only validation · 8,192 trials per checkpoint · latest step {max(x):,}',fontsize=12,color='#555555')
for j,(step,label,kind) in enumerate(events):
 col=j//6;row=j%6;xp=.08+.46*col;yp=.285-.034*row
 fig.text(xp,yp,f'{j+1}',fontsize=9,color='white',ha='center',va='center',bbox=dict(boxstyle='circle,pad=.25',fc=colors[kind],ec='none'))
 fig.text(xp+.018,yp,f'{step:,}  {label}',fontsize=9.3,va='center')
fig.text(.08,.049,'Markers indicate changes after the named checkpoint. Additional dead-enemy import repair resumed at step 4,550.',fontsize=9,color='#555555')
fig.text(.08,.025,'Unresolved trials count as nonwins. Changes also affected evaluation; jumps do not isolate learning gains.',fontsize=9,color='#555555')
for ext in ['png','svg']:fig.savefig(O/f'validation_winrate_annotated.{ext}',dpi=180,facecolor='white')
(R/'benchmarks/history/primary-validation-annotated-data.json').write_text(json.dumps({'validation':rows,'policy_validation':policy_rows,'policy_scope':'Recorded scheduled checkpoints only; no inferred earlier values or off-schedule tests.','events':[dict(step=s,label=l,kind=k) for s,l,k in events],'metric':'wins/requested; unresolved counted as nonwins'},indent=2)+'\n')
print('Latest',x[-1],y[-1])

svg=O/"validation_winrate_annotated.svg"
svg.write_text("\n".join(line.rstrip() for line in svg.read_text().splitlines())+"\n")
