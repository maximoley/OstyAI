//! Execute factual forced moves without creating a tree or requesting GPU inference.
use super::*;

pub(super) fn advance(
    record: &mut Record,
    cfg: &Config,
    schema: &Schema,
    stats: &WorkerStats,
) -> Result<()> {
    if !cfg.skip_forced_actions || record.simulation_budget == 0 || record.search.is_some() {
        return Ok(());
    }
    record.session.refresh_commands();
    while !record.session.done() && record.session.commands.len() == 1 {
        if cfg.record_replay {
            let begun = cfg.profile.then(Instant::now);
            let mut raw = Vec::new();
            record
                .session
                .numeric_direct(schema, &mut raw, record.seed)?;
            if raw.len() > cfg.row_capacity {
                return Err("forced-action observation exceeds row capacity".into());
            }
            raw[0][8] = record.id as i32;
            stats.exports.fetch_add(1, Ordering::Relaxed);
            stats.rows.fetch_add(raw.len() as u64, Ordering::Relaxed);
            stats.max_rows.fetch_max(raw.len(), Ordering::Relaxed);
            if let Some(t) = begun {
                stats
                    .export_ns
                    .fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
            }
            record.trajectory.push(Sample {
                raw,
                policy: vec![1.0],
                value: 0.0, // Filled with the realized combat return by Queue::commit.
                combat: (cfg.episode << 32) | record.id as u64,
                version: cfg.actor_version,
            });
        }
        let begun = cfg.profile.then(Instant::now);
        record.session.step(0)?;
        record.session.refresh_commands();
        if cfg.record_actions {
            record.actions.push(0);
        }
        stats.steps.fetch_add(1, Ordering::Relaxed);
        stats.forced_actions.fetch_add(1, Ordering::Relaxed);
        record.forced_actions += 1;
        if let Some(t) = begun {
            stats
                .step_ns
                .fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Schema, Record) {
        sts2_content::cards::dispatch::install_default_registry();
        sts2_content::relics::dispatch::ensure_installed();
        sts2_content::potions::dispatch::ensure_installed();
        let f: Value = serde_json::from_str(include_str!("search_fixture.json")).unwrap();
        let schema = Schema::new(&[], json!({})).unwrap();
        let state = sts2_wire::state_from_json(&f["wire"]).unwrap();
        let mut session = Session::from_parts(&schema, state, f["context"].clone(), 500).unwrap();
        session.state.player.hand.clear();
        session.state.player.draw.clear();
        session.state.player.discard.clear();
        session.state.player.potions.clear();
        session.state.player.potion_slot_indices.clear();
        session.max_actions = session.decisions + 1;
        session.refresh_commands();
        let mut names = Vec::<String>::new();
        for c in &session.state.creatures {
            if let Some(ai) = &c.ai {
                names.push(ai.current_move().id.into());
                names.extend(ai.move_history().iter().map(|v| (*v).to_owned()));
            }
        }
        for power in &session.state.powers {
            names.push(power.kind.python_name().to_owned());
        }
        let schema = Schema::new(&[], json!({"names":names,"context":*session.context})).unwrap();

        // A factual sole EndTurn action, then the normal command generator resumes.
        session.commands = vec![super::super::super::Command::Core(
            sts2_core::Action::EndTurn,
        )];
        (
            schema,
            Record {
                id: 7,
                simulation_budget: 64,
                search_tier: "uniform",
                searched_decisions: 0,
                forced_actions: 0,
                simulations: 0,
                session,
                seed: 11,
                rows: 0,
                actions: vec![],
                search: None,
                retained_search: None,
                feedback: None,
                root_rows: vec![],
                trajectory: vec![],
            },
        )
    }
    #[test]
    fn forced_moves_preserve_state_rng_and_replay() {
        let (schema, mut record) = fixture();
        let mut expected = record.session.clone();
        let mut n = 0;
        while !expected.done() && expected.commands.len() == 1 {
            expected.step(0).unwrap();
            expected.refresh_commands();
            n += 1;
        }
        assert!(n > 0);
        let cfg = Config {
            simulations: 64,
            row_capacity: 4096,
            episode: 42,
            actor_version: 99,
            ..Config::default()
        };
        let stats = WorkerStats::default();
        advance(&mut record, &cfg, &schema, &stats).unwrap();
        assert_eq!(
            sts2_core::state_hash(&record.session.state),
            sts2_core::state_hash(&expected.state)
        );
        assert_eq!(
            record.session.state.run_rng.combat_counter_fingerprint(),
            expected.state.run_rng.combat_counter_fingerprint()
        );
        assert_eq!(
            record.session.state.rng.counter(),
            expected.state.rng.counter()
        );
        assert_eq!(record.session.decisions, expected.decisions);
        assert_eq!(record.trajectory.len(), n);
        assert_eq!(stats.forced_actions.load(Ordering::Relaxed), n as u64);
        assert_eq!(stats.simulations.load(Ordering::Relaxed), 0);
        assert_eq!(record.actions, vec![0; n]);
        let mut queue = Queue::default();
        queue.commit(std::mem::take(&mut record.trajectory), 1.25, 1024);
        for sample in &queue.samples {
            assert_eq!(sample.policy, vec![1.0]);
            assert_eq!(sample.value, 1.25);
            assert_eq!(sample.version, 99);
            assert_eq!(sample.combat, (42 << 32) | 7);
            assert_eq!(sample.raw[0][8], 7);
        }
    }
    #[test]
    fn forced_moves_respect_action_cap_and_disabled_path() {
        let (schema, mut record) = fixture();
        record.session.max_actions = record.session.decisions + 1;
        let cfg = Config {
            simulations: 64,
            record_replay: false,
            ..Config::default()
        };
        let stats = WorkerStats::default();
        let before = record.session.decisions;
        advance(&mut record, &cfg, &schema, &stats).unwrap();
        assert_eq!(record.session.decisions, before + 1);
        assert!(record.session.done());
        assert!(record.trajectory.is_empty());
        let (_, mut disabled) = fixture();
        let before = disabled.session.decisions;
        let cfg = Config {
            skip_forced_actions: false,
            ..cfg
        };
        advance(&mut disabled, &cfg, &schema, &stats).unwrap();
        assert_eq!(disabled.session.decisions, before);
    }
}
