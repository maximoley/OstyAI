"""Rebuild the starting-potion cost/use plot from frozen validation observations.

Run from any directory: python scripts/plot_potion_sensitivity.py
Requires matplotlib; no training runtime or GPU is needed.
"""
import csv
import gzip
import hashlib
import json
from pathlib import Path

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from matplotlib.ticker import PercentFormatter

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / 'benchmarks/potion-sensitivity'
source = DATA / 'validation-potion-observations.json.gz'
data = json.loads(gzip.decompress(source.read_bytes()))
rows, summary, signatures = [], [], {}
for series in data['series']:
    assert len(series['trials']) == 8192
    assert len({(r['replicate'], r['id']) for r in series['trials']}) == 8192
    bins = [[] for _ in range(10)]
    for trial in series['trials']:
        key = trial['replicate'], trial['id']
        costs = [p[0] for p in trial['potions']]
        assert key not in signatures or signatures[key] == costs
        signatures[key] = costs
        for cost, used in trial['potions']:
            assert 0 <= cost <= 1 and isinstance(used, bool)
            bins[min(9, int(cost * 10))].append((cost, used))
    local = []
    for i, potions in enumerate(bins):
        assert potions
        row = dict(step=series['step'], mode=series['mode'],
                   bin_lower=i / 10, bin_upper=(i + 1) / 10,
                   potions=len(potions), used=sum(u for _, u in potions),
                   mean_cost=sum(c for c, _ in potions) / len(potions),
                   use_rate=sum(u for _, u in potions) / len(potions))
        local.append(row)
    rows.extend(local)
    summary.append(dict(step=series['step'], mode=series['mode'], trials=8192,
                        starting_potions=sum(r['potions'] for r in local),
                        unresolved_trials=sum(not r['terminal'] or r['truncated'] for r in series['trials']),
                        low_cost_use_rate=local[0]['use_rate'],
                        high_cost_use_rate=local[-1]['use_rate'],
                        endpoint_drop_percentage_points=100 * (local[0]['use_rate']-local[-1]['use_rate'])))
with (DATA / 'binned-use.csv').open('w', newline='') as f:
    writer = csv.DictWriter(f, fieldnames=list(rows[0]))
    writer.writeheader(); writer.writerows(rows)
report = dict(source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
              definition='Used starting potion instances / all starting potion instances in each cost bin; final bin includes 1.0.',
              includes_unresolved=True, excludes_potions_acquired_during_combat=True,
              frozen_costs_match=True, series=summary)
(DATA / 'summary.json').write_text(json.dumps(report, indent=2)+'\n')

plt.rcParams.update({'font.family':'DejaVu Sans', 'font.size':12,
                     'axes.spines.top':False,'axes.spines.right':False,'svg.hashsalt':'ostyai-potion-sensitivity'})
fig, axes = plt.subplots(1, 2, figsize=(13.5, 6.9), sharey=True)
fig.subplots_adjust(left=.075, right=.975, bottom=.25, top=.74, wspace=.13)
fig.text(.075, .94, 'Potion use declines as assigned penalty rises', fontsize=23, weight='bold')
fig.text(.075, .883, 'Primary validation · checkpoints 5,300–5,500 · 8,192 trials per checkpoint and mode', fontsize=12.5, color='#555555')
colors = {5300:'#8b9db5', 5400:'#b66e32', 5500:'#087f8c'}
for ax, mode, title, subtitle in zip(axes, ['search','policy_only'],
        ['Policy + Gumbel search','Greedy policy only'],
        ['Tier budgets 32 / 64 / 96 / 128','Zero search simulations']):
    for step in [5300,5400,5500]:
        points=[r for r in rows if r['mode']==mode and r['step']==step]
        ax.plot([r['mean_cost'] for r in points],[r['use_rate'] for r in points],
                marker='o', markersize=5, linewidth=2.5 if step==5500 else 1.8,
                color=colors[step], label=f'Step {step:,}')
        if step==5500:
            for p in [points[0],points[-1]]:
                ax.annotate(f"{p['use_rate']:.1%}",(p['mean_cost'],p['use_rate']),
                            xytext=(0,12),textcoords='offset points',ha='center',
                            fontsize=12,weight='bold',color=colors[step])
    ax.set_title(title, loc='left', fontsize=17, weight='bold', pad=30)
    ax.text(0,1.025,subtitle,transform=ax.transAxes,color='#666666',fontsize=11)
    ax.set_xlim(0,1); ax.set_ylim(0,1)
    ax.set_xticks([0,.2,.4,.6,.8,1]); ax.yaxis.set_major_formatter(PercentFormatter(1))
    ax.grid(axis='y',alpha=.2);ax.set_axisbelow(True)
    ax.set_xlabel('Assigned potion penalty (cost)',labelpad=10)
axes[0].set_ylabel('Starting potions used',labelpad=10)
axes[1].legend(loc='upper right',frameon=False)
fig.text(.075,.145,'12,780 starting potion instances per curve; points aggregate cost bins of width 0.1.',fontsize=11,color='#555555')
fig.text(.075,.101,'Same fixtures and frozen costs across checkpoints/modes. Includes failures and unresolved trials at the 1,000-action cap.',fontsize=10.5,color='#555555')
fig.text(.075,.057,'Observed cost/use association, not a controlled cost sweep of the same state. Potions acquired during combat are excluded.',fontsize=10.5,color='#555555')
out=ROOT/'docs/assets/potion_use_vs_penalty.png'
fig.savefig(out,dpi=180,facecolor='white')
fig.savefig(out.with_suffix('.svg'),facecolor='white',metadata={'Date':None})
plt.close(fig)
print(json.dumps(report,indent=2))
