//! Critic-only sidecars, exact reward events and seed/action journals for PPO.
use super::*;
use serde::{Serialize,Deserialize};
#[derive(Default)]
pub(super) struct PpoState {
 pub seed:u64,pub actions:Vec<i32>,pub cleared:BTreeSet<(u8,i32)>,pub event_counts:[i32;4],pub combat_actions:usize,pub combat_limit:usize,pub cap_from_action:usize,pub timed_out:bool,
}
impl Slot {
 pub(super) fn enable_combat_limit(&mut self,limit:usize)->Result<()> {
  if self.ppo.combat_limit==limit{return Ok(())}
  if self.ppo.combat_limit!=0{return Err("Changing an active run's nonzero combat cap needs explicit migration".into())}
  self.ppo.combat_limit=limit;self.ppo.cap_from_action=self.ppo.actions.len();Ok(())
 }

 pub(super) fn private_rows(&self)->Result<Vec<Row>> {
  let mut out=vec![[0;16]];
  if self.rm.phase()==Phase::Combat {if let Some(c)=self.rm.combat.as_ref(){
   if c.player.deferred_draw_order {return Err("real PPO worker has search-only deferred draw order".into())}
   for (i,uid) in c.player.draw.iter().enumerate(){let x=sts2_core::card_by_uid(&c.cards,*uid).ok_or("private draw card missing")?;row(&mut out,1,1+2*x.card_id as i32+i32::from(x.upgraded),-1,i as i32,&[x.cost.into(),x.base_damage,x.base_block]);}
  }}
  let rs=&self.rm.run_state;
  if let Some(g)=rs.act_index.checked_sub(1).and_then(|i|rs.generated_rooms.get(i as usize)){
   for (kind,id) in [(0,g.normal_encounters.get(rs.monster_combats_this_act as usize).copied()),(1,g.elite_encounters.get(rs.elite_combats_this_act as usize).copied()),(2,Some(g.boss_id)),(3,g.second_boss_id)]{
    if let Some(id)=id{let def=sts2_content::encounters::find(id).ok_or_else(||format!("unknown privileged encounter {id}"))?;row(&mut out,2,sts2_content::encounters::encounter_index(def) as i32+1,-1,kind,&[]);}
   }
  }
  out[0][10]=out.len() as i32-1;Ok(out)
 }
 pub(super) fn ppo_advance(&mut self,index:i32)->Result<[i32;8]> {
  let before_phase=self.rm.phase();let before_act=self.rm.run_state.act_index;let before_floor=self.rm.run_state.total_floor;
  let before_room=self.rm.run_state.current_room;
  let event_combat=self.rm.run_state.event_combat.is_some();
  let a=*self.actions.get(index as usize).ok_or("PPO action out of bounds")?;
  let already_over_limit=before_phase==Phase::Combat&&self.ppo.combat_limit>0&&self.ppo.combat_actions>=self.ppo.combat_limit;
  if !already_over_limit {
   let result=self.rm.take_action(a);
   if !matches!(result,sts2_core::run_manager::ActionResult::Advanced|sts2_core::run_manager::ActionResult::RunOver){return Err(format!("rejected PPO action {a:?}: {result:?}"))}
  }
  if before_phase==Phase::Combat{self.ppo.combat_actions+=1;}
  if before_phase!=Phase::Combat&&self.rm.phase()==Phase::Combat{self.ppo.combat_actions=0;self.ppo.timed_out=false;}
  // A real win/death on action500 takes precedence. Otherwise the run loses,
  // with no floor/elite/boss/win reward and terminal GAE (not a bootstrap).
  if already_over_limit||(before_phase==Phase::Combat&&self.ppo.combat_limit>0&&self.ppo.combat_actions>=self.ppo.combat_limit&&self.rm.phase()==Phase::Combat&&self.rm.combat.as_ref().is_some_and(|c|!c.is_over)){
   self.rm.resolve_combat_fast(0.,false);self.ppo.timed_out=true;
   if self.rm.phase()!=Phase::RunOver||self.rm.run_state.player_won{return Err("Combat timeout did not terminate as a run loss".into())}
  }
  self.decisions+=1;self.generation+=1;self.ppo.actions.push(index);
  self.prepare_decision()?;
  self.ppo_account(before_phase,before_act,before_floor,before_room,event_combat)
 }
 pub(super) fn ppo_account(&mut self,before_phase:Phase,before_act:u8,before_floor:i32,before_room:Option<sts2_core::runstate::Room>,event_combat:bool)->Result<[i32;8]>{
  let rs=&self.rm.run_state;let done=self.rm.phase()==Phase::RunOver;
  let won_combat=before_phase==Phase::Combat&&(self.rm.combat.as_ref().is_some_and(|c|c.is_over&&c.player_won)||(self.rm.phase()!=Phase::Combat&&rs.current_hp>0));
  let mut events=[0;8];
  let terminal_win=done&&rs.player_won;
  let room_complete=(won_combat&&!event_combat)||((self.rm.phase()==Phase::MapChoice||rs.act_index!=before_act||terminal_win)&&before_phase!=Phase::MapChoice&&!(done&&!terminal_win));
  if room_complete&&before_floor>0&&before_room.is_some_and(|r|r.map_coord.is_some())&&self.ppo.cleared.insert((before_act,before_floor)){events[0]=1;}
  // Empty/automatic rooms can return directly to the map in one Move.
  if before_phase==Phase::MapChoice&&self.rm.phase()==Phase::MapChoice&&rs.total_floor>before_floor&&self.ppo.cleared.insert((rs.act_index,rs.total_floor)){events[0]+=1;}
  if won_combat {match before_room.map(|r|r.room_type){Some(sts2_core::runstate::RoomType::Elite)=>events[1]=1,Some(sts2_core::runstate::RoomType::Boss)=>events[2]=1,_=>{}}}
  events[3]=i32::from(terminal_win);events[4]=i32::from(done);events[5]=i32::from(won_combat);events[6]=phase(before_phase);events[7]=phase(self.rm.phase());
  for i in 0..4{self.ppo.event_counts[i]+=events[i];}
  Ok(events)
 }
}
#[no_mangle]
pub unsafe extern "C" fn sm_fr_encode_private(p:*mut c_void,dst:*mut i32,capacity:usize,lengths:*mut u32)->i32{guarded(||{
 let p=&mut*(p as *mut Pool);let n=p.slots.len();let data=std::slice::from_raw_parts_mut(dst,n*capacity*16);let lens=std::slice::from_raw_parts_mut(lengths,n);
 let all:Result<Vec<_>>=p.threads.install(||p.slots.par_iter().map(Slot::private_rows).collect());
 for (i,rows) in all?.iter().enumerate(){if rows.len()>capacity{return Err(format!("privileged capacity {capacity} < {}",rows.len()))}for (j,row) in rows.iter().enumerate(){data[i*capacity*16+j*16..i*capacity*16+(j+1)*16].copy_from_slice(row);}lens[i]=rows.len() as u32;}Ok(())
})}
#[no_mangle]
pub unsafe extern "C" fn sm_fr_step_ppo(p:*mut c_void,indices:*const i32,generations:*const u64,events:*mut i32)->i32{guarded(||{
 let p=&mut*(p as *mut Pool);for s in &mut p.slots{s.enable_combat_limit(p.combat_limit)?;}let n=p.slots.len();let a=std::slice::from_raw_parts(indices,n);let g=std::slice::from_raw_parts(generations,n);
 for(i,s)in p.slots.iter().enumerate(){if a[i]>=0&&(g[i]!=s.generation||a[i] as usize>=s.actions.len()){return Err(format!("stale PPO generation or action in slot {i}"));}}
 let output=std::slice::from_raw_parts_mut(events,n*8);
 p.threads.install(||p.slots.par_iter_mut().zip(a.par_iter()).zip(output.par_chunks_mut(8)).try_for_each(|((s,&a),out)|{out.fill(0);if a>=0{out.copy_from_slice(&s.ppo_advance(a)?);}Ok(())}))
})}
#[derive(Serialize,Deserialize)]struct Journal{version:u32,slots:Vec<JournalSlot>}
#[derive(Serialize,Deserialize)]struct JournalSlot{seed:u64,actions:Vec<i32>,state_hash:u64,generation:u64,event_counts:[i32;4],#[serde(default)]combat_actions:Option<usize>,#[serde(default)]combat_limit:usize,#[serde(default)]cap_from_action:usize,#[serde(default)]timed_out:bool}
#[no_mangle]
pub unsafe extern "C" fn sm_fr_journal(p:*mut c_void)->*const c_char {
 let mut out=std::ptr::null();guarded(||{let p=&mut*(p as *mut Pool);for s in &mut p.slots{s.enable_combat_limit(p.combat_limit)?;}let j=Journal{version:2,slots:p.slots.iter().map(|s|JournalSlot{seed:s.ppo.seed,actions:s.ppo.actions.clone(),state_hash:sts2_core::hashing::run_hash(&s.rm),generation:s.generation,event_counts:s.ppo.event_counts,combat_actions:Some(s.ppo.combat_actions),combat_limit:s.ppo.combat_limit,cap_from_action:s.ppo.cap_from_action,timed_out:s.ppo.timed_out}).collect()};OUTPUT.with(|x|{*x.borrow_mut()=CString::new(serde_json::to_string(&j).unwrap()).unwrap();out=x.borrow().as_ptr()});Ok(())});out
}
#[no_mangle]
pub unsafe extern "C" fn sm_fr_restore_journal(p:*mut c_void,journal:*const c_char)->i32{guarded(||{
 let p=&mut*(p as *mut Pool);let j:Journal=serde_json::from_str(CStr::from_ptr(journal).to_str().map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
 if ![1,2].contains(&j.version)||j.slots.len()!=p.slots.len(){return Err("journal version/slot mismatch".into())}
 let schema=&p.schema;
 let restored:Result<Vec<_>>=p.threads.install(||j.slots.par_iter().map(|j|{let mut s=Slot::new(j.seed);for(i,a)in j.actions.iter().enumerate(){if i==j.cap_from_action{s.ppo.combat_limit=j.combat_limit;s.ppo.cap_from_action=j.cap_from_action;}s.encode(schema)?;s.ppo_advance(*a)?;}s.ppo.combat_limit=j.combat_limit;s.ppo.cap_from_action=j.cap_from_action;s.generation=j.generation;
 if let Some(expected)=j.combat_actions{if s.ppo.combat_actions!=expected||s.ppo.timed_out!=j.timed_out{return Err("journal combat budget/timeout replay mismatch".into())}}
 if sts2_core::hashing::run_hash(&s.rm)!=j.state_hash||s.ppo.event_counts!=j.event_counts{return Err("journal replay state/event mismatch".into())}
 s.enable_combat_limit(p.combat_limit)?;s.encode(schema)?;Ok(s)}).collect());
 p.reset_cache=None;p.slots=restored?;Ok(())
})}
// Diagnostic: mutate only critic inputs. The actor public observation must stay identical.
#[no_mangle]
pub unsafe extern "C" fn sm_fr_test_privileged_intervention(p:*mut c_void,index:usize)->i32{guarded(||{
 let p=&mut*(p as *mut Pool);let s=p.slots.get_mut(index).ok_or("intervention slot")?;
 if let Some(c)=s.rm.combat.as_mut(){c.player.draw.reverse();}
 let rs=&mut s.rm.run_state;if let Some(g)=rs.act_index.checked_sub(1).and_then(|i|rs.generated_rooms.get_mut(i as usize)){g.normal_encounters.reverse();g.elite_encounters.reverse();}
 Ok(())
})}

// Reward-contract witnesses only: synthetic wins/losses never enter rollout training.
#[no_mangle]
pub unsafe extern "C" fn sm_fr_test_reward_fixture(p:*mut c_void,index:usize,kind:u32,events:*mut i32)->i32{guarded(||{
 let p=&mut*(p as *mut Pool);let s=p.slots.get_mut(index).ok_or("reward fixture slot")?;*s=Slot::new(7342+index as u64);
 for _ in 0..40{if s.rm.phase()==Phase::Combat{break}let a=*s.rm.get_available_actions().first().ok_or("reward fixture path")?;s.rm.take_action(a);s.prepare_decision()?;}
 if s.rm.phase()!=Phase::Combat{return Err("reward fixture has no combat".into())}
 let mut room=s.rm.run_state.current_room.ok_or("reward fixture room")?;
 room.room_type=match kind{2=>sts2_core::runstate::RoomType::Elite,3|4=>sts2_core::runstate::RoomType::Boss,_=>sts2_core::runstate::RoomType::Monster};s.rm.run_state.current_room=Some(room);
 if kind==4{s.rm.run_state.act_index=s.rm.run_state.acts.len() as u8;s.rm.run_state.has_double_boss=false;}
 let act=s.rm.run_state.act_index;let floor=s.rm.run_state.total_floor;s.rm.resolve_combat_fast(0.,kind!=5);s.prepare_decision()?;
 let e=s.ppo_account(Phase::Combat,act,floor,Some(room),false)?;std::slice::from_raw_parts_mut(events,8).copy_from_slice(&e);s.generation+=1;Ok(())
})}

// Ready next episodes use exactly the same per-slot seed stream as synchronous resets.
// Jobs share the existing 80-worker Rayon pool; no additional simulation workers.
pub(super) struct ResetCache {bins:Vec<(std::sync::Mutex<Option<Result<Slot>>>,std::sync::Condvar)>,pub stride:u64}
impl ResetCache {
 pub fn schedule(self:&Arc<Self>,threads:&Arc<rayon::ThreadPool>,index:usize,seed:u64){let cache=self.clone();threads.spawn(move||{
  let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||Slot::new(seed))).map_err(|_|format!("next-run initialization panicked for seed {seed}"));
  let (lock,ready)=&cache.bins[index];*lock.lock().unwrap()=Some(result);ready.notify_all();
 });}
 pub fn take(&self,index:usize)->Result<Slot>{let (lock,ready)=&self.bins[index];let mut item=lock.lock().map_err(|e|e.to_string())?;while item.is_none(){item=ready.wait(item).map_err(|e|e.to_string())?;}item.take().unwrap()}
 fn ensure_ready(&self,index:usize)->Result<()>{let (lock,ready)=&self.bins[index];let mut item=lock.lock().map_err(|e|e.to_string())?;while item.is_none(){item=ready.wait(item).map_err(|e|e.to_string())?;}if let Some(Err(e))=&*item{return Err(e.clone())}Ok(())}
}
#[no_mangle]
pub unsafe extern "C" fn sm_fr_prefill_resets(p:*mut c_void,enabled:bool)->i32{guarded(||{
 let p=&mut*(p as *mut Pool);p.reset_cache=None;
 if enabled{let cache=Arc::new(ResetCache{bins:(0..p.slots.len()).map(|_|(std::sync::Mutex::new(None),std::sync::Condvar::new())).collect(),stride:p.reset_stride});
 for(i,s)in p.slots.iter().enumerate(){cache.schedule(&p.threads,i,s.ppo.seed+cache.stride);}
 for i in 0..p.slots.len(){cache.ensure_ready(i)?;}p.reset_cache=Some(cache);
 }Ok(())
})}

