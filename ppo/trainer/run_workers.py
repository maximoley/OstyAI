"""One native pool for full runs and run-cloned practice; no Python game stepping."""
import ctypes as C
import json
import numpy as np
from worker import R, SCHEMA, CapacityExceeded, capacity_error, next_capacity

class RunWorkers:
    def __init__(self,n=512,workers=80,seed=20261105,rows=2048,private_rows=512,pinned=True,training=True,character=None):
        if character is None:character=json.loads((R/'config.json').read_text())['environment']['character']
        self.lib=C.CDLL(str(R/'loop_native.so'));self.error=self.lib.sm_fr_error;self.error.restype=C.c_char_p
        identity=self.lib.sm_fr_character;identity.restype=C.c_char_p
        actual=identity().decode()
        if actual!=character:raise RuntimeError(f'Native character {actual} does not match requested {character}')
        self.character=actual
        specs=[('create',[C.c_char_p,C.c_size_t,C.c_size_t,C.c_uint64,C.c_bool],C.c_void_p),('destroy',[C.c_void_p],None),('encode',[C.c_void_p,C.c_void_p,C.c_size_t,C.c_void_p,C.c_void_p,C.c_void_p,C.c_size_t,C.c_void_p,C.c_void_p],C.c_int),('step',[C.c_void_p,C.c_void_p,C.c_void_p,C.c_void_p],C.c_int),('refill',[C.c_void_p,C.c_size_t],C.c_int),('reset',[C.c_void_p],C.c_int),('journal',[C.c_void_p],C.c_char_p),('restore',[C.c_void_p,C.c_char_p],C.c_int),('stats',[C.c_void_p],C.c_char_p)]
        for name,args,res in specs:
            f=getattr(self.lib,'sm_rv_'+name);f.argtypes=args;f.restype=res;setattr(self,'native_'+name,f)
        self.n=n;self.rows=rows;self.private_rows=private_rows;self.pinned=pinned;self.capacity_events=[]
        self.pool=self.native_create(SCHEMA.encode(),n,workers,seed,training)
        if not self.pool:raise RuntimeError(self.error().decode())
        self.tensor,self.raw=self.arena((n,rows,16));self.private_tensor,self.private=self.arena((n,private_rows,16))
        self.lengths=np.zeros(n,np.uint32);self.counts=np.zeros(n,np.uint32);self.private_lengths=np.zeros(n,np.uint32);self.metadata=np.zeros((n,8),np.int64);self.events=np.zeros((n,8),np.int32)
    def arena(self,shape):
        if self.pinned:
            import torch
            t=torch.empty(shape,dtype=torch.int32,pin_memory=True);return t,t.numpy()
        return None,np.empty(shape,np.int32)
    def check(self,code):
        if code<0:
            msg=self.error().decode();e=capacity_error(msg,self.rows,self.private_rows)
            raise e if e is not None else RuntimeError(msg)
        return code
    def observe_all(self):
        while True:
            try:
                self.check(self.native_encode(self.pool,self.raw.ctypes.data,self.rows,self.lengths.ctypes.data,self.counts.ctypes.data,self.private.ctypes.data,self.private_rows,self.private_lengths.ctypes.data,self.metadata.ctypes.data));return self.raw,self.private
            except CapacityExceeded as e:
                event=dict(old_public=self.rows,old_private=self.private_rows,required_public=e.public,required_private=e.private)
                if e.public>self.rows:self.rows=next_capacity(e.public);self.tensor,self.raw=self.arena((self.n,self.rows,16))
                if e.private>self.private_rows:self.private_rows=next_capacity(e.private);self.private_tensor,self.private=self.arena((self.n,self.private_rows,16))
                self.capacity_events.append(dict(event,new_public=self.rows,new_private=self.private_rows))
    def advance(self,actions):
        a=np.ascontiguousarray(actions,np.int32);assert len(a)==self.n
        active=a>=0;assert (a[active]<self.counts[active]).all()
        g=self.raw[:,0,6].astype(np.uint32).astype(np.uint64)+(self.raw[:,0,7].astype(np.uint32).astype(np.uint64)<<np.uint64(32))
        self.check(self.native_step(self.pool,a.ctypes.data,g.ctypes.data,self.events.ctypes.data));return self.events
    def reset(self):return self.check(self.native_reset(self.pool))
    def refill(self,depth):return self.check(self.native_refill(self.pool,depth))
    def info(self):
        s=self.native_stats(self.pool)
        if s is None:raise RuntimeError(self.error().decode())
        return json.loads(s)
    def audit_character(self):
        f=self.lib.sm_rv_character_audit;f.argtypes=[C.c_void_p];f.restype=C.c_char_p
        data=f(self.pool)
        if data is None:raise RuntimeError(self.error().decode())
        result=json.loads(data)
        assert result['compiled_character']==self.character
        assert result['active']=={self.character:self.n}
        assert all(set(result[k])<={self.character} for k in ['ready','practice_bank'])
        return result
    def save_journal(self):
        s=self.native_journal(self.pool)
        if s is None:raise RuntimeError(self.error().decode())
        return json.loads(s)
    def load_journal(self,j):self.check(self.native_restore(self.pool,json.dumps(j,separators=(',',':')).encode()))
    def close(self):
        if self.pool:self.native_destroy(self.pool);self.pool=None
