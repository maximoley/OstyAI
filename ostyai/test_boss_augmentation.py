import unittest
from boss_augmentation import classify, verify_additions

REGISTRY = {'QUEEN_BOSS': 'boss', 'TOADPOLES_WEAK': 'easy', 'KNIGHTS_ELITE': 'elite'}
def row(name, kind='human', encounter='QUEEN_BOSS', room='boss', run='train'):
    return dict(source=kind+':'+name, metadata=dict(encounter=encounter, room_type=room, run_hash=run))

class Tests(unittest.TestCase):
    def test_double_bosses_preserves_other_pools_and_sources(self):
        base=[row('a'),row('b','generated'),row('c',encounter='TOADPOLES_WEAK',room='monster'),row('d',encounter='unknown',room='normal'),row('e',encounter='KNIGHTS_ELITE',room='elite')]
        counts=verify_additions(base,[row('f'),row('g','generated')],REGISTRY,{'heldout'})
        self.assertEqual(counts['final']['by_pool'],dict(easy=1,hard=1,elite=1,boss=4))
    def test_reject_duplicate(self):
        with self.assertRaisesRegex(ValueError,'Duplicate'):verify_additions([row('a')],[row('a')],REGISTRY,set())
    def test_reject_nonboss(self):
        with self.assertRaisesRegex(ValueError,'Only boss'):verify_additions([row('a')],[row('b',encounter='unknown',room='normal')],REGISTRY,set())
    def test_reject_holdout(self):
        with self.assertRaisesRegex(ValueError,'Held-out'):verify_additions([row('a')],[row('b',run='heldout')],REGISTRY,{'heldout'})
    def test_reject_source_mixture_change(self):
        with self.assertRaisesRegex(ValueError,'source stratum'):verify_additions([row('a')],[row('b','generated')],REGISTRY,set())
    def test_zero_bosses(self):
        base=[row('a',encounter='TOADPOLES_WEAK',room='monster')]
        self.assertEqual(verify_additions(base,[],REGISTRY,set())['final']['by_pool']['boss'],0)
    def test_native_registry_takes_precedence(self):
        self.assertEqual(classify(row('a',room='monster'),REGISTRY),'boss')
        self.assertEqual(classify(row('a',encounter='ENCOUNTER.QUEEN_BOSS'),REGISTRY),'boss')
    def test_fail_closed(self):
        with self.assertRaisesRegex(ValueError,'Cannot classify'):classify(row('a',encounter='unknown',room='monster'),REGISTRY)

if __name__=='__main__':unittest.main()
