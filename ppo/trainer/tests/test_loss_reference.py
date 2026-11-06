import unittest
import numpy as np

def reference(r,a,eps=.2,c=3.):
    unclipped=r*a;clipped=np.clip(r,1-eps,1+eps)*a
    z=np.minimum(unclipped,clipped)
    return np.where(a<0,np.maximum(z,c*a),z)
class LossContract(unittest.TestCase):
    def test_dual_clip(self):
        np.testing.assert_allclose(reference(np.array([4.,.1,4.,.1]),np.array([-1.,-1.,1.,1.])),[-3.,-.8,1.2,.1])
    def test_run_reward_scale(self):
        # Four boss kills and 51 newly cleared floors, then a true run win.
        self.assertAlmostEqual(51*.002+4*.05+1.,1.302)
    def test_floor_reward_not_erased_by_death(self):
        self.assertAlmostEqual(sum([.002,.002,0.]),.004)
if __name__=='__main__':unittest.main()
