//! Independent run-continuation PPO adapter. Core game rules are unchanged.
//! All transitions, forced steps, snapshots and resets share ONE Rayon pool.
use super::*;
use serde::{Serialize,Deserialize};
use std::collections::VecDeque;

#[derive(Clone,Default,Serialize,Deserialize,Debug,PartialEq)]
struct Trace {seed:u64,prefix:Vec<i32>,reseed:Option<u64>,actions:Vec<i32>}
#[derive(Clone,Default,Serialize,Deserialize)]
struct Meta {
 trace:Trace,lane:u8,cell:usize,segment_done:bool,terminal:bool,bootstrap:bool,cap:bool,
 meaningful:usize,combat_meaningful:usize,forced:usize,merged:usize,uid:u64,
 #[serde(skip)] reps:Vec<usize>,
 #[serde(skip)] captures:Vec<Snapshot>,
 // Derived observation caches are not part of simulator or checkpoint state.
 #[serde(skip)] public_generation:Option<u64>,
 #[serde(skip)] private_generation:Option<u64>,
 #[serde(skip)] private_cache:Vec<Row>,
 age:u64, capture_enabled:bool,
}
#[derive(Clone,Serialize,Deserialize)]
struct SnapDesc {trace:Trace,move_index:i32,created:u64,uses:u32,hash:u64,cell:usize}
#[derive(Clone)]
struct Snapshot {desc:SnapDesc,rm:RunManager}
struct Ready {seed:u64,rm:RunManager}
struct RVPool {
 base:Box<Pool>,meta:Vec<Meta>,bank:Vec<VecDeque<Snapshot>>,ready:VecDeque<Ready>,
 next_seed:u64,next_uid:u64,rng:u64,decisions:u64,events:[u64;8],admitted:[u64;6],fallbacks:u64,
 training:bool,queue_limit:usize,min_entries:usize,combat_cap:usize,turn_cap:u16,run_cap:usize,
}
fn slot_from(rm:RunManager,seed:u64)->Slot {Slot{rm,view:dummy_session(),rows:Vec::with_capacity(1024),actions:Vec::new(),decisions:0,generation:0,ppo:PpoState{seed,..Default::default()}}}
fn random(x:&mut u64)->u64 {*x=x.wrapping_add(0x9e3779b97f4a7c15);let mut z=*x;z=(z^(z>>30)).wrapping_mul(0xbf58476d1ce4e5b9);z=(z^(z>>27)).wrapping_mul(0x94d049bb133111eb);z^(z>>31)}
fn draw(x:&mut u64)->f64 {(random(x)>>11) as f64/((1u64<<53) as f64)}
fn sum_events(a:&mut[i32;8],b:[i32;8]) {for i in 0..6{a[i]+=b[i];}a[7]=b[7];}
fn is_terminal(s:&Slot,m:&Meta)->bool {m.cap||s.rm.phase()==Phase::RunOver}

