"""Isolated real replay migration, ingestion and one full GPU optimizer update."""
from pathlib import Path
import copy,json,hashlib,time
import numpy as np
import torch
from production_train import ROOT,config,collect
from accumulated_learner import AccumulatedLearner
from prioritized_replay import PrioritizedReplay
from pool_metadata import restore_legacy_pool_tags
from pool_sampling import quotas,POOL_CODES
from corpus_stream import atomic_json
from loop import GPU,Native

def main():
 torch.set_num_threads(1);torch.set_float32_matmul_precision('high');out=ROOT/'changes-20261025-pool-sampling'
 ck=torch.load(out/'paused.pt',map_location='cpu',weights_only=False)
 data=torch.load(ck['replay_file'],map_location='cpu',weights_only=False)
 initial=torch.load(ROOT/'initial_weights.pt',map_location='cpu',weights_only=False)
 replay=PrioritizedReplay(65536,rows=1024);replay.load_state_dict(data)
 before=copy.deepcopy(replay.rng.bit_generator.state);prior=replay.priorities.copy()
 migration=restore_legacy_pool_tags(replay,ROOT);assert migration['migrated']==65536
 assert replay.rng.bit_generator.state==before;np.testing.assert_array_equal(prior,replay.priorities)
 for key in ['raw','policy','value']:
  a=getattr(replay,key);b=data[key]
  assert torch.equal(a[:len(b),:b.shape[1]].cpu() if b.ndim>1 else a[:len(b)].cpu(),b)
 for key in ['ids','lengths','versions']:np.testing.assert_array_equal(getattr(replay,key)[:replay.count],data[key])
 tags=replay.tiers.copy();state=replay.state_dict();replay.load_state_dict(state);np.testing.assert_array_equal(replay.tiers,tags);assert restore_legacy_pool_tags(replay,ROOT)==dict(migrated=0)
 del state,data
 gpu=GPU();gpu.model.load_state_dict(ck['actor'])
 # Use native output labels for fresh samples; production state is untouched.
 fixture=ROOT/'changes-20261025-tier-budgets/training-fixtures.jsonl'
 report=collect(Native(),gpu,config(fixture,16,9000000,ck['updates']),replay)
 assert report['replay_ingest']['added']>0
 ids=replay.ids;fresh=np.flatnonzero((ids>>np.uint64(32))==9000000)
 outcomes={r['id']:r for r in report['outcomes']}
 for i in fresh:assert replay.tiers[i]==POOL_CODES[outcomes[int(ids[i]&np.uint64(0xffffffff))]['search_tier']]
 del gpu
 torch.cuda.empty_cache()
 learner=AccumulatedLearner(initial,microbatch=256);learner.model.load_state_dict(ck['model']);learner.optimizer.load_state_dict(ck['optimizer']);learner.updates=ck['updates'];learner.examples=ck['examples']
 plan=replay.sample_plan(256,64)
 metrics=learner.update(replay,plan=plan)
 assert metrics['updates']==ck['updates']+1
 assert {t:v['sampled']for t,v in metrics['replay_sampling']['by_pool'].items()}==quotas(16384)
 assert all(torch.isfinite(v).all() for v in learner.model.state_dict().values() if torch.is_tensor(v))
 result=dict(migration=migration,replay_state_preserved=True,new_tag_checkpoint_roundtrip=True,native_ingestion_verified=True,update=metrics,diagnostic_weights_discarded=True)
 atomic_json(out/'smoke-results.json',result)
 print('POOL_SAMPLING_GPU_SMOKE_PASSED',json.dumps(metrics['replay_sampling']),flush=True)
if __name__=='__main__':main()
