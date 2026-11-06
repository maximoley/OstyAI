"""Own one trainer; wait for idle GPU, honor STOP, resume exact committed state."""
import fcntl,json,os,signal,subprocess,time,traceback
from pathlib import Path
from training_schedule import deadline_reached
from operations import atomic_json,append_json,foreign_gpu
R=Path(__file__).resolve().parent;O=R/'production'

def main():
 O.mkdir(exist_ok=True);lock=(O/'supervisor.lock').open('w');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB);halt=[False];child=None
 signal.signal(signal.SIGTERM,lambda *args:halt.__setitem__(0,True));signal.signal(signal.SIGINT,lambda *args:halt.__setitem__(0,True))
 def stopped():return halt[0] or (R/'STOP').exists() or (O/'STOP').exists()
 cfg=json.loads((R/'config.json').read_text())
 retries=0;previous_update=-1
 while not stopped():
  if deadline_reached(cfg):
   atomic_json(O/'service.json',dict(stage='scheduled_stop',reason='wall_deadline',supervisor_pid=os.getpid(),time=time.time()));return
  try:
   others=foreign_gpu()
   if others:
    atomic_json(O/'service.json',dict(stage='waiting_for_idle_gpu',supervisor_pid=os.getpid(),foreign=others,time=time.time()));time.sleep(10);continue
   if (O/'status.json').exists():
    status=json.loads((O/'status.json').read_text())
    if status.get('completed'):break
   args=['bash',str(R/'run.sh'),str(R/'train_production.py')]
   if (O/'latest.pt').exists():args+=['--resume',str(O/'latest.pt')]
   atomic_json(O/'service.json',dict(stage='starting',supervisor_pid=os.getpid(),args=args,time=time.time()))
   with (O/'trainer.log').open('a') as log:child=subprocess.Popen(args,cwd=R,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
   launched=time.time();stall=False
   while child.poll() is None:
    info=json.loads((O/'status.json').read_text()) if (O/'status.json').exists() else {}
    atomic_json(O/'service.json',dict(stage='running',supervisor_pid=os.getpid(),trainer_pid=child.pid,command=args,trainer_stage=info.get('stage'),update=info.get('update'),time=time.time()))
    expired=deadline_reached(cfg)
    if stopped() or expired:child.terminate()
    elif time.time()-max(info.get('heartbeat',launched),launched)>1800:
     append_json(O/'service-events.jsonl',dict(event='stale_trainer_terminated',pid=child.pid,seconds_since_heartbeat=time.time()-info.get('heartbeat',launched),time=time.time()));child.terminate();stall=True
    if stopped() or expired or stall:
     try:child.wait(timeout=300)
     except subprocess.TimeoutExpired:child.kill();child.wait()
     break
    time.sleep(5)
   code=child.wait();child=None
   info=json.loads((O/'status.json').read_text()) if (O/'status.json').exists() else {}
   append_json(O/'service-events.jsonl',dict(event='trainer_exit',returncode=code,status=info.get('stage'),update=info.get('update'),time=time.time()))
   if stopped() or deadline_reached(cfg) or info.get('completed') or (code==0 and info.get('stage')=='paused'):break
   if code==0 and info.get('stage')=='waiting_for_gpu':continue
   # A persistent simulator or data error must become visible instead of looping forever.
   committed=json.loads((O/'checkpoint.json').read_text()).get('update',0) if (O/'checkpoint.json').exists() else 0
   retries=1 if committed>previous_update else retries+1;previous_update=committed
   if retries>=3:
    atomic_json(O/'service.json',dict(stage='blocked',reason='Three consecutive failures without checkpoint progress; inspect failure.json',supervisor_pid=os.getpid(),returncode=code,time=time.time()));return
   atomic_json(O/'service.json',dict(stage='retry_wait',attempt=retries,returncode=code,committed_update=committed,supervisor_pid=os.getpid(),time=time.time()));time.sleep(30)
  except Exception as e:
   atomic_json(O/'service.json',dict(stage='blocked',error=repr(e),traceback=traceback.format_exc(),supervisor_pid=os.getpid(),time=time.time()));return
 atomic_json(O/'service.json',dict(stage='stopped' if stopped() else 'complete_or_paused',supervisor_pid=os.getpid(),time=time.time()))
if __name__=='__main__':main()
