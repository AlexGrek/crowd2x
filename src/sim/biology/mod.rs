//! What goes on inside a living body: [`Biology`] — the [`Stats`] it has and
//! the [`Process`]es that change them.
//!
//! Plain Rust, like the rest of `sim`. Everything a body does *by itself* is a
//! process here: getting hungry, getting thirsty, a bladder filling, getting
//! tired. What a unit decides to do about it is the brain's; what that then
//! does to the body comes back in here as an [`Event`].
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
//! * [`Process::advance`] — `dt` **world** seconds going by. Hunger rising.
//! * [`Process::handle`] — something happened to the body. Hunger falling
//!   because food was eaten; a bladder filling because water was drunk.
//!
//! Every rate here is per world second, and a world second is a hundred and
//! twentieth of a watched one ([`crate::sim::clock`]): a body ages on the
//! world's clock, because getting hungry takes hours and hours are what that
//! clock counts. `Think::game_dt` is what a caller hands
//! [`Biology::advance`]; handing it `dt` would make a day's worth of hunger
//! take a day.
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
pub mod energy;
pub mod fun;
pub mod health;
pub mod hunger;
pub mod recollection;
pub mod satisfaction;
pub mod stats;
pub mod thirst;

pub use bladder::Bladder;
pub use energy::Energy;
pub use fun::Fun;
pub use health::Health;
pub use hunger::Hunger;
pub use recollection::{Experience, Recollection};
pub use satisfaction::Satisfaction;
pub use stats::Stats;
pub use thirst::Thirst;

use crate::save::{invalid, pb, Invalid};

use super::background::Due;
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
    Fun = 3,
    Energy = 4,
    Satisfaction = 5,
    Health = 6,
}

impl ProcessId {
    pub const COUNT: usize = 7;
    pub const ALL: [ProcessId; ProcessId::COUNT] = [
        ProcessId::Hunger,
        ProcessId::Thirst,
        ProcessId::Bladder,
        ProcessId::Fun,
        ProcessId::Energy,
        ProcessId::Satisfaction,
        ProcessId::Health,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            ProcessId::Hunger => "hunger",
            ProcessId::Thirst => "thirst",
            ProcessId::Bladder => "bladder",
            ProcessId::Fun => "fun",
            ProcessId::Energy => "energy",
            ProcessId::Satisfaction => "satisfaction",
            ProcessId::Health => "health",
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
///
/// `PartialEq` only: [`Event::Slept`] and [`Event::Worked`] carry a duration,
/// and an `f32` is not `Eq`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Event {
    /// Something was eaten or drunk and is gone.
    Ingested(ItemKind),
    Treated { hp: f32 },
    /// A toilet was used.
    Relieved,
    /// Something entertaining was done — a go on a computer.
    Entertained,
    /// Time was spent asleep. **How long**, in world seconds — not how much
    /// rest that is, which is for [`Energy`] to say.
    Slept { world_seconds: f32 },
    /// Somebody came into view who was not in mind — met for the first time,
    /// or again after being forgotten. Said by the brain's attention
    /// (`brain::attention`), not by a task: nothing was *done*, a face was
    /// noticed.
    Met,
    /// Time was spent working — a stretch at a desk. **How long**, in world
    /// seconds; what that costs the body is [`Energy`]'s to say, and what it
    /// does *not* give — any fun — is [`Fun`]'s, by having no arm for it.
    Worked { world_seconds: f32 },
    /// A deathdog's teeth: `damage` health gone at once
    /// ([`Health`](health::Health)'s to take, so a body with it switched off
    /// shrugs a bite off). Felt by the one bitten in its own react
    /// (`sim::wounds`).
    Bitten { damage: u8 },
}

/// One thing a body does by itself.
///
/// Called through `&mut dyn Process` over [`Biology`]'s own fields — nothing
/// boxed, nothing allocated, a few indirect calls per unit per tick.
pub trait Process {
    fn id(&self) -> ProcessId;

    /// `dt` **world** seconds of this process going on — the scale every rate
    /// in this module is written at.
    fn advance(&mut self, stats: &mut Stats, dt: f32);

    /// How much of its pace it keeps while the body is **asleep**: 1 for a
    /// process that does not care (the default), 0 for one that stops.
    ///
    /// The one thing a process is told about what its body is doing, because
    /// it is the one thing that changes how fast a body ages: a sleeper burns
    /// less, makes less water, and does not get bored. A unit that got
    /// hungry, thirsty and bored at a waking pace all night was out of bed
    /// every hour for one of them, and never rested.
    fn asleep_pace(&self) -> f32 {
        1.0
    }

