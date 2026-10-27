"""Scheduled human budgets remain stable when training search budgets change."""
import unittest
from pathlib import Path
from unittest.mock import patch
import production_train as train

class HumanBudgetTests(unittest.TestCase):
    def test_human_budget_independent_of_training_and_primary(self):
        with patch.dict(train.SEARCH_BUDGETS,dict(easy=32,hard=64,elite=96,boss=256),clear=True):
            human=train.human_validation_config(Path('fixtures.jsonl'),36,2000000,6000)
            training=train.config(Path('bank.jsonl'),1200,6000,5999)
            primary=train.config(Path('fixtures.jsonl'),4096,1000000,6000,True)
            self.assertEqual(human['simulation_budgets'],dict(easy=32,hard=64,elite=96,boss=128))
            self.assertEqual(training['simulation_budgets']['boss'],256)
            self.assertEqual(primary['simulation_budgets']['boss'],256)
            for key in primary:
                if key not in ('simulation_budgets','fixtures','cases','episode','replay_capacity'):
                    self.assertEqual(human[key],primary[key],key)
            self.assertEqual(human['replay_capacity'],human['cases']*500)
            self.assertEqual(human['max_actions'],1000)
            self.assertEqual(human['workers'],80)
            human['simulation_budgets']['boss']=999
            self.assertEqual(train.human_validation_config('x',1,0,0)['simulation_budgets']['boss'],128)
            self.assertEqual(train.SEARCH_BUDGETS['boss'],256)
    def test_future_training_changes_do_not_change_human_baseline(self):
        with patch.dict(train.SEARCH_BUDGETS,dict(easy=64,hard=96,elite=128,boss=512),clear=True):
            self.assertEqual(train.human_validation_config('x',1,0,0)['simulation_budgets'],dict(easy=32,hard=64,elite=96,boss=128))

if __name__=='__main__':unittest.main()
