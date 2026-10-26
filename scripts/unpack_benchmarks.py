"""Unpack only benchmark fixtures; verify every frozen human shard."""
from pathlib import Path
import gzip,hashlib,json
root=Path(__file__).resolve().parents[1]
for srcname,dstname in [('human','human-win-validation'),('primary','validation')]:
    src=root/'benchmarks/fixtures'/srcname;dst=root/'ostyai'/dstname
    for f in src.rglob('*'):
        if not f.is_file():continue
        rel=f.relative_to(src);out=dst/rel
        if f.suffix=='.gz':out=out.with_suffix('');data=gzip.decompress(f.read_bytes())
        else:data=f.read_bytes()
        out.parent.mkdir(parents=True,exist_ok=True)
        if out.exists() and out.read_bytes()!=data:raise SystemExit(f'Refusing to replace changed fixture: {out}')
        if not out.exists():out.write_bytes(data)
manifest=json.loads((root/'ostyai/human-win-validation/manifest.json').read_text())
for part in manifest['shards']:
    p=root/'ostyai/human-win-validation'/part['file']
    assert hashlib.sha256(p.read_bytes()).hexdigest()==part['sha256'],p
print('Frozen human and primary benchmark fixtures verified and unpacked.')
