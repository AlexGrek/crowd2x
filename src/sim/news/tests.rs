//! The news: stories, records, the propagandist's articles, reading them on
//! a phone, and saving it all.

use super::*;
use crate::map::{Map, Point, Size, FLOOR};
use crate::sim::biology::fun::PHONE_FUN;
use crate::sim::biology::mind::{BAD_NEWS, GOOD_NEWS};
use crate::sim::biology::Stats;
use crate::sim::clock::{TIME_SCALE, DAY};
use crate::sim::effect::{apply, Effect};
use crate::sim::testing::{a_death, needy_human, prop_at, World};
use crate::sim::uid::EntityType;
use crate::sim::{GameEntity, Human};

fn human(n: u64) -> Uid {
    Uid::new(EntityType::Human, n)
}

fn death(n: u64) -> Story {
    a_death(n)
}

/// `hours` o'clock on day `day` (day 1 a Monday; the clock starts at 08:00).
fn at(day: u32, hours: f32) -> Clock {
    let world = (day - 1) as f64 * DAY as f64 + (hours as f64 - 8.0) * HOUR as f64;
    Clock::after_watching(world / TIME_SCALE as f64)
}

/// Every hundred of one thing made is a record, told once, and each thing is
/// counted on its own.
#[test]
fn every_hundred_of_anything_made_is_a_record() {
    let mut news = News::default();
    news.made(ItemKind::Scrap, 99);
    assert_eq!(news.waiting(), 0);
    news.made(ItemKind::Steel, 99);
    assert_eq!(news.waiting(), 0, "scrap and steel are counted apart");
    news.made(ItemKind::Scrap, 1);
    assert_eq!(news.stories(), [Story::Record { item: ItemKind::Scrap, total: 100 }]);
    news.made(ItemKind::Scrap, 99);
    assert_eq!(news.waiting(), 1, "199 is no new record");
    news.made(ItemKind::Scrap, 150);
    assert_eq!(news.stories()[1], Story::Record { item: ItemKind::Scrap, total: 300 }, "one record for a jump past two");
    assert_eq!(news.made_of(ItemKind::Scrap), 349);
    assert!(news.stories()[0].is_good());
    assert!(!death(1).is_good());
}