// Only merge plays with equal complete instance data (except identity UID) and
// target. Choices remain distinct: an index is not proof of semantic equality.
fn play_key(s:&Slot,a:RunAction)->Option<(Card,Option<u32>)> {
 if let RunAction::PlayCard{hand_idx,target}=a {
  let c=s.rm.combat.as_ref()?;let uid=*c.player.hand.get(hand_idx)?;
  let mut card=*sts2_core::card_by_uid(&c.cards,uid)?;card.uid=Card::pod_defaults().uid;
  Some((card,target))
 } else {None}
}
fn encode_classes(s:&mut Slot,m:&mut Meta,schema:&Schema)->Result<()> {
 s.encode(schema)?;let mut keys=Vec::new();m.reps.clear();
 for (i,&a) in s.actions.iter().enumerate(){
  if let Some(k)=play_key(s,a){if keys.contains(&k){continue}keys.push(k);}
  m.reps.push(i);
 }
 let start=s.rows[0][11] as usize+1;let mut rows=s.rows[..start].to_vec();
 for (j,&i) in m.reps.iter().enumerate(){let mut r=s.rows[start+i];r[3]=j as i32;rows.push(r);}
 rows[0][10]=rows.len() as i32-1;rows[0][12]=m.reps.len() as i32;
 // Full-run and practice states have identical task encoding/reward semantics.
 if let Some(r)=rows.iter_mut().find(|r|r[0]==31&&r[1]==6){r[4..7].fill(0);}
 if m.terminal{rows[0][14]=1;}
 s.rows=rows;m.public_generation=Some(s.generation);Ok(())
}
fn capture_cell(s:&Slot,raw_index:usize)->Option<usize> {
 let RunAction::Move{col}=*s.actions.get(raw_index)? else{return None};
 // The already public map-route token refers to this precise legal Move,
 // including WingedBoots and both final boss nodes. No hidden encounter read.
 let start=s.rows[0][11] as usize+1;
 let owner=s.rows[start+raw_index][2];
 let route=s.rows.get((owner+1) as usize)?;
 if route[0]!=23{return None}
 let kind=route[1]; // map node row has kind in identity
 let _=col;
 let act=s.rm.run_state.act_index.saturating_sub(1).min(2) as usize;
 if kind==MapPointType::Elite as i32 {Some(act)}
 else if kind==MapPointType::Boss as i32 {
  // Do not make an act3 boss2-only cell.
  let rs=&s.rm.run_state;
  if act==2&&rs.awaiting_second_boss(){return None}
  Some(3+act)
 } else {None}
}
fn cap_after(s:&mut Slot,m:&mut Meta,combat_cap:usize,turn_cap:u16,run_cap:usize) {
 if s.rm.phase()==Phase::RunOver{return}
 let in_combat=s.rm.phase()==Phase::Combat;
 let over=m.meaningful>=run_cap||(in_combat&&(m.combat_meaningful>=combat_cap||s.rm.combat.as_ref().is_some_and(|c|c.round>turn_cap)));
 if over {
  if in_combat{s.rm.resolve_combat_fast(0.,false);}
  m.cap=true;m.terminal=true;m.segment_done=true;m.bootstrap=false;
 }
}
fn advance_raw(s:&mut Slot,m:&mut Meta,index:usize,meaningful:bool,limits:(usize,u16,usize))->Result<[i32;8]> {
 // Raw command indices are journalled, preserving engine action ordering.
 if m.capture_enabled&&m.lane==0 {if let Some(cell)=capture_cell(s,index){let desc=SnapDesc{trace:trace_for_snapshot(m),move_index:index as i32,created:m.age,uses:0,cell,hash:sts2_core::hashing::run_hash(&s.rm)};m.captures.push(Snapshot{desc,rm:s.rm.clone()});let _=cell;}}
 let before=s.rm.phase();let e=s.ppo_advance(index as i32)?;
 m.trace.actions.push(index as i32);
 if meaningful {m.meaningful+=1;if before==Phase::Combat{m.combat_meaningful+=1;}}
 else {m.forced+=1;}
 if before!=Phase::Combat&&s.rm.phase()==Phase::Combat{m.combat_meaningful=0;}
 cap_after(s,m,limits.0,limits.1,limits.2);
 if is_terminal(s,m){m.terminal=true;m.segment_done=true;m.bootstrap=false;}
 // Ordinary practice stops immediately after combat, before reward selection.
 if m.lane==1&&m.cell!=5&&e[5]>0&&!m.terminal {m.segment_done=true;m.bootstrap=true;}
 Ok(e)
}
fn settle(s:&mut Slot,m:&mut Meta,schema:&Schema,limits:(usize,u16,usize),events:&mut[i32;8])->Result<()> {
 for _ in 0..100_000 {
  if m.segment_done{encode_classes(s,m,schema)?;return Ok(())}
  encode_classes(s,m,schema)?;
  if m.reps.len()>1{return Ok(())}
  if m.reps.is_empty(){return Err(format!("nonterminal has zero actions: phase {:?}",s.rm.phase()))}
  let index=m.reps[0];s.encode(schema)?;
  sum_events(events,advance_raw(s,m,index,false,limits)?);
 }
 Err("forced chain exceeded safety bound; refusing to silently skip a state".into())
}
fn replay(t:&Trace,schema:&Schema)->Result<Slot> {
 let mut s=Slot::new(t.seed);
 for &i in &t.prefix{s.encode(schema)?;s.ppo_advance(i)?;}
 if let Some(seed)=t.reseed{s.rm.run_state.rng=sts2_core::RunRngSet::new(seed);s.decisions=0;s.ppo=PpoState{seed:t.seed,..Default::default()};}
 for &i in &t.actions{s.encode(schema)?;s.ppo_advance(i)?;}
 Ok(s)
}
fn trace_for_snapshot(m:&Meta)->Trace {
 debug_assert_eq!(m.lane,0);debug_assert!(m.trace.reseed.is_none());m.trace.clone()
}
#[no_mangle]
pub unsafe extern "C" fn sm_rv_create(schema:*const c_char,n:usize,workers:usize,seed:u64,training:bool)->*mut c_void {
 let mut out=std::ptr::null_mut();guarded(||{
  let p=sm_fr_create(schema,n,workers,seed);if p.is_null(){return Err("base pool creation failed".into())}
  let base=Box::from_raw(p as *mut Pool);
  let mut rv=RVPool{base,meta:(0..n).map(|i|Meta{trace:Trace{seed:seed+i as u64,..Default::default()},uid:i as u64,capture_enabled:training,..Default::default()}).collect(),bank:(0..6).map(|_|VecDeque::new()).collect(),ready:VecDeque::new(),next_seed:seed+n as u64,next_uid:n as u64,rng:seed^0xf009178a,decisions:0,events:[0;8],admitted:[0;6],fallbacks:0,training,queue_limit:2560,min_entries:8,combat_cap:1000,turn_cap:100,run_cap:6000};
  let schema=rv.base.schema.clone();rv.base.threads.install(||rv.base.slots.par_iter_mut().zip(rv.meta.par_iter_mut()).try_for_each(|(s,m)|settle(s,m,&schema,(1000,100,6000),&mut[0;8])))?;
  out=Box::into_raw(Box::new(rv)) as *mut c_void;Ok(())
 });out
}
#[no_mangle]pub unsafe extern "C" fn sm_rv_destroy(p:*mut c_void){if !p.is_null(){drop(Box::from_raw(p as *mut RVPool));}}
#[no_mangle]pub unsafe extern "C" fn sm_rv_encode(p:*mut c_void,dst:*mut i32,capacity:usize,lengths:*mut u32,counts:*mut u32,private:*mut i32,pcap:usize,plengths:*mut u32,metadata:*mut i64)->i32{guarded(||{
 let r=&mut*(p as *mut RVPool);let n=r.meta.len();let schema=r.base.schema.clone();
 let all:Result<Vec<_>>=r.base.threads.install(||r.base.slots.par_iter_mut().zip(r.meta.par_iter_mut()).map(|(s,m)|{encode_classes(s,m,&schema)?;Ok((s.rows.clone(),s.private_rows()?))}).collect());
 let d=std::slice::from_raw_parts_mut(dst,n*capacity*16);let pr=std::slice::from_raw_parts_mut(private,n*pcap*16);let l=std::slice::from_raw_parts_mut(lengths,n);let c=std::slice::from_raw_parts_mut(counts,n);let pl=std::slice::from_raw_parts_mut(plengths,n);let md=std::slice::from_raw_parts_mut(metadata,n*8);
 for (i,(rows,privs)) in all?.iter().enumerate(){
  if rows.len()>capacity{return Err(format!("row capacity {capacity} < required {}",rows.len()))}if privs.len()>pcap{return Err(format!("privileged capacity {pcap} < {}",privs.len()))}
  for(j,row)in rows.iter().enumerate(){d[(i*capacity+j)*16..(i*capacity+j+1)*16].copy_from_slice(row);}
  for(j,row)in privs.iter().enumerate(){pr[(i*pcap+j)*16..(i*pcap+j+1)*16].copy_from_slice(row);}
  l[i]=rows.len() as u32;pl[i]=privs.len() as u32;c[i]=r.meta[i].reps.len() as u32;
  let m=&r.meta[i];md[i*8..i*8+8].copy_from_slice(&[m.lane as i64,m.uid as i64,m.segment_done as i64,m.terminal as i64,m.bootstrap as i64,m.meaningful as i64,m.forced as i64,m.cell as i64]);
 }Ok(())
})}
#[no_mangle]pub unsafe extern "C" fn sm_rv_step(p:*mut c_void,actions:*const i32,generations:*const u64,events:*mut i32)->i32{guarded(||{
 let r=&mut*(p as *mut RVPool);let n=r.meta.len();let a=std::slice::from_raw_parts(actions,n);let g=std::slice::from_raw_parts(generations,n);let schema=r.base.schema.clone();let limits=(r.combat_cap,r.turn_cap,r.run_cap);
 for i in 0..n {if a[i]<0{continue}let s=&r.base.slots[i];let m=&mut r.meta[i];if m.segment_done||g[i]!=s.generation{return Err(format!("stale/finished slot {i}"))}if a[i] as usize>=m.reps.len(){return Err("class index out of range".into())}m.age=r.decisions;m.capture_enabled=r.training;}
 let output=std::slice::from_raw_parts_mut(events,n*8);
 r.base.threads.install(||r.base.slots.par_iter_mut().zip(r.meta.par_iter_mut()).zip(a.par_iter()).zip(output.par_chunks_mut(8)).try_for_each(|(((s,m),&a),out)|{
  out.fill(0);if a<0{return Ok::<(),String>(())}let index=m.reps[a as usize];let before=phase(s.rm.phase());let original=s.actions.len();m.merged+=original-m.reps.len();s.encode(&schema)?;
  let mut e=advance_raw(s,m,index,true,limits)?;settle(s,m,&schema,limits,&mut e)?;e[4]=i32::from(m.segment_done);e[6]=before;e[7]=phase(s.rm.phase());out.copy_from_slice(&e);Ok(())
 }))?;
 for m in &mut r.meta {for snap in m.captures.drain(..){let cell=snap.desc.cell;r.bank[cell].push_back(snap);if r.bank[cell].len()>r.queue_limit{r.bank[cell].pop_front();}}}
 r.decisions+=a.iter().filter(|&&i|i>=0).count() as u64;
 for e in output.chunks(8){for i in 0..6{r.events[i]+=e[i] as u64;}}
 Ok(())
})}
#[no_mangle]pub unsafe extern "C" fn sm_rv_refill(p:*mut c_void,depth:usize)->i32{guarded(||{
 let r=&mut*(p as *mut RVPool);if depth<r.meta.len()*64||depth>200_000{return Err("start depth outside bounded safe range".into())}
 if r.ready.len()>depth/2{return Ok(())}let count=depth-r.ready.len();let first=r.next_seed;
 if first.checked_add(count as u64).is_none_or(|s|s>=1u64<<63){return Err("training seed entered held-out namespace".into())}
 let ready:Vec<_>=r.base.threads.install(||(0..count).into_par_iter().map(|i|Ready{seed:first+i as u64,rm:Slot::new(first+i as u64).rm}).collect());
 r.next_seed+=count as u64;r.ready.extend(ready);Ok(())
})}
#[no_mangle]pub unsafe extern "C" fn sm_rv_reset(p:*mut c_void)->i32{let mut count=0;let code=guarded(||{
 let r=&mut*(p as *mut RVPool);let fraction=(r.decisions as f64/2e9).min(1.);let practice_fraction=0.4-0.15*fraction;let boss_fraction=0.4+0.35*fraction;
 for bank in &mut r.bank {bank.retain(|s|r.decisions-s.desc.created<=50_000_000&&s.desc.uses<4);}
 let mut jobs=Vec::new();let active=r.meta.iter().filter(|m|m.lane==1&&!m.segment_done).count();let mut practice_active=active;
 for i in 0..r.meta.len(){if !r.meta[i].segment_done{continue}if !r.training{continue}count+=1;
  let want=(practice_active as f64)<practice_fraction*r.meta.len() as f64;
  let mut selected=None;
  if want {let boss=draw(&mut r.rng)<boss_fraction;let u=draw(&mut r.rng);let weights=if boss{[0.15,0.2,0.65]}else{[0.2,0.4,0.4]};let j=if u<weights[0]{0}else if u<weights[0]+weights[1]{1}else{2};let cell=j+if boss{3}else{0};
   if r.bank[cell].len()>=r.min_entries {let choices=r.bank[cell].len();let pick=(random(&mut r.rng) as usize)%choices;let snap=&mut r.bank[cell][pick];snap.desc.uses+=1;let reseed=600_000_000+random(&mut r.rng)%40_000_000;
    let mut trace=snap.desc.trace.clone();trace.prefix=trace.actions.clone();trace.actions=vec![snap.desc.move_index];trace.reseed=Some(reseed);
    let mut rm=snap.rm.clone();rm.run_state.rng=sts2_core::RunRngSet::new(reseed);selected=Some((rm,Meta{trace,lane:1,cell,uid:r.next_uid,age:r.decisions,..Default::default()},Some(snap.desc.move_index)));r.admitted[cell]+=1;practice_active+=1;
   }else{r.fallbacks+=1;}
  }
  if selected.is_none(){let ready=r.ready.pop_front().ok_or("prepared full-run start queue exhausted (no on-the-fly fallback)")?;selected=Some((ready.rm,Meta{trace:Trace{seed:ready.seed,..Default::default()},uid:r.next_uid,age:r.decisions,capture_enabled:r.training,..Default::default()},None));}
  r.next_uid+=1;jobs.push((i,selected.unwrap()));
 }
 let schema=r.base.schema.clone();let limits=(r.combat_cap,r.turn_cap,r.run_cap);
 let done:Result<Vec<_>>=r.base.threads.install(||jobs.into_par_iter().map(|(i,(rm,mut m,mov))|{
  let mut s=slot_from(rm,m.trace.seed);let mut initial=[0;8];
  if let Some(mov)=mov {m.trace.actions.clear();s.encode(&schema)?;sum_events(&mut initial,advance_raw(&mut s,&mut m,mov as usize,false,limits)?);}
  settle(&mut s,&mut m,&schema,limits,&mut initial)?;
  if initial[..4].iter().any(|&x|x!=0)||m.segment_done{return Err("reset advanced a rewarded or terminal episode without a learner row".into())}
  Ok((i,s,m))
 }).collect());
 for(i,s,m)in done?{r.base.slots[i]=s;r.meta[i]=m;}Ok(())
 });if code<0{code}else{count}}
