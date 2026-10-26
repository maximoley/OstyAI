//! Offline-only compatibility preparation; never called inside timed search.
use std::{fs::{self, File}, io::{BufRead, BufReader, BufWriter, Write}, path::PathBuf};
use serde_json::{json, Value};
fn main() {
    let args:Vec<String>=std::env::args().collect();
    assert_eq!(args.len(),3,"prepare_mcts_fixtures SOURCE_DIRECTORY OUTPUT_DIRECTORY");
    let source=PathBuf::from(&args[1]);let output=PathBuf::from(&args[2]);
    assert!(!output.exists(),"Refuse to overwrite prepared benchmark fixtures");fs::create_dir_all(&output).unwrap();
    sts2_content::cards::dispatch::install_default_registry();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();
    let manifest:Value=serde_json::from_slice(&fs::read(source.join("manifest.json")).unwrap()).unwrap();
    let mut count=0;let mut edits=Vec::new();let mut informational=Vec::new();let start=std::time::Instant::now();
    for part in manifest["shards"].as_array().unwrap() {
        let file=part["file"].as_str().unwrap();let path=output.join(file);fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut writer=BufWriter::new(File::create(path).unwrap());
        for (index,line) in BufReader::new(File::open(source.join(file)).unwrap()).lines().enumerate() {
            let mut f:Value=serde_json::from_str(&line.unwrap()).unwrap();
            let original=f.clone();let mut reference=sts2_wire::state_from_fixture_json(&f["wire"]).unwrap();
            for (i,enemy)in f["wire"]["encounter"].as_array_mut().unwrap().iter_mut().enumerate() {
                let kind=enemy["kind"].as_str().unwrap_or("");let old=enemy["move_state"].as_str().unwrap_or("");
                if matches!((kind,old),("SkulkingColony","SMASH_MOVE")|("HauntedShip","RAMMING_SPEED_MOVE")|("Axebot","SHARPEN_MOVE"))
                    && enemy["hp"].as_i64().unwrap_or(0)>0 && !enemy["is_dead"].as_bool().unwrap_or(false) {
                    let c=sts2_core::creature_by_id(&reference.creatures,reference.enemy_uids[i].0).unwrap();
                    let ai=c.ai.as_ref().unwrap();let new=ai.current_move().id;
                    edits.push(json!({"file":file,"index":index,"enemy":i,"kind":kind,"from":old,"to":new}));
                    enemy["move_state"]=json!(new);
                    enemy["intents"]=json!(ai.current_move().intents.first().copied().map(sts2_wire::emit::combat::bare_intent_to_value).into_iter().collect::<Vec<_>>());
                }
            }
            // Full state equality, including RNG, AI, HP, powers and continuations.
            let plain=sts2_wire::state_from_json(&f["wire"]).unwrap();
            for (i,(before,after)) in reference.creatures.iter_mut().zip(plain.creatures.iter()).enumerate() {
                if before.intent!=after.intent && before.species==Some(sts2_core::MonsterKind::HauntedShip)
                    && before.ai.as_ref().is_some_and(|a|a.current_move().id=="HAUNT_MOVE") {
                    assert_eq!(before.intent,Some(sts2_core::Intent::Debuff{kind:sts2_core::PowerKind::Weak,amount:3}));
                    assert_eq!(after.intent,Some(sts2_core::Intent::Debuff{kind:sts2_core::PowerKind::Weak,amount:0}));
                    informational.push(json!({"file":file,"index":index,"creature":i,"change":"displayed debuff amount 3 to older-loader 0; native HAUNT effect still applies Weak 3"}));
                    before.intent=after.intent;
                }
            }
            assert!(sts2_wire::state_from_fixture_json(&f["wire"]).unwrap()==plain,"neural and MCTS import must match exactly after preparation");
            if reference!=plain {
                fs::write(output.join("mismatch-reference.json"),serde_json::to_vec_pretty(&sts2_wire::emit::combat::combat_state_to_value(&reference).unwrap()).unwrap()).unwrap();
                fs::write(output.join("mismatch-plain.json"),serde_json::to_vec_pretty(&sts2_wire::emit::combat::combat_state_to_value(&plain).unwrap()).unwrap()).unwrap();
                fs::write(output.join("mismatch-input.json"),serde_json::to_vec_pretty(&f).unwrap()).unwrap();
                panic!("state mismatch in {file}:{index}");
            }
            assert_eq!(f["wire"]["rng"],original["wire"]["rng"]);
            assert_eq!(f["context"],original["context"]);assert_eq!(f["metadata"],original["metadata"]);assert_eq!(f["evaluation"],original["evaluation"]);
            serde_json::to_writer(&mut writer,&f).unwrap();writeln!(writer).unwrap();count+=1;
        }
    }
    fs::write(output.join("preparation.json"),serde_json::to_vec_pretty(&json!({"trials":count,"changed_enemies":edits,"state_equality_except_recorded_informational_intent":true,"neural_and_mcts_prepared_state_equality":true,"informational_intent_normalizations":informational,"rng_and_context_preserved":true,"seconds":start.elapsed().as_secs_f64()})).unwrap()).unwrap();
    println!("Verified {count} fixture states; {} obsolete openings normalized offline",edits.len());
}
