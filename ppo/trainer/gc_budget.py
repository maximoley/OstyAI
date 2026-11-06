"""Bound cyclic-GC work; reference counting remains active on every object."""
import gc,time

class GCBudget:
    def __init__(self,interval=10):self.interval=interval;self.was_enabled=None
    def start(self):
        self.was_enabled=gc.isenabled();gc.disable()
    def tick(self,update):
        start=time.perf_counter();full=(update%self.interval)==0
        collected=gc.collect(2 if full else 0)
        return dict(seconds=time.perf_counter()-start,full=full,objects_collected=collected,interval=self.interval)
    def close(self):
        if self.was_enabled:gc.enable()
