"""Small durable operations shared by trainer, validator and supervisor."""
from pathlib import Path
import os,json,hashlib,time

def atomic_json(path,value):
 path=Path(path);path.parent.mkdir(parents=True,exist_ok=True);tmp=path.with_suffix(path.suffix+'.tmp')
 with tmp.open('w') as f:json.dump(value,f,indent=2,allow_nan=False);f.write('\n');f.flush();os.fsync(f.fileno())
 os.replace(tmp,path)
def append_json(path,value):
 with Path(path).open('a') as f:f.write(json.dumps(value,allow_nan=False)+'\n')
def append_json_many(path,values):
 values=list(values)
 if not values:return
 payload=''.join(json.dumps(value,allow_nan=False)+'\n' for value in values)
 with Path(path).open('a') as f:f.write(payload)

def sha(path):
 h=hashlib.sha256()
 with Path(path).open('rb') as f:
  for b in iter(lambda:f.read(8<<20),b''):h.update(b)
 return h.hexdigest()
def tensor_hash(model):
 h=hashlib.sha256()
 for name,v in sorted(model.state_dict().items()):
  x=v.detach().cpu().contiguous();h.update(name.encode());h.update(str(x.dtype).encode());h.update(str(tuple(x.shape)).encode());h.update(x.numpy().tobytes())
 return h.hexdigest()
def process_gpu():
 import subprocess
 p=subprocess.run(['nvidia-smi','--query-compute-apps=pid,process_name,used_memory','--format=csv,noheader'],capture_output=True,text=True,timeout=15)
 if p.returncode:raise RuntimeError('nvidia-smi failed: '+p.stderr)
 return [x.strip() for x in p.stdout.splitlines() if x.strip()]
def foreign_gpu(allowed=()):
 allowed={str(x) for x in allowed}
 return [x for x in process_gpu() if x.split(',')[0].strip() not in allowed]