    /// Something happened to the body. Most processes care about few events,
    /// so the default ignores them all.
    ///
    /// `novelty` is how fresh it felt, from 1 (never done, or long forgotten)
    /// down towards a quarter (done again and again today) — what
    /// [`Recollection::novelty`] said before it was remembered. A process about
    /// the mind scales what it does by it; one about the body ignores it,
    /// since a meal is as filling the third time as the first.
    fn handle(&mut self, event: Event, novelty: f32, stats: &mut Stats) {
        let _ = (event, novelty, stats);
    }

    /// State of its own worth showing a debugger. Allocates; never asked in a
    /// tick.
    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }
}

/// A living body: its stats, its processes, which of them are running, and
/// what it has done lately.
///
/// Inline in the kind that has one, and `Copy` — every process is plain data.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Biology {
    stats: Stats,
    switches: Switches,
    /// What it has done lately, fading — asked before an event reaches the
    /// processes, so a treat had twice today is less of one. Always on: it is
    /// not a process and changes no stat. Faded by [`Biology::background`],
    /// not by `advance`.
    recent: Recollection,
    hunger: Hunger,
    thirst: Thirst,
    bladder: Bladder,
    fun: Fun,
    energy: Energy,
    satisfaction: Satisfaction,
    health: Health,
}

impl Biology {
    /// A body with these stats and every process running.
    pub fn new(stats: Stats) -> Biology {
        Biology {
            stats,
            switches: Switches::ALL,
            recent: Recollection::default(),
            hunger: Hunger,
            thirst: Thirst,
            bladder: Bladder::default(),
            fun: Fun,
            energy: Energy,
            satisfaction: Satisfaction,
            health: Health,
        }
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    pub fn switches(&self) -> Switches {
        self.switches
    }

    /// What it has done lately.
    pub fn recollection(&self) -> &Recollection {
        &self.recent
    }

    pub fn is_running(&self, process: ProcessId) -> bool {
        self.switches.is_on(process)
    }

    /// Switch `process` on or off in this body. Its stats stay where they are
    /// either way; they only stop, or start, moving.
    pub fn set_running(&mut self, process: ProcessId, running: bool) {
        self.switches = self.switches.with(process, running);
    }

    /// Involuntary relief, settled with the world's surface changes. A
    /// stopped bladder holds its value even at critical fullness.
    pub(crate) fn relieve_if_critical(&mut self) -> bool {
        if !self.is_running(ProcessId::Bladder) || self.stats.bladder() < bladder::CRITICAL {
            return false;
        }
        self.handle(Event::Relieved);
        true
    }

    /// Time passing: `dt` **world** seconds of every running process, in
    /// [`ProcessId`] order. That is
    /// [`Think::game_dt`](crate::sim::entity::Think::game_dt), not the tick's
    /// own `dt`.
    pub fn advance(&mut self, dt: f32) {
        self.advance_while(dt, false, false);
    }

    /// Time passing for a body that is **asleep**: every running process at
    /// its [`Process::asleep_pace`] — hunger, thirst and the bladder slower,
    /// boredom not at all.
    pub fn advance_asleep(&mut self, dt: f32) {
        self.advance_while(dt, true, false);
    }

    pub fn advance_hospital(&mut self, dt: f32, asleep: bool) {
        self.advance_while(dt, asleep, true);
    }

    fn advance_while(&mut self, dt: f32, asleep: bool, hospital: bool) {
        let (stats, switches, processes) = self.parts();
        for process in processes {
            if switches.is_on(process.id()) {
                let pace = if asleep { process.asleep_pace() } else { 1.0 };
                if hospital && process.id() == ProcessId::Health {
                    Health::advance_hospital(stats, dt);
                } else {
                    process.advance(stats, dt * pace);
                }
            }
        }
    }

    /// The body's background tasks that the unit's
    /// [`Schedule`](crate::sim::background::Schedule) says are `due` this
    /// tick, each handed the world time since it last ran. So far that is
    /// recent memory fading ([`Recollection`]); see [`crate::sim::background`].
    pub fn background(&mut self, due: &Due) {
        due.run(&mut self.recent);
    }

    /// Tell every running process that `event` happened, in [`ProcessId`]
    /// order, and how fresh it felt; then remember it.
    ///
    /// Returns what it was and how fresh, **when it was still in recent
    /// memory** and so worth less than it would have been — for the task that
    /// caused it to say so. `None` for an event that is no
    /// [`Experience`], or one long enough ago to be forgotten.
    pub fn handle(&mut self, event: Event) -> Option<(Experience, f32)> {
        let experience = Experience::of(event);
        let novelty = experience.map_or(recollection::FRESH, |e| self.recent.novelty(e));
        let (stats, switches, processes) = self.parts();
        for process in processes {
            if switches.is_on(process.id()) {
                process.handle(event, novelty, stats);
            }
        }
        let experience = experience?;
        let recalled = self.recent.recalls(experience);
        self.recent.remember(experience);
        recalled.then_some((experience, novelty))
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
            fun,
            energy,
            satisfaction,
            health,
            recent: _,
        } = self;
        (stats, *switches, [hunger, thirst, bladder, fun, energy, satisfaction, health])
    }

