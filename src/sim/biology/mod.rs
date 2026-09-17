//! What goes on inside a living body: [`Biology`] — the [`Stats`] it has and
//! the [`Process`]es that change them.
//!
//! Plain Rust, like the rest of `sim`. Everything a body does *by itself* is a
//! process here: getting hungry, getting thirsty, a bladder filling. What a
//! unit decides to do about it is the brain's; what that then does to the
//! body comes back in here as an [`Event`].
//!
//! # One writer
//!
//! **A stat is changed by a process and by nothing else.** The setters on
//! [`Stats`] are private to this module, so that is a guarantee the compiler
//! holds: a task that finishes a meal cannot take hunger away itself, it can
//! only say [`Event::Ingested`]. Which processes hear that, and what each does
//! with it, is decided in one place — so eating something that also fills the
//! bladder is a change to a process, not a hunt through every task that ever
//! consumes anything.
//!
//! # A process
//!
//! One struct, one file, two hooks:
//!
//! * [`Process::advance`] — `dt` seconds going by. Hunger rising.
//! * [`Process::handle`] — something happened to the body. Hunger falling
//!   because food was eaten; a bladder filling because water was drunk.
//!
//! A process may keep state of its own between the two (a [`Bladder`] keeps
//! what has been drunk but has not reached it yet), which is what a more
//! involved process — digestion — will need.
//!
//! # Switching a process off
//!
//! Every body carries its own [`Switches`], a bit per [`ProcessId`], all on
//! for a new body. **Off means the process does not happen in that body at
//! all:** its stats hold still in both directions, events reach it and do
//! nothing, and a routine that meets the need it drives stops wanting to
//! (`NeedRoutine` asks [`Biology::is_running`]). Otherwise a human with hunger
//! switched off at 80 would go back to the fridge forever for meals that
//! could never fill it.
//!
//! Set from code — a kind's constructor, a test — or from outside the world
//! with `Command::SetProcess`, on the same queue as a freeze.
//!
//! # Adding a process
//!
//! A [`ProcessId`] variant; a struct implementing [`Process`] in its own file;
//! a field on [`Biology`]; its slot in [`Biology::parts`] and
//! [`Biology::processes`], in id order. A test fails if the slots and the ids
//! stop lining up.

pub mod bladder;
pub mod hunger;
pub mod stats;
pub mod thirst;

pub use bladder::Bladder;
pub use hunger::Hunger;
pub use stats::Stats;
pub use thirst::Thirst;

use super::item::ItemKind;

/// Every process a body can have.
///
/// The discriminant is the bit in [`Switches`] and the order processes run
/// in, so a tick advances them the same way every time.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum ProcessId {
    Hunger = 0,
    Thirst = 1,
    Bladder = 2,
}

impl ProcessId {
    pub const COUNT: usize = 3;
    pub const ALL: [ProcessId; ProcessId::COUNT] = [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder];

    pub const fn name(self) -> &'static str {
        match self {
            ProcessId::Hunger => "hunger",
            ProcessId::Thirst => "thirst",
            ProcessId::Bladder => "bladder",
        }
    }

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

// One bit each in a `u8`.
const _: () = assert!(ProcessId::COUNT <= 8);

/// Which of a body's processes are running.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Switches(u8);

impl Switches {
    pub const ALL: Switches = Switches(((1u16 << ProcessId::COUNT) - 1) as u8);
    pub const NONE: Switches = Switches(0);

    pub const fn is_on(self, process: ProcessId) -> bool {
        self.0 & process.bit() != 0
    }

    /// The same switches with `process` turned on or off.
    pub const fn with(self, process: ProcessId, on: bool) -> Switches {
        if on {
            Switches(self.0 | process.bit())
        } else {
            Switches(self.0 & !process.bit())
        }
    }
}

impl Default for Switches {
    fn default() -> Switches {
        Switches::ALL
    }
}

/// Something that happened to a body, for its processes to hear.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// Something was eaten or drunk and is gone.
    Ingested(ItemKind),
    /// A toilet was used.
    Relieved,
}

/// One thing a body does by itself.
///
/// Called through `&mut dyn Process` over [`Biology`]'s own fields — nothing
/// boxed, nothing allocated, a few indirect calls per unit per tick.
pub trait Process {
    fn id(&self) -> ProcessId;

    /// `dt` seconds of this process going on.
    fn advance(&mut self, stats: &mut Stats, dt: f32);

    /// Something happened to the body. Most processes care about few events,
    /// so the default ignores them all.
    fn handle(&mut self, event: Event, stats: &mut Stats) {
        let _ = (event, stats);
    }

    /// State of its own worth showing a debugger. Allocates; never asked in a
    /// tick.
    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }
}

/// A living body: its stats, its processes, and which of them are running.
///
/// Inline in the kind that has one, and `Copy` — every process is plain data.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Biology {
    stats: Stats,
    switches: Switches,
    hunger: Hunger,
    thirst: Thirst,
    bladder: Bladder,
}

