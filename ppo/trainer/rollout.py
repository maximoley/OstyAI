"""Bounded on-policy, variable-length public/private observations and terminal-aware GAE."""
from dataclasses import dataclass
import numpy as np

TOKEN_BUCKETS=(48,56,64,96,128,160,192,256,384,512,768,1024,1536,2048)
ACTION_BUCKETS=(8,16,32,64,128,256,512,1024,2048)
PRIVATE_BUCKETS=(16,32,64,96,128,192,256,384,512)
def bucket(value,bounds):
 for b in bounds:
  if value<=b:return b
 return 1 << (int(value)-1).bit_length()  # No observation/action truncation; extend buckets geometrically.

def compute_gae(rewards,values,dones,bootstrap,gamma=1.,lam=.99):
 """Only actual run terminals zero the bootstrap; rollout boundaries do not."""
 rewards=np.asarray(rewards,np.float32);values=np.asarray(values,np.float32);dones=np.asarray(dones,bool)
 assert rewards.shape==values.shape==dones.shape
 advantages=np.empty_like(rewards);last=np.zeros(rewards.shape[1],np.float32);next_value=np.asarray(bootstrap,np.float32)
 for t in range(len(rewards)-1,-1,-1):
  continuing=(~dones[t]).astype(np.float32);delta=rewards[t]+gamma*next_value*continuing-values[t]
  last=delta+gamma*lam*continuing*last;advantages[t]=last;next_value=values[t]
 return advantages,advantages+values

class Rollout:
 def __init__(self,steps,slots,record_budget=128):
  assert steps%slots==0
  self.steps=steps;self.slots=slots;self.horizon=steps//slots;self.tick=0
  # Packed arenas grow, but never truncate. Typical capacity is far below max padded storage.
  self.public=np.empty((steps*record_budget,16),np.int32);self.private=np.empty((steps*32,16),np.int32)
  self.public_at=0;self.private_at=0
  self.offsets=np.zeros((steps,2),np.int64);self.lengths=np.zeros((steps,2),np.int32)
  self.actions=np.empty(steps,np.int32);self.logp=np.empty(steps,np.float32);self.values=np.empty(steps,np.float32)
  self.rewards=np.empty(steps,np.float32);self.dones=np.empty(steps,bool);self.versions=np.empty(steps,np.int32)
  self.phases=np.empty(steps,np.int32);self.combat=np.empty(steps,bool);self.action_counts=np.empty(steps,np.int32)
  self.tasks=np.zeros(steps,np.int32)
  self.events=np.empty((steps,4),np.int32)
 def _grow(self,key,need):
  old=getattr(self,key)
  if need>len(old):
   x=np.empty((max(need,len(old)*2),16),np.int32);x[:len(old)]=old;setattr(self,key,x)
 def append(self,public,private,pub_lengths,priv_lengths,actions,logp,values,events,reward_weights,version,reward_override=None,tasks=None):
  n=self.slots;start=self.tick*n;assert self.tick<self.horizon
  a=np.asarray(actions);assert ((a>=0)&(a<public[:,0,12])).all()
  pub_total=int(np.sum(pub_lengths));priv_total=int(np.sum(priv_lengths));self._grow('public',self.public_at+pub_total);self._grow('private',self.private_at+priv_total)
  for j in range(n):
   ix=start+j;pl=int(pub_lengths[j]);ql=int(priv_lengths[j]);self.offsets[ix]=[self.public_at,self.private_at];self.lengths[ix]=[pl,ql]
   self.public[self.public_at:self.public_at+pl]=public[j,:pl];self.private[self.private_at:self.private_at+ql]=private[j,:ql]
   self.public_at+=pl;self.private_at+=ql
  end=start+n;self.actions[start:end]=a;self.logp[start:end]=logp;self.values[start:end]=values
  self.events[start:end]=events[:,:4];self.rewards[start:end]=events[:,:4]@reward_weights if reward_override is None else reward_override;self.tasks[start:end]=0 if tasks is None else tasks;self.dones[start:end]=events[:,4].astype(bool)
  self.phases[start:end]=public[:,0,9];self.combat[start:end]=public[:,0,8];self.action_counts[start:end]=public[:,0,12];self.versions[start:end]=version;self.tick+=1
 def finish(self,bootstrap,gamma,lam,version):
  assert self.tick==self.horizon and np.all(self.versions==version)
  a,r=compute_gae(self.rewards.reshape(self.horizon,self.slots),self.values.reshape(self.horizon,self.slots),self.dones.reshape(self.horizon,self.slots),bootstrap,gamma,lam)
  self.raw_advantages=a.ravel().copy();self.advantages=a.ravel();self.returns=r.ravel();
  for task in np.unique(self.tasks):
   mask=self.tasks==task;self.advantages[mask]=(self.advantages[mask]-self.advantages[mask].mean())/(self.advantages[mask].std()+1e-8)
  assert np.isfinite(self.returns).all() and np.isfinite(self.advantages).all()
 def shuffled_groups(self,optimizer_steps,microbatch,rng,max_tokens=98304):
  """Length grouping, randomized minibatch order; each real example once per epoch."""
  assert self.steps%optimizer_steps==0
  large=self.steps//optimizer_steps
  # Jitter tie ordering, then token-sort; shuffle the resulting optimizer minibatches.
  order=rng.permutation(self.steps);order=order[np.argsort(self.lengths[order,0],kind='stable')]
  groups=[order[i:i+large] for i in range(0,self.steps,large)]
  for g in rng.permutation(len(groups)):
   ids=groups[g];t=bucket(int(self.lengths[ids,0].max()),TOKEN_BUCKETS)
   limit=min(microbatch,max(32,(max_tokens//t//32)*32));limit=min(limit,len(ids))
   yield [ids[i:i+limit] for i in range(0,len(ids),limit)]
 def batch(self,ids):
  t=bucket(int(self.lengths[ids,0].max()),TOKEN_BUCKETS);p=bucket(int(self.lengths[ids,1].max()),PRIVATE_BUCKETS);a=bucket(int(self.action_counts[ids].max()),ACTION_BUCKETS)
  raw=np.zeros((len(ids),t,16),np.int32);private=np.zeros((len(ids),p,16),np.int32)
  for j,ix in enumerate(ids):
   start,ps=self.offsets[ix];length,pl=self.lengths[ix];raw[j,:length]=self.public[start:start+length];private[j,:pl]=self.private[ps:ps+pl]
  return raw,private,a
 def describe(self):
  return dict(reward_by_task={str(k):dict(steps=int((self.tasks==k).sum()),sum=float(self.rewards[self.tasks==k].sum()),mean=float(self.rewards[self.tasks==k].mean()),terminals=int(self.dones[self.tasks==k].sum()),positive_terminal_rewards=int(((self.tasks==k)&self.dones&(self.rewards>0)).sum())) for k in np.unique(self.tasks)},task_steps={str(k):int((self.tasks==k).sum()) for k in np.unique(self.tasks)},steps=self.steps,slots=self.slots,horizon=self.horizon,public_records=self.public_at,private_records=self.private_at,packed_observation_bytes=(self.public_at+self.private_at)*64,phase_counts={str(k):int((self.phases==k).sum()) for k in np.unique(self.phases)},reward_components=self.events.sum(0).tolist(),mean_reward=float(self.rewards.mean()),terminal_runs=int(self.dones.sum()),public_length_percentiles=np.quantile(self.lengths[:,0],[0,.5,.95,1]).tolist(),single_behavior_policy_version=bool((self.versions==self.versions[0]).all()))