    /// Every process, read-only. Slot `i` is `ProcessId` `i`.
    fn processes(&self) -> [&dyn Process; ProcessId::COUNT] {
        [&self.hunger, &self.thirst, &self.bladder, &self.fun, &self.energy, &self.satisfaction, &self.health]
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
        fields.push(self.recent.debug_field());
        fields
    }

    /// Change the stats directly, for a test that wants to start somebody
    /// hungry. Nothing outside a test gets to.
    #[cfg(test)]
    pub(crate) fn edit(&mut self, change: impl FnOnce(Stats) -> Stats) {
        self.stats = change(self.stats);
    }

    // --- saving ---

    pub(crate) fn save(&self) -> pb::Biology {
        // The processes with no state of their own are named so that one
        // which grows some breaks this until it is saved.
        let Biology {
            stats,
            switches,
            recent,
            hunger: Hunger,
            thirst: Thirst,
            bladder,
            fun: Fun,
            energy: Energy,
            satisfaction: Satisfaction,
            health: Health,
        } = self;
        pb::Biology {
            stats: Some(stats.save()),
            off: ProcessId::ALL
                .iter()
                .filter(|&&process| !switches.is_on(process))
                .map(|&process| pb::ProcessId::from(process) as i32)
                .collect(),
            familiarity: recent.save(),
            bladder_on_its_way: bladder.on_its_way(),
        }
    }

    pub(crate) fn restore(saved: &pb::Biology) -> Result<Biology, Invalid> {
        let mut biology = Biology::new(Stats::restore(pb::required(&saved.stats, "stats")?)?);
        for &off in &saved.off {
            biology.set_running(ProcessId::from(pb::known::<pb::ProcessId>(off, "process")?), false);
        }
        biology.recent = Recollection::restore(&saved.familiarity)?;
        if !saved.bladder_on_its_way.is_finite() {
            return invalid(format!("bladder: {} on its way", saved.bladder_on_its_way));
        }
        biology.bladder = Bladder::restore(saved.bladder_on_its_way);
        Ok(biology)
    }
}

impl From<ProcessId> for pb::ProcessId {
    fn from(process: ProcessId) -> pb::ProcessId {
        match process {
            ProcessId::Hunger => pb::ProcessId::Hunger,
            ProcessId::Thirst => pb::ProcessId::Thirst,
            ProcessId::Bladder => pb::ProcessId::Bladder,
            ProcessId::Fun => pb::ProcessId::Fun,
            ProcessId::Energy => pb::ProcessId::Energy,
            ProcessId::Satisfaction => pb::ProcessId::Satisfaction,
            ProcessId::Health => pb::ProcessId::Health,
        }
    }
}

