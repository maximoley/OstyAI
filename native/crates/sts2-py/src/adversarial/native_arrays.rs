//! Native-layout card images and raw pile UID arrays; GPU builds public rows.
//! Only initialized, public scalar fields are copied. Padding/unused bytes are zero.
use super::{
    effective_target_type, json, numeric, Card, CombatState, PyAdversarialCombatBatch, Result,
    Schema, Session,
};
use pyo3::{exceptions::PyValueError, prelude::*};
use rayon::prelude::*;
use serde_json::Value;
use std::mem::{offset_of, size_of, size_of_val};
use sts2_core::cards::{EnchantmentId, VarKey};
use sts2_core::CardUid;

const META: usize = 16;
const SIDE: usize = 12;

fn layout(card: &Card) -> Value {
    let ptr = std::ptr::from_ref(card) as usize;
    let mut fields = serde_json::Map::new();
    macro_rules! field {
        ($($f:ident),*) => {$(fields.insert(stringify!($f).into(), json!([offset_of!(Card,$f),size_of_val(&card.$f)]));)*};
    }
    field!(
        uid,
        card_id,
        cost,
        card_type,
        target_type,
        upgraded,
        base_damage,
        base_block,
        base_replay_count
    );
    json!({"card_bytes":size_of::<Card>(), "fields":fields,
        "vars":[card.combat_vars.as_slice().as_ptr() as usize-ptr,size_of::<(VarKey,i32)>(),offset_of!((VarKey,i32),0),size_of::<VarKey>(),offset_of!((VarKey,i32),1)],
        "enchants":[card.enchantments.as_slice().as_ptr() as usize-ptr,size_of::<(EnchantmentId,i32)>(),offset_of!((EnchantmentId,i32),0),size_of::<EnchantmentId>(),offset_of!((EnchantmentId,i32),1)]})
}

/// Call only with initialized scalar fields without padding (integers, bools,
/// fieldless enums and transparent UID wrappers), never whole Rust structs.
unsafe fn scalar<T>(value: &T, base: usize, out: &mut [u8]) {
    let offset = std::ptr::from_ref(value) as usize - base;
    // SAFETY: caller guarantees all bytes of this scalar are initialized.
    let src = unsafe {
        std::slice::from_raw_parts(std::ptr::from_ref(value).cast::<u8>(), size_of::<T>())
    };
    out[offset..offset + src.len()].copy_from_slice(src);
}

fn copy_card(card: &Card, out: &mut [u8]) {
    out.fill(0);
    let base = std::ptr::from_ref(card) as usize;
    // SAFETY: all listed fields are padding-free scalar types. Do not add
    // Option, tuple, map or struct fields here. Maps are copied scalar by scalar.
    unsafe {
        macro_rules! fields { ($($f:ident),*) => {$(scalar(&card.$f,base,out);)*}; }
        fields!(
            uid,
            card_id,
            cost,
            card_type,
            target_type,
            upgraded,
            base_damage,
            base_block,
            base_replay_count
        );
        for (key, value) in card.combat_vars.as_slice() {
            scalar(key, base, out);
            scalar(value, base, out);
        }
        for (key, value) in card.enchantments.as_slice() {
            scalar(key, base, out);
            scalar(value, base, out);
        }
    }
}

fn piles(s: &CombatState) -> [&[CardUid]; 5] {
    [
        &s.player.hand,
        &s.player.draw,
        &s.player.discard,
        &s.player.exhaust,
        &s.player.play,
    ]
}

impl Session {
    fn prepare_native(&mut self, schema: &Schema) -> Result<()> {
        self.numeric_observation_mode(schema, true)?;
        let s = &self.state;
        self.native_sidecars.clear();
        for c in &s.cards {
            let flags = i32::from(sts2_core::card_effects::card_ethereal_inst(c))
                | (i32::from(sts2_core::card_effects::card_unplayable_inst(c)) << 1)
                | (i32::from(sts2_core::card_effects::card_innate_inst(c)) << 2)
                | (i32::from(sts2_core::card_effects::card_retain_inst(c)) << 3)
                | (i32::from(sts2_core::card_effects::card_exhausts_inst(c)) << 4)
                | (i32::from(s.player.retain_this_turn.contains(&c.uid)) << 5);
            self.native_sidecars.push([
                sts2_core::hooks::effective_card_cost(s, c.uid),
                sts2_core::effective_star_cost_in_combat(s, c),
                self.initial_costs
                    .get(&c.uid)
                    .copied()
                    .unwrap_or(c.cost)
                    .into(),
                flags,
                effective_target_type(s, c) as i32,
                c.keyword_override.map_or(-1, |v| v.0.into()),
                c.affliction.map_or(0, |a| a.kind as i32 + 1),
                c.affliction.map_or(0, |a| a.stacks),
                c.affliction.map_or(0, |a| i32::from(a.applied_exhaust)),
                c.combat_vars.len() as i32,
                c.enchantments.len() as i32,
                0,
            ]);
        }
        Ok(())
    }

