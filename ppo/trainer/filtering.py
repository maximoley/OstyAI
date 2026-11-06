"""Ataraxos-style example selection, before any centered advantage normalization.

Reference: AtaraxosAI/stratego, commit92db29e8, pyengine/core/buffer.py240-241.
Selection changes the fitted distribution: both critic and policy use the retained rows.
"""
import numpy as np
from rollout import bucket,TOKEN_BUCKETS

def select(rollout,cfg):
 spec=cfg.get('advantage_filter',{});raw=getattr(rollout,'raw_advantages',None)
 reconstructed=raw is None
 if raw is None:raw=rollout.returns-rollout.values
 raw=np.asarray(raw,np.float32);assert np.isfinite(raw).all()
 if spec.get('enabled',False):
  keep=np.zeros(rollout.steps,bool);task_thresholds={}
  for task in np.unique(rollout.tasks):
   mask=rollout.tasks==task;q=float(np.quantile(np.abs(raw[mask]),spec['quantile']));threshold=max(q,float(spec['min_abs_advantage']));keep[mask]=np.abs(raw[mask])>=threshold;task_thresholds[str(task)]=dict(quantile=q,threshold=threshold,collected=int(mask.sum()),retained=int(keep[mask].sum()),advantage_mean=float(raw[mask].mean()),advantage_std=float(raw[mask].std()))
  q=task_thresholds;threshold=task_thresholds
 else:q=None;threshold=None;keep=np.ones(rollout.steps,bool)
 ids=np.flatnonzero(keep);phase={str(k):dict(collected=int((rollout.phases==k).sum()),retained=int(((rollout.phases==k)&keep).sum())) for k in np.unique(rollout.phases)}
 metadata=dict(enabled=spec.get('enabled',False),quantile=spec.get('quantile'),minimum_absolute_advantage=spec.get('min_abs_advantage'),quantile_value=q,threshold=threshold,collected=rollout.steps,retained=len(ids),retained_fraction=len(ids)/rollout.steps,positive_raw=int((raw[ids]>0).sum()),negative_raw=int((raw[ids]<0).sum()),zero_raw=int((raw[ids]==0).sum()),by_phase=phase,scope='per_task',applies_to='policy_value_entropy',raw_advantages_reconstructed_from_returns_minus_values=reconstructed,all_collected_decisions_used_for_gae=True)
 return ids,metadata

def groups(rollout,selected,minibatch,microbatch,rng,max_tokens):
 # Random length strata: every optimizer group gets a share of each length
 # stratum. Sorting happens only INSIDE an optimizer group for efficient padding.
 order=selected[rng.permutation(len(selected))]
 order=order[np.argsort(rollout.lengths[order,0],kind='stable')]
 n=(len(order)+minibatch-1)//minibatch
 bins=[[] for _ in range(n)]
 for start in range(0,len(order),n):
  stripe=order[start:start+n]
  for j,i in enumerate(rng.permutation(n)[:len(stripe)]):bins[i].append(stripe[j])
 for group in rng.permutation(n):
  ids=np.array(bins[group],np.int64);ids=ids[np.argsort(rollout.lengths[ids,0],kind='stable')]
  batches=[];at=0;lengths=rollout.lengths[ids,0]
  while at<len(ids):
   tokens=bucket(int(lengths[at]),TOKEN_BUCKETS);end=int(np.searchsorted(lengths,tokens,side='right'));limit=max(1,min(microbatch,max_tokens//tokens));stop=min(end,at+limit);batches.append(ids[at:stop]);at=stop
  assert sum(map(len,batches))==len(ids) and len(ids)<=minibatch
  yield batches
