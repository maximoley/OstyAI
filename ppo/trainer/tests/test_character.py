import sys,json,unittest,numpy as np
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from worker import Workers
from run_workers import RunWorkers
from fast_workers import FastWorkers
CHARACTER=json.loads((Path(__file__).resolve().parents[1]/'config.json').read_text())['environment']['character']
OTHER='Ironclad' if CHARACTER=='Necrobinder' else 'Necrobinder'

class CharacterContract(unittest.TestCase):
 def test_real_starter_deck(self):
  w=Workers(1,1,pinned=False)
  try:
   d=w.inspect(0);self.assertEqual(d['character'],CHARACTER)
   names=[name for name,upgraded in d['deck']]
   self.assertIn('Strike'+CHARACTER,names);self.assertIn('Defend'+CHARACTER,names)
   self.assertNotIn('Strike'+OTHER,names);self.assertIn('AscendersBane',names)
  finally:w.close()

 def test_wrong_character_fails_before_pool_creation(self):
  with self.assertRaisesRegex(RuntimeError,'does not match requested'):
   RunWorkers(1,1,pinned=False,character=OTHER)
 def test_starts_restore_and_wrong_character_journal(self):
  w=FastWorkers(16,4,pinned=False)
  try:
   w.refill(2048);w.observe_all();audit=w.audit_character()
   self.assertEqual(audit['active'],{CHARACTER:16});self.assertEqual(audit['ready'],{CHARACTER:2048})
   self.assertEqual({s['character'] for s in w.info()['slots']},{CHARACTER})
   j=w.save_journal();self.assertEqual(j['character'],CHARACTER);self.assertEqual(j['version'],2)
   bad=dict(j,character=OTHER)
   with self.assertRaisesRegex(RuntimeError,'journal layout mismatch'):w.load_journal(bad)
   self.assertEqual(j,w.save_journal())
   w.load_journal(j);self.assertEqual(j,w.save_journal());self.assertEqual(audit,w.audit_character())
  finally:w.close()
 def test_cached_vs_reference_workers_selected_character(self):
  a=RunWorkers(16,1,pinned=False);b=FastWorkers(16,80,pinned=False);rng=np.random.default_rng(29)
  try:
   a.refill(2048);b.refill(2048)
   for _ in range(256):
    a.observe_all();b.observe_all()
    for name in ['counts','lengths','private_lengths','metadata']:np.testing.assert_array_equal(getattr(a,name),getattr(b,name))
    for i in range(a.n):
     np.testing.assert_array_equal(a.raw[i,:a.lengths[i]],b.raw[i,:b.lengths[i]])
     np.testing.assert_array_equal(a.private[i,:a.private_lengths[i]],b.private[i,:b.private_lengths[i]])
    acts=np.array([rng.integers(c) for c in a.counts],np.int32)
    np.testing.assert_array_equal(a.advance(acts),b.advance(acts))
    a.reset();b.reset()
   self.assertEqual(a.save_journal(),b.save_journal())
   self.assertEqual(a.audit_character(),b.audit_character())
  finally:a.close();b.close()
if __name__=='__main__':unittest.main()