#[no_mangle]pub unsafe extern "C" fn sm_rv_stats(p:*mut c_void)->*const c_char{let mut out=std::ptr::null();guarded(||{
 let r=&*(p as *mut RVPool);let rows:Vec<_>=r.base.slots.iter().zip(&r.meta).map(|(s,m)|{let rs=&s.rm.run_state;serde_json::json!({"character":format!("{:?}",s.rm.character_id),"seed":m.trace.seed,"uid":m.uid,"lane":m.lane,"cell":m.cell,"meaningful_decisions":m.meaningful,"raw_decisions":m.trace.actions.len(),"forced":m.forced,"merged":m.merged,"terminal":is_terminal(s,m),"segment_done":m.segment_done,"bootstrap":m.bootstrap,"won":!m.cap&&rs.player_won,"cap":m.cap,"phase":phase(s.rm.phase()),"act":rs.act_index,"floor":rs.total_floor,"hp":rs.current_hp,"max_hp":rs.max_hp,"hash":sts2_core::hashing::run_hash(&s.rm)})}).collect();
 let value=serde_json::json!({"character":format!("{RUN_CHARACTER:?}"),"slots":rows,"decisions":r.decisions,"events":r.events,"admitted":r.admitted,"fallbacks":r.fallbacks,"bank_sizes":r.bank.iter().map(|x|x.len()).collect::<Vec<_>>(),"ready_starts":r.ready.len(),"next_seed":r.next_seed});OUTPUT.with(|x|{*x.borrow_mut()=CString::new(value.to_string()).unwrap();out=x.borrow().as_ptr()});Ok(())});out}
