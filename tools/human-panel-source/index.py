"""Audit all solo standard A10 Necrobinder winners; keep exclusion/provenance evidence."""
from pathlib import Path
import gzip,json,pickle,hashlib,os
from collections import Counter,defaultdict
ROOT=Path(os.environ['OSTYAI_SOURCE_RUN'])
OUT=ROOT/'human-win-validation'
RAW=Path(os.environ['OSTYAI_HUMAN_SOURCE'])
CACHE=Path(os.environ['OSTYAI_HUMAN_CACHE'])
def short(x):return str(x).rsplit('.',1)[-1]
def audit():
 OUT.mkdir(exist_ok=True)
 catalog=json.loads((ROOT/'schema/metadata.json').read_text())
 cards={k.rsplit(':',1)[0] for k in catalog['cards']};cards |= {k.removesuffix('_CARD') for k in cards};relics=set(catalog['relics'])
 candidates={};excluded=[];stats=Counter();sources={};seen=set()
 for path in sorted(RAW.glob('necrobinder-a10-*.jsonl.gz')):
  sources[path.name]=dict(sha256=hashlib.sha256(path.read_bytes()).hexdigest(),bytes=path.stat().st_size)
  with gzip.open(path,'rt') as f:
   for line in f:
    run=json.loads(line);stats['source_runs']+=1
    h=run['run_hash']
    if h in seen:continue
    seen.add(h)
    players=run.get('players',[])
    if len(players)!=1 or players[0].get('character')!='CHARACTER.NECROBINDER' or run.get('ascension')!=10 or not run.get('win'):continue
    stats['solo_a10_winners']+=1
    if run.get('game_mode','standard')!='standard' or run.get('modifiers'):
     excluded.append(dict(run_hash=h,reason='nonstandard_mode_or_modifiers'));continue
    player=players[0];usedcards=list(player.get('deck',[]));usedrelics=[r['id'] for r in player.get('relics',[])]
    rooms=[]
    for ai,act in enumerate(run.get('map_point_history',[]),1):
     for fi,floor in enumerate(act,1):
      st=next((s for s in floor.get('player_stats',[]) if str(s.get('player_id'))==str(player.get('id'))),{})
      usedcards.extend(st.get('cards_gained',[]));usedcards.extend(st.get('cards_removed',[]))
      usedcards.extend(x.get('card',{}) for x in st.get('card_choices',[]) if x.get('was_picked'))
      usedrelics.extend(x.get('choice','') for x in st.get('relic_choices',[]) if x.get('was_picked'));usedrelics.extend(st.get('relics_removed',[]))
      for ri,room in enumerate(floor.get('rooms',[])):
       if room.get('room_type') in ('monster','elite','boss') or room.get('monster_ids'):
        rooms.append(dict(act=ai,act_floor=fi,room_index=ri,encounter=room.get('model_id'),human_potion_used=st.get('potion_used',[]),human_potion_discarded=st.get('potion_discarded',[]),single_room=len(floor.get('rooms',[]))==1))
    badcards=sorted({str(c.get('id')) if isinstance(c,dict) else str(c) for c in usedcards if (c.get('id') if isinstance(c,dict) else c) and (short(c.get('id') if isinstance(c,dict) else c) not in cards or not str(c.get('id') if isinstance(c,dict) else c).startswith('CARD.'))})
    badrelics=sorted({str(x) for x in usedrelics if x and (short(x) not in relics or not str(x).startswith('RELIC.'))})
    if badcards or badrelics:
     excluded.append(dict(run_hash=h,reason='non_base_or_unrecognized_card_relic',cards=badcards,relics=badrelics));continue
    candidates[h]=dict(run_hash=h,version=run.get('build_id'),source_file=path.name,raw_combats=rooms,final_potions=player.get('potions',[]))
  print(json.dumps(dict(file=path.name,candidates=len(candidates),stats=stats)),flush=True)
 cases,oldstats,oldsources=pickle.loads(CACHE.read_bytes())
 assert sources==oldsources,'Human reconstruction cache is stale'
 groups=defaultdict(list)
 for c in cases:
  if c['run_hash'] in candidates:groups[c['run_hash']].append(c)
 jobs=[];unknown=[]
 for h,meta in sorted(candidates.items()):
  rows=sorted(groups[h],key=lambda c:(c['floor'],c['room_index']))
  bykey={(c['act'],c['act_floor'],c['room_index']):c for c in rows}
  meta['case_ids']=[]
  for i,room in enumerate(meta['raw_combats']):
   key=(room['act'],room['act_floor'],room['room_index']);case=bykey.get(key)
   if case is None:
    cid=f'{h}:missing:{key}';meta['case_ids'].append(cid);unknown.append(dict(case_id=cid,run_hash=h,reason='missing_reconstructed_start',room=room));continue
   cid=case['case_id'];meta['case_ids'].append(cid)
   k=rows.index(case);following=rows[k+1]['potions'] if k+1<len(rows) else [short(p.get('id','')) if isinstance(p,dict) else short(p) for p in meta['final_potions'] if p]
   before=[short(p) for p in case['potions'] if p];after=Counter(short(p) for p in following if p)
   disappeared=Counter(before)-after
   explicit=Counter(short(p) for p in room['human_potion_used']) if room['single_room'] else Counter()
   used=disappeared|explicit
   costs=[]
   for p in before:
    costs.append(0.0 if used[p]>0 else 1.0);used[p]-=int(used[p]>0)
   jobs.append(dict(case=case,potion_costs=costs,potion_evidence=dict(start_inventory=before,next_inventory=list(following),disappeared=dict(disappeared),recorded_used=dict(explicit),next_is_final_inventory=k+1==len(rows),rule='cost 0 for disappeared instances or explicit single-room uses; cost 1 otherwise; duplicate matches in starting-slot order')))
 manifest=dict(version=1,scope='All catalog-recognized solo standard human A10 Necrobinder wins; training overlap permitted',stats=dict(stats),eligible_runs=len(candidates),expected_combats=sum(len(m['raw_combats']) for m in candidates.values()),reconstructed_combats=len(jobs),missing_combats=unknown,excluded_runs=excluded,runs=list(candidates.values()),sources=sources,cache_sha256=hashlib.sha256(CACHE.read_bytes()).hexdigest(),catalog_sha256=hashlib.sha256((ROOT/'schema/metadata.json').read_bytes()).hexdigest())
 (OUT/'index.json').write_text(json.dumps(manifest,indent=2));(OUT/'jobs.pkl').write_bytes(pickle.dumps(jobs,protocol=5))
 print(json.dumps({k:v for k,v in manifest.items() if k not in ['runs','sources','excluded_runs','missing_combats']}),flush=True)
 print('Excluded reasons',Counter(e['reason'] for e in excluded),'unknown IDs',Counter(x for e in excluded for x in e.get('cards',[])+e.get('relics',[])),flush=True)
if __name__=='__main__':audit()
