//! v30 terminal utility and persistent, cloned per-combat reward context.
use sts2_core::run_rng_set::splitmix64;
use sts2_core::{CombatState, PotionKind};
#[derive(Debug, Clone)]
pub(super) struct Potion {
    pub id: u32,
    pub slot: u8,
    pub kind: PotionKind,
    pub cost: f32,
    pub used: bool,
}
#[derive(Debug, Clone, Default)]
pub(super) struct Context {
    pub enabled: bool,
    pub initial_hp: f32,
    pub damage: Vec<(u32, i32, i32)>,
    pub ledger: Vec<Potion>,
    pub active: Vec<usize>,
    pub spent: f32,
    seed: u64,
    fixed: Option<f32>,
}
impl Context {
    #[cfg(feature = "native-gpu-feeder")]
    pub fn new(s: &CombatState, seed: u64, fixed: Option<f32>) -> Self {
        let mut c = Self {
            enabled: true,
            initial_hp: s.player.current_hp as f32,
            seed,
            fixed,
            ..Self::default()
        };
        c.damage = s
            .enemy_uids
            .iter()
            .filter_map(|u| {
                s.creatures
                    .iter()
                    .find(|v| v.id == u.0)
                    .map(|v| (u.0.get(), v.current_hp.max(0), v.current_hp.max(0)))
            })
            .collect();
        c.update(s, None);
        c
    }
    #[cfg(feature = "native-gpu-feeder")]
    pub fn set_initial_costs(&mut self, costs: &[f32]) -> Result<(), String> {
        if costs.len() != self.active.len()
            || costs
                .iter()
                .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
            || self.spent != 0.0
            || self.ledger.iter().any(|p| p.used)
        {
            return Err("invalid initial potion costs".into());
        }
        for (&id, &cost) in self.active.iter().zip(costs) {
            self.ledger[id].cost = cost;
        }
        Ok(())
    }
    pub fn update(&mut self, s: &CombatState, used: Option<usize>) {
        if !self.enabled {
            return;
        }
        if let Some(i) = used {
            let id = self.active.remove(i);
            let p = &mut self.ledger[id];
            assert!(!p.used, "potion instance consumed twice");
            p.used = true;
            self.spent += p.cost;
        }
        let old = std::mem::take(&mut self.active);
        for (i, &kind) in s.player.potions.iter().enumerate() {
            let slot = *s.player.potion_slot_indices.get(i).unwrap_or(&(i as u8));
            let found = old.iter().copied().find(|&id| {
                self.ledger[id].kind == kind
                    && self.ledger[id].slot == slot
                    && !self.active.contains(&id)
            });
            let id = found.unwrap_or_else(|| {
                let id = self.ledger.len();
                let bits = splitmix64(self.seed ^ splitmix64(id as u64 + 734));
                let cost = self
                    .fixed
                    .unwrap_or(((bits >> 11) as f64 / (1u64 << 53) as f64) as f32);
                self.ledger.push(Potion {
                    id: id as u32,
                    slot,
                    kind,
                    cost,
                    used: false,
                });
                id
            });
            self.active.push(id);
        }
        for (uid, start, min) in &mut self.damage {
            if let Some(c) = s.creatures.iter().find(|v| v.id.get() == *uid) {
                *min = (*min).min(c.current_hp.clamp(0, *start));
            }
        }
    }
    pub fn initial_enemy_hp(&self) -> i32 {
        self.damage.iter().map(|(_, h, _)| *h).sum()
    }
    pub fn progress(&self) -> f32 {
        let total = self.initial_enemy_hp();
        if total == 0 {
            return 0.0;
        }
        self.damage.iter().map(|(_, h, m)| h - m).sum::<i32>() as f32 / total as f32
    }
    #[cfg(feature = "native-gpu-feeder")]
    pub fn utility(&self, s: &CombatState) -> f32 {
        let win = s.is_over && s.player_won && s.player.current_hp > 0;
        utility(
            win,
            s.player.current_hp.max(0) as f32,
            self.initial_hp,
            self.spent,
            self.progress(),
        )
    }
}
#[cfg(any(feature = "native-gpu-feeder", test))]
pub(super) fn utility(win: bool, hp: f32, start: f32, spent: f32, progress: f32) -> f32 {
    f32::from(u8::from(win))
        + 0.5 * (hp / start) * (1.0 - spent)
        + if win { 0.0 } else { 0.05 * progress }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_v30_no_clipping_and_loss_bonus() {
        assert!((utility(true, 120.0, 100.0, 0.0, 1.0) - 1.6).abs() < 1e-6);
        assert!((utility(true, 100.0, 100.0, 1.7, 1.0) - 0.65).abs() < 1e-6);
        assert!((utility(false, 0.0, 100.0, 0.8, 0.6) - 0.03).abs() < 1e-6);
    }
}
