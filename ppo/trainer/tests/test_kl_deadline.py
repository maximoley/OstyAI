import copy,json,sys,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import torch,numpy as np
from learner import Learner
from returns import Row,Dataset
from training_schedule import timestamp
CFG=json.loads((Path(__file__).resolve().parents[1]/'config.json').read_text())
class Tiny(torch.nn.Module):
 def __init__(self):
  super().__init__();self.policy=torch.nn.Linear(1,3);self.privileged=torch.nn.Linear(1,2);self.combat_value=torch.nn.Linear(2,1);self.noncombat_value=torch.nn.Linear(2,1)
 def forward(self,raw,private,a):
  x=raw[:,0,:1].float()/10;z=self.policy(x);logits=torch.nn.functional.pad(z,(0,a-3),value=-1e9)
  legal=torch.arange(a,device=x.device)[None,:].expand(len(x),a)<3
  v=self.combat_value(self.privileged(x)).squeeze(-1)
  return logits,v,legal

def setup():
 torch.manual_seed(100);cfg=copy.deepcopy(CFG);cfg.update(compile=False,optimizer_minibatch=4,microbatch=2,kl_probe_rows=12)
 cfg['advantage_filter']['enabled']=False;now=[timestamp(cfg['schedule']['anneal_start'])-1000]
 m=Tiny();l=Learner(m,cfg,'cpu',clock=lambda:now[0]);rows=[]
 for i in range(12):
  r=np.zeros((1,16),np.int32);r[0,0]=i+1;r[0,8]=1;r[0,12]=3
  with torch.no_grad():z,v,_=m(torch.from_numpy(r[None]),torch.from_numpy(r[None]),8)
  rows.append(Row(r,r.copy(),0,float(z.log_softmax(-1)[0,0]),float(v[0]),0.,i,0,True,0,advantage=.2+.02*i,target=.4))
 return l,Dataset(rows),now
class LearnerGates(unittest.TestCase):
 def test_no_stop_visits_all(self):
  l,d,now=setup();l.probe_kl=lambda p:.01;r=l.train(d,np.random.default_rng(9),12)
  self.assertEqual(r['trained_decisions'],12);self.assertEqual(r['optimizer_steps'],3);self.assertEqual(len(r['kl_checks']),3)
  self.assertTrue(r['selected_rows_visited_once']);self.assertFalse(r['early_stop'])
 def test_kl_hard_stop_including_boundary(self):
  for kl in [.06,.07]:
   l,d,now=setup();l.probe_kl=lambda p:kl;r=l.train(d,np.random.default_rng(9),12)
   self.assertEqual(l.optimizer_steps,1);self.assertEqual(r['trained_decisions'],4);self.assertEqual(r['skipped_selected_decisions'],8)
   self.assertEqual(r['stop_reason'],'kl_hard_stop');self.assertAlmostEqual(l.lr_multiplier,2/3)
   self.assertIsNone(r['exact_visits_per_selected_example']);self.assertEqual(len(r['kl_checks']),1)
 def test_deadline_discards_incomplete_gradient(self):
  l,d,now=setup();before=copy.deepcopy(l.model.state_dict());ema=copy.deepcopy(l.ema);original=l.loss
  def loss(*args):
   out=original(*args);now[0]=timestamp(CFG['schedule']['stop_at']);return out
  l.loss=loss;r=l.train(d,np.random.default_rng(9),12)
  self.assertEqual(r['optimizer_steps'],0);self.assertEqual(r['stop_reason'],'wall_deadline');self.assertIsNone(r['kl_exact_categorical'])
  for k,v in before.items():self.assertTrue(torch.equal(v,l.model.state_dict()[k]));self.assertIsNone(dict(l.model.named_parameters())[k].grad)
  for decay,state in ema.items():
   for k,v in state.items():self.assertTrue(torch.equal(v,l.ema[decay][k]))
 def test_lr_frozen_and_checkpointed(self):
  l,d,now=setup();now[0]=timestamp(CFG['schedule']['anneal_start'])+43200;l.lr_multiplier=1.5;l.probe_kl=lambda p:.001
  r=l.train(d,np.random.default_rng(9),12);self.assertEqual(l.lr_multiplier,1.5)
  for step in r['step_learning_rates']:self.assertAlmostEqual(step['anneal_factor'],.65)
  again,_,_=setup();again.load_state_dict(l.state_dict());self.assertEqual(again.schedule.state_dict(),l.schedule.state_dict())
 def test_kl_probe_identical_model_zero(self):
  l,d,now=setup();probe=l.make_probe(d,np.random.default_rng(9));self.assertLess(abs(l.probe_kl(probe)),1e-7)
 def test_optimizer_parameter_groups(self):
  l,_,_=setup();value={id(p) for n,p in l.model.named_parameters() if n.startswith(('privileged.','combat_value.','noncombat_value.'))}
  self.assertEqual({id(p) for p in l.optimizer.param_groups[1]['params']},value)
  self.assertEqual([g['base_lr'] for g in l.optimizer.param_groups],[.0001,.0003])
  self.assertTrue(all(g['weight_decay']==.0001 for g in l.optimizer.param_groups))
if __name__=='__main__':torch.set_num_threads(1);unittest.main()