    fn native_words(&self) -> usize {
        self.state.cards.len() * (size_of::<Card>() / 4 + SIDE)
            + piles(&self.state).iter().map(|v| v.len()).sum::<usize>()
            + self.numeric_cache.rows.len() * 16
    }

    fn write_native(&self, out: &mut [u8]) {
        let mut offset = 0;
        for card in &self.state.cards {
            copy_card(card, &mut out[offset..offset + size_of::<Card>()]);
            offset += size_of::<Card>();
        }
        // SAFETY: sidecars are initialized arrays of i32 with no padding.
        let side = unsafe {
            std::slice::from_raw_parts(
                self.native_sidecars.as_ptr().cast::<u8>(),
                self.native_sidecars.len() * SIDE * 4,
            )
        };
        out[offset..offset + side.len()].copy_from_slice(side);
        offset += side.len();
        assert_eq!(size_of::<CardUid>(), 4);
        for pile in piles(&self.state) {
            // SAFETY: CardUid transparently wraps initialized NonZeroU32.
            let data =
                unsafe { std::slice::from_raw_parts(pile.as_ptr().cast::<u8>(), pile.len() * 4) };
            out[offset..offset + data.len()].copy_from_slice(data);
            offset += data.len();
        }
        numeric::copy_rows(&self.numeric_cache.rows, &mut out[offset..]);
    }
}

impl PyAdversarialCombatBatch {
    pub(super) fn native_layout_impl(&self) -> PyResult<String> {
        let card = self
            .sessions
            .iter()
            .flat_map(|s| s.state.cards.iter())
            .next()
            .ok_or_else(|| PyValueError::new_err("native layout needs one card"))?;
        let mut value = layout(card);
        let mut vars = vec![0i32; 65536];
        for k in &[
            VarKey::RampageDamage,
            VarKey::ClawBuff,
            VarKey::GeneticAlgorithmBlock,
            VarKey::RitualDaggerDamage,
            VarKey::FreeThisTurn,
            VarKey::EtherealThisCombat,
            VarKey::StarCostFreeThisCombat,
            VarKey::MakeItSoInterval,
            VarKey::StompCostReductionThisTurn,
            VarKey::PinpointCostReductionThisTurn,
            VarKey::SneckoCostDeltaThisTurn,
            VarKey::ReturnIfPlayedRound,
            VarKey::DampenDowngradedMarker,
            VarKey::IsDupe,
            VarKey::GeneratedInCombat,
            VarKey::CostAbsolutelySet,
            VarKey::ThrashFoldRemNum,
            VarKey::ThrashFoldRemDen,
            VarKey::FreeUntilPlayed,
            VarKey::WitherFakeUpgradeLevel,
            VarKey::PendingBatchAutoPlay,
            VarKey::AutoPlayStarX,
            VarKey::WireTransientCostThisTurn,
            VarKey::WireCostExpiringOnPlay,
            VarKey::WireCostStable,
            VarKey::Generic,
            VarKey::EnergyDiscountUntilPlayed,
            VarKey::MasterDeckIndex,
            VarKey::RemoveMasterDeckInstance,
            VarKey::DowsingRoomsEntered,
            VarKey::SneckoPriorDiscountThisTurn,
            VarKey::SneckoPriorAbsoluteThisTurn,
        ] {
            let name = sts2_wire::state_dict::var_key_wire_name(*k);
            if self.schema.numeric.public_vars.contains(name) {
                vars[*k as usize] = self
                    .schema
                    .numeric
                    .id(name)
                    .map_err(PyValueError::new_err)?;
            }
        }
        value["public_vars"] = json!(vars);
        Ok(value.to_string())
    }

