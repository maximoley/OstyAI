//! Opt-in, thread-local trace of every HP-loss event and who caused it.
//!
//! # Why this exists
//!
//! A mirror desync says only *what* diverged — "this enemy has 6 more HP in
//! the world than in the mirror". It never says *which effect* spent the
//! difference, and the reconstruction is re-imposed from reality at the next
//! decision, so the evidence is gone one step later. GH #368.1 (a `+6`
//! enemy-HP class on the Defect arm that survived the Dark-orb evoke fix)
//! is exactly that shape: the *magnitude* was reproducible across 23
//! independent fights and the *source* was unobservable.
//!
//! This module makes the source observable. [`apply_damage_with_final_impl`]
//! and the player-side [`crate::combat_loop::apply_damage_to_player`] are the
//! two chokepoints every HP loss funnels through; when tracing is armed they
//! append a [`DamageEvent`] carrying the numbers *and* the innermost
//! [`SourceGuard`] label pushed by whatever dispatcher is mid-flight.
//!
//! # Cost when disabled
//!
//! One thread-local `bool` read per damage event and per source push. The
//! `Vec`s are only touched inside the `armed()` branch — a disarmed
//! [`push_source`] returns before it even builds its label — so a non-tracing
//! run allocates nothing. Tracing is **off** unless a caller arms it.
//!
//! # Threading
//!
//! Arming AND the buffer are both thread-local. A rollout that fans out
//! across rayon workers therefore records nothing: the workers were never
//! armed, so they neither allocate nor grow a buffer nobody will ever drain.
//! That is the point — a process-global arm would have every search thread
//! accumulating rows for the lifetime of the run.
//!
//! The consequence to know: if the caller arms on thread A and the step it
//! wanted to trace runs on thread B, the trace comes back **empty**. Empty is
//! the honest answer there, not a silent partial one.

use std::cell::{Cell, RefCell};

/// Hard cap on buffered rows per thread. A single mirror step produces at
/// most a few dozen, so this only ever bites a caller that armed and never
/// drained — and even then the overflow is COUNTED ([`dropped`]) rather than
/// silently discarded, because a truncated trace that looks complete is worse
/// than no trace.
const MAX_EVENTS: usize = 4096;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static DROPPED: Cell<u32> = const { Cell::new(0) };
    static EVENTS: RefCell<Vec<DamageEvent>> = const { RefCell::new(Vec::new()) };
    static SOURCES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// One HP-loss event, as seen at the damage chokepoint.
///
/// `hp_before`/`hp_after` are the target's own HP either side of the write,
/// so a row is self-checking: `hp_before - hp_after` must equal `hp_loss`
/// except on an overkill (where `hp_after` clamps at 0).
#[derive(Debug, Clone)]
pub struct DamageEvent {
    /// Monotonic index within the current trace window, so a consumer can
    /// order rows that share a target and amount.
    pub seq: u32,
    /// Innermost armed [`SourceGuard`] label — `"card:Zap"`, `"orb:Dark:evoke"`,
    /// `"intent:Rocket:Attack"`. Empty when the damage came from outside all
    /// three instrumented dispatchers, which narrows rather than fails: the
    /// remaining families are relic listeners and power hooks, and the
    /// record's own relic/power lists are the candidate set (that is how
    /// Metronome's 30-to-all-enemies was identified). Adding frames to the
    /// ~30 `relic_effects::channels` dispatch loops is the obvious next step
    /// if unlabelled rows start dominating a census.
    pub source: String,
    /// The full source stack, outermost first. A card that channels an orb
    /// that evokes shows all three frames, which is what separates "Zap dealt
    /// 6" from "a Dark orb evoked for 6 during Zap".
    pub source_stack: Vec<String>,
    /// `"player"`, `"enemy:<uid>"`, or `"none"` — the dealer as the damage
    /// pipeline understood it.
    pub dealer: String,
    /// Arena uid of the creature that lost HP.
    pub target_uid: u32,
    /// The target's content id (monster id, or `"player"`), for readability.
    pub target_id: String,
    /// Post-`modify_damage` amount that entered block absorption.
    pub final_damage: i32,
    /// How much of `final_damage` the target's Block soaked.
    pub blocked: i32,
    /// Realized HP loss, clamped to the target's remaining HP.
    pub hp_loss: i32,
    pub hp_before: i32,
    pub hp_after: i32,
    /// Combat round the event landed in, so a consumer can align the trace
    /// with the wire's `turn`.
    pub turn: i32,
}

/// RAII frame naming whatever dispatcher is about to deal damage.
///
/// Push one at every site that *knows* what it is (a card play, an orb evoke,
/// a monster intent). Nesting is expected and preserved: the innermost frame
/// becomes [`DamageEvent::source`] and the whole stack is recorded, because
/// "Dark orb evoke, during Zap" is a different finding from either half.
#[derive(Debug)]
#[must_use = "the frame pops when the guard drops; binding it to `_` pops immediately"]
pub struct SourceGuard {
    /// `false` when tracing was disarmed at push time — the drop is then a
    /// no-op, so an arm that races a push cannot pop a frame it never made.
    pushed: bool,
}

impl Drop for SourceGuard {
    fn drop(&mut self) {
        if self.pushed {
            SOURCES.with(|s| {
                s.borrow_mut().pop();
            });
        }
    }
}

