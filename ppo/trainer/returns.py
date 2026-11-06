"""Run-utility targets with separate combat and out-of-combat GAE traces.

OOC observations are held until episode/segment completion or four rollouts.
Bootstrap-only observations never become training rows. A practice boundary is
NOT a true terminal: its run critic value survives the segment-end mask.
"""
from dataclasses import dataclass
import numpy as np
from rollout import bucket,TOKEN_BUCKETS,PRIVATE_BUCKETS,ACTION_BUCKETS

@dataclass
class Row:
    public:np.ndarray
    private:np.ndarray
    action:int
    behavior_logp:float
    value:float
    reward:float
    uid:int
    lane:int
    combat:bool
    version:int
    terminal:bool=False
    segment_end:bool=False
    boundary_value:float=0.
    rsum:float=0.
    advantage:float=0.
    target:float=0.
    prox_logp:float=None
    def __post_init__(self):
        if self.prox_logp is None:self.prox_logp=self.behavior_logp

class Dataset:
    def __init__(self,rows):
        self.records=rows;self.steps=len(rows)
        self.lengths=np.array([[len(r.public),len(r.private)] for r in rows],np.int32).reshape(-1,2)
        self.actions=np.array([r.action for r in rows],np.int64)
        self.logp=np.array([r.behavior_logp for r in rows],np.float32)
        self.prox_logp=np.array([r.prox_logp for r in rows],np.float32)
        self.values=np.array([r.value for r in rows],np.float32)
        self.raw_advantages=np.array([r.advantage for r in rows],np.float32)
        self.advantages=self.raw_advantages.copy();self.returns=np.array([r.target for r in rows],np.float32)
        self.tasks=np.array([r.lane for r in rows],np.int32)
        self.combat=np.array([r.combat for r in rows],bool)
        self.phases=np.array([r.public[0,9] for r in rows],np.int32)
        self.action_counts=np.array([r.public[0,12] for r in rows],np.int32)
        self.versions=np.array([r.version for r in rows],np.int32)
    def batch(self,ids):
        t=bucket(int(self.lengths[ids,0].max()),TOKEN_BUCKETS);p=bucket(int(self.lengths[ids,1].max()),PRIVATE_BUCKETS);a=bucket(int(self.action_counts[ids].max()),ACTION_BUCKETS)
        raw=np.zeros((len(ids),t,16),np.int32);private=np.zeros((len(ids),p,16),np.int32)
        for j,i in enumerate(ids):
            r=self.records[int(i)];raw[j,:len(r.public)]=r.public;private[j,:len(r.private)]=r.private
        return raw,private,a

class Returns:
    def __init__(self,cfg):
        self.cfg=cfg;self.pending={};self.ended={};self.released=0
    def ingest(self,rows):
        for r in rows:
            if not r.combat:
                self.pending.setdefault(r.uid,[]).append(r)
            if r.uid in self.pending:
                self.pending[r.uid][-1].rsum+=r.reward
            if r.segment_end:
                self.ended[r.uid]=0. if r.terminal else r.boundary_value
    def held_rows(self,update):
        return [r for rs in self.pending.values() for r in rs if r.version<update]
    def finish(self,rows,tails,update):
        # Combat GAE ends its lambda trace at an OOC successor, but its TD
        # residual still uses that successor's run value.
        by_uid={}
        for r in rows:by_uid.setdefault(r.uid,[]).append(r)
        combat=[]
        for uid,rs in by_uid.items():
            next_adv=0.
            for i in range(len(rs)-1,-1,-1):
                r=rs[i]
                if not r.combat:
                    next_adv=0.;continue
                if r.segment_end:
                    nv=0. if r.terminal else r.boundary_value;nc=False
                elif i+1<len(rs):nv=rs[i+1].value;nc=rs[i+1].combat
                else:nv,nc=tails[uid]
                delta=r.reward+nv-r.value
                r.advantage=delta+(self.cfg['combat_lambda']*next_adv if nc and not r.segment_end else 0.)
                r.target=r.value+r.advantage;next_adv=r.advantage;combat.append(r)
        released=[];ages=[];bootstrap_segments=0
        for uid,rs in list(self.pending.items()):
            age=update+1-rs[0].version
            if uid not in self.ended and age<self.cfg['out_of_combat_hold_updates']:continue
            if uid in self.ended:tail=self.ended.pop(uid)
            else:tail=tails[uid][0]
            adv=0.;nv=tail
            for r in reversed(rs):
                adv=r.rsum+nv-r.value+self.cfg['run_lambda']*adv
                r.advantage=adv;r.target=r.value+adv;nv=r.value
            released.extend(rs);ages.extend([update-r.version for r in rs]);del self.pending[uid]
        # Completed combat-only practice segments have no pending OOC row.
        self.ended={uid:tail for uid,tail in self.ended.items() if uid in self.pending}
        self.released+=len(released)
        result=combat+released
        assert len({(r.uid,r.version,int(r.public[0,6]),int(r.public[0,7])) for r in result})==len(result),'A decision was fitted twice'
        assert all(np.isfinite(r.target) and np.isfinite(r.advantage) for r in result)
        return Dataset(result),dict(combat_rows=len(combat),ooc_released=len(released),ooc_held=sum(map(len,self.pending.values())),oldest_released_policy_age=max(ages,default=0),held_episodes=len(self.pending),total_ooc_released=self.released)
    def state_dict(self):return dict(pending=self.pending,ended=self.ended,released=self.released)
    def load_state_dict(self,state):self.pending=state['pending'];self.ended=state['ended'];self.released=state['released']

class AdvantageMoments:
    def __init__(self,cfg):self.cfg=cfg;self.stats={}
    def normalize(self,data):
        for kind in [False,True]:
            mask=data.combat==kind
            if not mask.any():continue
            x=data.raw_advantages[mask].astype(np.float64);mean=float(x.mean());second=float(np.square(x).mean());key=str(int(kind));old=self.stats.get(key)
            d=self.cfg['ew_decay']
            if old:mean=d*old[0]+(1-d)*mean;second=d*old[1]+(1-d)*second
            self.stats[key]=[mean,second]
            data.advantages[mask]=(x-mean)/max(self.cfg['epsilon'],max(0.,second-mean*mean)**.5)
        return self.stats
