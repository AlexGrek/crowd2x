//! The simulation's log: a bounded, lock-free queue of strings.
//!
//! Any thread may write to it, and it is written from the think phase, which
//! is the one part of a tick that is meant to run in parallel. So every method
//! that adds a line takes `&self` — a `&mut` here would be exactly the shared
//! mutable state the tick is shaped to avoid, and a `Mutex` around it would
//! serialise every thinking agent on the one thing agents do most.
//!
//! Two decisions worth keeping:
//!
//! * **Bounded, not unbounded.** The reader is a Bevy system that draws the
//!   last few lines on screen. If it stops draining — the game screen is left,
//!   the panel is hidden, a test never looks — an unbounded queue grows for as
//!   long as the process runs. [`ArrayQueue::force_push`] instead evicts the
//!   oldest line, which is the right thing to lose: a log is read from the end.
//! * **Drops are counted, not silent.** A gap in a log that does not say it is
//!   a gap is worse than no log, because it reads as "nothing happened".

use std::sync::atomic::{AtomicU64, Ordering};

use crossbeam_queue::ArrayQueue;

/// How many lines are held before the oldest starts falling off.
///
/// The panel shows a handful and a QA assertion looks at a few hundred at
/// most; this is sized so a burst between two frames survives, not so the
/// whole history does.
pub const CAPACITY: usize = 1024;

pub struct Log {
    lines: ArrayQueue<String>,
    dropped: AtomicU64,
}

impl Log {
    pub fn new() -> Log {
        Log::with_capacity(CAPACITY)
    }

    /// `capacity` is clamped to at least one: `ArrayQueue::new(0)` panics, and
    /// a log that cannot hold a line is a configuration mistake rather than
    /// something to crash the simulation over.
    pub fn with_capacity(capacity: usize) -> Log {
        Log {
            lines: ArrayQueue::new(capacity.max(1)),
            dropped: AtomicU64::new(0),
        }
    }

    /// Add a line, evicting the oldest if the queue is full.
    ///
    /// `&self`, so a thinking agent on any thread can call it.
    pub fn push(&self, line: impl Into<String>) {
        if self.lines.force_push(line.into()).is_some() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Take everything currently queued, oldest first.
    ///
    /// Not an iterator over the queue: it pops, so a line is delivered once.
    /// Bounded by the length at entry, so a writer racing the reader cannot
    /// keep this spinning.
    pub fn drain(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(self.lines.len());
        while let Some(line) = self.lines.pop() {
            out.push(line);
        }
        out
    }

    /// How many lines are waiting to be read.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// How many lines were pushed out by newer ones before anybody read them.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Default for Log {
    fn default() -> Log {
        Log::new()
    }
}

impl std::fmt::Debug for Log {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Log")
            .field("queued", &self.len())
            .field("dropped", &self.dropped())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_come_back_in_the_order_they_went_in() {
        let log = Log::new();
        log.push("first");
        log.push("second");
        assert_eq!(log.drain(), ["first", "second"]);
        // Draining takes them: a line is delivered once.
        assert!(log.drain().is_empty());
    }

    #[test]
    fn a_full_log_loses_the_oldest_line_and_says_so() {
        let log = Log::with_capacity(2);
        log.push("a");
        log.push("b");
        log.push("c");

        assert_eq!(log.dropped(), 1);
        // The end of the log is what a reader wants, so that is what survives.
        assert_eq!(log.drain(), ["b", "c"]);
    }

    #[test]
    fn nothing_is_dropped_while_there_is_room() {
        let log = Log::with_capacity(8);
        for i in 0..8 {
            log.push(format!("line {i}"));
        }
        assert_eq!(log.dropped(), 0);
        assert_eq!(log.len(), 8);
    }

    #[test]
    fn a_zero_capacity_log_still_accepts_a_line() {
        // `ArrayQueue::new(0)` panics; the simulation must not.
        let log = Log::with_capacity(0);
        log.push("kept");
        assert_eq!(log.drain(), ["kept"]);
    }

    /// The whole reason `push` takes `&self`: the think phase writes from
    /// several threads at once and must not need a lock to do it.
    #[test]
    fn many_threads_can_write_at_once() {
        let log = Log::with_capacity(1024);
        std::thread::scope(|scope| {
            for thread in 0..8 {
                let log = &log;
                scope.spawn(move || {
                    for i in 0..100 {
                        log.push(format!("{thread}:{i}"));
                    }
                });
            }
        });
        assert_eq!(log.len() as u64 + log.dropped(), 800);
    }
}