#[derive(Serialize,Deserialize)]struct RVJournal{version:u32,character:String,meta:Vec<Meta>,hashes:Vec<u64>,generations:Vec<u64>,bank:Vec<Vec<SnapDesc>>,ready:Vec<u64>,next_seed:u64,next_uid:u64,rng:u64,decisions:u64,events:[u64;8],admitted:[u64;6],fallbacks:u64}
#[no_mangle]pub unsafe extern "C" fn sm_rv_journal(p:*mut c_void)->*const c_char{let mut out=std::ptr::null();guarded(||{let r=&*(p as *mut RVPool);let j=RVJournal{version:2,character:format!("{RUN_CHARACTER:?}"),meta:r.meta.clone(),hashes:r.base.slots.iter().map(|s|sts2_core::hashing::run_hash(&s.rm)).collect(),generations:r.base.slots.iter().map(|s|s.generation).collect(),bank:r.bank.iter().map(|b|b.iter().map(|s|s.desc.clone()).collect()).collect(),ready:r.ready.iter().map(|s|s.seed).collect(),next_seed:r.next_seed,next_uid:r.next_uid,rng:r.rng,decisions:r.decisions,events:r.events,admitted:r.admitted,fallbacks:r.fallbacks};OUTPUT.with(|x|{*x.borrow_mut()=CString::new(serde_json::to_string(&j).unwrap()).unwrap();out=x.borrow().as_ptr()});Ok(())});out}
#[no_mangle]pub unsafe extern "C" fn sm_rv_restore(p:*mut c_void,json:*const c_char)->i32{guarded(||{
 let r=&mut*(p as *mut RVPool);let j:RVJournal=serde_json::from_str(CStr::from_ptr(json).to_str().map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;if j.version!=2||j.character!=format!("{RUN_CHARACTER:?}")||j.meta.len()!=r.meta.len(){return Err("runvalue journal layout mismatch".into())}
 let schema=r.base.schema.clone();let restored:Result<Vec<_>>=r.base.threads.install(||j.meta.par_iter().enumerate().map(|(i,m)|{let mut s=replay(&m.trace,&schema)?;if m.cap&&s.rm.phase()==Phase::Combat{s.rm.resolve_combat_fast(0.,false);}if sts2_core::hashing::run_hash(&s.rm)!=j.hashes[i]{return Err(format!("slot {i} journal state hash mismatch"))}s.generation=j.generations[i];Ok(s)}).collect());r.base.slots=restored?;
 let bank:Result<Vec<_>>=r.base.threads.install(||j.bank.par_iter().map(|b|b.iter().map(|d|{let s=replay(&d.trace,&schema)?;if sts2_core::hashing::run_hash(&s.rm)!=d.hash{return Err("snapshot state hash mismatch".into())}Ok(Snapshot{desc:d.clone(),rm:s.rm})}).collect::<Result<VecDeque<_>>>()).collect());r.bank=bank?;
 r.ready=r.base.threads.install(||j.ready.par_iter().map(|&seed|Ready{seed,rm:Slot::new(seed).rm}).collect());r.meta=j.meta;r.next_seed=j.next_seed;r.next_uid=j.next_uid;r.rng=j.rng;r.decisions=j.decisions;r.events=j.events;r.admitted=j.admitted;r.fallbacks=j.fallbacks;
 for(s,m)in r.base.slots.iter_mut().zip(&mut r.meta){encode_classes(s,m,&schema)?;}Ok(())
})}

