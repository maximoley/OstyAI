"""Run the exported actor with historical or current search configuration."""
import argparse,hashlib,json,os,sys,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'ostyai'))
import torch
from loop import Native,GPU
from production_train import config,collect,actor_fingerprint
from secondary_validation import evaluate

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument('--suite',choices=['primary','human','smoke'],default='smoke')
    ap.add_argument('--policy-only',action='store_true')
    ap.add_argument('--reuse',action='store_true',help='Enable current persistent search; historical step5500 used no reuse')
    ap.add_argument('--model',type=Path,default=ROOT/'models/best/model.pt')
    ap.add_argument('--output',type=Path,default=ROOT/'results/evaluation')
    ap.add_argument('--workers',type=int,default=80)
    ap.add_argument('--cases',type=int,default=24,help='Smoke suite only')
    a=ap.parse_args()
    if a.suite=='human' and a.policy_only:ap.error('Published human benchmark is search-based; use primary for policy-only comparison.')
    if (a.output/'result.json').exists():ap.error('Choose a fresh output directory; results are never overwritten.')
    if a.cases<1 or a.workers<1:ap.error('cases and workers must be positive')
    required=ROOT/'ostyai'/('human-win-validation/manifest.json' if a.suite=='human' else 'validation/fixtures.jsonl')
    if not required.exists():ap.error('Run scripts/unpack_benchmarks.py first')
    os.environ['OSTYAI_MODEL']=str(a.model.resolve())
    ck=torch.load(a.model,map_location='cpu',weights_only=True);version=ck['step']
    torch.set_num_threads(1);native=Native();gpu=GPU(lanes=2)
    def cfg(path,n,episode,v,validation=True):
        x=config(path,n,episode,v,validation)
        x.update(workers=a.workers,persistent_search=a.reuse and not a.policy_only,record_replay=False)
        if a.policy_only:x.update(simulations=0,simulation_budgets=None,skip_forced_actions=False)
        return x
    a.output.mkdir(parents=True,exist_ok=True)
    with torch.inference_mode(),torch.autocast('cuda',dtype=torch.bfloat16,cache_enabled=False):
        if a.suite=='human':
            result=evaluate(native,gpu,a.output,version,collect,cfg)
        else:
            path=ROOT/'ostyai/validation/fixtures.jsonl';results=[]
            for i in range(2 if a.suite=='primary' else 1):
                evaluation_config=cfg(path,4096 if a.suite=='primary' else a.cases,1000000+i,version)
                report=collect(native,gpu,evaluation_config)
                assert not report['fatal'] and report['completed']==evaluation_config['cases']
                assert report['replay_added']==0
                if a.policy_only:
                    assert all(report[k]==0 for k in ['simulations','search_transitions','forced_search_transitions','chance_samples'])
                report['evaluation_config']=evaluation_config
                results.append(report)
            requested=(8192 if a.suite=='primary' else a.cases)
            wins=sum(sum(bool(o['win']) and o.get('terminal',False) for o in x['outcomes'])for x in results)
            result=dict(suite=a.suite,step=version,requested=requested,wins=wins,win_rate=wins/requested,
                        native_sha256=native.sha256,persistent_search=a.reuse,policy_only=a.policy_only,
                        model_sha256=actor_fingerprint(gpu),fixture_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                        torch_version=torch.__version__,reports=results)
    (a.output/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items()if k not in {'reports','by'}},indent=2))
if __name__=='__main__':main()