/// The oldest story first, an article a story, on the phones for a day and
/// read in order, one at a time — and every article kept after, for good.
#[test]
fn the_oldest_story_goes_up_first_is_on_the_phones_for_a_day_and_is_kept_for_good() {
    let log = Log::default();
    let mut news = News::default();
    assert_eq!(news.publish(0.0, &log), None, "nothing to tell");
    news.report(death(1));
    news.report(Story::Record { item: ItemKind::Wheat, total: 100 });
    let first = news.publish(10.0, &log).unwrap().clone();
    assert_eq!((first.id, first.story), (1, death(1)));
    let second = news.publish(20.0, &log).unwrap().clone();
    assert_eq!((second.id, second.story.is_good()), (2, true));
    assert_eq!(news.waiting(), 0);

    assert_eq!(news.unread(0, 30.0).map(|a| a.id), Some(1));
    assert_eq!(news.unread(1, 30.0).map(|a| a.id), Some(2));
    assert_eq!(news.unread(2, 30.0), None, "all read");
    // The first is a day old and off the phones; the second has ten seconds more.
    assert_eq!(news.unread(0, 10.0 + SHELF_LIFE).map(|a| a.id), Some(2));
    assert_eq!(news.unread(0, 20.0 + SHELF_LIFE), None);
    assert_eq!(news.current(15.0 + SHELF_LIFE).len(), 1);

    // Weeks on, nothing is let go.
    news.report(death(2));
    let later = 40.0 * SHELF_LIFE;
    assert_eq!(news.publish(later, &log).map(|a| a.id), Some(3));
    assert_eq!(news.articles().iter().map(|a| a.id).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(news.current(later).len(), 1);
    assert_eq!(news.unread(0, later).map(|a| a.id), Some(3));
    assert!(log.drain().iter().any(|line| line.contains("the news (bad): Ann Lee")));
}

/// Nobody to write them up: the stories wait, every one, however many.
#[test]
fn with_nobody_to_write_them_up_every_story_waits() {
    let mut news = News::default();
    for n in 0..500 {
        news.report(death(n + 1));
    }
    assert_eq!(news.waiting(), 500);
    assert_eq!(news.stories()[0], death(1));
}

/// A death is told with its case: who, what it did, what of, where, when.
#[test]
fn a_death_is_told_with_its_case() {
    assert_eq!(
        death(12).headline(),
        "Ann Lee, a guard, died of thirst at 4, 2 on day 1 Mon  12:00",
    );
    let nameless = Story::Death(Death { name: String::new(), job: None, ..match death(3) {
        Story::Death(death) => death,
        _ => unreachable!(),
    } });
    assert_eq!(nameless.headline(), format!("{} died of thirst at 4, 2 on day 1 Mon  12:00", human(3)));
    assert_eq!(Story::Record { item: ItemKind::Steel, total: 200 }.headline(), "a record: 200 steel made");
}

#[test]
fn the_news_comes_back_from_a_save_as_it_was_and_a_broken_one_is_refused() {
    let log = Log::default();
    let mut news = News::default();
    news.made(ItemKind::Steel, 130);
    news.report(death(9));
    news.report(death(10));
    let _ = news.publish(5.0, &log);
    let saved = news.save();
    assert_eq!(News::restore(Some(&saved)), Ok(news.clone()));
    assert_eq!(News::restore(None), Ok(News::default()), "an older save: no news yet");

    let mut broken = saved.clone();
    broken.next = 1;
    assert!(News::restore(Some(&broken)).is_err(), "an article numbered past the next");
    let mut broken = saved.clone();
    broken.stories[0] = pb::Story { item: Some("steel".to_string()), total: 150, ..pb::Story::default() };
    assert!(News::restore(Some(&broken)).is_err(), "a record not of a hundred");
    let mut broken = saved.clone();
    broken.stories[0].cause = None;
    assert!(News::restore(Some(&broken)).is_err(), "a death without its cause");
    let mut broken = saved.clone();
    broken.stories[0].job = Some(999);
    assert!(News::restore(Some(&broken)).is_err(), "no such job");
    let mut broken = saved.clone();
    broken.made[0].item = "unobtainium".to_string();
    assert!(News::restore(Some(&broken)).is_err());
}

/// A person's death is a story, with its case as it happened.
#[test]
fn a_death_is_a_story_waiting_for_the_propagandist() {
    use crate::sim::job::{Job, WATCHES};
    let mut world = World::new(Map::new(Size::new(9, 9), FLOOR));
    let mut person = needy_human(Point::new(3, 3), 0.0, 0.0, 0.0);
    assert!(person.hire(Job::guard(Point::new(0, 0), WATCHES[0])));
    let name = person.identity().name().to_string();
    let when = at(2, 14.5);
    let World { map, facilities, occupancy, log, .. } = &mut world;
    apply(Effect::Died { cause: "starvation", when }, &mut person, map, facilities, occupancy, log);
    let Story::Death(death) = &world.news.stories()[0] else { panic!("{:?}", world.news.stories()) };
    assert_eq!(
        (death.dead, death.name.as_str(), death.job, death.cause.as_str(), death.at, death.when),
        (person.uid(), name.as_str(), Some(crate::sim::JobKind::Guard), "starvation", Point::new(3, 3), when.elapsed())
    );
    assert!(world.news.stories()[0].headline().contains(", a guard, died of starvation at 3, 3 on day 2 Tue  14:30"));
}

/// What goes into the store its trade keeps its work in is made and counts
/// for a record; what is carried on from there does not count again.
#[test]
fn what_goes_into_the_store_its_trade_keeps_counts_and_what_is_carried_on_does_not() {
    let mut map = Map::new(Size::new(9, 9), FLOOR);
    prop_at(&mut map, "scrap bin", Point::new(1, 1));
    prop_at(&mut map, "scrap hopper", Point::new(5, 1));
    let mut world = World::new(map);
    let mut person = needy_human(Point::new(3, 3), 0.0, 0.0, 0.0);
    for _ in 0..40 {
        let World { map, facilities, occupancy, log, .. } = &mut world;
        apply(Effect::Deposit { at: Point::new(1, 1), pieces: 3 }, &mut person, map, facilities, occupancy, log);
        apply(Effect::Deposit { at: Point::new(5, 1), pieces: 3 }, &mut person, map, facilities, occupancy, log);
    }
    assert_eq!(world.news.made_of(ItemKind::Scrap), 120, "the bin's, not the hopper's");
    assert_eq!(world.news.stories(), [Story::Record { item: ItemKind::Scrap, total: 100 }]);
}

/// A body with every process running, calm, at `mental_health`.
fn reader(mental_health: f32) -> Biology {
    Biology::new(Stats::calm().with_mental_health(mental_health).with_fun(30.0))
}

/// One article a look, oldest first: bad news wears the mind down and makes
/// the reader more obedient for twelve hours, good news mends it, and with
/// nothing new on the phone it is a little fun instead.
#[test]
fn a_look_at_the_phone_reads_the_next_article_or_is_a_little_fun() {
    let mut world = World::new(Map::new(Size::new(9, 9), FLOOR));
    world.clock = at(1, 12.0);
    let now = world.clock.elapsed();
    let log = Log::default();
    world.news.report(death(5));
    world.news.report(Story::Record { item: ItemKind::Wheat, total: 100 });
    let _ = world.news.publish(now - 60.0, &log);
    let _ = world.news.publish(now - 30.0, &log);

    let person = needy_human(Point::new(3, 3), 0.0, 0.0, 0.0);
    let mut memory = Memory::new();
    let mut body = reader(50.0);
    let own = person.body().obedience();

    read(&world.ctx(), person.uid(), &mut memory, Some(&mut body));
    assert_eq!(memory.news_read, 1);
    assert_eq!(body.stats().mental_health(), 50.0 - BAD_NEWS);
    assert_eq!(body.stats().fun(), 30.0, "news is not fun");
    assert_eq!(obedience(person.body(), &memory, world.clock), (own + BAD_NEWS_OBEDIENCE).min(100));
    assert_eq!(obedience(person.body(), &memory, at(1, 23.9)), (own + BAD_NEWS_OBEDIENCE).min(100));
    assert_eq!(obedience(person.body(), &memory, at(2, 0.1)), own, "twelve hours on, it is itself again");

    read(&world.ctx(), person.uid(), &mut memory, Some(&mut body));
    assert_eq!(memory.news_read, 2);
    assert_eq!(body.stats().mental_health(), 50.0 - BAD_NEWS + GOOD_NEWS);

    read(&world.ctx(), person.uid(), &mut memory, Some(&mut body));
    assert_eq!(memory.news_read, 2, "nothing new to read");
    assert_eq!(body.stats().fun(), 30.0 + PHONE_FUN, "only a little fun");
    assert_eq!(world.log.drain().iter().filter(|line| line.contains("read the news: ")).count(), 2);
}

/// Bad news makes somebody just short of obeying a guard obey.
#[test]
fn bad_news_tips_somebody_just_short_of_it_into_obeying() {
    let mut person = needy_human(Point::new(3, 3), 0.0, 0.0, 0.0);
    person.body_mut().set_obedience(55);
    let mut memory = Memory::new();
    let clock = at(1, 12.0);
    assert_eq!(obedience(person.body(), &memory, clock), 55);
    memory.obedient_until = Some(clock.elapsed() + BAD_NEWS_LASTS);
    assert_eq!(obedience(person.body(), &memory, clock), 65);
    assert_eq!(person.obedience(clock), 55, "this memory is not its brain's");
    person.brain_mut().memory_mut().obedient_until = memory.obedient_until;
    assert_eq!(person.obedience(clock), 65, "its brain's is");
    person.body_mut().set_obedience(95);
    assert_eq!(person.obedience(clock), 100, "never past 100");
}

/// A propagandist hired at the palace's propaganda computer, every need but
/// tiredness off.
fn propagandist(cell: Point, desk: Point) -> Human {
    use crate::sim::biology::ProcessId;
    use crate::sim::job::{Job, JobKind, PROPAGANDA_HOURS};
    let mut human = needy_human(cell, 0.0, 0.0, 0.0);
    human.set_stamina(100.0);
    let biology = human.biology_mut().unwrap();
    for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun] {
        biology.set_running(process, false);
    }
    assert!(human.hire(Job::new(JobKind::Propagandist, desk, PROPAGANDA_HOURS)));
    human
}