// Synthetic contract witnesses ONLY. Never called by training or validation.
// They deliberately fast-forward fights and must not enter learner datasets.
#[no_mangle]pub unsafe extern "C" fn sm_rv_test_fixture(p:*mut c_void,index:usize,cell:usize)->i32{guarded(||{
 let r=&mut*(p as *mut RVPool);if index>=r.meta.len()||cell>=6{return Err("test fixture bounds".into())}let schema=r.base.schema.clone();let target_act=(cell%3+1) as u8;
 for trial in 0..64 {
  let mut s=Slot::new(7342+trial);let mut m=Meta::default();m.capture_enabled=false;
  for _ in 0..4000 {
   if s.rm.phase()==Phase::RunOver||s.rm.run_state.act_index>target_act{break}
   s.encode(&schema)?;
   if s.rm.phase()==Phase::Combat{s.rm.resolve_combat_fast(0.,true);s.prepare_decision()?;continue}
   let candidate=(0..s.actions.len()).find(|&i|capture_cell(&s,i)==Some(cell));
   if let Some(i)=candidate {
    s.rm.run_state.rng=sts2_core::RunRngSet::new(601234567);m=Meta{lane:1,cell,trace:Trace{seed:7342+trial,reseed:Some(601234567),..Default::default()},uid:9000000+cell as u64,..Default::default()};
    let mut e=advance_raw(&mut s,&mut m,i,false,(1000,100,6000))?;settle(&mut s,&mut m,&schema,(1000,100,6000),&mut e)?;
    if s.rm.phase()!=Phase::Combat{return Err("practice fixture Move did not enter combat".into())}r.base.slots[index]=s;r.meta[index]=m;return Ok(())
   }
   let i=(0..s.actions.len()).find(|&i|capture_cell(&s,i).is_some_and(|c|c<3)).unwrap_or(0);
   s.ppo_advance(i as i32)?;
  }
 }
 Err("could not find live own-run practice fixture".into())
})}
#[no_mangle]pub unsafe extern "C" fn sm_rv_test_resolve(p:*mut c_void,index:usize,won:bool)->i32{guarded(||{
 let r=&mut*(p as *mut RVPool);let s=&mut r.base.slots[index];let m=&mut r.meta[index];if s.rm.phase()!=Phase::Combat{return Err("test resolve requires combat".into())}
 let rs=&s.rm.run_state;let (act,floor,room,event)=(rs.act_index,rs.total_floor,rs.current_room,rs.event_combat.is_some());
 s.rm.resolve_combat_fast(0.,won);s.prepare_decision()?;let mut e=s.ppo_account(Phase::Combat,act,floor,room,event)?;
 if s.rm.phase()==Phase::RunOver {m.segment_done=true;m.terminal=true;m.bootstrap=false;}
 else if m.lane==1&&m.cell!=5 {m.segment_done=true;m.bootstrap=true;}
 settle(s,m,&r.base.schema,(1000,100,6000),&mut e)?;Ok(())
})}
#[no_mangle]pub unsafe extern "C" fn sm_rv_test_intervention(p:*mut c_void,index:usize)->i32{guarded(||{
 let r=&mut*(p as *mut RVPool);let s=&mut r.base.slots[index];if let Some(c)=s.rm.combat.as_mut(){c.player.draw.reverse();}let rs=&mut s.rm.run_state;if let Some(g)=rs.act_index.checked_sub(1).and_then(|i|rs.generated_rooms.get_mut(i as usize)){g.normal_encounters.reverse();g.elite_encounters.reverse();}Ok(())
})}
#[no_mangle]pub unsafe extern "C" fn sm_rv_test_cap(p:*mut c_void,index:usize,kind:u32)->i32{guarded(||{
 let r=&mut*(p as *mut RVPool);let m=&mut r.meta[index];let s=&mut r.base.slots[index];match kind{0=>m.combat_meaningful=1000,1=>{s.rm.combat.as_mut().ok_or("cap fixture needs combat")?.round=101;},_=>m.meaningful=6000};cap_after(s,m,1000,100,6000);encode_classes(s,m,&r.base.schema)?;Ok(())
})}
#[no_mangle]pub unsafe extern "C" fn sm_rv_test_dedup(p:*mut c_void,index:usize,different:bool)->i32{guarded(||{
 let r=&mut*(p as *mut RVPool);let s=&mut r.base.slots[index];let c=s.rm.combat.as_mut().ok_or("dedup witness needs combat")?;
 if c.player.hand.len()<2{return Err("dedup witness needs two cards".into())}let u0=c.player.hand[0];let u1=c.player.hand[1];let mut clone=*sts2_core::card_by_uid(&c.cards,u0).ok_or("card0")?;clone.uid=u1;if different{clone.cost=(clone.cost-1).max(0);clone.base_damage+=1;}*c.cards.iter_mut().find(|x|x.uid==u1).ok_or("card1")?=clone;encode_classes(s,&mut r.meta[index],&r.base.schema)?;Ok(())
})}


