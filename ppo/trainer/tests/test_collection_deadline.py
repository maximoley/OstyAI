import sys,unittest,types
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from collect_fast import collect
class Workers:
 def __init__(self):
  self.n=2;self.raw=np.zeros((2,1,16),np.int32);self.raw[:,0,8]=1;self.private=self.raw.copy();self.lengths=np.ones(2,int);self.private_lengths=self.lengths.copy();self.counts=np.full(2,2);self.metadata=np.zeros((2,5),np.int32);self.metadata[:,1]=[1,2];self.steps=0
 def observe_all(self):pass
 def advance(self,a):self.steps+=1;return np.zeros((2,8),np.float32)
 def reset(self):pass
class Tests(unittest.TestCase):
 def test_retains_partial_rows_without_extra_transition(self):
  w=Workers();a=types.SimpleNamespace(run=lambda w:(np.zeros(2,int),np.zeros(2),np.ones(2)))
  rows,tails,completed,stats=collect(w,a,dict(rollout_steps=8,rewards=dict(floor=.002,elite=0,boss=.05,run_win=1)),0,should_stop=lambda:w.steps>=1)
  self.assertEqual((w.steps,len(rows)),(1,2));self.assertEqual(tails,{});self.assertFalse(stats['complete']);self.assertEqual(stats['expected_rows'],8)
 def test_check_again_after_inference(self):
  w=Workers();expired=[False]
  def run(w):expired[0]=True;return np.zeros(2,int),np.zeros(2),np.ones(2)
  rows,_,_,_=collect(w,types.SimpleNamespace(run=run),dict(rollout_steps=8,rewards=dict(floor=.002,elite=0,boss=.05,run_win=1)),0,should_stop=lambda:expired[0])
  self.assertEqual((w.steps,len(rows)),(0,0))
if __name__=='__main__':unittest.main()
