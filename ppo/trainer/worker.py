"""Bulk native run workers: numeric arrays in/out, JSON only for audit."""
import ctypes as C,json,re
from pathlib import Path
import numpy as np
R=Path(__file__).resolve().parent
SCHEMA=str(R/'schema')
class CapacityExceeded(RuntimeError):
 def __init__(self,message,public=0,private=0):super().__init__(message);self.public=public;self.private=private

def capacity_error(message,rows,private_rows=0):
 m=re.search(r'row capacity \d+ < required (\d+)',message)
 if m:return CapacityExceeded(message,public=int(m[1]))
 m=re.search(r'privileged capacity \d+ < (\d+)',message)
 if m:return CapacityExceeded(message,private=int(m[1]))
 m=re.search(r'curriculum observation capacity exceeded in slot \d+: (\d+) / (\d+)',message)
 if m:return CapacityExceeded(message,public=int(m[1]) if int(m[1])>rows else 0,private=int(m[2]) if int(m[2])>private_rows else 0)
 return None

def next_capacity(need):return 1 << (int(need)-1).bit_length()

class Workers:
 def __init__(self,n=64,workers=8,seed=0,rows=2048,pinned=False):
  self.lib=C.CDLL(str(R/'loop_native.so'));L=self.lib
  for name,args,res in [('create',[C.c_char_p,C.c_size_t,C.c_size_t,C.c_uint64],C.c_void_p),('destroy',[C.c_void_p],None),('error',[],C.c_char_p),('encode',[C.c_void_p,C.c_void_p,C.c_size_t,C.c_void_p,C.c_void_p],C.c_int),('step',[C.c_void_p,C.c_void_p],C.c_int),('step_checked',[C.c_void_p,C.c_void_p,C.c_void_p],C.c_int),('coverage_combat',[C.c_void_p],C.c_int),('debug',[C.c_void_p,C.c_size_t],C.c_char_p),('reset_terminal',[C.c_void_p,C.c_uint64],C.c_int)]:
   f=getattr(L,'sm_fr_'+name);f.argtypes=args;f.restype=res;setattr(self,name,f)
  self.pool=self.create(SCHEMA.encode(),n,workers,seed);assert self.pool,self.error()
  self.n=n;self.rows=rows;self.pinned=pinned;self.capacity_managed=False;self.capacity_events=[]
  if pinned:
   import torch
   self.tensor=torch.empty((n,rows,16),dtype=torch.int32,pin_memory=True);self.raw=self.tensor.numpy()
  else:self.raw=np.empty((n,rows,16),np.int32)
  self.lengths=np.zeros(n,np.uint32);self.counts=np.zeros(n,np.uint32)
 def check(self,code):
  if code<0:
   message=self.error().decode();err=capacity_error(message,self.rows,getattr(self,'private_rows',0))
   raise err if err is not None else RuntimeError(message)
 def observe(self):
  while True:
   try:self.check(self.encode(self.pool,self.raw.ctypes.data,self.rows,self.lengths.ctypes.data,self.counts.ctypes.data));return self.raw
   except CapacityExceeded as e:
    if self.capacity_managed:raise
    self.grow(public=e.public,private=e.private)
 def grow(self,public=0,private=0):
  event=dict(old_public=self.rows,old_private=getattr(self,'private_rows',0),required_public=public,required_private=private)
  def arena(shape):
   if self.pinned:
    import torch
    t=torch.empty(shape,dtype=torch.int32,pin_memory=True);return t,t.numpy()
   return None,np.empty(shape,np.int32)
  if public>self.rows:self.rows=next_capacity(public);self.tensor,self.raw=arena((self.n,self.rows,16))
  if private>getattr(self,'private_rows',0):self.private_rows=next_capacity(private);self.private_tensor,self.private=arena((self.n,self.private_rows,16))
  event.update(new_public=self.rows,new_private=getattr(self,'private_rows',0));self.capacity_events.append(event)
 def advance(self,a):
  a=np.ascontiguousarray(a,dtype=np.int32);assert len(a)==self.n;generations=(self.raw[:,0,6].astype(np.uint32).astype(np.uint64)+(self.raw[:,0,7].astype(np.uint32).astype(np.uint64)<<np.uint64(32)));self.check(self.step_checked(self.pool,a.ctypes.data,generations.ctypes.data))
 def inspect(self,i):
  data=self.debug(self.pool,i)
  if data is None:raise RuntimeError(self.error().decode())
  return json.loads(data)
 def close(self):
  if self.pool:self.destroy(self.pool);self.pool=None