// ABI 1: compact transition outcomes and generation-keyed observation copies.
// No game transitions or reward/accounting code is duplicated here.
#[no_mangle]pub extern "C" fn sm_rv_fast_abi()->u32{1}
fn fast_metadata(m:&Meta)->[i64;8]{[m.lane as i64,m.uid as i64,m.segment_done as i64,m.terminal as i64,m.bootstrap as i64,m.meaningful as i64,m.forced as i64,m.cell as i64]}
fn fast_outcome(s:&Slot,m:&Meta)->[i64;24]{
 let rs=&s.rm.run_state;let mut o=[0;24];
 o[..19].copy_from_slice(&[m.trace.seed as i64,m.uid as i64,m.lane as i64,m.cell as i64,m.meaningful as i64,m.trace.actions.len() as i64,m.forced as i64,m.merged as i64,is_terminal(s,m) as i64,m.segment_done as i64,m.bootstrap as i64,(!m.cap&&rs.player_won) as i64,m.cap as i64,phase(s.rm.phase()) as i64,rs.act_index as i64,rs.total_floor as i64,rs.current_hp as i64,rs.max_hp as i64,sts2_core::hashing::run_hash(&s.rm) as i64]);o
}
#[no_mangle]pub unsafe extern "C" fn sm_rv_step_fast(p:*mut c_void,actions:*const i32,generations:*const u64,events:*mut i32,metadata:*mut i64,outcomes:*mut i64)->i32{
 let code=sm_rv_step(p,actions,generations,events);if code<0{return code}
 guarded(||{
  let r=&*(p as *mut RVPool);let n=r.meta.len();let a=std::slice::from_raw_parts(actions,n);let md=std::slice::from_raw_parts_mut(metadata,n*8);let out=std::slice::from_raw_parts_mut(outcomes,n*24);out.fill(0);
  for(i,(s,m))in r.base.slots.iter().zip(&r.meta).enumerate(){md[i*8..i*8+8].copy_from_slice(&fast_metadata(m));if a[i]>=0&&m.segment_done{out[i*24..i*24+24].copy_from_slice(&fast_outcome(s,m));}}
  Ok(())
 })
}
#[no_mangle]pub unsafe extern "C" fn sm_rv_encode_selected(p:*mut c_void,indices:*const u32,n:usize,dst:*mut i32,capacity:usize,lengths:*mut u32,counts:*mut u32,private:*mut i32,pcap:usize,plengths:*mut u32,metadata:*mut i64)->i32{guarded(||{
 let r=&mut*(p as *mut RVPool);let ids=std::slice::from_raw_parts(indices,n);
 let mut seen=vec![false;r.meta.len()];for &i in ids{if i as usize>=seen.len()||seen[i as usize]{return Err("invalid/duplicate selected observation slot".into())}seen[i as usize]=true;}
 let d=std::slice::from_raw_parts_mut(dst,n*capacity*16);let pr=std::slice::from_raw_parts_mut(private,n*pcap*16);let l=std::slice::from_raw_parts_mut(lengths,n);let c=std::slice::from_raw_parts_mut(counts,n);let pl=std::slice::from_raw_parts_mut(plengths,n);let md=std::slice::from_raw_parts_mut(metadata,n*8);
 for(j,&i)in ids.iter().enumerate(){
  let s=&mut r.base.slots[i as usize];let m=&mut r.meta[i as usize];
  if m.public_generation!=Some(s.generation){encode_classes(s,m,&r.base.schema)?;}
  if m.private_generation!=Some(s.generation){m.private_cache=s.private_rows()?;m.private_generation=Some(s.generation);}
  if s.rows.len()>capacity{return Err(format!("row capacity {capacity} < required {}",s.rows.len()))}
  if m.private_cache.len()>pcap{return Err(format!("privileged capacity {pcap} < {}",m.private_cache.len()))}
  let public_len=s.rows.len()*16;let private_len=m.private_cache.len()*16;
  d[j*capacity*16..j*capacity*16+public_len].copy_from_slice(std::slice::from_raw_parts(s.rows.as_ptr() as *const i32,public_len));
  pr[j*pcap*16..j*pcap*16+private_len].copy_from_slice(std::slice::from_raw_parts(m.private_cache.as_ptr() as *const i32,private_len));
  l[j]=s.rows.len() as u32;pl[j]=m.private_cache.len() as u32;c[j]=m.reps.len() as u32;md[j*8..j*8+8].copy_from_slice(&fast_metadata(m));
 }Ok(())
})}
#[no_mangle]pub unsafe extern "C" fn sm_rv_fast_invalidate(p:*mut c_void)->i32{guarded(||{let r=&mut*(p as *mut RVPool);for m in &mut r.meta{m.public_generation=None;m.private_generation=None;}Ok(())})}
#[no_mangle]pub unsafe extern "C" fn sm_rv_summary(p:*mut c_void)->*const c_char{let mut out=std::ptr::null();guarded(||{
 let r=&*(p as *mut RVPool);let value=serde_json::json!({"character":format!("{RUN_CHARACTER:?}"),"decisions":r.decisions,"events":r.events,"admitted":r.admitted,"fallbacks":r.fallbacks,"bank_sizes":r.bank.iter().map(|b|b.len()).collect::<Vec<_>>(),"ready_starts":r.ready.len(),"next_seed":r.next_seed});
 OUTPUT.with(|x|{*x.borrow_mut()=CString::new(value.to_string()).unwrap();out=x.borrow().as_ptr()});Ok(())
});out}

// Diagnostic-only audit: never traversed by the hot collection loop.
#[no_mangle]pub unsafe extern "C" fn sm_rv_character_audit(p:*mut c_void)->*const c_char{
 let mut out=std::ptr::null();guarded(||{
  let r=&*(p as *mut RVPool);
  let mut active=std::collections::BTreeMap::<String,usize>::new();
  let mut ready=std::collections::BTreeMap::<String,usize>::new();
  let mut bank=std::collections::BTreeMap::<String,usize>::new();
  for s in &r.base.slots{*active.entry(format!("{:?}",s.rm.character_id)).or_default()+=1;}
  for s in &r.ready{*ready.entry(format!("{:?}",s.rm.character_id)).or_default()+=1;}
  for cell in &r.bank{for s in cell{*bank.entry(format!("{:?}",s.rm.character_id)).or_default()+=1;}}
  let value=serde_json::json!({"compiled_character":format!("{RUN_CHARACTER:?}"),"active":active,"ready":ready,"practice_bank":bank});
  OUTPUT.with(|x|{*x.borrow_mut()=CString::new(value.to_string()).unwrap();out=x.borrow().as_ptr()});Ok(())
 });out
}
