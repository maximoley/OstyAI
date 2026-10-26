"""Validate frozen inputs and recompute all human aggregate results."""
import gzip,hashlib,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'ostyai'))
from human_validation_metrics import summarize
hp=ROOT/'benchmarks/human-validation-005500';fp=ROOT/'benchmarks/fixtures/human'
mfile=fp/'manifest.json';manifest=json.loads(mfile.read_text());prov=json.loads((hp/'provenance.json').read_text())
assert hashlib.sha256(mfile.read_bytes()).hexdigest()==prov['manifest_sha256']
for part in manifest['shards']:
    raw=gzip.decompress((fp/(part['file']+'.gz')).read_bytes())
    assert hashlib.sha256(raw).hexdigest()==part['sha256'],part['file']
trials=json.loads(gzip.decompress((hp/'trials.json.gz').read_bytes()))
s,cs,rs,ts=summarize(manifest,trials);saved=json.loads((hp/'summary.json').read_text())
for k,v in s.items():
    if k in saved:assert v==saved[k],k
assert cs==json.loads(gzip.decompress((hp/'combats.json.gz').read_bytes()))
assert rs==json.loads(gzip.decompress((hp/'runs.json.gz').read_bytes()))
assert (s['runs'],s['combats'],s['trials'])==(512,12099,36297)
primary=ROOT/'benchmarks/fixtures/primary/fixtures.jsonl.gz'
raw=gzip.decompress(primary.read_bytes())
pv=json.loads((ROOT/'benchmarks/validation-005500/provenance.json').read_text())
assert hashlib.sha256(raw).hexdigest()==pv['fixture_sha256']
assert len(raw.splitlines())==4096
for f in (ROOT/'benchmarks').rglob('*.gz'):
    with gzip.open(f,'rb')as stream:
        while stream.read(1024*1024):pass
print(json.dumps({k:s[k]for k in ['runs','combats','trials','trial_wins','wins','runs_won_every_fight','unresolved_trials']},indent=2))
