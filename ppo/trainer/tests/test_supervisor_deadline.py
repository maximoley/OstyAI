import json,sys,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import supervise_run as s
CFG=json.loads((Path(__file__).resolve().parents[1]/'config.json').read_text())
class Tests(unittest.TestCase):
 def test_expired_supervisor_never_queries_gpu_or_launches(self):
  with tempfile.TemporaryDirectory(dir=Path(__file__).resolve().parent) as folder:
   root=Path(folder);cfg=dict(CFG,schedule=dict(CFG['schedule'],stop_at='2000-01-01T00:00:00+00:00'))
   (root/'config.json').write_text(json.dumps(cfg))
   with patch.object(s,'R',root),patch.object(s,'O',root/'production'),patch.object(s,'foreign_gpu',side_effect=AssertionError('Should not query GPU')),patch.object(s.subprocess,'Popen',side_effect=AssertionError('Should not launch')):
    s.main()
   status=json.loads((root/'production/service.json').read_text());self.assertEqual(status['stage'],'scheduled_stop')
 def test_stop_file_wins(self):
  with tempfile.TemporaryDirectory(dir=Path(__file__).resolve().parent) as folder:
   root=Path(folder);(root/'STOP').write_text('test');(root/'config.json').write_text(json.dumps(CFG))
   with patch.object(s,'R',root),patch.object(s,'O',root/'production'),patch.object(s,'foreign_gpu',side_effect=AssertionError('Should not query GPU')):
    s.main()
   self.assertEqual(json.loads((root/'production/service.json').read_text())['stage'],'stopped')
if __name__=='__main__':unittest.main()
