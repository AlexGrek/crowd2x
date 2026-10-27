//! Reserved hospital beds and a person's recovery and sick leave.
//! Reservations survive need breaks. Care is counted by the world, then
//! heard exactly once by its patient in the next parallel reaction round.

use super::biology::{Biology, Event};
use super::brain::{GoalId, Goals};
use super::{Entities, Think, Uid};
use crate::map::{Map, ObjectLayer, Point};
use crate::save::{Invalid, invalid, pb};

pub const ADMISSION_HEALTH: f32 = 90.0;

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Medical {
    pub recovering: bool,
    pub sick_day: Option<u32>,
    care_seen: u32,
}

impl Medical {
    pub fn on_leave(&self, day: u32) -> bool {
        self.recovering || self.sick_day == Some(day)
    }

    pub fn update(&mut self, ctx: &Think<'_>, uid: Uid, biology: Option<&mut Biology>) {
        let Some(biology) = biology else { return };
        if !biology.is_running(super::biology::ProcessId::Health) {
            self.recovering = false;
            self.care_seen = ctx.hospital.bed_for(uid).map_or(0, |bed| bed.care);
            return;
        }
        // Stamp the day before applying treatment: full recovery just after
        // midnight still makes that new day a sick day.
        if self.recovering || biology.stats().health() < ADMISSION_HEALTH {
            self.recovering = true;
            self.sick_day = Some(ctx.clock.day());
        }
        if let Some(bed) = ctx.hospital.bed_for(uid) {
            let care = bed.care.saturating_sub(self.care_seen);
            if care > 0 {
                biology.handle(Event::Treated { hp: care as f32 });
            }
            self.care_seen = bed.care;
        } else {
            self.care_seen = 0;
        }
        if biology.stats().health() >= 100.0 {
            self.recovering = false;
        }
    }

    /// `in_bed`: whether a hospital bed is reserved for it.
    pub fn arrange(&self, day: u32, in_bed: bool, goals: &mut Goals) {
        if self.recovering {
            // Only essential needs take a patient out of bed. Even when all
            // beds are full or unreachable, they wait rather than go to work.
            for goal in [GoalId::Wander, GoalId::Play, GoalId::Salvage] {
                goals.suppress(goal);
            }
            // A bed sleeps it (`HospitalRest` is sleep); waiting for one, it
            // sleeps as anybody does, or it dies of exhaustion while it waits.
            if in_bed {
                goals.suppress(GoalId::Sleep);
            }
            for kind in super::JobKind::ALL {
                goals.suppress(kind.goal());
            }
            goals.raise_to(GoalId::Recover, 1.0);
        } else if self.on_leave(day) {
            for kind in super::JobKind::ALL {
                goals.suppress(kind.goal());
            }
        }
    }

    pub(crate) fn save(&self) -> pb::Medical {
        let Self {
            recovering,
            sick_day,
            care_seen,
        } = *self;
        pb::Medical {
            recovering,
            sick_day,
            care_seen,
        }
    }

