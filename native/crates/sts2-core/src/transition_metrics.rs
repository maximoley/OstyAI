//! Opt-in, thread-local accounting of action applications, independent of game state.
//!
//! Counts top-level `apply` invocations once (including rejected/no-op attempts),
//! plus MCTS's equivalent cached/split EndTurn resolutions. Existing tree nodes
//! visited without recomputing their state do not count as applications. Split
//! EndTurn phases are also counted separately, never added as whole actions.
//! Timers cover rules resolution only; cloning, RNG reseeding and policy work
//! outside these entry points remain outside the transition timer.

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::time::Instant;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Count,
    Time,
}

#[derive(Debug, Clone, Copy)]
pub enum Phase {
    Actual = 0,
    Tree = 1,
    Rollout = 2,
}

/// Arrays are ordered `[actual play, tree/search, rollout]`.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Metrics {
    pub apply_calls: [u64; 3],
    pub split_endturn_actions: [u64; 3],
    pub split_endturn_phase_calls: [u64; 3],
    pub transition_ns: [u64; 3],
}

/// Counters plus an exact histogram of executed rollout lengths.
#[derive(Debug, Serialize)]
pub struct Measurement {
    #[serde(flatten)]
    pub counters: Metrics,
    pub rollout_length_histogram: Vec<u64>,
}

impl Deref for Measurement {
    type Target = Metrics;
    fn deref(&self) -> &Metrics {
        &self.counters
    }
}

#[derive(Debug, Clone, Copy)]
struct State {
    mode: Mode,
    phase: Phase,
    inside_transition: bool,
    metrics: Metrics,
}

const EMPTY: State = State {
    mode: Mode::Off,
    phase: Phase::Actual,
    inside_transition: false,
    metrics: Metrics {
        apply_calls: [0; 3],
        split_endturn_actions: [0; 3],
        split_endturn_phase_calls: [0; 3],
        transition_ns: [0; 3],
    },
};