/// Name the dispatcher that is about to run. Cheap no-op when disarmed.
///
/// The label is built by the caller, so pass a `&str` when it is static and
/// only format when the id matters — the `format!` runs even when disarmed
/// unless the caller guards it, which is why hot sites should check
/// [`armed`] first.
pub fn push_source(label: impl Into<String>) -> SourceGuard {
    if !armed() {
        return SourceGuard { pushed: false };
    }
    SOURCES.with(|s| s.borrow_mut().push(label.into()));
    SourceGuard { pushed: true }
}

/// Is tracing armed **on this thread**?
#[inline]
pub fn armed() -> bool {
    ARMED.with(Cell::get)
}

/// Arm or disarm tracing on this thread. Arming also clears any buffered
/// events, so a consumer always starts from a known-empty window.
pub fn set_armed(on: bool) {
    ARMED.with(|a| a.set(on));
    if on {
        clear();
    }
}

/// Drop every buffered event, source frame, and overflow count on this thread.
pub fn clear() {
    EVENTS.with(|e| e.borrow_mut().clear());
    SOURCES.with(|s| s.borrow_mut().clear());
    DROPPED.with(|d| d.set(0));
}

/// Take and clear this thread's buffered events.
pub fn take() -> Vec<DamageEvent> {
    EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}

/// Events discarded since the last [`clear`] because the per-thread buffer
/// was full. Non-zero means the rows you have are a PREFIX, not the whole
/// window — read it before concluding anything from a trace.
pub fn dropped() -> u32 {
    DROPPED.with(Cell::get)
}

/// Record one event. Callers must have checked [`armed`] — this re-checks
/// anyway so a missed guard degrades to a dropped row rather than a leak.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record(
    dealer: String,
    target_uid: u32,
    target_id: String,
    final_damage: i32,
    blocked: i32,
    hp_loss: i32,
    hp_before: i32,
    hp_after: i32,
    turn: i32,
) {
    if !armed() {
        return;
    }
    let stack = SOURCES.with(|s| s.borrow().clone());
    let source = stack.last().cloned().unwrap_or_default();
    EVENTS.with(|e| {
        let mut buf = e.borrow_mut();
        if buf.len() >= MAX_EVENTS {
            DROPPED.with(|d| d.set(d.get().saturating_add(1)));
            return;
        }
        let seq = buf.len() as u32;
        buf.push(DamageEvent {
            seq,
            source,
            source_stack: stack,
            dealer,
            target_uid,
            target_id,
            final_damage,
            blocked,
            hp_loss,
            hp_before,
            hp_after,
            turn,
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disarmed_records_nothing_and_guards_are_inert() {
        set_armed(false);
        let _g = push_source("card:Zap");
        record("player".into(), 1, "e".into(), 6, 0, 6, 20, 14, 1);
        assert!(take().is_empty());
    }

    #[test]
    fn innermost_frame_wins_and_the_stack_is_kept() {
        set_armed(true);
        {
            let _outer = push_source("card:Zap");
            let _inner = push_source("orb:Dark:evoke");
            record("player".into(), 7, "Rocket".into(), 6, 0, 6, 20, 14, 3);
        }
        let ev = take();
        set_armed(false);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].source, "orb:Dark:evoke");
        assert_eq!(ev[0].source_stack, vec!["card:Zap", "orb:Dark:evoke"]);
        assert_eq!(ev[0].seq, 0);
    }

    #[test]
    fn frames_pop_so_a_later_sibling_is_not_nested_under_the_first() {
        set_armed(true);
        {
            let _a = push_source("card:Strike");
        }
        {
            let _b = push_source("card:Defend");
            record("player".into(), 1, "e".into(), 1, 0, 1, 5, 4, 1);
        }
        let ev = take();
        set_armed(false);
        assert_eq!(ev[0].source_stack, vec!["card:Defend"]);
    }

    #[test]
    fn an_unarmed_worker_thread_records_nothing_even_while_the_owner_traces() {
        set_armed(true);
        let handle = std::thread::spawn(|| {
            let _g = push_source("card:Zap");
            record("player".into(), 1, "e".into(), 6, 0, 6, 20, 14, 1);
            (take().len(), armed())
        });
        let (worker_rows, worker_armed) = handle.join().expect("worker panicked");
        record("player".into(), 2, "e2".into(), 3, 0, 3, 9, 6, 1);
        let mine = take();
        set_armed(false);
        assert!(!worker_armed, "arming must not leak across threads");
        assert_eq!(worker_rows, 0, "an unarmed thread must not buffer");
        assert_eq!(mine.len(), 1, "the arming thread still records its own");
    }

    #[test]
    fn overflow_is_counted_not_silently_truncated() {
        set_armed(true);
        for _ in 0..(MAX_EVENTS + 5) {
            record("player".into(), 1, "e".into(), 1, 0, 1, 99, 98, 1);
        }
        let rows = take();
        let lost = dropped();
        set_armed(false);
        assert_eq!(rows.len(), MAX_EVENTS);
        assert_eq!(
            lost, 5,
            "the caller must be able to see the trace is a prefix"
        );
    }

    #[test]
    fn arming_clears_a_stale_window() {
        set_armed(true);
        record("player".into(), 1, "e".into(), 1, 0, 1, 5, 4, 1);
        set_armed(true);
        let ev = take();
        set_armed(false);
        assert!(ev.is_empty(), "re-arming must not hand back the old window");
    }
}