// Preserve the original explicitly seeded reset API for non-PPO callers.
#[no_mangle]
pub unsafe extern "C" fn sm_fr_reset_terminal(p:*mut c_void,seed:u64)->i32{
 let mut count=0;let code=guarded(||{let p=&mut*(p as *mut Pool);p.reset_cache=None;
 count=p.threads.install(||p.slots.par_iter_mut().enumerate().map(|(i,s)|{if s.rm.phase()==Phase::RunOver{let generation=s.generation+1;*s=Slot::new(seed+i as u64);s.generation=generation;1}else{0}}).sum());Ok(())});if code<0{code}else{count}
}

/// Public, observed context only. This does not consult generated normal/elite lists.
pub(super) fn public_combat_context(rm:&RunManager,out:&mut Vec<Row>,schema:&Schema)->Result<()>{
 row(out,31,6,-1,0,&[0,0,0]);
 if rm.phase()!=Phase::Combat{return Ok(())}let Some(c)=rm.combat.as_ref() else{return Ok(())};let log=&c.combat_log;
 // Replace placeholder adapter counters with the native public counters.
 if let Some(r)=out.iter_mut().find(|r|r[0]==13){r[4]=log.cards_drawn_by_effect_this_turn as i32;r[5]=log.cards_discarded_this_turn as i32;r[6]=log.cards_exhausted_this_turn as i32;r[7]=log.hp_lost_this_turn as i32;r[11]=log.card_plays_finished_this_turn as i32;}
 row(out,31,4,1,0,&[log.player_card_plays_this_combat as i32,log.card_plays_finished_last_player_turn as i32,log.status_cards_drawn_this_turn as i32,log.osty_attacks_this_turn,log.unblocked_damage_received_this_combat as i32,log.zero_cost_attacks_played_this_turn as i32]);
 if c.player.orbs.capacity>0{let focus=sts2_core::orbs::focus_bonus(c);row(out,36,0,1,-1,&[c.player.orbs.capacity.into(),c.player.orbs.slots.len() as i32,focus]);for(i,o)in c.player.orbs.slots.iter().enumerate(){use sts2_core::Orb::*;let(kind,passive,evoke)=match o{Empty=>(1,0,0),Lightning=>(2,(3+focus).max(0),(8+focus).max(0)),Frost=>(3,(2+focus).max(0),(5+focus).max(0)),Dark{stored}=>(4,(6+focus).max(0),stored+6),Plasma=>(5,1,2),Glass{current_passive}=>(6,(current_passive+focus).max(0),(2*current_passive+focus).max(0))};row(out,36,kind,1,i as i32,&[passive,evoke]);}}
 let rs=&rm.run_state;
 if let Some(b)=rs.pinned_act_boss.as_ref(){row(out,37,schema.numeric.id(b.lead_kind.python_name())?,1,0,&[]);}
 let Some(m)=rs.current_act_map.as_ref()else{return Ok(())};let Some(origin)=rs.current_room.and_then(|r|r.map_coord).and_then(|x|m.grid.get(&x).copied().or_else(||m.boss.filter(|i|m.point(*i).coord==x)))else{return Ok(())};
 let mut live:BTreeSet<_>=m.grid.values().copied().collect();live.extend(m.boss);live.extend(m.second_boss);
 let mut order:Vec<_>=live.iter().copied().collect();order.sort_by_key(|i|std::cmp::Reverse(m.point(*i).coord.row));let mut dp:BTreeMap<MapPointIdx,[i32;5]>=BTreeMap::new();
 for i in order{let node=m.point(i);let children:Vec<_>=node.children.iter().filter_map(|c|dp.get(c)).collect();let best=|k:usize|children.iter().filter_map(|v|if v[k]>=0{Some(v[k])}else{None}).min();
 let rest=if node.point_type==MapPointType::RestSite{0}else{best(0).map_or(-1,|x|x+1)};let shop=if node.point_type==MapPointType::Shop{0}else{best(1).map_or(-1,|x|x+1)};let boss=if node.point_type==MapPointType::Boss{0}else{best(2).map_or(-1,|x|x+1)};let elite=if node.point_type==MapPointType::RestSite{0}else{best(3).map_or(-1,|x|x+i32::from(node.point_type==MapPointType::Elite))};let fights=if node.point_type==MapPointType::Boss{1}else{best(4).map_or(-1,|x|x+i32::from(node.point_type.is_combat()))};dp.insert(i,[rest,shop,boss,elite,fights]);}
 for &i in &m.point(origin).children{if let Some(d)=dp.get(&i){let node=m.point(i);row(out,31,3,1,node.coord.col,&[node.point_type as i32,node.coord.row,node.coord.col,d[2],d[0],d[1],d[3],d[4]]);}}
 Ok(())
}
#[no_mangle]
pub unsafe extern "C" fn sm_fr_test_public_extras(p:*mut c_void,index:usize)->i32{guarded(||{let p=&mut*(p as *mut Pool);let s=p.slots.get_mut(index).ok_or("extras fixture slot")?;let c=s.rm.combat.as_mut().ok_or("extras need combat")?;c.player.orbs=sts2_core::OrbSlots::with_capacity(3);c.player.orbs.channel(sts2_core::Orb::Frost);c.player.orbs.channel(sts2_core::Orb::Dark{stored:7});c.combat_log.cards_drawn_by_effect_this_turn=2;c.combat_log.cards_discarded_this_turn=3;c.combat_log.cards_exhausted_this_turn=4;c.combat_log.hp_lost_this_turn=5;Ok(())})}

