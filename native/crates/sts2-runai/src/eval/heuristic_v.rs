//! `HeuristicV` — the tier-1 run-level value function (plan §5/M1).
//!
//! Maps a [`VisibleRunView`] to a win-likelihood-ish scalar. Features
//! (plan §5/M1): deck histogram with upgrades + curve metrics
//! (frontloaded damage, AoE count, scaling count, block density, energy
//! curve), relic flat values, HP fraction × act-progress weight, gold,
//! pinned-boss identity, ascension scalar. All weights live in
//! [`Weights`] (TOML, Texel-style tunable — `data/runai/heuristic_v.toml`).
//!
//! Deliberately simple, monotone, documented heuristics — this is the
//! M1 prior that M2's rollout labels retune, not a final evaluator.
//! Consumes ONLY the `VisibleRunView` (§2.5 hard rule, see `view.rs`).

use crate::eval::card_stats::card_stats;
use crate::eval::Weights;
use crate::view::VisibleRunView;

/// Tier-1 heuristic value function over [`VisibleRunView`]s.
#[derive(Clone, Debug, Default)]
pub struct HeuristicV {
    pub weights: Weights,
}

/// Deck-feature accumulator shared by `deck_term` and `feature_map`.
#[derive(Clone, Copy, Debug, Default)]
struct DeckAccum {
    n: f64,
    frontload: f64,
    block: f64,
    aoe: f64,
    scaling: f64,
    upgrades: f64,
    draw: f64,
    energy: f64,
    curses: f64,
    heavy: f64,
}

impl HeuristicV {
    #[must_use]
    pub fn new(weights: Weights) -> Self {
        Self { weights }
    }

    /// Full score: deck + relics + HP + gold + potions + boss identity +
    /// ascension. Higher is better; roughly 0-centered around "starter
    /// deck, full HP".
    #[must_use]
    pub fn score(&self, v: &VisibleRunView) -> f64 {
        self.deck_term(v)
            + self.relic_term(v)
            + self.hp_term(v.hp, v.max_hp, v.act_progress())
            + self.gold_term(v.gold)
            + self.potion_term(v)
            + self.boss_offset_term(v)
            + self.ascension_term(v)
    }

    /// HP component (public so `projection.rs` can swap the current HP
    /// term for the projected-at-boss HP term without re-scoring the
    /// whole view). Monotone in `hp`:
    /// `hp_frac * (hp_base + hp_act * progress) + low_hp * min(0, hp_frac - 0.5)`.
    #[must_use]
    pub fn hp_term(&self, hp: i32, max_hp: i32, act_progress: f64) -> f64 {
        let frac = if max_hp <= 0 {
            0.0
        } else {
            (f64::from(hp) / f64::from(max_hp)).clamp(0.0, 1.0)
        };
        let w = &self.weights;
        frac * (w.hp_base + w.hp_act * act_progress) + w.low_hp * (frac - 0.5).min(0.0)
    }

    /// Gold component (public for projection): diminishing sqrt curve,
    /// monotone.
    #[must_use]
    pub fn gold_term(&self, gold: i32) -> f64 {
        self.weights.gold_sqrt * f64::from(gold.max(0)).sqrt()
    }

    /// Deck component: density terms (frontload damage / block per card)
    /// + count terms (AoE, scaling, upgrades, draw, energy) − penalties
    /// (curses, deck bloat, heavy costs).
    #[must_use]
    pub fn deck_term(&self, v: &VisibleRunView) -> f64 {
        let w = &self.weights;
        let a = Self::deck_accum(v);

        let boss_aoe = if self.boss_is_summon_heavy(v) {
            w.boss.aoe_bonus * a.aoe
        } else {
            0.0
        };

        w.frontload_per_card * (a.frontload / a.n)
            + w.block_per_card * (a.block / a.n)
            + w.aoe_count * a.aoe
            + boss_aoe
            + w.scaling_count * a.scaling
            + w.upgrade_count * a.upgrades
            + w.draw_total * a.draw
            + w.energy_total * a.energy
            + w.curse_count * a.curses
            + w.deck_size_penalty * (a.n - w.deck_size_target).max(0.0)
            + w.heavy_cost_penalty * a.heavy
    }

    /// Shared deck-feature accumulation used by both [`Self::deck_term`]
    /// and [`Self::feature_map`] so the score and the exported feature
    /// vector can never drift (M2 retune pipeline, bead
    /// `sts2-rs-40ikw.6`).
    fn deck_accum(v: &VisibleRunView) -> DeckAccum {
        let mut a = DeckAccum {
            n: v.deck.len().max(1) as f64,
            ..DeckAccum::default()
        };
        for card in &v.deck {
            let s = card_stats(card.card_id, card.upgraded);
            a.frontload += s.frontload();
            a.block += s.block_per_play;
            if s.aoe {
                a.aoe += 1.0;
            }
            if s.scaling {
                a.scaling += 1.0;
            }
            if card.upgraded {
                a.upgrades += 1.0;
            }
            a.draw += s.draw;
            a.energy += s.energy;
            if s.curse_like {
                a.curses += 1.0;
            }
            a.heavy += (s.cost - 2.0).max(0.0);
        }
        a
    }