/// On duty from noon, a story waiting is written up within the hour and goes
/// up on the phones; with none, the propagandist only works its hours at the
/// computer — and off duty it writes nothing.
#[test]
fn the_propagandist_on_duty_writes_up_each_story_waiting() {
    let desk = Point::new(6, 5);
    let mut map = Map::new(Size::new(14, 10), FLOOR);
    prop_at(&mut map, "propaganda computer", desk);
    let mut world = World::new(map);
    world.clock = at(1, 9.0);
    world.news.report(death(3));
    world.news.report(death(4));
    let mut writer = propagandist(Point::new(5, 5), desk);
    let ticks = |world: &World, hours: f32| (hours * HOUR / TIME_SCALE / world.dt) as u32;

    // The morning: off duty, nothing written.
    for _ in 0..ticks(&world, 0.5) {
        world.step_in_time(&mut writer);
    }
    assert_eq!(world.news.waiting(), 2, "not before its hours");

    world.clock = at(1, 12.0);
    for _ in 0..ticks(&world, 1.4) {
        world.step_in_time(&mut writer);
    }
    assert_eq!(world.news.waiting(), 0, "{:?}", world.lines());
    assert_eq!(world.news.articles().iter().map(|a| a.story.clone()).collect::<Vec<_>>(), [death(3), death(4)]);
    assert!(world.log_contains("the news (bad): Ann Lee, a guard, died of thirst"));
    for _ in 0..ticks(&world, 1.2) {
        world.step_in_time(&mut writer);
    }
    assert!(world.log_contains("wrote copy an hour at the propaganda computer"), "{:?}", world.lines());
}
