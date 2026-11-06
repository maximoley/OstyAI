import copy,json,sys,unittest
from pathlib import Path
from datetime import datetime,timezone
from zoneinfo import ZoneInfo
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from training_schedule import LRControl,timestamp,deadline_reached,next_rollout_size
CFG=json.loads((Path(__file__).resolve().parents[1]/'config.json').read_text())
class ScheduleTests(unittest.TestCase):
 def test_timezones(self):
  s=CFG['schedule']
  for key,day in [('anneal_start',11),('stop_at',12)]:
   d=datetime.fromtimestamp(timestamp(s[key]),ZoneInfo('America/New_York'))
   self.assertEqual((d.year,d.month,d.day,d.hour),(2026,11,day,6))
   self.assertEqual(d.astimezone(timezone.utc).hour,11)
  self.assertEqual(datetime.fromtimestamp(timestamp(s['anneal_start']),timezone.utc).weekday(),2)
 def test_anneal(self):
  ctl=LRControl(CFG);start=ctl.start
  for delta,want in [(-1,1),(0,1),(43200,.65),(86400,.3),(99999,.3)]:
   self.assertAlmostEqual(ctl.snapshot(1.2,now=start+delta)['anneal_factor'],want)
  for x,y in zip(ctl.snapshot(1.2,now=start+43200)['learning_rates'],[1e-4*1.2*.65,3e-4*1.2*.65]):self.assertAlmostEqual(x,y)
 def test_adaptation_bounds_and_warmup(self):
  ctl=LRControl(CFG,lambda:timestamp(CFG['schedule']['anneal_start'])-100)
  self.assertEqual(ctl.adapt(1.,.004),1.5);self.assertAlmostEqual(ctl.adapt(1.,.021),2/3)
  self.assertEqual(ctl.adapt(2.,.004),2.);self.assertEqual(ctl.adapt(.3,.021),.3)
  self.assertEqual(ctl.adapt(1.,.005),1.);self.assertEqual(ctl.adapt(1.,.02),1.)
  self.assertAlmostEqual(ctl.snapshot(1.,.1)['learning_rates'][0],1e-5)
 def test_freeze_and_restore(self):
  now=[timestamp(CFG['schedule']['anneal_start'])];ctl=LRControl(CFG,lambda:now[0])
  self.assertEqual(ctl.adapt(1.5,.001),1.5)
  self.assertEqual(ctl.adapt(1.5,.1),1.5)
  now[0]+=43200;again=LRControl(CFG,lambda:now[0]);again.load_state_dict(ctl.state_dict())
  self.assertEqual(again.snapshot(.3)['multiplier'],1.5)
  self.assertAlmostEqual(again.snapshot(.3)['learning_rates'][0],.00015*.65)
 def test_stop_boundary(self):
  end=timestamp(CFG['schedule']['stop_at'])
  self.assertFalse(deadline_reached(CFG,end-.001));self.assertTrue(deadline_reached(CFG,end))
 def test_exact_cap(self):
  cap=CFG['max_collected_decisions'];n=CFG['rollout_steps']
  self.assertEqual((cap+n-1)//n,CFG['total_policy_updates'])
  before=cap//n*n;self.assertEqual(next_rollout_size(CFG,before),25600)
  self.assertEqual(next_rollout_size(CFG,before)+before,cap)
  self.assertEqual(next_rollout_size(CFG,cap),0)
 def test_bad_partial_batch_rejected(self):
  c=copy.deepcopy(CFG);c['max_collected_decisions']+=1
  with self.assertRaises(ValueError):next_rollout_size(c,CFG['max_collected_decisions'])
if __name__=='__main__':unittest.main()
