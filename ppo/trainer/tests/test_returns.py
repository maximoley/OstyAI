import sys,unittest,json
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import numpy as np
from returns import Row,Returns,Dataset,AdvantageMoments
from filtering import groups,select
CFG=json.loads((Path(__file__).resolve().parents[1]/'config.json').read_text())
def row(uid,n,combat,value,reward=0.,version=0,**kw):
    public=np.zeros((8,16),np.int32);public[0,6]=n;public[0,8]=combat;public[0,9]=2 if combat else 1;public[0,10]=7;public[0,12]=2
    return Row(public,np.zeros((1,16),np.int32),0,-.7,value,reward,uid,0,combat,version,**kw)
class Targets(unittest.TestCase):
    def test_practice_bootstraps_run_value(self):
        r=row(1,1,True,.2,.05,segment_end=True,boundary_value=.7)
        m=Returns(CFG);m.ingest([r]);d,_=m.finish([r],{},0)
        self.assertAlmostEqual(float(d.returns[0]),.75,places=6)
        r=row(1,1,True,.2,0.,segment_end=True,terminal=True,boundary_value=.7)
        m=Returns(CFG);m.ingest([r]);d,_=m.finish([r],{},0);self.assertEqual(d.returns[0],0.)
    def test_combat_lambda_stops_at_ooc(self):
        rs=[row(1,0,True,.1,.002),row(1,1,True,.2,.05),row(1,2,False,.6)]
        m=Returns(CFG);m.ingest(rs);d,_=m.finish(rs,{1:(.7,False)},0)
        self.assertAlmostEqual(rs[1].advantage,.45);self.assertAlmostEqual(rs[0].advantage,.102+.95*.45)
    def test_ooc_uses_intervening_combat_rewards(self):
        rs=[row(1,0,False,.1),row(1,1,True,.2,.05),row(1,2,False,.4),row(1,3,True,.5,1.,terminal=True,segment_end=True)]
        m=Returns(CFG);m.ingest(rs);d,report=m.finish(rs,{},0)
        self.assertEqual(report['ooc_released'],2)
        self.assertAlmostEqual(rs[2].target,1.)
        self.assertAlmostEqual(rs[0].target,.1+(.05+.4-.1)+.97*(1.-.4))
    def test_four_rollout_hold_and_once_only(self):
        m=Returns(CFG);r=row(9,0,False,.2,.002);m.ingest([r])
        for u in range(3):
            d,_=m.finish([r] if u==0 else [],{9:(.3,True)},u);self.assertEqual(d.steps,0)
        d,_=m.finish([],{9:(.3,True)},3);self.assertEqual(d.steps,1)
        d,_=m.finish([],{9:(.3,True)},4);self.assertEqual(d.steps,0)
    def test_mixed_length_minibatches_exact(self):
        rs=[row(i,0,i%2==0,.1) for i in range(20000)]
        d=Dataset(rs);d.lengths[:,0]=np.arange(20000)%400+10
        seen=[];means=[]
        for g in groups(d,np.arange(d.steps),8192,1024,np.random.default_rng(1),98304):
            ids=np.concatenate(g);seen.extend(ids);means.append(d.lengths[ids,0].mean())
        self.assertEqual(sorted(seen),list(range(d.steps)));self.assertLess(max(means)-min(means),3)
    def test_filter_keeps_existing_quantile_and_floor(self):
        d=Dataset([row(i,0,True,.1) for i in range(100)]);d.raw_advantages=np.linspace(-1,1,100,dtype=np.float32)
        ids,meta=select(d,CFG);self.assertEqual(len(ids),40);self.assertEqual(meta['minimum_absolute_advantage'],.001)
    def test_normalization_is_by_decision_type_not_lane(self):
        d=Dataset([row(1,0,True,.1),row(2,0,True,.1),row(3,0,False,.1),row(4,0,False,.1)])
        d.tasks=np.array([0,1,0,1]);d.raw_advantages=np.array([1,3,100,104],np.float32)
        m=AdvantageMoments(CFG['advantage_normalization']);m.normalize(d);np.testing.assert_allclose(d.advantages,[-1,1,-1,1])
if __name__=='__main__':unittest.main()
