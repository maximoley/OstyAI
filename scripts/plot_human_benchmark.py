"""Plot scheduled human benchmark history from the bundled verified aggregates."""
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from matplotlib.ticker import PercentFormatter
ROOT=Path(__file__).resolve().parents[1]
data=json.loads((ROOT/'benchmarks/history/human-benchmark-plot-data.json').read_text())
rows=data['points'];x=[r['update'] for r in rows]
assert len(set(x))==len(x) and len({r['manifest_sha256'] for r in rows})==1
plt.rcParams.update({'font.family':'DejaVu Sans','font.size':11,'axes.spines.top':False,'axes.spines.right':False})
fig,axes=plt.subplots(1,2,figsize=(16,7.6))
for key,label,color,dy in [('trial_win_rate','Individual trials (all 3 seeds)','#167494',-22),('win_rate','Combat majority (at least 2 of 3)','#c38a2c',12)]:
 y=[r[key]*100 for r in rows];axes[0].plot(x,y,'o-',color=color,lw=2.5,ms=7,label=label)
 for a,b in zip(x,y):axes[0].annotate(f'{b:.2f}%',(a,b),xytext=(0,dy),textcoords='offset points',ha='center',color=color,fontsize=10)
axes[0].set_title('Combat win rate',loc='left',fontweight='bold');axes[0].set_ylim(89.5,94.5);axes[0].legend(loc='lower right',frameon=False,fontsize=10)
for key,label,color in [('runs_every_combat_at_least_one_win','Every fight: at least 1 of 3','#29846d'),('runs_won_every_fight','Every fight: at least 2 of 3','#7255a0')]:
 y=[r[key]/512*100 for r in rows];axes[1].plot(x,y,'o-',color=color,lw=2.5,ms=7,label=label)
 for a,b,r in zip(x,y,rows):axes[1].annotate(f'{b:.2f}%\n{r[key]}/512',(a,b),xytext=(0,12),textcoords='offset points',ha='center',color=color,fontsize=9)
axes[1].set_title('Runs meeting the rule in every combat',loc='left',fontweight='bold');axes[1].set_ylim(0,72);axes[1].legend(loc='lower right',frameon=False,fontsize=10)
for ax in axes:
 ax.set_xticks(x);ax.set_xlim(min(x)-160,max(x)+160);ax.set_xlabel('Optimizer step');ax.set_ylabel('Win rate');ax.yaxis.set_major_formatter(PercentFormatter(100));ax.grid(alpha=.18);ax.set_axisbelow(True)
fig.suptitle('Human benchmark results over training',fontsize=23,fontweight='bold',x=.065,ha='left',y=.97)
fig.text(.065,.895,'512 fixed A10 human winning runs · 12,099 combats · 36,297 frozen RNG trials per checkpoint',color='#555555',fontsize=12)
fig.text(.065,.088,'Run scores aggregate independent historical combat starts; they are not end-to-end played runs.',color='#555555',fontsize=10)
fig.text(.065,.045,'Unresolved trials count as nonwins. Steps 5,500 and 6,000 use boss128 without reuse; earlier search settings and simulator versions differ.',color='#555555',fontsize=10)
fig.subplots_adjust(left=.065,right=.975,top=.78,bottom=.23,wspace=.25)
for ext in ['png','svg']:fig.savefig(ROOT/f'docs/assets/human_benchmark_vs_optimizer_step.{ext}',dpi=180,facecolor='white')

svg=ROOT/"docs/assets/human_benchmark_vs_optimizer_step.svg"
svg.write_text("\n".join(line.rstrip() for line in svg.read_text().splitlines())+"\n")