// Offline-in-process bank: complete seeded RunManagers, not just integer seeds.
// No future-start data is encoded into the actor or critic's current observation.
pub(super) struct StartBank {pub base:u64, pub starts:Vec<Option<RunManager>>, pub consumed:usize, pub seconds:f64, pub rolling:Option<Vec<std::collections::VecDeque<(u64,RunManager)>>>,pub depth:usize,pub generated:usize,pub refills:usize}
#[no_mangle]
pub unsafe extern "C" fn sm_fr_prepare_start_bank(p:*mut c_void,base:u64,count:usize)->i32{guarded(||{
 let p=&mut*(p as *mut Pool);if count==0||count>2_000_000||base.checked_add(count as u64).is_none(){return Err("invalid start-bank range".into())}
 let begun=std::time::Instant::now();
 let starts=p.threads.install(||(0..count).into_par_iter().map(|i|Some(Slot::new(base+i as u64).rm)).collect());
 p.reset_cache=None;p.start_bank=Some(StartBank{base,starts,consumed:0,seconds:begun.elapsed().as_secs_f64(),rolling:None,depth:0,generated:count,refills:0});Ok(())
})}
#[no_mangle]
pub unsafe extern "C" fn sm_fr_start_bank_stats(p:*mut c_void)->*const c_char{
 let mut out=std::ptr::null();guarded(||{let p=&mut*(p as *mut Pool);let b=p.start_bank.as_ref().ok_or("no start bank")?;
 let value=if let Some(q)=&b.rolling{serde_json::json!({"mode":"rolling_per_slot","depth":b.depth,"count":b.generated,"consumed":b.consumed,"remaining":q.iter().map(|x|x.len()).sum::<usize>(),"minimum_ready_per_slot":q.iter().map(|x|x.len()).min(),"refills":b.refills,"preparation_seconds":b.seconds,"fallback_generation":false})}else{serde_json::json!({"base":b.base,"count":b.starts.len(),"consumed":b.consumed,"remaining":b.starts.len()-b.consumed,"preparation_seconds":b.seconds,"storage":"complete in-memory RunManager objects","fallback_generation":false})};
 OUTPUT.with(|x|{*x.borrow_mut()=CString::new(value.to_string()).unwrap();out=x.borrow().as_ptr()});Ok(())});out
}