thread_local! {
    static STATE: Cell<State> = const { Cell::new(EMPTY) };
    // Separate storage keeps the per-action counter hot path fixed-size.
    static HISTOGRAM: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// A measurement session; dropping it also disables accounting on error paths.
#[derive(Debug)]
pub struct Recorder;

impl Recorder {
    pub fn start(mode: Mode) -> Self {
        STATE.with(|cell| {
            assert_eq!(cell.get().mode, Mode::Off, "nested transition recorder");
            cell.set(State { mode, ..EMPTY });
        });
        HISTOGRAM.with(|hist| {
            *hist.borrow_mut() = if mode == Mode::Off {
                Vec::new()
            } else {
                vec![0; 256]
            }
        });
        Self
    }

    pub fn finish(self) -> Measurement {
        Measurement {
            counters: STATE.with(|cell| cell.get().metrics),
            rollout_length_histogram: HISTOGRAM
                .with(|hist| std::mem::take(&mut *hist.borrow_mut())),
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        STATE.with(|cell| cell.set(EMPTY));
        HISTOGRAM.with(|hist| hist.borrow_mut().clear());
    }
}

#[derive(Debug)]
pub struct PhaseGuard(Phase, Option<u64>);

pub fn phase(phase: Phase) -> PhaseGuard {
    STATE.with(|cell| {
        let mut state = cell.get();
        let previous = state.phase;
        state.phase = phase;
        cell.set(state);
        let rollout_start = if state.mode != Mode::Off && matches!(phase, Phase::Rollout) {
            Some(state.metrics.apply_calls[Phase::Rollout as usize])
        } else {
            None
        };
        PhaseGuard(previous, rollout_start)
    })
}

impl Drop for PhaseGuard {
    fn drop(&mut self) {
        STATE.with(|cell| {
            let mut state = cell.get();
            if let Some(start) = self.1 {
                let length =
                    usize::try_from(state.metrics.apply_calls[Phase::Rollout as usize] - start)
                        .expect("rollout length fits usize");
                HISTOGRAM.with(|hist| {
                    let mut bins = hist.borrow_mut();
                    if bins.len() <= length {
                        bins.resize(length + 1, 0);
                    }
                    bins[length] += 1;
                });
            }
            state.phase = self.0;
            cell.set(state);
        });
    }
}

/// One whole EndTurn resolved by the search's split/cached path, bypassing apply.
pub fn split_endturn_action() {
    STATE.with(|cell| {
        let mut state = cell.get();
        if state.mode != Mode::Off {
            state.metrics.split_endturn_actions[state.phase as usize] += 1;
            cell.set(state);
        }
    });
}

#[derive(Debug)]
pub struct TransitionGuard {
    active: bool,
    phase: usize,
    started: Option<Instant>,
}

/// Called at the rules kernel's entry points. Nested aliases and phases inside
/// an ordinary apply are deliberately neither counted nor timed a second time.
#[inline]
pub(crate) fn begin(split_phase: bool) -> TransitionGuard {
    STATE.with(|cell| {
        let mut state = cell.get();
        let active = state.mode != Mode::Off && !state.inside_transition;
        if active {
            let counts = if split_phase {
                &mut state.metrics.split_endturn_phase_calls
            } else {
                &mut state.metrics.apply_calls
            };
            counts[state.phase as usize] += 1;
            state.inside_transition = true;
            cell.set(state);
        }
        TransitionGuard {
            active,
            phase: state.phase as usize,
            started: (active && state.mode == Mode::Time).then(Instant::now),
        }
    })
}

impl Drop for TransitionGuard {
    #[inline]
    fn drop(&mut self) {
        if self.active {
            let elapsed = self
                .started
                .map_or(0, |start| start.elapsed().as_nanos() as u64);
            STATE.with(|cell| {
                let mut state = cell.get();
                state.inside_transition = false;
                state.metrics.transition_ns[self.phase] += elapsed;
                cell.set(state);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{apply, Action, CombatState};

    #[test]
    fn phases_aliases_and_split_endturn_are_counted_without_double_counting() {
        let recorder = Recorder::start(Mode::Count);
        let mut state = CombatState::default();
        // UsePotion delegates to apply(DrinkPotion); it is one action attempt.
        let _ = apply(
            &mut state,
            Action::UsePotion {
                slot: 0,
                target: None,
            },
        );
        {
            let _tree = phase(Phase::Tree);
            let _ = apply(&mut state, Action::EndTurn);
            {
                let _rollout = phase(Phase::Rollout);
                let _ = apply(&mut state, Action::EndTurn);
            }
            split_endturn_action();
            // Simulate an external cached EndTurn phase, then a phase nested
            // within a full application. Only the external phase is separate.
            drop(begin(true));
            let application = begin(false);
            drop(begin(true));
            drop(application);
        }
        let metrics = recorder.finish();
        assert_eq!(metrics.apply_calls, [1, 2, 1]);
        assert_eq!(metrics.split_endturn_actions, [0, 1, 0]);
        assert_eq!(metrics.split_endturn_phase_calls, [0, 1, 0]);
        assert_eq!(metrics.transition_ns, [0; 3]);
        assert_eq!(metrics.rollout_length_histogram[1], 1);
        assert_eq!(STATE.with(|cell| cell.get().mode), Mode::Off);
    }

    #[test]
    fn timers_and_error_cleanup_do_not_leak_across_sessions_or_threads() {
        {
            let _aborted = Recorder::start(Mode::Count);
            drop(begin(false));
        }
        let recorder = Recorder::start(Mode::Time);
        drop(begin(false));
        let other = std::thread::spawn(|| {
            let recorder = Recorder::start(Mode::Count);
            drop(begin(false));
            drop(begin(false));
            recorder.finish()
        })
        .join()
        .unwrap();
        let metrics = recorder.finish();
        assert_eq!(metrics.apply_calls, [1, 0, 0]);
        assert_eq!(other.apply_calls, [2, 0, 0]);
        assert!(metrics.transition_ns[0] > 0);
        assert_eq!(other.transition_ns, [0; 3]);
    }
    #[test]
    fn rollout_histogram_counts_individual_lengths_not_combat_means() {
        let recorder = Recorder::start(Mode::Count);
        let mut state = CombatState::default();
        for length in [0, 1, 3, 300] {
            let _rollout = phase(Phase::Rollout);
            for _ in 0..length {
                let _ = apply(&mut state, Action::EndTurn);
            }
        }
        {
            let _tree = phase(Phase::Tree);
            let _ = apply(&mut state, Action::EndTurn);
        }
        let result = recorder.finish();
        let histogram = &result.rollout_length_histogram;
        assert_eq!(histogram.iter().sum::<u64>(), 4);
        for length in [0, 1, 3, 300] {
            assert_eq!(histogram[length], 1);
        }
        assert_eq!(
            histogram
                .iter()
                .enumerate()
                .map(|(i, n)| i as u64 * n)
                .sum::<u64>(),
            result.apply_calls[2]
        );
        let recorder = Recorder::start(Mode::Off);
        {
            let _rollout = phase(Phase::Rollout);
            let _ = apply(&mut state, Action::EndTurn);
        }
        assert!(recorder.finish().rollout_length_histogram.is_empty());
    }
}
