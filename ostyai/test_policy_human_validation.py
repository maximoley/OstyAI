"""CPU-only integration checks for the policy-only human benchmark orchestration."""
import contextlib,hashlib,io,json,tempfile,unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
import secondary_validation as validation

class PolicyHumanValidationTests(unittest.TestCase):
    def test_metrics_cache_identity_and_zero_search_guard(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);fixtures=root/'fixtures';fixtures.mkdir()
            (root/'loop_native.so').write_bytes(b'test-native')
            rows=[dict(metadata=dict(case_id='c',run_hash='r',replicate=i,act=1),evaluation=dict(potion_costs=[0,1]))for i in range(3)]
            raw='\n'.join(json.dumps(r)for r in rows).encode()
            (fixtures/'part.jsonl').write_bytes(raw)
            manifest=dict(runs=[dict(run_hash='r',case_ids=['c'])],shards=[dict(file='part.jsonl',sha256=hashlib.sha256(raw).hexdigest())],requested_trials=3,conversion_errors=[])
            (fixtures/'manifest.json').write_text(json.dumps(manifest))
            gpu=SimpleNamespace(model=SimpleNamespace(state_dict=lambda:{}))
            mode={'policy':True};calls=[]
            def config(*args):
                return dict(simulations=0 if mode['policy'] else 32,simulation_budgets=None if mode['policy'] else dict(easy=32),skip_forced_actions=not mode['policy'],workers=80,persistent_search=False)
            def collect(native,gpu,cfg):
                calls.append(cfg)
                return dict(completed=3,fixture_potion_costs=True,fatal=False,workers=80,
                    **{k:0 for k in ['replay_added','simulations','search_transitions','forced_search_transitions','chance_samples','forced_actions']},
                    outcomes=[dict(id=i,win=i<2,terminal=True,truncated=False,initial_hp=50,hp=40 if i<2 else 0,potions=[dict(cost=0),dict(cost=1)])for i in range(3)])
            output=root/'result';output.mkdir()
            with patch.object(validation,'ROOT',root),contextlib.redirect_stdout(io.StringIO()):
                result=validation.evaluate(None,gpu,output,5500,collect,config,fixture_root=fixtures)
                self.assertEqual(result['trial_win_rate'],2/3)
                self.assertEqual(result['run_win_rate'],1)
                self.assertEqual(result['action_selection'],'legal_argmax')
                self.assertEqual(result['simulations'],0)
                self.assertFalse(result['skip_forced_actions'])
                self.assertEqual(result['workers'],80)
                self.assertFalse(calls[0]['record_replay'])
                self.assertTrue(calls[0]['fixture_potion_costs'])
                validation.evaluate(None,gpu,output,5500,collect,config,fixture_root=fixtures)
                self.assertEqual(len(calls),1,'Matching completed policy results should be reusable')
                mode['policy']=False
                with self.assertRaisesRegex(AssertionError,'action mode mismatch'):
                    validation.evaluate(None,gpu,output,5500,collect,config,fixture_root=fixtures)
                mode['policy']=True
                bad_output=root/'bad';bad_output.mkdir()
                def bad_collect(*args):
                    report=collect(*args);report['simulations']=1;return report
                with self.assertRaises(AssertionError):
                    validation.evaluate(None,gpu,bad_output,5500,bad_collect,config,fixture_root=fixtures)

if __name__=='__main__':unittest.main()