    /// Export the linear feature vector `f` such that
    /// `score(v) == Σ_name weights[name] · f[name] + f["_fixed_offset"]`
    /// — the M2 retune surface (`retune_v.py` fits new weights via
    /// logistic regression on these feature *diffs*; bead
    /// `sts2-rs-40ikw.6`). Keys match the TOML weight names
    /// (section-qualified for the section weights). `_fixed_offset`
    /// collects the non-refittable parts: per-relic value overrides and
    /// per-boss calibration offsets. `deck_size_target` is folded into
    /// the `deck_size_penalty` feature and stays fixed during a refit.
    #[must_use]
    pub fn feature_map(&self, v: &VisibleRunView) -> std::collections::BTreeMap<&'static str, f64> {
        let w = &self.weights;
        let a = Self::deck_accum(v);

        let hp_frac = if v.max_hp <= 0 {
            0.0
        } else {
            (f64::from(v.hp) / f64::from(v.max_hp)).clamp(0.0, 1.0)
        };
        let progress = v.act_progress();

        // Relics split: default-valued relics are a refittable count;
        // table-overridden relics fold into the fixed offset.
        let mut relic_default_count = 0.0;
        let mut fixed = 0.0;
        for r in &v.relics {
            match w.relic_values.get(format!("{r:?}").as_str()) {
                Some(val) => fixed += val,
                None => relic_default_count += 1.0,
            }
        }
        fixed += self.boss_offset_term(v);

