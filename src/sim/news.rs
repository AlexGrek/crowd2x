//! [`News`]: what is on everybody's phone.
//!
//! **A story** is something that happened and is worth telling: a death
//! ([`Story::Death`], bad news — filed by `effect::died` for every person who
//! dies, with the case: who it was, what it did, what it died of, where and
//! when, [`Death`]) or a production record ([`Story::Record`], good news —
//! every [`RECORD`] pieces of anything made, counted as they go into the store
//! its trade keeps its work in, `StoreDef::made`). A story waits for **the
//! propagandist** (`JobKind::Propagandist`), who on duty at the propaganda
//! computer in the palace makes the oldest into **an article**
//! (`Task::write_news`, `Effect::Publish`). Nothing is ever let go: a story
//! waits as long as it takes, and every article is kept, in the order
//! published, for the player to read (`game::news`).
//!
//! **Reading it**: an article is on the phones for [`SHELF_LIFE`], a day. A
//! look at the phone seen through to its end (`Task::phone`) is the oldest
//! article still on them that the person has not read ([`read`]: one a look,
//! never the same twice, `Memory::news_read`), or, with nothing new on it, a
//! little fun. Good news mends the mind; bad news wears it down and leaves the
//! reader [`BAD_NEWS_OBEDIENCE`] more obedient for [`BAD_NEWS_LASTS`]
//! ([`obedience`]).
//!
//! World state like any other (`Facilities::news`, `Think::news`): written in
//! the world step only, read by brains in react, saved (`pb::World.news`).

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};

use super::biology::{Biology, Event};
use super::brain::Memory;
use super::clock::{Clock, DAY, HOUR, TIME_SCALE};
use super::entity::{Body, Think};
use super::item::ItemKind;
use super::job::JobKind;
use super::log::Log;
use super::uid::Uid;

/// Every this many pieces of one thing made is a record, and good news.
pub const RECORD: u32 = 100;

/// How long an article is on the phones, in world seconds: a day. It stays
/// in the archive after.
pub const SHELF_LIFE: f64 = DAY as f64;

/// How much more obedient bad news leaves its reader...
pub const BAD_NEWS_OBEDIENCE: u8 = 10;

/// ...and for how long, in world seconds: twelve hours.
pub const BAD_NEWS_LASTS: f64 = 12.0 * HOUR as f64;

/// The case of a death, as the news tells it.
#[derive(Clone, PartialEq, Debug)]
pub struct Death {
    pub dead: Uid,
    /// Its first and last name; empty for one with none.
    pub name: String,
    /// What it did for a living when it died.
    pub job: Option<JobKind>,
    /// What it died of: `thirst`, `starvation`, `exhaustion`, `wounds`,
    /// `deathdog bites` (`biology::health::cause_of_death`).
    pub cause: String,
    /// Where it fell.
    pub at: Point,
    /// When, in world seconds since the world began.
    pub when: f64,
}

/// Something that happened, worth telling.
#[derive(Clone, PartialEq, Debug)]
pub enum Story {
    /// Somebody died. Bad news.
    Death(Death),
    /// `total` pieces of `item` made since the world began, a multiple of
    /// [`RECORD`]. Good news.
    Record { item: ItemKind, total: u32 },
}

/// The time of day and the day of world second `when`: `day 3 Wed  14:05`.
pub fn time_of(when: f64) -> String {
    let clock = Clock::after_watching(when / TIME_SCALE as f64);
    format!("day {} {}  {:02}:{:02}", clock.day(), clock.weekday().short(), clock.hour(), clock.minute())
}

impl Story {
    pub fn is_good(&self) -> bool {
        matches!(self, Story::Record { .. })
    }

    /// What the article says, for the log, the reader and the player:
    /// `Ann Lee, a guard, died of thirst at 40, 22 on day 3 Wed  14:05` (the
    /// uid where there is no name).
    pub fn headline(&self) -> String {
        match self {
            Story::Death(Death { dead, name, job, cause, at, when }) => {
                let who = if name.is_empty() { dead.to_string() } else { name.clone() };
                let what = job.map_or(String::new(), |job| format!(", {},", job.with_article()));
                format!("{who}{what} died of {cause} at {}, {} on {}", at.x, at.y, time_of(*when))
            }
            Story::Record { item, total } => format!("a record: {total} {} made", item.name()),
        }
    }

