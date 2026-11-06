"""Derived observation caching and numeric transition results; unchanged game API."""
import ctypes as C
import json
from types import SimpleNamespace
import numpy as np
from run_workers import RunWorkers
from worker import CapacityExceeded,next_capacity

OUTCOME_FIELDS=('seed','uid','lane','cell','meaningful_decisions','raw_decisions','forced','merged','terminal','segment_done','bootstrap','won','cap','phase','act','floor','hp','max_hp','hash')
BOOL_FIELDS={'terminal','segment_done','bootstrap','won','cap'}

class FastWorkers(RunWorkers):
    def __init__(self,*args,**kwargs):
        super().__init__(*args,**kwargs)
        specs=[('fast_abi',[],C.c_uint32),('step_fast',[C.c_void_p,C.c_void_p,C.c_void_p,C.c_void_p,C.c_void_p,C.c_void_p],C.c_int),('encode_selected',[C.c_void_p,C.c_void_p,C.c_size_t,C.c_void_p,C.c_size_t,C.c_void_p,C.c_void_p,C.c_void_p,C.c_size_t,C.c_void_p,C.c_void_p],C.c_int),('fast_invalidate',[C.c_void_p],C.c_int),('summary',[C.c_void_p],C.c_char_p)]
        for name,args,result in specs:
            f=getattr(self.lib,'sm_rv_'+name);f.argtypes=args;f.restype=result;setattr(self,'native_'+name,f)
        assert self.native_fast_abi()==1,'Unsupported compact transition ABI'
        self.outcomes=np.zeros((self.n,24),np.int64);self.full_ids=np.arange(self.n,dtype=np.uint32);self.subset_buffer=None
    def grow_target(self,t,error):
        event=dict(subset=t is not self,old_public=t.rows,old_private=t.private_rows,required_public=error.public,required_private=error.private)
        if error.public>t.rows:t.rows=next_capacity(error.public);t.tensor,t.raw=self.arena((t.capacity_n,t.rows,16))
        if error.private>t.private_rows:t.private_rows=next_capacity(error.private);t.private_tensor,t.private=self.arena((t.capacity_n,t.private_rows,16))
        self.capacity_events.append(dict(event,new_public=t.rows,new_private=t.private_rows))
    def encode_into(self,ids,t):
        while True:
            try:
                self.check(self.native_encode_selected(self.pool,ids.ctypes.data,len(ids),t.raw.ctypes.data,t.rows,t.lengths.ctypes.data,t.counts.ctypes.data,t.private.ctypes.data,t.private_rows,t.private_lengths.ctypes.data,t.metadata.ctypes.data));return t
            except CapacityExceeded as e:self.grow_target(t,e)
    def observe_all(self):
        self.capacity_n=self.n;self.encode_into(self.full_ids,self);return self.raw,self.private
    def observe_subset(self,ids):
        ids=np.ascontiguousarray(ids,np.uint32);n=len(ids);assert n and len(np.unique(ids))==n
        t=self.subset_buffer
        if t is None or t.capacity_n<n:
            capacity_n=max(8,next_capacity(n));t=SimpleNamespace(capacity_n=capacity_n,rows=self.rows,private_rows=self.private_rows)
            t.tensor,t.raw=self.arena((capacity_n,t.rows,16));t.private_tensor,t.private=self.arena((capacity_n,t.private_rows,16))
            t.lengths=np.zeros(capacity_n,np.uint32);t.private_lengths=np.zeros(capacity_n,np.uint32);t.counts=np.zeros(capacity_n,np.uint32);t.metadata=np.zeros((capacity_n,8),np.int64);self.subset_buffer=t
        self.encode_into(ids,t)
        return SimpleNamespace(n=n,raw=t.raw[:n],private=t.private[:n],lengths=t.lengths[:n],counts=t.counts[:n],private_lengths=t.private_lengths[:n],metadata=t.metadata[:n])
    def advance(self,actions):
        a=np.ascontiguousarray(actions,np.int32);assert len(a)==self.n
        active=a>=0;assert (a[active]<self.counts[active]).all()
        g=self.raw[:,0,6].astype(np.uint32).astype(np.uint64)+(self.raw[:,0,7].astype(np.uint32).astype(np.uint64)<<np.uint64(32))
        self.check(self.native_step_fast(self.pool,a.ctypes.data,g.ctypes.data,self.events.ctypes.data,self.metadata.ctypes.data,self.outcomes.ctypes.data));return self.events
    def completed_outcome(self,i):
        a=self.outcomes[int(i)];assert a[9]
        row={key:bool(a[j]) if key in BOOL_FIELDS else int(a[j]) for j,key in enumerate(OUTCOME_FIELDS)}
        for key in ['seed','uid','hash']:row[key]&=(1<<64)-1
        row['character']=self.character
        return row
    def summary(self):
        value=self.native_summary(self.pool)
        if value is None:raise RuntimeError(self.error().decode())
        return json.loads(value)
    def invalidate(self):self.check(self.native_fast_invalidate(self.pool))
