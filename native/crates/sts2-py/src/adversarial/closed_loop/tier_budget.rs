//! Resolve a combat's fixed search budget once, before any actions are played.
use super::*;
use sts2_content::encounters::{self, EncounterTier};

#[derive(Debug, Clone, Copy, Deserialize, serde::Serialize)]
pub(super) struct Budgets {
    pub easy: usize,
    pub hard: usize,
    pub elite: usize,
    pub boss: usize,
}
impl Budgets {
    pub fn valid(&self) -> bool {
        [self.easy, self.hard, self.elite, self.boss].iter().all(|&n| (1..=256).contains(&n))
    }
    pub fn resolve(&self, fixture: &Value) -> Result<(&'static str, usize)> {
        let id = fixture["metadata"]["encounter"].as_str().unwrap_or("").to_ascii_uppercase();
        let id = id.strip_prefix("ENCOUNTER.").unwrap_or(&id);
        let tier = if let Some(def) = encounters::find(id) {
            def.tier
        } else {
            // Generated corpus starts have explicit pool labels but may lack an encounter ID.
            match fixture["metadata"]["room_type"].as_str().unwrap_or("") {
                "weak" | "easy" => EncounterTier::Weak,
                "normal" | "hard" => EncounterTier::Normal,
                "elite" => EncounterTier::Elite,
                "boss" => EncounterTier::Boss,
                "event" => EncounterTier::Normal,
                "monster" if id.ends_with("_WEAK") => EncounterTier::Weak,
                "monster" if id.ends_with("_NORMAL") || id.contains("EVENT") => EncounterTier::Normal,
                _ => return Err(format!("cannot classify search tier: encounter={id}, room_type={}", fixture["metadata"]["room_type"])),
            }
        };
        Ok(match tier {
            EncounterTier::Weak => ("easy", self.easy),
            EncounterTier::Normal => ("hard", self.hard),
            EncounterTier::Elite => ("elite", self.elite),
            EncounterTier::Boss => ("boss", self.boss),
            _ => return Err("unsupported future encounter tier".into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const B: Budgets = Budgets { easy:16, hard:64, elite:96, boss:128 };
    #[test]
    fn registry_and_generated_pool_labels() {
        for (id,tier,n) in [("SEAPUNK_WEAK","easy",16),("THE_OBSCURA_NORMAL","hard",64),("DECIMILLIPEDE_ELITE","elite",96),("QUEEN_BOSS","boss",128)] {
            assert_eq!(B.resolve(&json!({"metadata":{"encounter":id,"room_type":"monster"}})).unwrap(),(tier,n));
        }
        for (room,tier,n) in [("weak","easy",16),("normal","hard",64),("elite","elite",96),("boss","boss",128),("event","hard",64)] {
            assert_eq!(B.resolve(&json!({"metadata":{"encounter":"unknown","room_type":room}})).unwrap(),(tier,n));
        }
        assert_eq!(B.resolve(&json!({"metadata":{"encounter":"ENCOUNTER.SEAPUNK_WEAK","room_type":"monster"}})).unwrap(),("easy",16));
    }
    #[test]
    fn fail_closed_on_ambiguous_pool_and_invalid_budget() {
        assert!(B.resolve(&json!({"metadata":{"encounter":"unknown","room_type":"monster"}})).is_err());
        assert!(B.valid());assert!(!Budgets{easy:0,..B}.valid());assert!(!Budgets{boss:257,..B}.valid());
    }
}
