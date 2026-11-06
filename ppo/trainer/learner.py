"""One-epoch, decoupled PPO with conditional entropy, KL adaptation and EMAs."""
import math,time
import numpy as np
import torch
from filtering import select,groups
from returns import Dataset,AdvantageMoments
from training_schedule import LRControl,deadline_reached

class Learner:
    def __init__(self,model,cfg,device='cuda',clock=time.time):
        self.model=model;self.cfg=cfg;self.device=device;self.updates=0;self.optimizer_steps=0;self.lr_multiplier=1.;self.decisions=0
        self.clock=clock;self.schedule=LRControl(cfg,clock)
        critic=[];trunk=[]
        for name,p in model.named_parameters():
            (critic if name.startswith(('privileged.','combat_value.','noncombat_value.')) else trunk).append(p)
        self.optimizer=torch.optim.AdamW([dict(params=trunk,lr=cfg['learning_rate'],base_lr=cfg['learning_rate']),dict(params=critic,lr=cfg['critic_learning_rate'],base_lr=cfg['critic_learning_rate'])],weight_decay=cfg['weight_decay'],fused=device=='cuda')
        torch._dynamo.config.cache_size_limit=128
        self.forward=torch.compile(model.forward,fullgraph=True,dynamic=True) if cfg['compile'] and device=='cuda' else model.forward
        self.moments=AdvantageMoments(cfg['advantage_normalization'])
        self.low_entropy=[0,0];self.boosts=[0,0]
        self.ema={str(d):{k:v.detach().clone() for k,v in model.state_dict().items()} for d in cfg['ema_decays']}
    def autocast(self):return torch.autocast(self.device,dtype=torch.bfloat16,enabled=self.device=='cuda')
    def inputs(self,data,ids,aux=True):
        raw,private,a=data.batch(ids)
        arrays=[raw,private]
        tensors=[torch.from_numpy(x).pin_memory().to(self.device,non_blocking=True) if self.device=='cuda' else torch.from_numpy(x) for x in arrays]
        if aux:tensors += [torch.as_tensor(x[ids],device=self.device) for x in [data.actions,data.logp,data.prox_logp,data.advantages,data.returns,data.combat]]
        return *tensors[:2],a,*tensors[2:]
    @torch.no_grad()
    def refresh(self,rows):
        if not rows:return
        data=Dataset(rows);self.model.eval()
        order=np.argsort(data.lengths[:,0],kind='stable');at=0
        while at<len(order):
            t=int(data.lengths[order[at],0]);end=at+1
            while end<len(order) and end-at<self.cfg['microbatch'] and int(data.lengths[order[end],0])<=max(t*1.25,t+16) and (end-at+1)*int(data.lengths[order[end],0])<=self.cfg['max_microbatch_tokens']:end+=1
            ids=order[at:end];raw,private,a=self.inputs(data,ids,False)
            with self.autocast():logits,v,legal=self.forward(raw,private,a)
            action=torch.as_tensor(data.actions[ids],device=self.device);lp=logits.float().log_softmax(-1).gather(1,action[:,None])[:,0]
            values=v.cpu().numpy();logp=lp.cpu().numpy()
            for j,i in enumerate(ids):rows[int(i)].value=float(values[j]);rows[int(i)].prox_logp=float(logp[j])
            at=end
    def coefficients(self):
        e=self.cfg['entropy'];f=min(1.,self.decisions/e['schedule_decisions'])
        return [e[k][0]*(e[k][1]/e[k][0])**f*1.5**self.boosts[i] for i,k in enumerate(['noncombat','combat'])]
    def loss(self,raw,private,a,actions,behavior_logp,prox_logp,advantages,returns,combat):
        logits,v,legal=self.forward(raw,private,a);lp=logits.float().log_softmax(-1);taken=lp.gather(1,actions[:,None])[:,0]
        ratio=(taken-prox_logp).exp();importance=(prox_logp-behavior_logp).exp().clamp_max(self.cfg['importance_cap']).detach()
        surrogate=torch.minimum(ratio*advantages,ratio.clamp(1-self.cfg['clip_ratio'],1+self.cfg['clip_ratio'])*advantages)
        surrogate=torch.where(advantages<0,torch.maximum(surrogate,self.cfg['dual_clip']*advantages),surrogate)
        pg=-(importance*surrogate).mean();vf=(v.float()-returns).square().mean()
        entropy=-(lp.exp()*lp.masked_fill(~legal,0.)).sum(-1);conditional=entropy/legal.sum(-1).float().clamp_min(2).log()
        co=self.coefficients();coef=torch.where(combat,co[1],co[0]);ent_loss=-(coef*conditional).mean()
        loss=pg+self.cfg['value_coef']*vf+ent_loss
        delta=taken-prox_logp
        metrics=torch.stack([pg.detach(),vf.detach(),entropy.mean().detach(),conditional.mean().detach(),((ratio-1)-delta).mean().detach(),((ratio-1).abs()>self.cfg['clip_ratio']).float().mean().detach(),importance.mean().detach(),entropy.masked_fill(combat,0.).mean().detach(),(~combat).float().mean(),entropy.masked_fill(~combat,0.).mean().detach(),combat.float().mean()])
        return loss,metrics
    @torch.no_grad()
    def make_probe(self,data,rng):
        ids=rng.permutation(data.steps)[:self.cfg['kl_probe_rows']];probe=[]
        for batches in groups(data,ids,len(ids),self.cfg['microbatch'],rng,self.cfg['max_microbatch_tokens']):
            for ix in batches:
                inputs=self.inputs(data,ix,False)
                with self.autocast():logits,_,legal=self.forward(*inputs)
                probe.append((inputs,logits.float().log_softmax(-1).detach(),legal.detach()))
        return probe
    @torch.no_grad()
    def probe_kl(self,probe):
        total=torch.zeros((),device=self.device);n=0
        for inputs,old,legal in probe:
            with self.autocast():logits,_,_=self.forward(*inputs)
            new=logits.float().log_softmax(-1)
            total+=(old.exp()*(old.masked_fill(~legal,0)-new.masked_fill(~legal,0))).sum();n+=len(old)
        return float((total/n).cpu())
    def train(self,data,rng,collected):
        start=time.perf_counter();self.decisions+=collected;self.moments.normalize(data)
        selected,selection=select(data,self.cfg)
        self.model.train();lr_warm=min(1.,(self.updates+1)/self.cfg['warmup_updates'])
        def set_rates():
            snap=self.schedule.snapshot(self.lr_multiplier,lr_warm)
            for g,lr in zip(self.optimizer.param_groups,snap['learning_rates']):g['lr']=lr
            return snap
        lr_state=set_rates()
        if not len(selected):raise RuntimeError('Advantage filter retained no examples; objective was not silently changed')
        probe=self.make_probe(data,rng);pack=time.perf_counter()
        batches_plan=list(groups(data,selected,self.cfg['optimizer_minibatch'],self.cfg['microbatch'],rng,self.cfg['max_microbatch_tokens']))
        prepared=[];seen=np.zeros(data.steps,np.int32)
        for batches in batches_plan:
            count=sum(map(len,batches));prepared.append((count,[(len(ix),self.inputs(data,ix)) for ix in batches]))
            for ix in batches:seen[ix]+=1
        assert np.array_equal(np.flatnonzero(seen),selected) and (seen[selected]==1).all()
        if self.device=='cuda':torch.cuda.synchronize()
        pack_seconds=time.perf_counter()-pack;reports=[];counts=[];checks=[];stop_reason=None;kl=None;step_rates=[]
        for count,batches in prepared:
            self.optimizer.zero_grad(set_to_none=True);metrics=[]
            if deadline_reached(self.cfg,self.clock()):stop_reason='wall_deadline';break
            lr_state=set_rates()
            for size,inputs in batches:
                if deadline_reached(self.cfg,self.clock()):stop_reason='wall_deadline';break
                with self.autocast():loss,m=self.loss(*inputs)
                (loss*(size/count)).backward();metrics.append(m*(size/count))
            if stop_reason or deadline_reached(self.cfg,self.clock()):
                self.optimizer.zero_grad(set_to_none=True);stop_reason='wall_deadline';break
            norm=torch.nn.utils.clip_grad_norm_(self.model.parameters(),self.cfg['max_grad_norm'],foreach=True)
            assert torch.isfinite(norm),'Nonfinite gradient; refusing optimizer step'
            # Synchronizing the finite-gradient check can cross the wall deadline.
            if deadline_reached(self.cfg,self.clock()):
                self.optimizer.zero_grad(set_to_none=True);stop_reason='wall_deadline';break
            lr_state=set_rates()
            if deadline_reached(self.cfg,self.clock()):
                self.optimizer.zero_grad(set_to_none=True);stop_reason='wall_deadline';break
            self.optimizer.step();self.optimizer_steps+=1;step_rates.append(lr_state)
            reports.append(torch.cat([torch.stack(metrics).sum(0),norm.detach()[None]]));counts.append(count)
            kl=self.probe_kl(probe);assert math.isfinite(kl) and kl>=-1e-5
            checks.append(dict(optimizer_step=self.optimizer_steps,kl=kl,examples=sum(counts)))
            if kl>=self.cfg['kl_stop']:stop_reason='kl_hard_stop';break
        if reports:self.lr_multiplier=self.schedule.adapt(self.lr_multiplier,kl)
        if reports:
            arr=torch.stack(reports).cpu().numpy();assert np.isfinite(arr).all()
            means=np.average(arr,axis=0,weights=counts)
            for i,(sm,ct) in enumerate([(7,8),(9,10)]):
                raw=float(sum(arr[j,sm]*counts[j] for j in range(len(arr)))/max(1,sum(arr[j,ct]*counts[j] for j in range(len(arr)))))
                self.low_entropy[i]=self.low_entropy[i]+1 if raw<self.cfg['entropy']['low_entropy_threshold'] else 0
                if self.low_entropy[i]>=self.cfg['entropy']['low_entropy_updates'] and self.boosts[i]<self.cfg['entropy']['max_boosts']:self.boosts[i]+=1;self.low_entropy[i]=0
            metrics_out=dict(zip(['policy_loss','value_loss','entropy','normalized_entropy','approx_kl','clip_fraction','importance_weight'],map(float,means[:7])),grad_norm=float(means[-1]))
            with torch.no_grad():
                current=self.model.state_dict()
                for decay,ema in self.ema.items():
                    for k,v in current.items():ema[k].lerp_(v,1-float(decay))
        else:metrics_out={}
        self.updates+=1
        if self.device=='cuda':torch.cuda.synchronize()
        trained=sum(counts)
        return dict(seconds=time.perf_counter()-start,pack_and_transfer_seconds=pack_seconds,epochs=1,
            optimizer_steps=len(reports),trained_decisions=trained,unique_decisions=trained,selected_decisions=len(selected),
            skipped_selected_decisions=len(selected)-trained,eligible_rows=data.steps,new_collected_decisions=collected,
            filter=selection,exact_visits_per_selected_example=1 if trained==len(selected) else None,
            selected_rows_visited_once=trained==len(selected),kl_probe_rows=sum(len(x[1]) for x in probe),
            kl_exact_categorical=kl,kl_checks=checks,kl_above_stop=kl is not None and kl>=self.cfg['kl_stop'],
            early_stop=stop_reason is not None,stop_reason=stop_reason,
            kl_stop_reason='stop remaining minibatches; retain crossing optimizer step' if stop_reason=='kl_hard_stop' else None,
            lr_multiplier_next=self.lr_multiplier,lr_schedule=lr_state,step_learning_rates=step_rates,
            learning_rates=[g['lr'] for g in self.optimizer.param_groups],entropy_coefficients=self.coefficients(),
            advantage_moments=self.moments.stats,metrics=metrics_out,
            minibatches=[dict(examples=n,combat_fraction=float(data.combat[np.concatenate(ix)].mean()),mean_tokens=float(data.lengths[np.concatenate(ix),0].mean())) for n,ix in zip(counts,batches_plan)])
    def state_dict(self):
        return dict(updates=self.updates,optimizer_steps=self.optimizer_steps,lr_multiplier=self.lr_multiplier,decisions=self.decisions,moments=self.moments.stats,low_entropy=self.low_entropy,boosts=self.boosts,ema=self.ema,schedule=self.schedule.state_dict())
    def load_state_dict(self,s):
        for k in ['updates','optimizer_steps','lr_multiplier','decisions','low_entropy','boosts','ema']:setattr(self,k,s[k])
        self.moments.stats=s['moments'];self.schedule.load_state_dict(s['schedule'])