    pub(crate) fn restore(saved: Option<&pb::Medical>) -> Self {
        saved.map_or(Self::default(), |s| Self {
            recovering: s.recovering,
            sick_day: s.sick_day,
            care_seen: s.care_seen,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bed {
    pub cell: Point,
    pub patient: Option<Uid>,
    pub care: u32,
}

#[derive(Default)]
pub struct Hospital {
    beds: Vec<Bed>,
}

impl Hospital {
    pub fn from_map(map: &Map) -> Self {
        let mut beds: Vec<_> = map
            .objects(ObjectLayer::Props)
            .iter()
            .filter(|o| o.kind.as_str() == "hospital bed")
            .map(|o| Bed {
                cell: o.cell(),
                patient: None,
                care: 0,
            })
            .collect();
        beds.sort_unstable_by_key(|b| b.cell);
        beds.dedup_by_key(|b| b.cell);
        Self { beds }
    }

    pub fn beds(&self) -> &[Bed] {
        &self.beds
    }
    pub fn at(&self, cell: Point) -> Option<&Bed> {
        self.beds.iter().find(|b| b.cell == cell)
    }
    pub fn bed_for(&self, uid: Uid) -> Option<&Bed> {
        self.beds.iter().find(|b| b.patient == Some(uid))
    }

    pub fn admit(&mut self, at: Point, uid: Uid) -> bool {
        if let Some(bed) = self.bed_for(uid) {
            return bed.cell == at;
        }
        let Some(bed) = self.beds.iter_mut().find(|b| b.cell == at && b.patient.is_none()) else {
            return false;
        };
        bed.patient = Some(uid);
        bed.care = 0;
        true
    }

    pub fn release(&mut self, uid: Uid) {
        if let Some(bed) = self.beds.iter_mut().find(|b| b.patient == Some(uid)) {
            bed.patient = None;
            bed.care = 0;
        }
    }

    pub fn care(&mut self, at: Point, uid: Uid) -> bool {
        let Some(bed) = self.beds.iter_mut().find(|b| b.cell == at && b.patient == Some(uid)) else {
            return false;
        };
        bed.care = bed.care.saturating_add(1);
        true
    }

    pub(crate) fn save(&self) -> Vec<pb::HospitalBed> {
        self.beds
            .iter()
            .filter_map(|b| {
                b.patient.map(|uid| pb::HospitalBed {
                    cell: Some(b.cell.into()),
                    patient: uid.raw(),
                    care: b.care,
                })
            })
            .collect()
    }

    pub(crate) fn restore(&mut self, saved: &[pb::HospitalBed], entities: &Entities) -> Result<(), Invalid> {
        for record in saved {
            let cell = pb::cell(record.cell, "hospital bed")?;
            let uid = Uid::from_raw(record.patient).ok_or_else(|| Invalid::new("hospital patient has an invalid id"))?;
            if !entities.get(uid).is_some_and(|e| !e.is_dead() && e.biology().is_some())
                || self.bed_for(uid).is_some()
                || self.at(cell).is_none_or(|b| b.patient.is_some())
                || !self.admit(cell, uid)
            {
                return invalid("hospital bed has a missing or duplicate patient or bed");
            }
            self.beds.iter_mut().find(|b| b.cell == cell).unwrap().care = record.care;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FLOOR, Size, WALL};
    use crate::sim::biology::{ProcessId, Stats};
    use crate::sim::brain::tasks::rig::Rig;
    use crate::sim::brain::{Action, Task, TaskResult};
    use crate::sim::clock::{Clock, DAY, HOUR, OPENING_TIME, TIME_SCALE};
    use crate::sim::job::{Job, JobKind, NURSE_SHIFTS};
    use crate::sim::testing::{World, needy_human, prop_at};
    use crate::sim::{GameEntity, GameState, Human, Input, process_game_state};

    const BED: Point = Point::new(5, 4);
    fn map() -> Map {
        let mut map = Map::new(Size::new(12, 9), FLOOR);
        prop_at(&mut map, "hospital bed", BED);
        map
    }
    fn wounded(hp: f32) -> Human {
        let mut h = needy_human(Point::new(2, 4), 0.0, 0.0, 0.0);
        for process in ProcessId::ALL {
            h.biology_mut().unwrap().set_running(process, process == ProcessId::Health);
        }
        h.biology_mut().unwrap().edit(|s| s.with_health(hp));
        h
    }
    fn in_bed(world: &mut World, h: &mut Human) {
        for _ in 0..1500 {
            world.step(h);
            if h.center_position() == BED && h.brain().is_asleep() {
                return;
            }
        }
        panic!("never reached bed: {:?}", h.brain().debug_fields());
    }

    #[test]
    fn health_below_ninety_admits_but_ninety_does_not_and_hospital_beds_are_not_homes() {
        for (hp, admitted) in [(89.0, true), (90.0, false)] {
            let mut world = World::new(map());
            let mut human = wounded(hp);
            for _ in 0..1000 {
                world.step(&mut human);
            }
            assert_eq!(world.hospital.bed_for(human.uid()).is_some(), admitted, "{hp}");
        }
        let mut state = GameState::new(map(), 4);
        let uid = state.spawn(super::super::EntityType::Human, Point::new(2, 4));
        assert!(!state.homes().is_owned(BED));
        assert!(!state.entities().get(uid).unwrap().is_on_sick_leave(state.clock()));
    }

    #[test]
    fn a_patient_keeps_the_bed_until_fully_healed_and_keeps_sick_leave_on_discharge_day() {
        let mut world = World::new(map());
        let mut human = wounded(89.0);
        assert!(human.hire(Job::office(Point::new(1, 1))));
        in_bed(&mut world, &mut human);
        // Crossing the admission threshold cannot discharge an existing patient.
        human.biology_mut().unwrap().edit(|s| s.with_health(99.0));
        world.step(&mut human);
        assert!(human.brain().is_hospital_patient());
        assert_eq!(human.current_goal(), Some(GoalId::Recover));
        for _ in 0..5000 {
            world.step(&mut human);
            if world.hospital.bed_for(human.uid()).is_none() {
                break;
            }
        }
        assert_eq!(human.biology().unwrap().stats().health(), 100.0);
        assert!(world.hospital.bed_for(human.uid()).is_none());
        assert!(human.is_on_sick_leave(world.clock));
        world.step(&mut human);
        assert_ne!(human.current_goal(), Some(GoalId::Work));
        world.clock = Clock::after_watching(((DAY + 10.0 * HOUR) as f64 - OPENING_TIME) / TIME_SCALE as f64);
        world.step(&mut human);
        assert!(!human.is_on_sick_leave(world.clock));
        assert_eq!(human.current_goal(), Some(GoalId::Work));
    }

    #[test]
    fn an_hour_actually_in_bed_heals_two_to_five_percent_and_interruption_keeps_partial_rest() {
        let mut world = World::new(map());
        let mut human = wounded(60.0);
        in_bed(&mut world, &mut human);
        let before = human.biology().unwrap().stats().health();
        for _ in 0..1920 {
            world.step(&mut human);
        }
        let gained = human.biology().unwrap().stats().health() - before;
        assert!((1.98..=5.02).contains(&gained), "an hour healed {gained}");
        assert!(world.hospital.bed_for(human.uid()).is_some());
    }

    #[test]
    fn missing_full_and_unreachable_hospitals_do_not_route_every_tick_or_send_patients_to_work() {
        for variant in 0..3 {
            let mut map = map();
            if variant == 0 {
                map = Map::new(Size::new(12, 9), FLOOR);
            }
            if variant == 2 {
                for y in 0..9 {
                    map.set_terrain(Point::new(4, y), WALL);
                }
            }
            let mut world = World::new(map);
            let mut human = wounded(80.0);
            assert!(human.hire(Job::office(Point::new(1, 1))));
            if variant == 1 {
                assert!(world.hospital.admit(BED, Uid::new(super::super::EntityType::Human, 99)));
            }
            crate::sim::walker::ROUTES_ASKED.with(|n| n.set(0));
            for _ in 0..1000 {
                world.step(&mut human);
            }
            let routes = crate::sim::walker::ROUTES_ASKED.with(|n| n.get());
            assert!(routes < 30, "variant {variant}: {routes} searches");
            assert_ne!(human.current_goal(), Some(GoalId::Work));
            assert_eq!(human.biology().unwrap().stats().health(), 80.0, "no healing while waiting");
        }
    }

    /// Found by the lab (`src/lab/`): on seed 7 every unit that died of
    /// exhaustion in a month had been bitten a day or two before, and waited
    /// for one of the hospital's four beds without ever lying down.
    #[test]
    fn a_patient_with_no_bed_to_be_had_still_sleeps_rather_than_dying_of_exhaustion() {
        let mut world = World::new(map());
        assert!(world.hospital.admit(BED, Uid::new(super::super::EntityType::Human, 99)));
        let mut human = wounded(50.0);
        human.biology_mut().unwrap().set_running(ProcessId::Energy, true);
        human.biology_mut().unwrap().edit(|s| s.with_stamina(2.0));
        let mut slept = false;
        for _ in 0..2000 {
            world.step(&mut human);
            slept |= human.brain().is_asleep();
        }
        assert!(slept, "{:?}", human.brain().debug_fields());
        assert!(human.brain().is_hospital_patient(), "still waiting for a bed");
        assert!(world.hospital.bed_for(human.uid()).is_none());
    }

    #[test]
    fn essential_needs_interrupt_bed_rest_and_the_reserved_bed_is_kept() {
        for (process, goal) in [
            (ProcessId::Hunger, GoalId::Eat),
            (ProcessId::Thirst, GoalId::Drink),
            (ProcessId::Bladder, GoalId::Relieve),
        ] {
            let mut map = map();
            prop_at(&mut map, "fridge", Point::new(9, 4));
            prop_at(&mut map, "toilet", Point::new(8, 7));
            let mut world = World::new(map);
            let mut human = wounded(50.0);
            in_bed(&mut world, &mut human);
            human.biology_mut().unwrap().set_running(process, true);
            human.biology_mut().unwrap().edit(|s| match process {
                ProcessId::Hunger => s.with_hunger(85.0),
                ProcessId::Thirst => s.with_thirst(85.0),
                _ => s.with_bladder(85.0),
            });
            world.step(&mut human);
            assert_eq!(human.current_goal(), Some(goal));
            assert_eq!(world.hospital.bed_for(human.uid()).unwrap().cell, BED);
            let mut left = false;
            let mut returned = false;
            for _ in 0..8000 {
                world.step(&mut human);
                left |= human.center_position() != BED;
                if left && human.center_position() == BED && human.brain().is_asleep() {
                    returned = true;
                    break;
                }
                assert!(!matches!(
                    human.current_goal(),
                    Some(GoalId::Work | GoalId::Play | GoalId::Sleep | GoalId::Wander)
                ));
            }
            assert!(left && returned, "{process:?} did not leave and return");
            assert!(world.hospital.bed_for(human.uid()).is_some());
        }
    }

    #[test]
    fn nurse_care_finishes_before_crediting_exactly_one_percent_and_never_heals_an_absent_patient() {
        let mut rig = Rig::new(map(), BED.offset(-1, 0));
        let patient = Uid::new(super::super::EntityType::Human, 2);
        assert!(rig.world.hospital.admit(BED, patient));
        assert!(rig.world.occupancy.claim(BED, patient).is_ok());
        let mut task = Task::care_patient(BED, 0.1);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.effect, super::super::Effect::None);
        assert_eq!(rig.run(&mut task, 100), TaskResult::Success);
        assert_eq!(rig.effect, super::super::Effect::Care { at: BED, patient });
        rig.world.occupancy.release(BED, patient);
        rig.action = Action::None;
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        let mut human = wounded(50.0);
        let mut world = World::new(map());
        in_bed(&mut world, &mut human);
        let before = human.biology().unwrap().stats().health();
        assert!(world.hospital.care(BED, human.uid()));
        world.dt = 0.0;
        world.step(&mut human);
        assert_eq!(human.biology().unwrap().stats().health(), before + 1.0);
        world.step(&mut human);
        assert_eq!(human.biology().unwrap().stats().health(), before + 1.0);
    }

    #[test]
    fn competing_admissions_keep_one_patient_per_bed_and_releasing_returns_it() {
        let mut hospital = Hospital::from_map(&map());
        let a = Uid::new(super::super::EntityType::Human, 1);
        let b = Uid::new(super::super::EntityType::Human, 2);
        assert!(hospital.admit(BED, a));
        assert!(!hospital.admit(BED, b));
        assert!(hospital.care(BED, a));
        hospital.release(a);
        assert!(hospital.admit(BED, b));
        assert_eq!(hospital.bed_for(b).unwrap().care, 0);
    }

    #[test]
    fn nurse_rounds_start_with_the_shift_and_stop_if_the_patient_leaves_mid_treatment() {
        let mut map = map();
        let station = Point::new(1, 1);
        prop_at(&mut map, "nurse station", station);
        let mut world = World::new(map.clone());
        let patient = Uid::new(super::super::EntityType::Human, 5);
        assert!(world.hospital.admit(BED, patient));
        assert!(world.occupancy.claim(BED, patient).is_ok());
        let mut nurse = wounded(100.0);
        assert!(nurse.hire(Job::new(JobKind::Nurse, station, NURSE_SHIFTS[1])));
        world.clock = Clock::after_watching(((11.5 * HOUR) as f64 - OPENING_TIME) / TIME_SCALE as f64);
        for _ in 0..1000 {
            world.step(&mut nurse);
        }
        assert_eq!(world.hospital.at(BED).unwrap().care, 0, "commute time is not nursing time");
        world.clock = Clock::after_watching(((12.0 * HOUR) as f64 - OPENING_TIME) / TIME_SCALE as f64);
        for _ in 0..1500 {
            world.step(&mut nurse);
        }
        assert!(world.hospital.at(BED).unwrap().care > 0, "shift never treated a patient");

        let mut rig = Rig::new(map, BED.offset(-1, 0));
        assert!(rig.world.hospital.admit(BED, patient));
        assert!(rig.world.occupancy.claim(BED, patient).is_ok());
        let mut task = Task::care_patient(BED, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        rig.world.occupancy.release(BED, patient);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.effect, super::super::Effect::None);
    }

    #[test]
    fn a_patient_who_dies_or_despawns_returns_the_reserved_bed() {
        for dies in [false, true] {
            let mut state = GameState::new(map(), 2);
            let uid = state.spawn(super::super::EntityType::Human, BED.offset(-1, 0));
            assert!(state.facilities.hospital.admit(BED, uid));
            if dies {
                state
                    .entities
                    .get_mut(uid)
                    .unwrap()
                    .biology_mut()
                    .unwrap()
                    .edit(|s| s.with_health(0.0));
                process_game_state(&mut state, 1.0 / 64.0, &Input::new());
                assert!(state.entities().get(uid).unwrap().is_dead());
            } else {
                assert!(state.despawn(uid));
            }
            assert!(state.facilities.hospital.bed_for(uid).is_none());
        }
    }

    #[test]
    fn real_nurses_take_care_of_patients_during_their_shifts() {
        let mut map = map();
        prop_at(&mut map, "nurse station", Point::new(1, 1));
        let mut state = GameState::new(map, 9);
        let nurse = state.spawn(super::super::EntityType::Human, Point::new(1, 2));
        let patient = state.spawn(super::super::EntityType::Human, Point::new(4, 4));
        assert_eq!(state.entities().get(nurse).unwrap().job().unwrap().kind, JobKind::Nurse);
        for uid in [patient, nurse] {
            let b = state.entities.get_mut(uid).unwrap().biology_mut().unwrap();
            b.edit(|_| {
                Stats::calm()
                    .with_health(if uid == patient { 60.0 } else { 100.0 })
                    .with_hunger(0.0)
                    .with_thirst(0.0)
            });
            for p in ProcessId::ALL {
                b.set_running(p, p == ProcessId::Health);
            }
        }
        for _ in 0..5000 {
            process_game_state(&mut state, 1.0 / 64.0, &Input::new());
        }
        let bed = state.facilities.hospital.bed_for(patient).unwrap();
        assert!(bed.care > 0, "no nurse care");
        assert!(state.entities().get(patient).unwrap().biology().unwrap().stats().health() > 63.0);
        assert_eq!(NURSE_SHIFTS.len() * crate::map::district::hospital::STATIONS, 4);
    }

    #[test]
    fn hospital_admissions_care_and_recovery_are_deterministic_in_parallel_rounds() {
        let build = || {
            let mut map = Map::new(Size::new(35, 20), FLOOR);
            for x in [5, 10, 15, 20] {
                prop_at(&mut map, "hospital bed", Point::new(x, 5));
            }
            for x in [5, 15] {
                prop_at(&mut map, "nurse station", Point::new(x, 2));
            }
            let mut state = GameState::new(map, 15);
            for i in 0..80 {
                let uid = state.spawn(super::super::EntityType::Human, Point::new(1 + i % 30, 7 + i / 30));
                let biology = state.entities.get_mut(uid).unwrap().biology_mut().unwrap();
                biology.edit(|s| s.with_health(if i < 4 { 100.0 } else { 70.0 }));
                for p in ProcessId::ALL {
                    biology.set_running(p, p == ProcessId::Health);
                }
            }
            state
        };
        let (mut a, mut b) = (build(), build());
        for _ in 0..1800 {
            process_game_state(&mut a, 1.0 / 64.0, &Input::new());
            process_game_state(&mut b, 1.0 / 64.0, &Input::new());
        }
        assert!(!a.save_world().hospital.is_empty(), "no hospital admission exercised");
        assert!(a.save_world().hospital.iter().any(|b| b.care > 0), "no nurse care exercised");
        assert_eq!(a.save_world(), b.save_world());
        assert_eq!(a.save_entities(1000, |s| s), b.save_entities(1000, |s| s));
    }
}
