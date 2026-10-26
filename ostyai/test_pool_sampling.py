import copy,json,tempfile,unittest
from pathlib import Path
from types import SimpleNamespace
import numpy as np
from pool_sampling import PoolPrioritySampler,POOL_CODES,POOL_NAMES,quotas
from pool_metadata import restore_legacy_pool_tags

class SamplingTests(unittest.TestCase):
    def fixture(self):
        tiers=np.repeat(np.arange(4,dtype=np.int8),[23,31,17,43]);n=len(tiers)
        priorities=np.geomspace(.001,200,n)
        groups={64:np.arange(0,n,3),96:np.arange(1,n,3),128:np.arange(2,n,3)}
        return priorities,tiers,groups
    def test_exact_quotas_and_packing(self):
        p,t,g=self.fixture();s=PoolPrioritySampler(p,t,g,np.random.default_rng(9));plan=s.plan(256,64)
        self.assertEqual(quotas(16384),dict(easy=1638,hard=3277,elite=4915,boss=6554))
        ids=np.concatenate([x[1] for x in plan]);self.assertEqual(len(plan),64)
        for name,code in POOL_CODES.items():self.assertEqual(int((t[ids]==code).sum()),quotas(16384)[name])
        for tokens,idx,w in plan:
            self.assertEqual(len(idx),256);self.assertTrue((s.tokens[idx]<=tokens).all());self.assertTrue(np.isfinite(w).all());self.assertTrue((w>0).all())
    def test_conditional_correction_and_cross_pool_independence(self):
        p,t,g=self.fixture();a=PoolPrioritySampler(p,t,g,np.random.default_rng(1));p2=p.copy();p2[t==3]*=1e8
        b=PoolPrioritySampler(p2,t,g,np.random.default_rng(1))
        for name in POOL_NAMES:
            np.testing.assert_allclose(a.probabilities[name],b.probabilities[name],rtol=1e-12)
            prob=a.probabilities[name];ids=a.members[name];loss=np.sin(ids)+ids**2;weights=1/(len(ids)*prob)
            self.assertAlmostEqual(float((prob*weights*loss).sum()),float(loss.mean()),places=10)
    def test_rng_resume(self):
        p,t,g=self.fixture();rng=np.random.default_rng(7);state=copy.deepcopy(rng.bit_generator.state)
        a=PoolPrioritySampler(p,t,g,rng).plan(256,64);rng.bit_generator.state=state;b=PoolPrioritySampler(p,t,g,rng).plan(256,64)
        for x,y in zip(a,b):
            self.assertEqual(x[0],y[0]);np.testing.assert_array_equal(x[1],y[1]);np.testing.assert_array_equal(x[2],y[2])
    def test_empty_bin_fails_explicitly(self):
        p,t,g=self.fixture();t[t==3]=2
        with self.assertRaisesRegex(ValueError,'boss'):PoolPrioritySampler(p,t,g,np.random.default_rng(0))
    def test_legacy_migration_preserves_other_state(self):
        with tempfile.TemporaryDirectory() as td:
            r=Path(td);src=r/'source/sts2-rs/crates/sts2-content/src/encounters';src.mkdir(parents=True)
            (src/'act_test.rs').write_text('id: "SEAPUNK_WEAK", tier: EncounterTier::Weak,')
            (r/'prepared').mkdir();meta=[dict(encounter='SEAPUNK_WEAK',room_type='monster')]+[dict(encounter='unknown',room_type=x)for x in ['normal','elite','boss']]
            (r/'prepared/bank-000004.jsonl').write_text(''.join(json.dumps(dict(metadata=m))+'\n'for m in meta))
            replay=SimpleNamespace(ids=np.array([(5<<32)|i for i in range(4)],np.uint64),count=4,tiers=np.full(4,-1,np.int8),rng=np.random.default_rng(22),priorities=np.arange(1,5,dtype=float))
            ids=replay.ids.copy();p=replay.priorities.copy();state=copy.deepcopy(replay.rng.bit_generator.state)
            report=restore_legacy_pool_tags(replay,r);self.assertEqual(report['migrated'],4);np.testing.assert_array_equal(replay.tiers,np.arange(4));np.testing.assert_array_equal(replay.ids,ids);np.testing.assert_array_equal(replay.priorities,p);self.assertEqual(replay.rng.bit_generator.state,state)
            self.assertEqual(restore_legacy_pool_tags(replay,r),dict(migrated=0))
if __name__=='__main__':unittest.main()