        let n_potions = v.potions.len() as f64;
        let mut f = std::collections::BTreeMap::new();
        f.insert("hp_base", hp_frac);
        f.insert("hp_act", hp_frac * progress);
        f.insert("low_hp", (hp_frac - 0.5).min(0.0));
        f.insert("gold_sqrt", f64::from(v.gold.max(0)).sqrt());
        f.insert("frontload_per_card", a.frontload / a.n);
        f.insert("block_per_card", a.block / a.n);
        f.insert("aoe_count", a.aoe);
        f.insert(
            "boss.aoe_bonus",
            if self.boss_is_summon_heavy(v) {
                a.aoe
            } else {
                0.0
            },
        );
        f.insert("scaling_count", a.scaling);
        f.insert("upgrade_count", a.upgrades);
        f.insert("draw_total", a.draw);
        f.insert("energy_total", a.energy);
        f.insert("curse_count", a.curses);
        f.insert("deck_size_penalty", (a.n - w.deck_size_target).max(0.0));
        f.insert("heavy_cost_penalty", a.heavy);
        f.insert("relic_default", relic_default_count);
        f.insert("potions.held", n_potions);
        f.insert("potions.boss_insurance", n_potions * v.boss_proximity());
        f.insert("ascension.per_level", f64::from(v.ascension));
        f.insert("_fixed_offset", fixed);
        f
    }

    /// Weight value for a [`Self::feature_map`] key (`_fixed_offset` →
    /// 1.0). Used by the dot-product consistency test and any consumer
    /// reconstructing the score from exported features.
    #[must_use]
    pub fn weight_for_feature(&self, name: &str) -> f64 {
        let w = &self.weights;
        match name {
            "hp_base" => w.hp_base,
            "hp_act" => w.hp_act,
            "low_hp" => w.low_hp,
            "gold_sqrt" => w.gold_sqrt,
            "frontload_per_card" => w.frontload_per_card,
            "block_per_card" => w.block_per_card,
            "aoe_count" => w.aoe_count,
            "boss.aoe_bonus" => w.boss.aoe_bonus,
            "scaling_count" => w.scaling_count,
            "upgrade_count" => w.upgrade_count,
            "draw_total" => w.draw_total,
            "energy_total" => w.energy_total,
            "curse_count" => w.curse_count,
            "deck_size_penalty" => w.deck_size_penalty,
            "heavy_cost_penalty" => w.heavy_cost_penalty,
            "relic_default" => w.relic_default,
            "potions.held" => w.potions.held,
            "potions.boss_insurance" => w.potions.boss_insurance,
            "ascension.per_level" => w.ascension.per_level,
            "_fixed_offset" => 1.0,
            _ => 0.0,
        }
    }

    /// Relic component: per-relic flat values (TOML table keyed by the
    /// `RelicId` Debug name, `relic_default` otherwise).
    #[must_use]
    pub fn relic_term(&self, v: &VisibleRunView) -> f64 {
        let w = &self.weights;
        v.relics
            .iter()
            .map(|r| {
                w.relic_values
                    .get(format!("{r:?}").as_str())
                    .copied()
                    .unwrap_or(w.relic_default)
            })
            .sum()
    }

    /// Potion component: hold value + boss insurance rising with
    /// proximity to the act boss (plan §2 potion economy).
    #[must_use]
    pub fn potion_term(&self, v: &VisibleRunView) -> f64 {
        let w = &self.weights.potions;
        let n = v.potions.len() as f64;
        n * (w.held + w.boss_insurance * v.boss_proximity())
    }

    /// `true` when the pinned act boss is in the summon-heavy list
    /// (AoE-count interaction in [`Self::deck_term`]).
    #[must_use]
    pub fn boss_is_summon_heavy(&self, v: &VisibleRunView) -> bool {
        v.boss.is_some_and(|b| {
            self.weights
                .boss
                .summon_boss_names
                .iter()
                .any(|n| n == b.display_name)
        })
    }

    /// Flat per-boss calibration offset (rank-inert across same-state
    /// candidates).
    #[must_use]
    pub fn boss_offset_term(&self, v: &VisibleRunView) -> f64 {
        v.boss.map_or(0.0, |b| {
            self.weights
                .boss
                .offsets
                .get(b.display_name)
                .copied()
                .unwrap_or(0.0)
        })
    }

    /// Ascension calibration scalar (rank-inert).
    #[must_use]
    pub fn ascension_term(&self, v: &VisibleRunView) -> f64 {
        self.weights.ascension.per_level * f64::from(v.ascension)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sts2_content::run_callbacks::new_run_manager_for_character;
    use sts2_core::cards::CardId;
    use sts2_core::potions::CharacterId;

    fn fresh_view() -> VisibleRunView {
        let rm = new_run_manager_for_character(11, CharacterId::Ironclad);
        VisibleRunView::from_run_manager(&rm)
    }

    /// V is monotone in HP (M1 acceptance test).
    #[test]
    fn v_monotone_in_hp() {
        let v = HeuristicV::default();
        let view = fresh_view();
        let mut prev = f64::NEG_INFINITY;
        for hp in [1, 20, 40, 60, view.max_hp] {
            let s = v.score(&view.with_hp(hp));
            assert!(s > prev, "V must rise with HP (hp={hp}: {s} <= {prev})");
            prev = s;
        }
    }

    /// Adding an upgraded good card beats adding nothing (M1 acceptance
    /// test), and a curse strictly hurts.
    #[test]
    fn v_rewards_good_card_and_punishes_curse() {
        let v = HeuristicV::default();
        let view = fresh_view();
        let base = v.score(&view);
        let with_inflame = v.score(&view.with_card_added(CardId::Inflame, true));
        assert!(
            with_inflame > base,
            "upgraded scaling card must raise V ({with_inflame} <= {base})"
        );
        let with_curse = v.score(&view.with_card_added(CardId::Decay, false));
        assert!(
            with_curse < base,
            "curse must lower V ({with_curse} >= {base})"
        );
    }

    /// Upgrading a deck card never lowers V.
    #[test]
    fn v_rewards_upgrades() {
        let v = HeuristicV::default();
        let view = fresh_view();
        let idx = view
            .deck
            .iter()
            .position(|c| !c.upgraded)
            .expect("starter deck has unupgraded cards");
        assert!(v.score(&view.with_deck_card_upgraded(idx)) > v.score(&view));
    }

    /// `feature_map` is a faithful linear decomposition of `score`:
    /// dot(weights, features) + fixed offset == score, on several views
    /// (M2 retune surface, bead `sts2-rs-40ikw.6`).
    #[test]
    fn feature_map_dot_product_reconstructs_score() {
        let v = HeuristicV::default();
        let base = fresh_view();
        let views = [
            base.clone(),
            base.with_card_added(CardId::Inflame, true),
            base.with_card_added(CardId::Decay, false),
            base.with_relic_added(sts2_core::RelicId::Vajra),
            base.with_gold_delta(140),
            base.with_hp(11),
        ];
        for view in &views {
            let f = v.feature_map(view);
            let dot: f64 = f
                .iter()
                .map(|(name, val)| v.weight_for_feature(name) * val)
                .sum();
            let score = v.score(view);
            assert!(
                (dot - score).abs() < 1e-9,
                "feature dot {dot} != score {score}"
            );
        }
    }

    /// Relics and gold are monotone too.
    #[test]
    fn v_monotone_in_relics_and_gold() {
        let v = HeuristicV::default();
        let view = fresh_view();
        let base = v.score(&view);
        assert!(v.score(&view.with_relic_added(sts2_core::RelicId::Anchor)) > base);
        assert!(v.score(&view.with_gold_delta(100)) > base);
    }
}