#[no_mangle]pub unsafe extern "C" fn sm_fr_seed_stride(p:*mut c_void,stride:u64)->i32{guarded(||{let p=&mut*(p as *mut Pool);if stride<p.slots.len() as u64{return Err("seed stride below slot count".into())}p.reset_stride=stride;Ok(())})}

// Runs at policy-update boundaries. All generation shares the existing full-run Rayon pool.
#[no_mangle]pub unsafe extern "C" fn sm_fr_refill_rolling_bank(p:*mut c_void,depth:usize)->i32{guarded(||{
 let p=&mut*(p as *mut Pool);if depth<256||depth>2048{return Err("rolling depth must cover >=4 rollouts and be bounded".into())}
 if p.start_bank.as_ref().is_none_or(|b|b.rolling.is_none()){p.reset_cache=None;p.start_bank=Some(StartBank{base:0,starts:Vec::new(),consumed:0,seconds:0.,rolling:Some((0..p.slots.len()).map(|_|std::collections::VecDeque::new()).collect()),depth,generated:0,refills:0});}
 let b=p.start_bank.as_mut().unwrap();if b.depth!=depth{return Err("rolling depth changed".into())}let q=b.rolling.as_mut().unwrap();let stride=p.reset_stride;let begun=std::time::Instant::now();
 let mut jobs=Vec::new();for(i,(queue,s))in q.iter().zip(&p.slots).enumerate(){if queue.len()<=depth/4{let mut next=queue.back().map(|x|x.0).unwrap_or(s.ppo.seed);for _ in queue.len()..depth{next=next.checked_add(stride).ok_or("seed overflow")?;if next>=1u64<<63{return Err("training seed entered heldout namespace".into())}jobs.push((i,next));}}}
 if !jobs.is_empty(){let ready:Vec<_>=p.threads.install(||jobs.par_iter().map(|&(i,seed)|(i,seed,Slot::new(seed).rm)).collect());b.generated+=ready.len();b.refills+=1;for(i,seed,rm)in ready{q[i].push_back((seed,rm));}b.seconds+=begun.elapsed().as_secs_f64();}Ok(())
})}
#[no_mangle]pub unsafe extern "C" fn sm_fr_run_stats(p:*mut c_void)->*const c_char{let mut out=std::ptr::null();guarded(||{let p=&*(p as *mut Pool);let rows:Vec<_>=p.slots.iter().map(|s|{let r=&s.rm.run_state;serde_json::json!({"seed":s.ppo.seed,"decisions":s.ppo.actions.len(),"phase":phase(s.rm.phase()),"terminal":s.rm.phase()==Phase::RunOver,"won":r.player_won,"act":r.act_index,"floor":r.total_floor,"hp":r.current_hp,"max_hp":r.max_hp,"combat_actions":s.ppo.combat_actions,"combat_action_limit":s.ppo.combat_limit,"combat_timeout":s.ppo.timed_out,"hash":sts2_core::hashing::run_hash(&s.rm)})}).collect();OUTPUT.with(|x|{*x.borrow_mut()=CString::new(serde_json::to_string(&rows).unwrap()).unwrap();out=x.borrow().as_ptr()});Ok(())});out}

// Cap is in the PPO adapter, not the game-rule engine or curriculum simulator.
#[no_mangle]pub unsafe extern "C" fn sm_fr_set_combat_limit(p:*mut c_void,limit:usize)->i32{guarded(||{let p=&mut*(p as *mut Pool);if limit>100_000{return Err("invalid combat action cap".into())}for s in &mut p.slots{s.enable_combat_limit(limit)?;}p.combat_limit=limit;Ok(())})}

// Isolated boundary witness only; not used by production or validation.
#[no_mangle]pub unsafe extern "C" fn sm_fr_test_combat_counter(p:*mut c_void,index:usize,actions:usize)->i32{guarded(||{let p=&mut*(p as *mut Pool);let s=p.slots.get_mut(index).ok_or("test slot")?;if s.rm.phase()!=Phase::Combat{return Err("counter witness requires combat".into())}s.ppo.combat_actions=actions;Ok(())})}