    fn save(&self) -> pb::Story {
        match self {
            Story::Death(Death { dead, name, job, cause, at, when }) => pb::Story {
                dead: Some(dead.raw()),
                name: Some(name.clone()),
                job: job.map(|job| job.number() as u32),
                cause: Some(cause.clone()),
                at: Some((*at).into()),
                when: Some(*when),
                ..pb::Story::default()
            },
            Story::Record { item, total } => {
                pb::Story { item: Some(item.name().to_string()), total: *total, ..pb::Story::default() }
            }
        }
    }

    fn restore(saved: &pb::Story) -> Result<Story, Invalid> {
        match (saved.dead, &saved.item) {
            (Some(dead), None) => {
                let Some(dead) = Uid::from_raw(dead) else {
                    return invalid(format!("news: a death of {dead:#x}, nobody"));
                };
                let job = match saved.job {
                    None => None,
                    Some(number) => match JobKind::from_number(number as i32) {
                        Some(job) => Some(job),
                        None => return invalid(format!("news: {dead} died in job {number}, no such job")),
                    },
                };
                let when = *pb::required(&saved.when, "news: when somebody died")?;
                if !when.is_finite() {
                    return invalid(format!("news: {dead} died at {when}"));
                }
                Ok(Story::Death(Death {
                    dead,
                    name: saved.name.clone().unwrap_or_default(),
                    job,
                    cause: pb::required(&saved.cause, "news: what somebody died of")?.clone(),
                    at: (*pb::required(&saved.at, "news: where somebody died")?).into(),
                    when,
                }))
            }
            (None, Some(name)) => match ItemKind::from_name(name) {
                Some(item) if saved.total > 0 && saved.total.is_multiple_of(RECORD) => {
                    Ok(Story::Record { item, total: saved.total })
                }
                Some(_) => invalid(format!("news: a record of {}", saved.total)),
                None => invalid(format!("news: a record of {name}, no such thing")),
            },
            _ => invalid("news: a story that is neither a death nor a record"),
        }
    }
}

/// A story made into the news: on the phones from `published` for
/// [`SHELF_LIFE`], and in the archive for good.
#[derive(Clone, PartialEq, Debug)]
pub struct Article {
    /// Numbered from 1 in the order published, so "the next one I have not
    /// read" is the first with a higher number than the last read.
    pub id: u32,
    pub story: Story,
    /// When, in world seconds since the world began.
    pub published: f64,
}

