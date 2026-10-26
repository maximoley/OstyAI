use std::{io::{BufRead,BufReader},fs::File};
use serde_json::{json,Value};
fn main() {
    sts2_content::cards::dispatch::install_default_registry();
    sts2_content::relics::dispatch::ensure_installed();
    sts2_content::potions::dispatch::ensure_installed();
    for path in std::env::args().skip(1) {
        let mut missing=Vec::new();let mut errors=Vec::new();let mut total=0;
        for (id,line) in BufReader::new(File::open(&path).unwrap()).lines().enumerate() {
            let f:Value=serde_json::from_str(&line.unwrap()).unwrap(); total+=1;
            match sts2_wire::state_from_fixture_json(&f["wire"]) {
                Ok(s)=>for (i,c) in s.creatures.iter().filter(|c|s.enemy_uids.iter().any(|u|u.0==c.id)).enumerate() {
                    if c.current_hp>0 && c.ai.is_none() {
                        missing.push(json!({"id":id,"enemy":i,"kind":format!("{:?}",c.species),"move":f["wire"]["encounter"][i]["move_state"],"round":s.round,"encounter":f["metadata"]["encounter"]}));
                    }
                },
                Err(e)=>errors.push(json!({"id":id,"error":e.to_string()}))
            }
        }
        println!("{}",json!({"file":path,"total":total,"missing":missing,"errors":errors}));
    }
}