class PPOWorkers(Workers):
 def __init__(self,n=512,workers=80,seed=20261104,rows=2048,private_rows=512,pinned=True,combat_cap=None):
  super().__init__(n,workers,seed,rows,pinned)
  for name,args,res in [('set_combat_limit',[C.c_void_p,C.c_size_t],C.c_int),('refill_rolling_bank',[C.c_void_p,C.c_size_t],C.c_int),('run_stats',[C.c_void_p],C.c_char_p),('prepare_start_bank',[C.c_void_p,C.c_uint64,C.c_size_t],C.c_int),('start_bank_stats',[C.c_void_p],C.c_char_p),('prefill_resets',[C.c_void_p,C.c_bool],C.c_int),('encode_private',[C.c_void_p,C.c_void_p,C.c_size_t,C.c_void_p],C.c_int),('step_ppo',[C.c_void_p,C.c_void_p,C.c_void_p,C.c_void_p],C.c_int),('journal',[C.c_void_p],C.c_char_p),('restore_journal',[C.c_void_p,C.c_char_p],C.c_int),('test_privileged_intervention',[C.c_void_p,C.c_size_t],C.c_int)]:
   f=getattr(self.lib,'sm_fr_'+name);f.argtypes=args;f.restype=res;setattr(self,name,f)
  self.reset_terminal=self.lib.sm_fr_reset_terminal_ppo;self.reset_terminal.argtypes=[C.c_void_p,C.c_uint64];self.reset_terminal.restype=C.c_int
  if combat_cap is None:combat_cap=json.loads((R/'config.json').read_text()).get('environment',{}).get('max_combat_actions',0)
  self.check(self.set_combat_limit(self.pool,combat_cap));self.combat_cap=combat_cap
  self.private_rows=private_rows
  if pinned:
   import torch
   self.private_tensor=torch.empty((n,private_rows,16),dtype=torch.int32,pin_memory=True);self.private=self.private_tensor.numpy()
  else:self.private=np.empty((n,private_rows,16),np.int32)
  self.private_lengths=np.zeros(n,np.uint32);self.events=np.zeros((n,8),np.int32)
 def observe_all(self):
  while True:
   try:
    self.observe();self.check(self.encode_private(self.pool,self.private.ctypes.data,self.private_rows,self.private_lengths.ctypes.data));return self.raw,self.private
   except CapacityExceeded as e:
    if self.capacity_managed:raise
    self.grow(public=e.public,private=e.private)
 def advance_ppo(self,actions):
  a=np.ascontiguousarray(actions,dtype=np.int32);assert len(a)==self.n
  generations=self.raw[:,0,6].astype(np.uint32).astype(np.uint64)+(self.raw[:,0,7].astype(np.uint32).astype(np.uint64)<<np.uint64(32))
  self.check(self.step_ppo(self.pool,a.ctypes.data,generations.ctypes.data,self.events.ctypes.data));return self.events
 def save_journal(self):
  data=self.journal(self.pool)
  if data is None:raise RuntimeError(self.error().decode())
  return json.loads(data)
 def load_journal(self,j):self.check(self.restore_journal(self.pool,json.dumps(j).encode()))

 def bank_stats(self):
  data=self.start_bank_stats(self.pool)
  if data is None:raise RuntimeError(self.error().decode())
  return json.loads(data)

 def info(self):
  data=self.run_stats(self.pool)
  if data is None:raise RuntimeError(self.error().decode())
  return json.loads(data)