impl Article {
    /// Still on the phones at `now`.
    pub fn is_current(&self, now: f64) -> bool {
        now < self.published + SHELF_LIFE
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct News {
    /// Waiting for the propagandist, oldest first.
    stories: Vec<Story>,
    /// Everything ever published, oldest first: numbered and timed in order,
    /// which is what lets [`News::unread`] find its place by halving.
    articles: Vec<Article>,
    /// Pieces of each item made so far, by `ItemKind::number`.
    made: [u32; ItemKind::COUNT],
    /// The number the next article goes up under.
    next: u32,
}

impl Default for News {
    fn default() -> News {
        News { stories: Vec::new(), articles: Vec::new(), made: [0; ItemKind::COUNT], next: 1 }
    }
}

impl News {
    /// File a story for the propagandist.
    pub fn report(&mut self, story: Story) {
        self.stories.push(story);
    }

    /// `pieces` of `item` made — gone into the store its trade keeps its work
    /// in — and a record reported if that passes the next [`RECORD`].
    pub fn made(&mut self, item: ItemKind, pieces: u32) {
        let made = &mut self.made[item.number() as usize];
        let before = *made;
        *made = made.saturating_add(pieces);
        let after = *made;
        if after / RECORD > before / RECORD {
            self.report(Story::Record { item, total: after / RECORD * RECORD });
        }
    }

    /// Pieces of `item` made so far.
    pub fn made_of(&self, item: ItemKind) -> u32 {
        self.made[item.number() as usize]
    }

    /// Stories waiting to be written up.
    pub fn waiting(&self) -> usize {
        self.stories.len()
    }

    pub fn stories(&self) -> &[Story] {
        &self.stories
    }

    /// Everything ever published, oldest first: the archive.
    pub fn articles(&self) -> &[Article] {
        &self.articles
    }

    /// Where the articles on the phones at `now`, numbered past `read`,
    /// begin: everything before is out of date or read. Both are a prefix of
    /// the archive, so their union is, and halving finds its end.
    fn first_past(&self, read: u32, now: f64) -> usize {
        self.articles.partition_point(|article| article.id <= read || !article.is_current(now))
    }

    /// Articles on the phones at `now`.
    pub fn current(&self, now: f64) -> &[Article] {
        &self.articles[self.first_past(0, now)..]
    }

    /// The oldest story made into an article at `now`. `None` with no story
    /// waiting.
    pub fn publish(&mut self, now: f64, log: &Log) -> Option<&Article> {
        if self.stories.is_empty() {
            return None;
        }
        let story = self.stories.remove(0);
        let tone = if story.is_good() { "good" } else { "bad" };
        log.important(format!("the news ({tone}): {}", story.headline()));
        self.articles.push(Article { id: self.next, story, published: now });
        self.next += 1;
        self.articles.last()
    }

    /// The oldest article on the phones at `now` numbered past `read`: what
    /// somebody who last read `read` reads next.
    pub fn unread(&self, read: u32, now: f64) -> Option<&Article> {
        self.articles.get(self.first_past(read, now))
    }

    // --- saving ---

    pub(crate) fn save(&self) -> pb::News {
        let News { stories, articles, made, next } = self;
        pb::News {
            stories: stories.iter().map(Story::save).collect(),
            articles: articles
                .iter()
                .map(|Article { id, story, published }| pb::Article {
                    id: *id,
                    story: Some(story.save()),
                    published: *published,
                })
                .collect(),
            made: ItemKind::ALL
                .into_iter()
                .filter(|item| made[item.number() as usize] > 0)
                .map(|item| pb::Made { item: item.name().to_string(), count: made[item.number() as usize] })
                .collect(),
            next: *next,
        }
    }

    pub(crate) fn restore(saved: Option<&pb::News>) -> Result<News, Invalid> {
        let Some(saved) = saved else {
            return Ok(News::default());
        };
        let mut news = News { next: saved.next, ..News::default() };
        if news.next == 0 {
            return invalid("news: the next article numbered 0");
        }
        news.stories = saved.stories.iter().map(Story::restore).collect::<Result<_, _>>()?;
        for article in &saved.articles {
            if !article.published.is_finite() {
                return invalid(format!("news: article {} published at {}", article.id, article.published));
            }
            let in_order = news.articles.last().is_none_or(|last: &Article| last.id < article.id && last.published <= article.published);
            if article.id >= news.next || !in_order {
                return invalid(format!("news: article {} out of order", article.id));
            }
            let story = Story::restore(pb::required(&article.story, "news: an article's story")?)?;
            news.articles.push(Article { id: article.id, story, published: article.published });
        }
        for made in &saved.made {
            let Some(item) = ItemKind::from_name(&made.item) else {
                return invalid(format!("news: {} made of {}, no such thing", made.count, made.item));
            };
            news.made[item.number() as usize] = made.count;
        }
        Ok(news)
    }
}

/// A look at the phone seen through to its end, by `uid`: the next article it
/// has not read — the mind told whether it was good or bad news
/// ([`Event::News`]), bad news remembered for its obedience — or, with
/// nothing new on the phone, a little fun ([`Event::Phoned`]).
pub(crate) fn read(think: &Think<'_>, uid: Uid, memory: &mut Memory, biology: Option<&mut Biology>) {
    let Some(biology) = biology else {
        return;
    };
    let now = think.clock.elapsed();
    let Some(article) = think.news.unread(memory.news_read, now) else {
        biology.handle(Event::Phoned);
        return;
    };
    memory.news_read = article.id;
    let good = article.story.is_good();
    biology.handle(Event::News { good });
    if !good {
        memory.obedient_until = Some(now + BAD_NEWS_LASTS);
    }
    think.log.unit(uid, format!("{uid} read the news: {}", article.story.headline()));
}

/// How obedient somebody is at `clock`: its own obedience, and
/// [`BAD_NEWS_OBEDIENCE`] more for [`BAD_NEWS_LASTS`] after bad news.
pub fn obedience(body: &Body, memory: &Memory, clock: Clock) -> u8 {
    let shaken = memory.obedient_until.is_some_and(|until| clock.elapsed() < until);
    body.obedience().saturating_add(if shaken { BAD_NEWS_OBEDIENCE } else { 0 }).min(100)
}

#[cfg(test)]
#[path = "news/tests.rs"]
mod tests;