    pub(super) fn observe_native_into_impl(
        &mut self,
        py: Python<'_>,
        buffer: &Bound<'_, PyAny>,
        policy_seeds: Vec<u64>,
    ) -> PyResult<usize> {
        if policy_seeds.len() != self.sessions.len() {
            return Err(PyValueError::new_err("one seed per session required"));
        }
        let out = pyo3::buffer::PyBuffer::<i32>::get(buffer)?;
        if out.readonly() || !out.is_c_contiguous() {
            return Err(PyValueError::new_err(
                "writable contiguous int32 buffer required",
            ));
        }
        let start = std::time::Instant::now();
        let previous = &self.active;
        let schema = &self.schema;
        let indices = py
            .allow_threads(|| -> Result<Vec<usize>> {
                let build = |(i, s): (usize, &mut Session)| -> Result<Option<usize>> {
                    if s.done() && previous.binary_search(&i).is_err() {
                        return Ok(None);
                    }
                    s.prepare_native(schema)?;
                    Ok(Some(i))
                };
                let result = if let Some(pool) = &self.pool {
                    pool.install(|| {
                        self.sessions
                            .par_iter_mut()
                            .enumerate()
                            .map(build)
                            .collect::<Result<Vec<_>>>()
                    })?
                } else {
                    self.sessions
                        .iter_mut()
                        .enumerate()
                        .map(build)
                        .collect::<Result<Vec<_>>>()?
                };
                Ok(result.into_iter().flatten().collect())
            })
            .map_err(PyValueError::new_err)?;
        let mut offset = 8 + META * indices.len();
        let mut sections = Vec::with_capacity(indices.len());
        for &i in &indices {
            let words = self.sessions[i].native_words();
            sections.push((i, offset, words));
            offset += words;
        }
        if out.item_count() < offset {
            return Err(PyValueError::new_err(format!(
                "native buffer too small: {offset}"
            )));
        }
        self.active = indices
            .iter()
            .copied()
            .filter(|&i| !self.sessions[i].done())
            .collect();
        let build_us = start.elapsed().as_micros() as i32;
        let pack = std::time::Instant::now();
        let ptr = out.buf_ptr() as usize;
        // SAFETY: capacity/format checked and output ownership exclusive for this call.
        let words = unsafe { std::slice::from_raw_parts_mut(out.buf_ptr().cast::<i32>(), offset) };
        words[..8].copy_from_slice(&[
            0x4e415431,
            1,
            indices.len() as i32,
            offset as i32,
            size_of::<Card>() as i32,
            META as i32,
            build_us,
            0,
        ]);
        for (j, &(i, at, _)) in sections.iter().enumerate() {
            let s = &self.sessions[i];
            let count = s.commands.len();
            let random = sts2_core::run_rng_set::splitmix64(
                policy_seeds[i]
                    .wrapping_add((s.decisions as u64 + 1).wrapping_mul(0x9e3779b97f4a7c15)),
            );
            let ps = piles(&s.state);
            let meta = [
                i as i32,
                at as i32,
                s.state.cards.len() as i32,
                s.numeric_cache.rows.len() as i32,
                s.numeric_cache.action_start as i32,
                count as i32,
                if count == 0 {
                    -1
                } else {
                    (random % count as u64) as i32
                },
                i32::from(s.done()),
                s.decisions as i32,
                ps[0].len() as i32,
                ps[1].len() as i32,
                ps[2].len() as i32,
                ps[3].len() as i32,
                ps[4].len() as i32,
                (s.numeric_cache.rows.len()
                    + s.native_sidecars
                        .iter()
                        .map(|v| 1 + v[9] as usize + v[10] as usize + usize::from(v[6] != 0))
                        .sum::<usize>()) as i32,
                0,
            ];
            words[8 + j * META..8 + (j + 1) * META].copy_from_slice(&meta);
        }
        py.allow_threads(|| {
            let copy = |&(i, at, n): &(usize, usize, usize)| {
                // SAFETY: sections are disjoint, within the validated allocation;
                // buffer stays borrowed, workers join before returning to Python.
                let slice =
                    unsafe { std::slice::from_raw_parts_mut((ptr + at * 4) as *mut u8, n * 4) };
                self.sessions[i].write_native(slice);
            };
            if let Some(pool) = &self.pool {
                pool.install(|| sections.par_iter().for_each(copy));
            } else {
                sections.iter().for_each(copy);
            }
        });
        // SAFETY: all worker writes have joined; header is disjoint from sections.
        unsafe {
            *out.buf_ptr().cast::<i32>().add(7) = pack.elapsed().as_micros() as i32;
        }
        Ok(offset)
    }
}
