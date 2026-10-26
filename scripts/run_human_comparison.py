"""Run four sequential 80-worker benchmarks and resume the saved production run."""
import argparse,fcntl,hashlib,json,os,subprocess,time
from pathlib import Path

def save(p,obj):
    t=p.with_suffix(p.suffix+'.tmp');t.write_text(json.dumps(obj,indent=2)+'\n');t.replace(p)
def sha(p):
    h=hashlib.sha256()
    with p.open('rb')as f:
        for b in iter(lambda:f.read(8*1024*1024),b''):h.update(b)
    return h.hexdigest()

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--root',type=Path,required=True);ap.add_argument('--production',type=Path,required=True);a=ap.parse_args()
    root=a.root;package=Path(__file__).resolve().parents[1];python=os.environ['OSTYAI_PYTHON'];prepared=root/'compatible-fixtures'
    lock=(root/'runner.lock').open('w');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    (root/'runner.pid').write_text(str(os.getpid()))
    pause=json.loads((root/'pause-verification.json').read_text());stop=a.production/'STOP';stop_text='Paused for OstyAI human comparison 20261026; owner /public/maxim/Codex/ostyai-human-comparison-20261026\n'
    assert stop.read_text()==stop_text
    assert sha(a.production/'production/latest.pt')==pause['checkpoint_sha256']
    assert sha(Path(pause['replay_file']))==pause['replay_sha256']
    lib_hash=sha(package/'ostyai/loop_native.so');assert lib_hash=='6a041f84c93019b6f32ed4977a95806e34e643b532126405925c5d718185e62f'
    env=os.environ.copy();env.update(OMP_NUM_THREADS='1',MKL_NUM_THREADS='1',TORCHINDUCTOR_COMPILE_THREADS='2')
    commands=[
        ('mentor_default',[python,str(package/'scripts/benchmark_mcts.py'),'--prepared',str(prepared),'--workers','80','--rollout-depth','200']),
        ('mentor_8000_depth400',[python,str(package/'scripts/benchmark_mcts.py'),'--prepared',str(prepared),'--workers','80','--simulations','8000','--rollout-depth','400']),
        ('model5500_tiered_no_reuse',[python,str(package/'scripts/evaluate.py'),'--suite','human','--workers','80','--human-fixtures',str(prepared)]),
        ('model5500_uniform32_no_reuse',[python,str(package/'scripts/evaluate.py'),'--suite','human','--workers','80','--human-fixtures',str(prepared),'--uniform-simulations','32']),
    ]
    all_started=time.time();rows=[]
    for name,cmd in commands:
        if (root/'STOP').exists():raise RuntimeError('Comparison STOP requested; training remains paused')
        out=root/name;assert not out.exists(),'Fresh output required for unbiased full-run wall timing: '+name
        cmd+=['--output',str(out)]
        save(root/'status.json',dict(stage='benchmark',configuration=name,pid=os.getpid(),completed_configurations=len(rows),started_at_unix=all_started,time=time.time()))
        begun=time.perf_counter();begun_wall=time.time()
        with (root/(name+'.log')).open('wb')as log:
            child=subprocess.Popen(cmd,cwd=package,env=env,stdout=log,stderr=subprocess.STDOUT)
            save(root/(name+'-process.json'),dict(pid=child.pid,command=cmd,started_at_unix=begun_wall,workers=80))
            code=child.wait()
        elapsed=time.perf_counter()-begun
        save(root/(name+'-timing.json'),dict(wall_seconds=elapsed,started_at_unix=begun_wall,finished_at_unix=time.time(),exit_code=code,includes_process_startup_imports_and_compilation=True,preparation_excluded=True,workers=80))
        if code:raise RuntimeError(f'{name} failed with exit code {code}; inspect log')
        summary_path=out/'summary.json' if name.startswith('mentor')else out/'human-validation-005500/summary.json'
        s=json.loads(summary_path.read_text());assert (s['runs'],s['combats'],s['trials'])==(512,12099,36297) and not s['partial_smoke']
        assert s['native_sha256']==lib_hash
        row={k:s[k]for k in ['trial_wins','trials','trial_win_rate','wins','combats','win_rate','runs_won_every_fight','runs','run_win_rate','unresolved_trials','trial_average_hp_lost','average_hp_lost']}
        row.update(configuration=name,wall_seconds=elapsed,workers=80,summary=str(summary_path),native_sha256=lib_hash,manifest_sha256=s['manifest_sha256'])
        rows.append(row);save(root/'comparison.json',dict(complete=len(rows)==4,results=rows,started_at_unix=all_started,time=time.time()))
    assert len({r['manifest_sha256']for r in rows})==1
    # Resume only our own pause, after all four complete, preserving any later STOP.
    assert not (root/'STOP').exists() and stop.read_text()==stop_text
    assert sha(a.production/'production/latest.pt')==pause['checkpoint_sha256']
    assert sha(Path(pause['replay_file']))==pause['replay_sha256']
    service=json.loads((a.production/'production/service.json').read_text())
    assert not Path(f"/proc/{service['trainer_pid']}").exists() and not Path(f"/proc/{service['supervisor_pid']}").exists()
    stop.unlink()
    with (root/'resume-supervisor.log').open('ab')as log:
        proc=subprocess.Popen(['bash',str(a.production/'run.sh'),str(a.production/'supervise_run.py')],cwd=a.production,stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    save(root/'status.json',dict(stage='benchmarks_complete_training_resume_started',supervisor_pid=proc.pid,paused_step=pause['step'],time=time.time()))
    save(root/'resume-started.json',dict(supervisor_pid=proc.pid,checkpoint_sha256=pause['checkpoint_sha256'],replay_sha256=pause['replay_sha256'],time=time.time()))

if __name__=='__main__':
    try:main()
    except BaseException as error:
        import traceback;traceback.print_exc();raise
