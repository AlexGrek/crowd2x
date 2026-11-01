//! Typed unit speech carried by a transient, omnidirectional sound field.
//! Emissions commit after react and are heard next tick, independently of sight cadence.
use super::brain::perception::Sightings;
use super::brain::{GoalId, Memory, Perception};
use super::entity::{Body, Think};
use super::job::{Job, JobKind};
use super::uid::{EntityType, Uid};
use crate::map::Point;
use crate::save::{Invalid, invalid, pb};
use rand::{RngExt, SeedableRng};

pub const SPEECH_RADIUS: i32 = 4;
pub const SHOUT_RADIUS: i32 = 10;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Volume {
    Speech,
    Shout,
}
impl Volume {
    pub fn radius(self) -> i32 {
        match self {
            Self::Speech => SPEECH_RADIUS,
            Self::Shout => SHOUT_RADIUS,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Audience {
    Broadcast,
    Person(Uid),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phrase {
    GetBackIn,
    Words(SpokenText),
}
/// Inline UTF-8 keeps talk effects Copy and avoids allocating during react.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpokenText {
    bytes: [u8; 256],
    len: u16,
}
impl Phrase {
    /// Unknown phrases can be spoken and displayed; only known commands are obeyed.
    pub fn words(text: &str) -> Option<Self> {
        if text == "GET BACK IN!" {
            return Some(Self::GetBackIn);
        }
        if text.is_empty() || text.len() > 256 {
            return None;
        }
        let mut bytes = [0; 256];
        bytes[..text.len()].copy_from_slice(text.as_bytes());
        Some(Self::Words(SpokenText {
            bytes,
            len: text.len() as u16,
        }))
    }
    pub fn text(&self) -> &str {
        match self {
            Self::GetBackIn => "GET BACK IN!",
            Self::Words(text) => std::str::from_utf8(&text.bytes[..usize::from(text.len)])
                .expect("speech is constructed from UTF-8"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Utterance {
    pub speaker: Uid,
    pub at: Point,
    pub audience: Audience,
    pub volume: Volume,
    pub phrase: Phrase,
}
/// Retains storage across ticks. Capacity is reserved by the spawn pass;
/// only active sounds are queried, never the entity table.
pub struct SoundField {
    sounds: Vec<Utterance>,
    // Presentation-only ring: catches emissions between rendered frames at high speed.
    history: Box<[Option<Utterance>; 128]>,
    sequence: u64,
}
impl Default for SoundField {
    fn default() -> Self {
        Self {
            sounds: Vec::new(),
            history: Box::new([None; 128]),
            sequence: 0,
        }
    }
}
impl SoundField {
    pub fn reserve(&mut self, units: usize) {
        self.sounds.reserve(units.saturating_sub(self.sounds.len()));
    }
    pub fn emit(&mut self, sound: Utterance) {
        self.sounds.push(sound);
        self.history[self.sequence as usize % 128] = Some(sound);
        self.sequence += 1;
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn since(&self, cursor: u64) -> impl Iterator<Item = &Utterance> {
        (cursor.max(self.sequence.saturating_sub(128))..self.sequence)
            .filter_map(|n| self.history[n as usize % 128].as_ref())
    }
    pub fn clear(&mut self) {
        self.sounds.clear();
    }
    pub fn heard_by(&self, listener: Uid, at: Point) -> impl Iterator<Item = &Utterance> {
        self.sounds
            .iter()
            .filter(move |s| {
                s.speaker != listener
                    && match s.audience {
                        Audience::Broadcast => true,
                        Audience::Person(to) => to == listener,
                    }
            })
            .filter(move |s| {
                let dx = i64::from(s.at.x) - i64::from(at.x);
                let dy = i64::from(s.at.y) - i64::from(at.y);
                dx.abs() <= i64::from(s.volume.radius())
                    && dy.abs() <= i64::from(s.volume.radius())
                    && dx * dx + dy * dy <= i64::from(s.volume.radius()).pow(2)
            })
    }
    pub(crate) fn save(&self) -> Vec<pb::Spoken> {
        self.sounds
            .iter()
            .map(|s| pb::Spoken {
                speaker: s.speaker.raw(),
                at: Some(s.at.into()),
                recipient: match s.audience {
                    Audience::Broadcast => None,
                    Audience::Person(uid) => Some(uid.raw()),
                },
                shout: s.volume == Volume::Shout,
                text: s.phrase.text().to_owned(),
            })
            .collect()
    }
    pub(crate) fn restore(saved: &[pb::Spoken]) -> Result<Self, Invalid> {
        let mut field = Self::default();
        for s in saved {
            let Some(speaker) = Uid::from_raw(s.speaker) else {
                return invalid("invalid speaker");
            };
            let audience = match s.recipient {
                None => Audience::Broadcast,
                Some(raw) => Audience::Person(
                    Uid::from_raw(raw).ok_or_else(|| Invalid::new("invalid recipient"))?,
                ),
            };
            field.emit(Utterance {
                speaker,
                at: (*pb::required(&s.at, "sound position")?).into(),
                audience,
                volume: if s.shout {
                    Volume::Shout
                } else {
                    Volume::Speech
                },
                phrase: if s.text.is_empty() {
                    Phrase::GetBackIn
                } else {
                    Phrase::words(&s.text)
                        .ok_or_else(|| Invalid::new("speech exceeds 256 UTF-8 bytes"))?
                },
            });
        }
        Ok(field)
    }
}
/// A rounded normal N(70, 15), clipped to the stat's bounds. Clipping keeps
/// the population median at 70, unlike rejecting only the upper tail.
/// The UID-local stream preserves the world's existing random sequence.
pub fn obedience(uid: Uid) -> u8 {
    let mut rng = rand::rngs::SmallRng::seed_from_u64(uid.raw() ^ 0x6f62656469656e63);
    let radius = (-2.0 * (1.0 - rng.random::<f64>()).ln()).sqrt();
    let angle = std::f64::consts::TAU * rng.random::<f64>();
    (70.0 + 15.0 * radius * angle.cos())
        .round()
        .clamp(0.0, 100.0) as u8
}

pub fn take_in(ctx: &Think<'_>, body: &Body, perception: &Perception, memory: &mut Memory) {
    if !ctx.clock.sun().is_dark() || ctx.map.site().contains(body.center_position()) {
        return;
    }
    for sound in perception.hear(ctx, body) {
        if sound.phrase == Phrase::GetBackIn
            && ctx
                .society
                .person(sound.speaker)
                .is_some_and(|p| p.job == Some(JobKind::Guard))
            && super::news::obedience(body, memory, ctx.clock) > 60
        {
            if !memory.returning_inside {
                ctx.log
                    .unit(body.uid(), format!("{} obeyed GET BACK IN!", body.uid()));
            }
            memory.returning_inside = true;
        }
    }
}

pub fn night_id(clock: super::clock::Clock) -> u32 {
    clock.day().saturating_sub(u32::from(clock.hour() < 12))
}

pub fn guard_notice(
    ctx: &Think<'_>,
    body: &Body,
    job: Option<&Job>,
    top: GoalId,
    seen: &Sightings,
    memory: &mut Memory,
) {
    let night = night_id(ctx.clock);
    if top == GoalId::Work
        && job.is_some_and(|j| {
            j.kind == JobKind::Guard
                && j.is_due(ctx.clock)
                && body.center_position().manhattan_distance(j.workplace) == 1
        })
        && ctx.clock.sun().is_dark()
        && memory.guard_warned_night != Some(night)
        && seen
            .iter()
            .any(|s| s.uid.kind() == Some(EntityType::Human) && !ctx.map.site().contains(s.cell))
    {
        memory.guard_warning_pending = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FLOOR, Map, Size};
    use crate::sim::clock::{Clock, HOUR, TIME_SCALE};
    use crate::sim::entity::GameEntity;
    use crate::sim::testing::{World, needy_human, prop_at};
    fn human(n: u64) -> Uid {
        Uid::new(EntityType::Human, n)
    }
    fn night() -> Clock {
        Clock::after_watching(15.0 * HOUR as f64 / TIME_SCALE as f64)
    }
    fn sound(volume: Volume, audience: Audience) -> Utterance {
        Utterance {
            speaker: human(1),
            at: Point::new(0, 0),
            audience,
            volume,
            phrase: Phrase::GetBackIn,
        }
    }
    #[test]
    fn arbitrary_long_speech_keeps_its_utf8_text_across_saves_without_becoming_a_command() {
        let text = "Please come inside the walls. It is dangerous outside at night. ?????!";
        let phrase = Phrase::words(text).unwrap();
        assert_eq!(phrase.text(), text);
        assert_ne!(phrase, Phrase::GetBackIn);
        let mut field = SoundField::default();
        field.emit(Utterance {
            phrase,
            ..sound(Volume::Speech, Audience::Broadcast)
        });
        assert_eq!(
            SoundField::restore(&field.save()).unwrap().sounds,
            field.sounds
        );
        assert_eq!(Phrase::words("GET BACK IN!"), Some(Phrase::GetBackIn));
        assert!(Phrase::words("").is_none());
        assert!(Phrase::words(&"x".repeat(257)).is_none());
    }
    #[test]
    fn speech_and_shouts_have_circular_ranges_and_p2p_checks_the_recipient() {
        let mut field = SoundField::default();
        field.emit(sound(Volume::Speech, Audience::Broadcast));
        assert_eq!(field.heard_by(human(2), Point::new(4, 0)).count(), 1);
        assert_eq!(field.heard_by(human(2), Point::new(4, 1)).count(), 0);
        assert_eq!(field.heard_by(human(1), Point::new(0, 0)).count(), 0);
        field.clear();
        field.emit(sound(Volume::Shout, Audience::Person(human(2))));
        assert_eq!(field.heard_by(human(2), Point::new(10, 0)).count(), 1);
        assert_eq!(field.heard_by(human(3), Point::new(1, 0)).count(), 0);
        assert_eq!(field.heard_by(human(2), Point::new(10, 1)).count(), 0);
        assert_eq!(
            SoundField::restore(&field.save()).unwrap().sounds,
            field.sounds
        );
        field.clear();
        assert_eq!(field.heard_by(human(2), Point::new(0, 0)).count(), 0);
    }
    #[test]
    fn obedience_is_seeded_bounded_and_has_a_normal_population_with_median_70() {
        let mut values: Vec<_> = (1..=50000).map(|i| obedience(human(i))).collect();
        assert!(values.iter().all(|v| *v <= 100));
        values.sort_unstable();
        assert_eq!(values[25000], 70);
        let central = values.iter().filter(|v| (55..=85).contains(*v)).count();
        assert!((0.67..0.72).contains(&(central as f64 / values.len() as f64)));
        let mut body = Body::at_cell(human(1), Point::new(0, 0));
        body.set_obedience(61);
        assert_eq!(Body::restore(&body.save()).unwrap().obedience(), 61);
        let mut old = body.save();
        old.obedience = None;
        assert_eq!(
            Body::restore(&old).unwrap().obedience(),
            obedience(human(1))
        );
        old.obedience = Some(101);
        assert!(Body::restore(&old).is_err());
    }
    #[test]
    fn only_obedient_people_outside_at_night_accept_a_guard_command() {
        let mut world = World::new(Map::new(Size::new(12, 12), FLOOR));
        world.clock = night();
        assert!(world.clock.sun().is_dark());
        world.society.join(human(1), None);
        world.society.set_job(human(1), Some(JobKind::Guard));
        world.sound.emit(sound(Volume::Shout, Audience::Broadcast));
        let mut body = Body::at_cell(human(2), Point::new(-1, 0));
        let mut memory = Memory::new();
        body.set_obedience(60);
        take_in(&world.ctx(), &body, &Perception::default(), &mut memory);
        assert!(!memory.returning_inside);
        body.set_obedience(61);
        take_in(&world.ctx(), &body, &Perception::default(), &mut memory);
        assert!(memory.returning_inside);
        memory.returning_inside = false;
        world.society.set_job(human(1), None);
        take_in(&world.ctx(), &body, &Perception::default(), &mut memory);
        assert!(!memory.returning_inside);
        world.society.set_job(human(1), Some(JobKind::Guard));
        world.clock = Clock::after_watching(0.0);
        take_in(&world.ctx(), &body, &Perception::default(), &mut memory);
        assert!(!memory.returning_inside);
        world.clock = night();
        body.set_position((3.5, 3.5));
        take_in(&world.ctx(), &body, &Perception::default(), &mut memory);
        assert!(!memory.returning_inside);
    }
    #[test]
    fn a_guard_shouts_once_stays_at_its_post_and_remembers_across_midnight_and_saves() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        let post = Point::new(1, 5);
        prop_at(&mut map, "guard post", post);
        let mut world = World::new(map.with_wasteland(6));
        world.clock = night();
        let mut guard = needy_human(Point::new(0, 5), 0.0, 0.0, 0.0);
        guard.hire(Job::guard(post, crate::sim::job::WATCHES[1]));
        world.occupancy.claim(Point::new(-1, 5), human(99)).unwrap();
        for _ in 0..80 {
            world.step(&mut guard);
        }
        assert_eq!(
            world
                .lines()
                .iter()
                .filter(|l| l.contains("said: GET BACK IN!"))
                .count(),
            1,
            "{:?}",
            world.lines()
        );
        assert_eq!(guard.center_position(), Point::new(0, 5));
        assert!(!guard.body().is_panicking());
        let saved = guard.brain().memory().save();
        assert!(saved.guard_warned_night.is_some());
    }
    #[test]
    fn a_spoken_order_makes_a_person_walk_inside_without_panicking() {
        let mut world = World::new(Map::new(Size::new(12, 12), FLOOR).with_wasteland(6));
        world.clock = night();
        world.society.join(human(1), None);
        world.society.set_job(human(1), Some(JobKind::Guard));
        let mut person = needy_human(Point::new(-1, 5), 0.0, 0.0, 0.0);
        person.body_mut().set_obedience(61);
        world.sound.emit(Utterance {
            at: Point::new(0, 5),
            ..sound(Volume::Shout, Audience::Broadcast)
        });
        world.step(&mut person);
        assert_eq!(person.brain().top_goal(), GoalId::ReturnInside);
        assert!(person.brain().memory().returning_inside);
        assert!(!person.body().is_panicking());
        assert_eq!(
            world.sound.sounds.len(),
            0,
            "speech expires after it has been heard"
        );
        for _ in 0..1500 {
            world.step(&mut person);
            assert!(!person.body().is_panicking());
            if !person.brain().memory().returning_inside {
                break;
            }
        }
        assert!(
            world.map.site().contains(person.center_position()),
            "{:?}",
            person.brain_fields()
        );
        assert!(!person.brain().memory().returning_inside);
    }
    #[test]
    fn a_return_order_with_no_way_in_is_held_off_instead_of_replanned_every_tick() {
        let mut world = World::new(Map::new(Size::new(12, 12), crate::map::WALL).with_wasteland(6));
        world.clock = night();
        let mut person = needy_human(Point::new(-1, 5), 0.0, 0.0, 0.0);
        person.brain_mut().memory_mut().returning_inside = true;
        for _ in 0..8 {
            world.step(&mut person);
        }
        assert!(person.brain().goals().cooldown(GoalId::ReturnInside) > 0);
    }
    #[test]
    fn speech_and_returning_inside_are_deterministic_in_parallel_and_across_a_save() {
        use crate::sim::{GameState, process_pass};
        fn build() -> (GameState, Uid) {
            let mut map = Map::new(Size::new(12, 12), FLOOR).with_wasteland(6);
            prop_at(&mut map, "guard post", Point::new(1, 4));
            let mut state = GameState::new(map, 42);
            state.elapsed = 15.0 * HOUR as f64 / TIME_SCALE as f64;
            let day_guard = state.spawn_commoner(Point::new(7, 7));
            state
                .entities
                .get_mut(day_guard)
                .unwrap()
                .body_mut()
                .set_frozen(true);
            let guard = state.spawn_commoner(Point::new(0, 4));
            let e = state.entities.get_mut(guard).unwrap();
            assert_eq!(e.job().unwrap().kind, JobKind::Guard);
            e.biology_mut()
                .unwrap()
                .edit(|_| crate::sim::biology::Stats::calm());
            for id in crate::sim::biology::ProcessId::ALL {
                e.biology_mut().unwrap().set_running(id, false);
            }
            state
                .facilities
                .society
                .set_job(guard, Some(JobKind::Guard));
            let visitor = state.spawn_commoner(Point::new(-1, 5));
            state.dismiss(visitor);
            let e = state.entities.get_mut(visitor).unwrap();
            e.body_mut().set_obedience(61);
            e.biology_mut()
                .unwrap()
                .edit(|_| crate::sim::biology::Stats::calm());
            for id in crate::sim::biology::ProcessId::ALL {
                e.biology_mut().unwrap().set_running(id, false);
            }
            for _ in 0..70 {
                let uid = state.spawn_commoner(Point::new(8, 8));
                state
                    .entities
                    .get_mut(uid)
                    .unwrap()
                    .body_mut()
                    .set_frozen(true);
            }
            state.facilities.sound.emit(Utterance {
                speaker: guard,
                at: Point::new(0, 4),
                audience: Audience::Broadcast,
                volume: Volume::Shout,
                phrase: Phrase::GetBackIn,
            });
            (state, visitor)
        }

        let (mut a, visitor) = build();
        let (b, _) = build();
        // Save with an utterance waiting for the next reaction round.
        let world = b.save_world();
        assert_eq!(world.sounds.len(), 1);
        let revision = b.map.navigation_revision();
        let entities = b
            .save_entities(1000, |shard| {
                shard
                    .slots
                    .into_iter()
                    .map(|record| {
                        crate::sim::saving::RestoredEntity::restore(&record, revision).unwrap()
                    })
                    .collect::<Vec<_>>()
            })
            .into_iter()
            .flatten()
            .collect();
        let mut b = GameState::restore(b.map.clone(), &world, entities).unwrap();
        let mut heard = false;
        let mut returned = false;
        for _ in 0..600 {
            process_pass(&mut a, 1.0 / 64.0);
            process_pass(&mut b, 1.0 / 64.0);
            for (left, right) in a.entities.iter().zip(b.entities.iter()) {
                assert_eq!(left.position(), right.position());
                assert_eq!(left.current_goal(), right.current_goal());
            }
            returned |= a
                .map
                .site()
                .contains(a.entities.get(visitor).unwrap().center_position());
            heard |= a.entities.get(visitor).unwrap().current_goal() == Some(GoalId::ReturnInside);
        }
        assert!(
            heard,
            "the return goal must actually run: {:?}",
            a.entities.get(visitor).unwrap().brain_fields()
        );
        assert!(
            returned,
            "the visitor must actually reach the site: {:?}",
            a.entities.get(visitor).unwrap().brain_fields()
        );
    }
}