impl From<pb::ProcessId> for ProcessId {
    fn from(process: pb::ProcessId) -> ProcessId {
        match process {
            pb::ProcessId::Hunger => ProcessId::Hunger,
            pb::ProcessId::Thirst => ProcessId::Thirst,
            pb::ProcessId::Bladder => ProcessId::Bladder,
            pb::ProcessId::Fun => ProcessId::Fun,
            pb::ProcessId::Energy => ProcessId::Energy,
            pb::ProcessId::Satisfaction => ProcessId::Satisfaction,
            pb::ProcessId::Health => ProcessId::Health,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::clock::HOUR;
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
        assert!(switches.is_on(ProcessId::Fun));
        assert!(switches.is_on(ProcessId::Energy));
        assert!(switches.is_on(ProcessId::Satisfaction));
        assert!(switches.is_on(ProcessId::Health));
        assert_eq!(switches.with(ProcessId::Thirst, true), Switches::ALL);
        assert_eq!(
            ProcessId::ALL.iter().fold(Switches::ALL, |s, &p| s.with(p, false)),
            Switches::NONE
        );
    }

    #[test]
    fn time_moves_every_running_process() {
        let mut biology = calm();
        biology.advance(HOUR);
        let stats = biology.stats();
        assert!(stats.hunger() > 50.0 && stats.thirst() > 50.0 && stats.bladder() > 50.0, "{stats:?}");
        assert!(stats.fun() < 50.0, "and the one that drains instead: {stats:?}");
        assert!(stats.stamina() < 50.0, "and so does the one that tires: {stats:?}");
        assert!(stats.satisfaction() < 50.0, "and contentment wears off: {stats:?}");
    }

    #[test]
    fn asleep_a_body_gets_hungry_thirsty_and_full_slower_and_not_bored_at_all() {
        let (mut awake, mut asleep) = (calm(), calm());
        awake.advance(2.0 * HOUR);
        asleep.advance_asleep(2.0 * HOUR);
        let (a, s) = (awake.stats(), asleep.stats());
        for (name, awake, asleep) in [
            ("hunger", a.hunger(), s.hunger()),
            ("thirst", a.thirst(), s.thirst()),
            ("bladder", a.bladder(), s.bladder()),
        ] {
            assert!(asleep > 50.0 && asleep < awake, "{name}: {asleep} asleep, {awake} awake");
        }
        assert_eq!(s.fun(), 50.0, "nobody gets bored in their sleep");
        assert!(a.fun() < 50.0);
        assert_eq!(s.stamina(), a.stamina(), "the drain is the same; sleep is paid back by the event");
        assert_eq!(s.satisfaction(), a.satisfaction());
    }

    #[test]
    fn a_process_switched_off_holds_its_stat_still_both_ways() {
        let mut biology = calm();
        biology.set_running(ProcessId::Hunger, false);

        biology.advance(HOUR);
        assert_eq!(biology.stats().hunger(), 50.0, "time passing");
        biology.handle(Event::Ingested(ItemKind::Food));
        assert_eq!(biology.stats().hunger(), 50.0, "a meal");
        assert!(biology.stats().thirst() > 50.0, "and thirst still ran");

        biology.set_running(ProcessId::Hunger, true);
        biology.handle(Event::Ingested(ItemKind::Food));
        assert_eq!(biology.stats().hunger(), 50.0 - MEAL.min(50.0), "running again");
    }

    /// The worst a crowd can do: sixteen new faces at every look — every 7th
    /// tick — for a whole day. Company slows discontent; it must not pin
    /// anybody at content, or satisfaction would read 100 in every crowd and
    /// mean nothing.
    #[test]
    fn a_whole_day_in_the_thickest_crowd_lifts_satisfaction_without_pinning_it() {
        use crate::sim::background::Schedule;
        use crate::sim::clock::TIME_SCALE;
        let day = |faces_per_look: usize| {
            let mut biology = calm();
            biology.edit(|stats| stats.with_satisfaction(100.0));
            let mut schedule = Schedule::new(5);
            let dt = TIME_SCALE / 64.0;
            for tick in 0..(24.0 * HOUR / dt) as u64 {
                biology.advance(dt);
                let due = schedule.tick(dt);
                biology.background(&due);
                if tick % 7 == 0 {
                    for _ in 0..faces_per_look {
                        biology.handle(Event::Met);
                    }
                }
            }
            biology.stats().satisfaction()
        };
        let alone = day(0);
        let crowded = day(16);
        assert!((alone - 50.0).abs() < 0.5, "a day wears half of it off: {alone}");
        assert!(crowded > alone + 10.0, "company is worth something: {crowded} vs {alone}");
        assert!(crowded < 80.0, "and is not everything: {crowded}");
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
        assert_eq!(processes(&biology).as_deref(), Some("hunger, thirst, bladder, fun, energy, satisfaction, health"));

        biology.set_running(ProcessId::Hunger, false);
        biology.set_running(ProcessId::Bladder, false);
        biology.set_running(ProcessId::Fun, false);
        biology.set_running(ProcessId::Energy, false);
        biology.set_running(ProcessId::Satisfaction, false);
        biology.set_running(ProcessId::Health, false);
        assert_eq!(processes(&biology).as_deref(), Some("thirst"));
        assert_eq!(names(&biology), before, "the unit panel lays these out once");
    }
}
