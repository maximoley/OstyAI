"""Length-bucketed pinned transfer, compiled public/private inference, CUDA-graph categorical sampling."""
import ctypes as C,time,collections
import numpy as np
import torch
from cuda_runtime import runtime_library_path
from model import sample_actions
from rollout import bucket,TOKEN_BUCKETS,ACTION_BUCKETS,PRIVATE_BUCKETS

class CUDA:
 def __init__(self):
  self.lib=C.CDLL(runtime_library_path());self.copy2d=self.lib.cudaMemcpy2DAsync
  self.copy2d.argtypes=[C.c_void_p,C.c_size_t,C.c_void_p,C.c_size_t,C.c_size_t,C.c_size_t,C.c_int,C.c_void_p];self.copy2d.restype=C.c_int
  self.copy=self.lib.cudaMemcpyAsync;self.copy.argtypes=[C.c_void_p,C.c_void_p,C.c_size_t,C.c_int,C.c_void_p];self.copy.restype=C.c_int
 @staticmethod
 def check(code):
  if code:raise RuntimeError(f'CUDA copy error {code}')
class Actor:
 def __init__(self,model,greedy=False):
  self.greedy=greedy;self.model=model;self.forward=torch.compile(model.actor if greedy else model.forward,fullgraph=True,dynamic=True);self.cuda=CUDA();self.stream=torch.cuda.Stream();self.cache={};self.preparation_seconds=0.;self.shapes=[]
 def evaluate(self,raw,private,a):
  if not self.greedy:
   logits,values,legal=self.forward(raw,private,a);actions,logp=sample_actions(logits,legal);return actions,logp,values
  logits,legal=self.forward(raw,a);actions=torch.where(legal.any(-1),logits.argmax(-1),-1).int();zeros=torch.zeros_like(actions,dtype=torch.float32);return actions,zeros,zeros
 def get(self,key):
  if key in self.cache:return self.cache[key]
  b,t,p,a=key;begun=time.perf_counter();raw=torch.zeros((b,t,16),device='cuda',dtype=torch.int32);private=torch.zeros((b,p,16),device='cuda',dtype=torch.int32);raw[:,0,14]=1
  host=torch.empty((b,t,16),dtype=torch.int32,pin_memory=True);phost=torch.empty((b,p,16),dtype=torch.int32,pin_memory=True)
  hout=torch.empty((b,),dtype=torch.int32,pin_memory=True);vout=torch.empty((2,b),dtype=torch.float32,pin_memory=True)
  saved_rng=torch.cuda.get_rng_state();self.model.eval()
  with torch.inference_mode(),torch.autocast('cuda',dtype=torch.bfloat16,cache_enabled=False),torch.cuda.stream(self.stream):
   for _ in range(3):
    actions,logp,values=self.evaluate(raw,private,a)
   self.stream.synchronize();graph=torch.cuda.CUDAGraph()
   with torch.cuda.graph(graph,stream=self.stream):
    actions,logp,values=self.evaluate(raw,private,a)
  self.stream.synchronize();torch.cuda.set_rng_state(saved_rng)
  result=dict(raw=raw,private=private,host=host,phost=phost,hout=hout,vout=vout,actions=actions,values=values,logp=logp,graph=graph)
  self.cache[key]=result;self.preparation_seconds+=time.perf_counter()-begun;self.shapes.append(key);print('Prepared actor graph',key,flush=True);return result
 @torch.inference_mode()
 def run(self,w):
  self.model.eval();actions=np.empty(w.n,np.int32);logp=np.empty(w.n,np.float32);values=np.empty(w.n,np.float32)
  # Coarse length classes keep a rare long event from padding every combat.
  bins=collections.defaultdict(list)
  for i,length in enumerate(w.lengths):bins[bucket(int(length),(48,56,64,96,128,192,256,384,512,768,1024,1536,2048))].append(i)
  for t,indices in bins.items():
   ids=np.asarray(indices,np.int32);n=len(ids);b=bucket(n,((1,2,4,8,16,32,64,96,128,192,256,384,512,768,1024) if t>2048 else (8,16,32,64,96,128,192,256,384,512,768,1024)));p=bucket(int(w.private_lengths[ids].max()),PRIVATE_BUCKETS);a=bucket(int(w.counts[ids].max()),ACTION_BUCKETS);slot=self.get((b,t,p,a))
   h=slot['host'].numpy();ph=slot['phost'].numpy();h[:n]=w.raw[ids,:t];ph[:n]=w.private[ids,:p]
   h[n:,0,:]=0;h[n:,0,14]=1;ph[n:,0,:]=0
   with torch.cuda.stream(self.stream):
    stream=self.stream.cuda_stream;self.cuda.check(self.cuda.copy2d(slot['raw'].data_ptr(),t*64,slot['host'].data_ptr(),t*64,t*64,b,1,stream));self.cuda.check(self.cuda.copy2d(slot['private'].data_ptr(),p*64,slot['phost'].data_ptr(),p*64,p*64,b,1,stream))
    slot['graph'].replay()
    self.cuda.check(self.cuda.copy(slot['hout'].data_ptr(),slot['actions'].data_ptr(),b*4,2,stream))
    self.cuda.check(self.cuda.copy(slot['vout'].data_ptr(),slot['logp'].data_ptr(),b*4,2,stream));self.cuda.check(self.cuda.copy(slot['vout'].data_ptr()+b*4,slot['values'].data_ptr(),b*4,2,stream))
   self.stream.synchronize();actions[ids]=slot['hout'].numpy()[:n];logp[ids]=slot['vout'].numpy()[0,:n];values[ids]=slot['vout'].numpy()[1,:n]
  return actions,logp,values