impl Biology {
    /// A body with these stats and every process running.
    pub fn new(stats: Stats) -> Biology {
        Biology {
            stats,
            switches: Switches::ALL,
            hunger: Hunger,
            thirst: Thirst,
            bladder: Bladder::default(),
        }
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    pub fn switches(&self) -> Switches {
        self.switches
    }

    pub fn is_running(&self, process: ProcessId) -> bool {
        self.switches.is_on(process)
    }

    /// Switch `process` on or off in this body. Its stats stay where they are
    /// either way; they only stop, or start, moving.
    pub fn set_running(&mut self, process: ProcessId, running: bool) {
        self.switches = self.switches.with(process, running);
    }

    /// Time passing: `dt` seconds of every running process, in [`ProcessId`]
    /// order.
    pub fn advance(&mut self, dt: f32) {
        let (stats, switches, processes) = self.parts();
        for process in processes {
            if switches.is_on(process.id()) {
                process.advance(stats, dt);
            }
        }
    }

    /// Tell every running process that `event` happened, in [`ProcessId`]
    /// order.
    pub fn handle(&mut self, event: Event) {
        let (stats, switches, processes) = self.parts();
        for process in processes {
            if switches.is_on(process.id()) {
                process.handle(event, stats);
            }
        }
    }

    /// The stats, the switches and every process, borrowed apart so a process
    /// can be handed the stats it changes. Slot `i` is `ProcessId` `i`.
    fn parts(&mut self) -> (&mut Stats, Switches, [&mut dyn Process; ProcessId::COUNT]) {
        let Biology {
            stats,
            switches,
            hunger,
            thirst,
            bladder,
        } = self;
        (stats, *switches, [hunger, thirst, bladder])
    }

    /// Every process, read-only. Slot `i` is `ProcessId` `i`.
    fn processes(&self) -> [&dyn Process; ProcessId::COUNT] {
        [&self.hunger, &self.thirst, &self.bladder]
    }

    /// The stats, which processes are running, and what each running process
    /// has to say about itself — for the debug menu. The set of fields does
    /// not change with the switches, only what `processes` says.
    pub fn debug_fields(&self) -> Vec<(&'static str, String)> {
        let mut fields: Vec<(&'static str, String)> = self
            .stats
            .fields()
            .into_iter()
            .map(|(name, value)| (name, format!("{value:.1}")))
            .collect();
        let running: Vec<&str> = ProcessId::ALL
            .iter()
            .filter(|&&process| self.is_running(process))
            .map(|process| process.name())
            .collect();
        fields.push((
            "processes",
            if running.is_empty() { "none".to_string() } else { running.join(", ") },
        ));
        for process in self.processes() {
            fields.extend(process.debug_fields());
        }
        fields
    }

    /// Change the stats directly, for a test that wants to start somebody
    /// hungry. Nothing outside a test gets to.
    #[cfg(test)]
    pub(crate) fn edit(&mut self, change: impl FnOnce(Stats) -> Stats) {
        self.stats = change(self.stats);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::item::MEAL;

    fn calm() -> Biology {
        Biology::new(Stats::calm())
    }

    #[test]
    fn every_process_sits_in_the_slot_of_its_own_id() {
        let mut biology = calm();
        for (i, process) in biology.processes().iter().enumerate() {
            assert_eq!(process.id() as usize, i);
            assert_eq!(process.id(), ProcessId::ALL[i]);
        }
        let (_, _, processes) = biology.parts();
        for (i, process) in processes.iter().enumerate() {
            assert_eq!(process.id() as usize, i);
        }
    }

    #[test]
    fn a_new_body_has_every_process_running() {
        let biology = calm();
        for process in ProcessId::ALL {
            assert!(biology.is_running(process), "{process:?}");
        }
        assert_eq!(biology.switches(), Switches::ALL);
    }

    #[test]
    fn switches_turn_one_process_off_and_on_and_leave_the_rest_alone() {
        let switches = Switches::ALL.with(ProcessId::Thirst, false);
        assert!(!switches.is_on(ProcessId::Thirst));
        assert!(switches.is_on(ProcessId::Hunger));
        assert!(switches.is_on(ProcessId::Bladder));
        assert_eq!(switches.with(ProcessId::Thirst, true), Switches::ALL);
        assert_eq!(
            ProcessId::ALL.iter().fold(Switches::ALL, |s, &p| s.with(p, false)),
            Switches::NONE
        );
    }

    #[test]
    fn time_moves_every_running_process() {
        let mut biology = calm();
        biology.advance(10.0);
        let stats = biology.stats();
        assert!(stats.hunger() > 50.0 && stats.thirst() > 50.0 && stats.bladder() > 50.0, "{stats:?}");
    }

    #[test]
    fn a_process_switched_off_holds_its_stat_still_both_ways() {
        let mut biology = calm();
        biology.set_running(ProcessId::Hunger, false);

        biology.advance(10.0);
        assert_eq!(biology.stats().hunger(), 50.0, "time passing");
        biology.handle(Event::Ingested(ItemKind::Food));
        assert_eq!(biology.stats().hunger(), 50.0, "a meal");
        assert!(biology.stats().thirst() > 50.0, "and thirst still ran");

        biology.set_running(ProcessId::Hunger, true);
        biology.handle(Event::Ingested(ItemKind::Food));
        assert_eq!(biology.stats().hunger(), 50.0 - MEAL.min(50.0), "running again");
    }

    #[test]
    fn the_debug_view_says_which_processes_are_running_and_keeps_its_shape() {
        let mut biology = calm();
        let names = |biology: &Biology| biology.debug_fields().iter().map(|(n, _)| *n).collect::<Vec<_>>();
        let before = names(&biology);
        let processes = |biology: &Biology| {
            biology
                .debug_fields()
                .into_iter()
                .find(|(name, _)| *name == "processes")
                .map(|(_, value)| value)
        };
        assert_eq!(processes(&biology).as_deref(), Some("hunger, thirst, bladder"));

        biology.set_running(ProcessId::Hunger, false);
        biology.set_running(ProcessId::Bladder, false);
        assert_eq!(processes(&biology).as_deref(), Some("thirst"));
        assert_eq!(names(&biology), before, "the unit panel lays these out once");
    }
}
