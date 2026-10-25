//! Relationships: what each person thinks of the people it knows, who is
//! family and who is a colleague, and the chats that bring two closer.
//!
//! Three parts, kept where the rest of the simulation would keep them:
//!
//! * [`Relations`] — **one person's** points for everybody it knows, and when
//!   it last chatted with each. In the unit's own memory
//!   ([`Memory::relations`](super::brain::Memory::relations)), written only
//!   by its own brain: a chat raises *its* points for the other, and the other
//!   raises its own. Lost a point a night to everyone
//!   ([`Relations::pass_nights`]).
//! * [`Society`] — the facts a floor is made of, about everybody: which house
//!   a person was born into (its **family**: everybody who spawned in one
//!   house, the family floor at least) and what job it does (its
//!   **colleagues**: everybody with the same kind of job, the colleague floor
//!   at least) — and the [`Rules`] all of this is run by. Kept by the spawn
//!   pass as people arrive, are hired, let go, die and leave; worked out again
//!   from the entities on load, so only its rules are saved. A brain asks it
//!   about somebody it has just seen ([`Think::society`](super::Think)),
//!   because one brain may not read another.
//! * [`Chats`] — **who is chatting with whom this tick**, and where they
//!   stand: rebuilt from the crowd by the think step before react reads it,
//!   like [`Panics`](super::panics::Panics), so never saved. What lets two
//!   people agree on a chat with no brain reading the other: one invites
//!   ([`Phase::Inviting`]), the other sees the invitation here and accepts
//!   ([`Phase::Talking`]), the first sees that and goes over.
//!
//! **The level** of one person's relationship with another is its points or
//! the floor, whichever is higher ([`Society::level`]): family never falls
//! below 30, colleagues below 10, whatever the nights take off the points.
//! It is what decides the chance of starting a chat ([`Rules::chance`]) and
//! what a chat adds to ([`Relations::chatted`]).
//!
//! **Every number is a rule** ([`Rules`]): its default is written once, in
//! `relationships.yaml` (built into the game), a new game reads that file,
//! a saved game keeps its own, and [`Command::SetRelationshipRule`] — a QA
//! script's `relationships` step — changes one while playing.
//!
//! [`Command::SetRelationshipRule`]: super::Command::SetRelationshipRule

use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::sync::LazyLock;

use rand::RngExt;

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};

use super::clock::{watched, MINUTE};
use super::entities::Entities;
use super::job::JobKind;
use super::property::PropertyId;
use super::uid::{EntityType, Uid};

/// Every number relationships are run by. Each is a [`Rule`], read from
/// `relationships.yaml` by its name ([`Rules::from_yaml`]); the checked-in
/// file is the defaults ([`Rules::default`]).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rules {
    /// The most a person can think of anybody; points run from 0 to this.
    pub most: f32,
    /// The least a person thinks of its family.
    pub family_floor: f32,
    /// The least a person thinks of its colleagues.
    pub colleague_floor: f32,
    /// What every night takes off everybody's points.
    pub nightly_loss: f32,
    /// The chance of asking a stranger for a chat, 0 to 1.
    pub stranger_chance: f32,
    /// ...an acquaintance (over [`Rules::acquaintance_over`]).
    pub acquaintance_chance: f32,
    /// ...a friend (over [`Rules::friend_over`]).
    pub friend_chance: f32,
    /// Over this level somebody is an acquaintance; at or under, a stranger.
    pub acquaintance_over: f32,
    /// Over this level somebody is a friend.
    pub friend_over: f32,
    /// What one chat adds to the level, between these two.
    pub least_chat_gain: f32,
    pub most_chat_gain: f32,
    /// How long a chat is, in world minutes.
    pub minutes_of_talk: f32,
    /// How long after a chat ends the two do not start another, in world
    /// minutes. Each remembers it ([`Relation::chatted`]).
    pub minutes_between_chats: f32,
    /// How long an invitation waits to be taken up, in world minutes.
    pub minutes_to_answer: f32,
    /// The longest from agreeing a chat to its end, in world minutes.
    pub minutes_to_meet: f32,
    /// How long one side done waits for the other to finish, in world
    /// minutes.
    pub minutes_to_wait_for_the_other: f32,
}

/// One number of the [`Rules`], for reading or setting it on its own — by
/// the name `relationships.yaml` writes it under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rule {
    Most,
    FamilyFloor,
    ColleagueFloor,
    NightlyLoss,
    StrangerChance,
    AcquaintanceChance,
    FriendChance,
    AcquaintanceOver,
    FriendOver,
    LeastChatGain,
    MostChatGain,
    MinutesOfTalk,
    MinutesBetweenChats,
    MinutesToAnswer,
    MinutesToMeet,
    MinutesToWaitForTheOther,
}

impl Rule {
    /// Every rule, in the order `relationships.yaml` lists them.
    pub const ALL: [Rule; 16] = [
        Rule::Most,
        Rule::FamilyFloor,
        Rule::ColleagueFloor,
        Rule::NightlyLoss,
        Rule::StrangerChance,
        Rule::AcquaintanceChance,
        Rule::FriendChance,
        Rule::AcquaintanceOver,
        Rule::FriendOver,
        Rule::LeastChatGain,
        Rule::MostChatGain,
        Rule::MinutesOfTalk,
        Rule::MinutesBetweenChats,
        Rule::MinutesToAnswer,
        Rule::MinutesToMeet,
        Rule::MinutesToWaitForTheOther,
    ];

    /// Its name in `relationships.yaml`, in a save, and in a QA script.
    pub const fn name(self) -> &'static str {
        match self {
            Rule::Most => "most",
            Rule::FamilyFloor => "family floor",
            Rule::ColleagueFloor => "colleague floor",
            Rule::NightlyLoss => "nightly loss",
            Rule::StrangerChance => "stranger chance",
            Rule::AcquaintanceChance => "acquaintance chance",
            Rule::FriendChance => "friend chance",
            Rule::AcquaintanceOver => "acquaintance over",
            Rule::FriendOver => "friend over",
            Rule::LeastChatGain => "least chat gain",
            Rule::MostChatGain => "most chat gain",
            Rule::MinutesOfTalk => "minutes of talk",
            Rule::MinutesBetweenChats => "minutes between chats",
            Rule::MinutesToAnswer => "minutes to answer",
            Rule::MinutesToMeet => "minutes to meet",
            Rule::MinutesToWaitForTheOther => "minutes to wait for the other",
        }
    }

    /// The rule called `name`, if there is one.
    pub fn from_name(name: &str) -> Option<Rule> {
        Rule::ALL.into_iter().find(|rule| rule.name() == name)
    }

    /// What it may be: a chance is a share, everything else a number of
    /// points or minutes, none below nothing and a minute's talk at least.
    pub const fn range(self) -> RangeInclusive<f32> {
        match self {
            Rule::StrangerChance | Rule::AcquaintanceChance | Rule::FriendChance => 0.0..=1.0,
            Rule::Most => 1.0..=10_000.0,
            Rule::MinutesOfTalk => 1.0..=24.0 * 60.0,
            _ => 0.0..=10_000.0,
        }
    }
}

/// The file the defaults are, built into the game.
pub const DEFAULTS_YAML: &str = include_str!("../../relationships.yaml");

static DEFAULTS: LazyLock<Rules> =
    LazyLock::new(|| Rules::from_yaml(DEFAULTS_YAML).unwrap_or_else(|error| panic!("relationships.yaml: {error}")));

/// The checked-in `relationships.yaml`: the one place a default is written.
impl Default for Rules {
    fn default() -> Rules {
        *DEFAULTS
    }
}

impl Rules {
    /// The rules `relationships.yaml` written as `text` says: every rule by
    /// its name ([`Rule::name`]), nothing else, each within its range
    /// ([`Rule::range`]), the floors no more than the most and the least chat
    /// gain no more than the most.
    pub fn from_yaml(text: &str) -> Result<Rules, String> {
        let file: BTreeMap<String, f64> = serde_yaml::from_str(text).map_err(|error| error.to_string())?;
        if let Some(name) = file.keys().find(|name| Rule::from_name(name).is_none()) {
            return Err(format!("{name:?} is no rule"));
        }
        let mut rules = DEFAULT_SHAPE;
        for rule in Rule::ALL {
            let value = *file.get(rule.name()).ok_or_else(|| format!("{:?} is missing", rule.name()))?;
            rules.check(rule, value)?;
            rules.set(rule, value as f32);
        }
        rules.consistent()?;
        Ok(rules)
    }

    /// Whether `value` is something `rule` may be.
    pub fn check(&self, rule: Rule, value: f64) -> Result<(), String> {
        if !value.is_finite() || !rule.range().contains(&(value as f32)) {
            let range = rule.range();
            return Err(format!("{}: {value} is not between {} and {}", rule.name(), range.start(), range.end()));
        }
        Ok(())
    }

    /// The rules agree with each other: nobody's floor over the most there
    /// is, the least a chat adds no more than the most.
    fn consistent(&self) -> Result<(), String> {
        for (rule, floor) in [(Rule::FamilyFloor, self.family_floor), (Rule::ColleagueFloor, self.colleague_floor)] {
            if floor > self.most {
                return Err(format!("{}: {floor} is over the most, {}", rule.name(), self.most));
            }
        }
        if self.least_chat_gain > self.most_chat_gain {
            return Err(format!("least chat gain: {} is more than the most, {}", self.least_chat_gain, self.most_chat_gain));
        }
        Ok(())
    }

    pub fn get(&self, rule: Rule) -> f32 {
        match rule {
            Rule::Most => self.most,
            Rule::FamilyFloor => self.family_floor,
            Rule::ColleagueFloor => self.colleague_floor,
            Rule::NightlyLoss => self.nightly_loss,
            Rule::StrangerChance => self.stranger_chance,
            Rule::AcquaintanceChance => self.acquaintance_chance,
            Rule::FriendChance => self.friend_chance,
            Rule::AcquaintanceOver => self.acquaintance_over,
            Rule::FriendOver => self.friend_over,
            Rule::LeastChatGain => self.least_chat_gain,
            Rule::MostChatGain => self.most_chat_gain,
            Rule::MinutesOfTalk => self.minutes_of_talk,
            Rule::MinutesBetweenChats => self.minutes_between_chats,
            Rule::MinutesToAnswer => self.minutes_to_answer,
            Rule::MinutesToMeet => self.minutes_to_meet,
            Rule::MinutesToWaitForTheOther => self.minutes_to_wait_for_the_other,
        }
    }

    /// Set `rule` to `value` as it is: [`Rules::check`] and
    /// [`Rules::change`] are what keep it in range.
    fn set(&mut self, rule: Rule, value: f32) {
        let field = match rule {
            Rule::Most => &mut self.most,
            Rule::FamilyFloor => &mut self.family_floor,
            Rule::ColleagueFloor => &mut self.colleague_floor,
            Rule::NightlyLoss => &mut self.nightly_loss,
            Rule::StrangerChance => &mut self.stranger_chance,
            Rule::AcquaintanceChance => &mut self.acquaintance_chance,
            Rule::FriendChance => &mut self.friend_chance,
            Rule::AcquaintanceOver => &mut self.acquaintance_over,
            Rule::FriendOver => &mut self.friend_over,
            Rule::LeastChatGain => &mut self.least_chat_gain,
            Rule::MostChatGain => &mut self.most_chat_gain,
            Rule::MinutesOfTalk => &mut self.minutes_of_talk,
            Rule::MinutesBetweenChats => &mut self.minutes_between_chats,
            Rule::MinutesToAnswer => &mut self.minutes_to_answer,
            Rule::MinutesToMeet => &mut self.minutes_to_meet,
            Rule::MinutesToWaitForTheOther => &mut self.minutes_to_wait_for_the_other,
        };
        *field = value;
    }

    /// Change `rule` to `value`, kept in its range and in step with the rest
    /// — a floor no higher than the most, the two chat gains in order — and
    /// say what it came to. While playing (`Command::SetRelationshipRule`).
    pub fn change(&mut self, rule: Rule, value: f64) -> f32 {
        let range = rule.range();
        let value = if value.is_finite() { (value as f32).clamp(*range.start(), *range.end()) } else { self.get(rule) };
        self.set(rule, value);
        match rule {
            Rule::Most => {
                self.family_floor = self.family_floor.min(self.most);
                self.colleague_floor = self.colleague_floor.min(self.most);
            }
            Rule::FamilyFloor => self.family_floor = self.family_floor.min(self.most),
            Rule::ColleagueFloor => self.colleague_floor = self.colleague_floor.min(self.most),
            Rule::LeastChatGain => self.most_chat_gain = self.most_chat_gain.max(self.least_chat_gain),
            Rule::MostChatGain => self.least_chat_gain = self.least_chat_gain.min(self.most_chat_gain),
            _ => {}
        }
        self.get(rule)
    }

    /// The chance of asking somebody at `level` ([`Society::level`]) for a
    /// chat.
    pub fn chance(&self, level: f32) -> f32 {
        if level > self.friend_over {
            self.friend_chance
        } else if level > self.acquaintance_over {
            self.acquaintance_chance
        } else {
            self.stranger_chance
        }
    }

    /// What one chat adds, between the least and the most chat gain, from
    /// one draw of `rng`.
    pub fn chat_gain(&self, rng: &mut impl RngExt) -> f32 {
        if self.least_chat_gain < self.most_chat_gain {
            rng.random_range(self.least_chat_gain..=self.most_chat_gain)
        } else {
            self.least_chat_gain
        }
    }

    /// How long a chat is, in **watched** seconds: what a talk's action
    /// counts.
    pub fn talk_seconds(&self) -> f32 {
        watched(self.minutes_of_talk * MINUTE)
    }

    /// [`Rules::minutes_between_chats`], in world seconds.
    pub fn chat_again_after(&self) -> f64 {
        (self.minutes_between_chats * MINUTE) as f64
    }

    /// [`Rules::minutes_to_answer`], in world seconds.
    pub fn invite_patience(&self) -> f64 {
        (self.minutes_to_answer * MINUTE) as f64
    }

    /// [`Rules::minutes_to_meet`], in world seconds.
    pub fn chat_patience(&self) -> f64 {
        (self.minutes_to_meet * MINUTE) as f64
    }

    /// [`Rules::minutes_to_wait_for_the_other`], in world seconds.
    pub fn done_patience(&self) -> f64 {
        (self.minutes_to_wait_for_the_other * MINUTE) as f64
    }

    /// What somebody at `level` is to a person, in a word, if not family or
    /// a colleague: a friend, an acquaintance or a stranger.
    pub fn standing(&self, level: f32) -> &'static str {
        if level > self.friend_over {
            "friend"
        } else if level > self.acquaintance_over {
            "acquaintance"
        } else {
            "stranger"
        }
    }

    // --- saving ---

    /// Every rule by name.
    pub(crate) fn save(&self) -> Vec<pb::RelationshipRule> {
        Rule::ALL.into_iter().map(|rule| pb::RelationshipRule { name: rule.name().to_string(), value: self.get(rule) }).collect()
    }

    /// What [`Rules::save`] kept. A rule the save does not name — every rule,
    /// in a save from before there were relationships — is its default; one
    /// the build does not know, or out of its range, is refused.
    pub(crate) fn restore(saved: &[pb::RelationshipRule]) -> Result<Rules, Invalid> {
        let mut rules = Rules::default();
        for rule in saved {
            let Some(which) = Rule::from_name(&rule.name) else {
                return invalid(format!("relationships: no rule {:?}", rule.name));
            };
            rules.check(which, rule.value as f64).map_err(Invalid::new)?;
            rules.set(which, rule.value);
        }
        rules.consistent().map_err(Invalid::new)?;
        Ok(rules)
    }
}

/// What [`Rules::from_yaml`] fills in, every field: nothing in it survives,
/// since every rule must be named.
const DEFAULT_SHAPE: Rules = Rules {
    most: 0.0,
    family_floor: 0.0,
    colleague_floor: 0.0,
    nightly_loss: 0.0,
    stranger_chance: 0.0,
    acquaintance_chance: 0.0,
    friend_chance: 0.0,
    acquaintance_over: 0.0,
    friend_over: 0.0,
    least_chat_gain: 0.0,
    most_chat_gain: 0.0,
    minutes_of_talk: 0.0,
    minutes_between_chats: 0.0,
    minutes_to_answer: 0.0,
    minutes_to_meet: 0.0,
    minutes_to_wait_for_the_other: 0.0,
};

/// What one person thinks of one other.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Relation {
    pub who: Uid,
    /// Its points, before any floor ([`Society::level`] adds those). Never
    /// below 0, never above the most there is ([`Rules::most`]).
    pub points: f32,
    /// When their last chat ended, in world seconds
    /// ([`Clock::elapsed`](super::clock::Clock::elapsed));
    /// `f64::NEG_INFINITY` if they never chatted.
    pub chatted: f64,
}

/// One person's relationships: everybody it knows, by uid, and the night it
/// last took the nightly loss for.
///
/// **A sorted `Vec`**, looked up by binary search: read when a wanderer sees
/// somebody new, written when a chat ends, and nothing in a tick walks it but
/// the nightly loss. It grows when somebody new is chatted with, the one time
/// it allocates. Iterated in uid order, so never in the order it was filled.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Relations {
    known: Vec<Relation>,
    /// The day whose night was last taken off ([`Relations::pass_nights`]);
    /// 0 before the first time it is asked, which only notes the day.
    night: u32,
}

impl Relations {
    pub fn new() -> Relations {
        Relations::default()
    }

    /// What it thinks of `who`, if it knows them at all.
    pub fn get(&self, who: Uid) -> Option<&Relation> {
        self.known.binary_search_by_key(&who, |relation| relation.who).ok().map(|at| &self.known[at])
    }

    /// Its points for `who`, before any floor: 0 for somebody it does not know.
    pub fn points(&self, who: Uid) -> f32 {
        self.get(who).map_or(0.0, |relation| relation.points)
    }

    /// Whether it chatted with `who` less than the minutes between chats
    /// before `now` (world seconds), and so will not start or take up another.
    pub fn chatted_lately(&self, who: Uid, now: f64, rules: &Rules) -> bool {
        self.get(who).is_some_and(|relation| now - relation.chatted < rules.chat_again_after())
    }

    /// A chat with `who` just ended, at `now`, with the relationship at
    /// `level` ([`Society::level`]) before it: its points become `level` and
    /// `gain` more, at most the most there is. Returns the new points.
    pub fn chatted(&mut self, who: Uid, level: f32, gain: f32, now: f64, rules: &Rules) -> f32 {
        let points = (level + gain).clamp(0.0, rules.most);
        let relation = Relation { who, points, chatted: now };
        match self.known.binary_search_by_key(&who, |relation| relation.who) {
            Ok(at) => self.known[at] = relation,
            Err(at) => self.known.insert(at, relation),
        }
        points
    }

    /// Take the nightly loss for every night since the last one taken, as of
    /// `day` (the clock's, counting from one) at `now` (world seconds): the
    /// rules' nightly loss off everybody's points a night, down to 0.
    /// Somebody down to 0 and not chatted with lately is forgotten — a floor
    /// is the [`Society`]'s, and comes back with nothing remembered.
    ///
    /// The first time it is asked it only notes the day: a person born today
    /// has had no night yet.
    pub fn pass_nights(&mut self, day: u32, now: f64, rules: &Rules) {
        if self.night == 0 {
            self.night = day;
            return;
        }
        if day <= self.night {
            return;
        }
        let loss = (day - self.night) as f32 * rules.nightly_loss;
        self.night = day;
        for relation in &mut self.known {
            relation.points = (relation.points - loss).max(0.0);
        }
        let lately = rules.chat_again_after();
        self.known.retain(|relation| relation.points > 0.0 || now - relation.chatted < lately);
    }

    /// Everybody it knows, in uid order.
    pub fn iter(&self) -> impl Iterator<Item = &Relation> {
        self.known.iter()
    }

    pub fn len(&self) -> usize {
        self.known.len()
    }

    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }

    /// Set `who`'s points outright — for a test that starts two people as
    /// friends.
    #[cfg(test)]
    pub(crate) fn set_points(&mut self, who: Uid, points: f32) {
        let chatted = self.get(who).map_or(f64::NEG_INFINITY, |relation| relation.chatted);
        let relation = Relation { who, points, chatted };
        match self.known.binary_search_by_key(&who, |relation| relation.who) {
            Ok(at) => self.known[at] = relation,
            Err(at) => self.known.insert(at, relation),
        }
    }

    // --- saving ---

    /// Everybody it knows, and the night last taken off.
    pub(crate) fn save(&self) -> (Vec<pb::Relation>, u32) {
        let Relations { known, night } = self;
        let known = known
            .iter()
            .map(|&Relation { who, points, chatted }| pb::Relation {
                who: who.raw(),
                points,
                chatted: chatted.is_finite().then_some(chatted),
            })
            .collect();
        (known, *night)
    }

    /// What [`Relations::save`] kept. Refuses nobody, the same person twice,
    /// and points or a time that are not numbers or below nothing; an older
    /// save has none and comes back knowing nobody, its night not yet noted.
    pub(crate) fn restore(saved: &[pb::Relation], night: u32) -> Result<Relations, Invalid> {
        let mut known = Vec::with_capacity(saved.len());
        for relation in saved {
            let Some(who) = Uid::from_raw(relation.who) else {
                return invalid("relations: somebody who is nobody");
            };
            if !(relation.points.is_finite() && relation.points >= 0.0) {
                return invalid(format!("relations: {} points for {who}", relation.points));
            }
            let chatted = match relation.chatted {
                Some(when) if when.is_finite() => when,
                Some(when) => return invalid(format!("relations: chatted with {who} at {when}")),
                None => f64::NEG_INFINITY,
            };
            known.push(Relation { who, points: relation.points, chatted });
        }
        known.sort_by_key(|relation| relation.who);
        if let Some(pair) = known.windows(2).find(|pair| pair[0].who == pair[1].who) {
            return invalid(format!("relations: {} twice", pair[0].who));
        }
        Ok(Relations { known, night })
    }
}

/// What the floors of a person's relationships are made of.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Person {
    /// The house it spawned in: everybody who spawned in it is its family.
    /// `None` for somebody who arrived anywhere else.
    pub family: Option<PropertyId>,
    /// The kind of job it does: everybody doing the same is a colleague.
    pub job: Option<JobKind>,
}

/// Everybody's family and job, by uid: what the floors are made of
/// ([`Society::floor`]).
///
/// Written by the spawn pass alone — an arrival joins, a hire or a dismissal
/// changes a job, a death ends one, leaving leaves — and read by brains in a
/// tick through [`Think::society`](super::Think), which is why it is here
/// and not asked of the entities. A `BTreeMap`, so the relationship tab lists
/// people in an order nothing about the run decides.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Society {
    people: BTreeMap<Uid, Person>,
    /// What every relationship is run by ([`Rules`]): read by every brain
    /// that weighs one up, set from outside the world.
    rules: Rules,
}

impl Society {
    pub fn new() -> Society {
        Society::default()
    }

    /// Everybody in `entities` who keeps relationships — the people — as the
    /// spawn pass would have kept them: the house each spawned in, the job
    /// each has now — run by `rules`. What a loaded world starts from.
    pub fn of(entities: &Entities, rules: Rules) -> Society {
        let mut society = Society { rules, ..Society::new() };
        for (_, entity) in entities.iter_slots() {
            if entity.kind() == Some(EntityType::Human) {
                society.join(entity.uid(), entity.body().family());
                society.set_job(entity.uid(), entity.job().map(|job| job.kind));
            }
        }
        society
    }

    /// What relationships are run by.
    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    /// Run relationships by `rules` from now on.
    pub fn set_rules(&mut self, rules: Rules) {
        self.rules = rules;
    }

    /// Change one rule, kept in bounds ([`Rules::change`]); what it came to.
    pub fn change_rule(&mut self, rule: Rule, value: f64) -> f32 {
        self.rules.change(rule, value)
    }

    /// `uid` arrived, born into `family` (or none), with no job yet.
    pub fn join(&mut self, uid: Uid, family: Option<PropertyId>) {
        self.people.insert(uid, Person { family, job: None });
    }

    /// `uid` left the world.
    pub fn leave(&mut self, uid: Uid) {
        self.people.remove(&uid);
    }

    /// `uid` now does `job` (or nothing). Somebody who never joined is not
    /// somebody whose relationships anybody keeps, and is ignored.
    pub fn set_job(&mut self, uid: Uid, job: Option<JobKind>) {
        if let Some(person) = self.people.get_mut(&uid) {
            person.job = job;
        }
    }

    pub fn person(&self, uid: Uid) -> Option<Person> {
        self.people.get(&uid).copied()
    }

    /// Whether `a` and `b` are two people who spawned in the same house.
    pub fn are_family(&self, a: Uid, b: Uid) -> bool {
        a != b
            && matches!(
                (self.person(a).and_then(|p| p.family), self.person(b).and_then(|p| p.family)),
                (Some(x), Some(y)) if x == y
            )
    }

    /// Whether `a` and `b` are two people doing the same kind of job.
    pub fn are_colleagues(&self, a: Uid, b: Uid) -> bool {
        a != b
            && matches!(
                (self.person(a).and_then(|p| p.job), self.person(b).and_then(|p| p.job)),
                (Some(x), Some(y)) if x == y
            )
    }

    /// The least `a`'s relationship with `b` can be: the family floor for
    /// family, the colleague floor for colleagues, the higher of the two for
    /// both, 0 for anybody else.
    pub fn floor(&self, a: Uid, b: Uid) -> f32 {
        let family = if self.are_family(a, b) { self.rules.family_floor } else { 0.0 };
        let colleagues = if self.are_colleagues(a, b) { self.rules.colleague_floor } else { 0.0 };
        family.max(colleagues)
    }

    /// The level of `a`'s relationship with `b`, `points` being what `a`
    /// remembers of them ([`Relations::points`]): those or the floor,
    /// whichever is higher.
    pub fn level(&self, a: Uid, b: Uid, points: f32) -> f32 {
        points.max(self.floor(a, b))
    }

    /// Everybody, in uid order.
    pub fn iter(&self) -> impl Iterator<Item = (Uid, Person)> + '_ {
        self.people.iter().map(|(&uid, &person)| (uid, person))
    }

    pub fn len(&self) -> usize {
        self.people.len()
    }

    pub fn is_empty(&self) -> bool {
        self.people.is_empty()
    }
}

/// Somebody one person has a relationship with, as the relationship tab
/// lists them ([`acquaintances`]).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Acquaintance {
    pub who: Uid,
    /// The level ([`Society::level`]): points or floor, whichever is higher.
    pub level: f32,
    pub family: bool,
    pub colleague: bool,
    /// When they last chatted, in world seconds; `f64::NEG_INFINITY` never.
    pub chatted: f64,
    /// What they are to each other, in a word: family and colleagues first,
    /// whatever the level, then a friend, an acquaintance or a stranger by the
    /// level ([`Rules::standing`]).
    pub kind: &'static str,
}

impl Acquaintance {
    pub fn kind(&self) -> &'static str {
        self.kind
    }
}

/// Everybody `me` has a relationship with: its family and colleagues
/// ([`Society`]), whatever it remembers of them, and everybody else it
/// remembers ([`Relations`]) — highest level first, ties by uid. A walk of
/// everybody in the world: for the relationship tab, never a tick.
pub fn acquaintances(me: Uid, relations: &Relations, society: &Society) -> Vec<Acquaintance> {
    let of = |who: Uid| {
        let (level, family, colleague) =
            (society.level(me, who, relations.points(who)), society.are_family(me, who), society.are_colleagues(me, who));
        Acquaintance {
            who,
            level,
            family,
            colleague,
            chatted: relations.get(who).map_or(f64::NEG_INFINITY, |relation| relation.chatted),
            kind: if family {
                "family"
            } else if colleague {
                "colleague"
            } else {
                society.rules().standing(level)
            },
        }
    };
    let mut known: Vec<Acquaintance> = society
        .iter()
        .map(|(who, _)| who)
        .filter(|&who| society.floor(me, who) > 0.0 && relations.get(who).is_none())
        .chain(relations.iter().map(|relation| relation.who))
        .map(of)
        .collect();
    known.sort_by(|a, b| b.level.total_cmp(&a.level).then(a.who.cmp(&b.who)));
    known
}

/// How far a chat has got, for one side of it ([`Chat`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// It asked; it has not seen the other take it up yet. Still wandering.
    Inviting,
    /// Agreed by both: going over, waiting for the other to come, or talking.
    Talking,
    /// It has had its say and its points; it stays where it is until the
    /// other has had theirs, so a chat ends for both together.
    Done,
}

/// A chat one person is in, as it remembers it
/// ([`Memory::chat`](super::brain::Memory::chat)).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Chat {
    /// Who with.
    pub with: Uid,
    pub phase: Phase,
    /// Whether it is the one who walks over — whoever asked; of two who asked
    /// each other at once, the lower uid. The other waits where it is.
    pub leads: bool,
    /// When it came to this phase, in world seconds: what [`INVITE_PATIENCE`]
    /// and [`CHAT_PATIENCE`] are counted from.
    pub since: f64,
}

/// One person in a chat, as the rest of the crowd sees it this tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chatter {
    pub who: Uid,
    pub with: Uid,
    pub phase: Phase,
    /// The cell it stood in as the last tick ended.
    pub cell: Point,
}

/// **Who is chatting with whom this tick** ([`Chatter`]), rebuilt from the
/// crowd every tick before react reads it — the think step, which touches
/// every unit anyway, writes each one's chat into a slot of its own, and
/// [`Chats::rebuild`] gathers them — so never saved: a loaded world's first
/// tick writes it exactly as the saved world's would have.
///
/// Two lists of the same few people, one sorted by who and one by whom with:
/// "is my partner still talking to me, and where is it" and "is anybody
/// asking me" are each a binary search. Grows only when more people chat at
/// once than ever before.
#[derive(Clone, Debug, Default)]
pub struct Chats {
    by_who: Vec<Chatter>,
    by_with: Vec<Chatter>,
}

impl Chats {
    /// Everybody in `slots` who is in a chat, ready to be asked.
    pub fn rebuild(&mut self, slots: &[Option<Chatter>]) {
        self.by_who.clear();
        self.by_who.extend(slots.iter().flatten().copied());
        self.finish();
    }

    /// Sort both lists. Uids are unique, so the order is the same on every
    /// run however they were gathered.
    fn finish(&mut self) {
        self.by_who.sort_unstable_by_key(|chatter| chatter.who);
        self.by_with.clear();
        self.by_with.extend_from_slice(&self.by_who);
        self.by_with.sort_unstable_by_key(|chatter| (chatter.with, chatter.who));
    }

    /// `who`'s chat, if it is in one.
    pub fn get(&self, who: Uid) -> Option<&Chatter> {
        self.by_who.binary_search_by_key(&who, |chatter| chatter.who).ok().map(|at| &self.by_who[at])
    }

    /// Everybody asking `to` for a chat and not yet answered, lowest uid first.
    pub fn invitations(&self, to: Uid) -> impl Iterator<Item = &Chatter> {
        let start = self.by_with.partition_point(|chatter| chatter.with < to);
        self.by_with[start..]
            .iter()
            .take_while(move |chatter| chatter.with == to)
            .filter(|chatter| chatter.phase == Phase::Inviting)
    }

    pub fn len(&self) -> usize {
        self.by_who.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_who.is_empty()
    }

    /// Say `who` is in `chatter`'s chat, or in none: for a world of one
    /// (`testing::World`), which has no think step over a crowd.
    pub fn set(&mut self, who: Uid, chatter: Option<Chatter>) {
        self.by_who.retain(|other| other.who != who);
        self.by_who.extend(chatter);
        self.finish();
    }
}

// --- saving ---

impl Phase {
    pub(crate) fn save(self) -> pb::ChatPhase {
        match self {
            Phase::Inviting => pb::ChatPhase::Inviting,
            Phase::Talking => pb::ChatPhase::Talking,
            Phase::Done => pb::ChatPhase::Done,
        }
    }

    pub(crate) fn restore(saved: i32) -> Result<Phase, Invalid> {
        Ok(match pb::known::<pb::ChatPhase>(saved, "chat phase")? {
            pb::ChatPhase::Inviting => Phase::Inviting,
            pb::ChatPhase::Talking => Phase::Talking,
            pb::ChatPhase::Done => Phase::Done,
        })
    }
}

impl Chat {
    pub(crate) fn save(&self) -> pb::Chat {
        let Chat { with, phase, leads, since } = *self;
        pb::Chat { with: with.raw(), phase: phase.save() as i32, leads, since }
    }

    pub(crate) fn restore(saved: &pb::Chat) -> Result<Chat, Invalid> {
        let Some(with) = Uid::from_raw(saved.with) else {
            return invalid("chat: with nobody");
        };
        if !saved.since.is_finite() {
            return invalid(format!("chat with {with}: since {}", saved.since));
        }
        Ok(Chat { with, phase: Phase::restore(saved.phase)?, leads: saved.leads, since: saved.since })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::clock::HOUR;

    /// The rules as checked in.
    fn rules() -> Rules {
        Rules::default()
    }

    fn person(n: u64) -> Uid {
        Uid::new(EntityType::Human, n)
    }

    #[test]
    fn the_chance_of_a_chat_rises_with_the_relationship() {
        let rules = rules();
        assert_eq!((rules.stranger_chance, rules.acquaintance_chance, rules.friend_chance), (0.10, 0.24, 0.40));
        assert_eq!((rules.acquaintance_over, rules.friend_over), (1.0, 45.0));
        assert_eq!(rules.chance(0.0), 0.10);
        assert_eq!(rules.chance(1.0), 0.10, "over one point, not at it");
        assert_eq!(rules.chance(1.5), 0.24);
        assert_eq!(rules.chance(45.0), 0.24, "over 45, not at it");
        assert_eq!(rules.chance(45.5), 0.40);
    }

    #[test]
    fn the_checked_in_rules_are_the_ones_asked_for() {
        let rules = rules();
        assert_eq!((rules.family_floor, rules.colleague_floor, rules.most, rules.nightly_loss), (30.0, 10.0, 100.0, 1.0));
        assert_eq!((rules.least_chat_gain, rules.most_chat_gain), (5.0, 10.0));
        assert_eq!(rules.chat_again_after(), HOUR as f64);
    }

    #[test]
    fn a_rules_file_names_every_rule_and_nothing_else_each_in_range() {
        assert!(Rules::from_yaml(DEFAULTS_YAML).is_ok());
        let without = DEFAULTS_YAML.replace("stranger chance: 0.10", "");
        assert!(Rules::from_yaml(&without).unwrap_err().contains("\"stranger chance\" is missing"));
        let unknown = format!("{DEFAULTS_YAML}\nenemy chance: 0.5\n");
        assert!(Rules::from_yaml(&unknown).unwrap_err().contains("enemy chance"));
        let certain = DEFAULTS_YAML.replace("stranger chance: 0.10", "stranger chance: 1.5");
        assert!(Rules::from_yaml(&certain).is_err(), "a chance over one");
        let backwards = DEFAULTS_YAML.replace("least chat gain: 5", "least chat gain: 50");
        assert!(Rules::from_yaml(&backwards).is_err());
        let high = DEFAULTS_YAML.replace("family floor: 30", "family floor: 300");
        assert!(Rules::from_yaml(&high).is_err(), "a floor over the most");
        let sure = DEFAULTS_YAML.replace("stranger chance: 0.10", "stranger chance: 1");
        assert_eq!(Rules::from_yaml(&sure).unwrap().stranger_chance, 1.0);
        assert!(Rule::ALL.iter().all(|rule| DEFAULTS_YAML.contains(&format!("\n{}:", rule.name()))));
    }

    #[test]
    fn a_rule_changed_in_play_is_kept_in_range_and_in_step_with_the_rest() {
        let mut rules = rules();
        assert_eq!(rules.change(Rule::StrangerChance, 1.0), 1.0);
        assert_eq!(rules.change(Rule::FriendChance, 7.0), 1.0, "a chance is a share");
        assert_eq!(rules.change(Rule::MinutesOfTalk, 0.0), 1.0, "a minute's talk at least");
        rules.change(Rule::LeastChatGain, 20.0);
        assert_eq!((rules.least_chat_gain, rules.most_chat_gain), (20.0, 20.0));
        rules.change(Rule::Most, 25.0);
        assert_eq!((rules.family_floor, rules.colleague_floor), (25.0, 10.0));
        assert_eq!(rules.change(Rule::NightlyLoss, f64::NAN), 1.0, "not a number changes nothing");
    }

    #[test]
    fn rules_come_back_from_a_save_as_they_were_and_an_older_save_has_the_defaults() {
        let mut rules = rules();
        rules.change(Rule::StrangerChance, 0.75);
        rules.change(Rule::MinutesBetweenChats, 90.0);
        assert_eq!(Rules::restore(&rules.save()).unwrap(), rules);
        assert_eq!(Rules::restore(&[]).unwrap(), Rules::default());
        let stranger = [pb::RelationshipRule { name: "enemy chance".into(), value: 0.5 }];
        assert!(Rules::restore(&stranger).is_err());
        let wild = [pb::RelationshipRule { name: "friend chance".into(), value: 3.0 }];
        assert!(Rules::restore(&wild).is_err());
    }

    #[test]
    fn a_chat_adds_to_the_level_and_is_remembered_for_an_hour() {
        let mut relations = Relations::new();
        let friend = person(7);
        assert_eq!(relations.points(friend), 0.0);
        let rules = rules();
        let hour = rules.chat_again_after();
        assert_eq!(relations.chatted(friend, 0.0, 6.0, 100.0, &rules), 6.0);
        assert!(relations.chatted_lately(friend, 100.0 + hour - 1.0, &rules));
        assert!(!relations.chatted_lately(friend, 100.0 + hour, &rules));
        assert!(!relations.chatted_lately(person(8), 100.0, &rules), "only with whoever it was");

        // From the level, floor and all: a colleague chatted with for the
        // first time starts from ten.
        assert_eq!(relations.chatted(person(9), rules.colleague_floor, 5.0, 200.0, &rules), 15.0);
        // Never past the top.
        assert_eq!(relations.chatted(friend, 98.0, 10.0, 300.0, &rules), rules.most);
    }

    #[test]
    fn every_night_takes_a_point_off_everybody_and_forgets_whoever_reaches_nothing() {
        let mut relations = Relations::new();
        let (old, new) = (person(1), person(2));
        relations.pass_nights(1, 0.0, &rules());
        relations.chatted(old, 0.0, 2.5, 0.0, &rules());
        relations.chatted(new, 0.0, 9.0, 0.0, &rules());

        relations.pass_nights(1, 10.0, &rules());
        assert_eq!(relations.points(old), 2.5, "no night has gone by");
        relations.pass_nights(2, 20.0 * HOUR as f64, &rules());
        assert_eq!((relations.points(old), relations.points(new)), (1.5, 8.0));
        // Three nights at once, for somebody held still over them.
        relations.pass_nights(5, 80.0 * HOUR as f64, &rules());
        assert_eq!(relations.points(new), 5.0);
        assert!(relations.get(old).is_none(), "down to nothing and long since chatted with: forgotten");
        assert_eq!(relations.len(), 1);
    }

    #[test]
    fn somebody_chatted_with_just_before_midnight_is_still_remembered_after_it() {
        let mut relations = Relations::new();
        let stranger = person(3);
        relations.pass_nights(1, 0.0, &rules());
        relations.chatted(stranger, 0.0, 0.5, 1000.0, &rules());
        relations.pass_nights(2, 1000.0 + 60.0, &rules());
        assert!(relations.chatted_lately(stranger, 1000.0 + 60.0, &rules()), "the hour is still the hour");
    }

    #[test]
    fn family_and_colleagues_never_fall_below_their_floors() {
        let mut society = Society::new();
        let house = PropertyId::new(4);
        let (mother, son, neighbour, stranger) = (person(1), person(2), person(3), person(4));
        society.join(mother, house);
        society.join(son, house);
        society.join(neighbour, PropertyId::new(5));
        society.join(stranger, None);
        society.set_job(son, Some(JobKind::Guard));
        society.set_job(neighbour, Some(JobKind::Guard));
        society.set_job(stranger, Some(JobKind::Farmer));

        assert!(society.are_family(mother, son) && !society.are_family(son, neighbour));
        assert!(!society.are_family(stranger, person(9)), "two with no house are not a family");
        assert_eq!(society.level(mother, son, 0.0), 30.0);
        assert_eq!(society.level(mother, son, 40.0), 40.0);
        assert_eq!(society.level(son, neighbour, 0.0), 10.0);
        assert_eq!(society.level(neighbour, stranger, 3.0), 3.0);
        assert_eq!(society.floor(son, son), 0.0, "nobody is their own family");

        // Let go: a colleague no more.
        society.set_job(neighbour, None);
        assert_eq!(society.level(son, neighbour, 0.0), 0.0);
        society.leave(mother);
        assert_eq!(society.level(son, mother, 0.0), 0.0);
    }

    #[test]
    fn the_relationship_tab_lists_family_colleagues_and_friends_best_first() {
        let mut society = Society::new();
        let house = PropertyId::new(1);
        let (me, sister, colleague, friend, stranger, nobody) =
            (person(1), person(2), person(3), person(4), person(5), person(6));
        for (uid, family) in [(me, house), (sister, house), (colleague, None), (friend, None), (stranger, None), (nobody, None)] {
            society.join(uid, family);
        }
        society.set_job(me, Some(JobKind::Guard));
        society.set_job(colleague, Some(JobKind::Guard));
        let mut relations = Relations::new();
        relations.chatted(friend, 40.0, 10.0, 5.0, &rules());
        relations.chatted(stranger, 0.0, 0.5, 6.0, &rules());
        relations.chatted(colleague, 10.0, 5.0, 7.0, &rules());

        let known = acquaintances(me, &relations, &society);
        let listed: Vec<(Uid, &str, f32)> = known.iter().map(|a| (a.who, a.kind(), a.level)).collect();
        assert_eq!(
            listed,
            [
                (friend, "friend", 50.0),
                (sister, "family", 30.0),
                (colleague, "colleague", 15.0),
                (stranger, "stranger", 0.5),
            ],
            "and nobody it has nothing to do with"
        );
        assert_eq!(known[0].chatted, 5.0);
        assert_eq!(known[1].chatted, f64::NEG_INFINITY);
    }

    #[test]
    fn the_chats_index_finds_a_chatter_and_whoever_is_asking_somebody() {
        let (a, b, c, d) = (person(1), person(2), person(3), person(4));
        let at = Point::new(0, 0);
        let mut chats = Chats::default();
        chats.rebuild(&[
            Some(Chatter { who: c, with: b, phase: Phase::Inviting, cell: at }),
            None,
            Some(Chatter { who: a, with: b, phase: Phase::Inviting, cell: at }),
            Some(Chatter { who: d, with: b, phase: Phase::Talking, cell: at }),
        ]);
        assert_eq!(chats.len(), 3);
        assert_eq!(chats.get(c).map(|chatter| chatter.with), Some(b));
        assert!(chats.get(b).is_none());
        let asking: Vec<Uid> = chats.invitations(b).map(|chatter| chatter.who).collect();
        assert_eq!(asking, [a, c], "lowest first, and not whoever is already talking");
        assert_eq!(chats.invitations(a).count(), 0);

        chats.set(a, None);
        assert!(chats.get(a).is_none());
        chats.rebuild(&[]);
        assert!(chats.is_empty());
    }

    #[test]
    fn relations_come_back_from_a_save_as_they_were() {
        let mut relations = Relations::new();
        relations.pass_nights(3, 0.0, &rules());
        relations.chatted(person(5), 0.0, 7.5, 12.0, &rules());
        relations.set_points(person(2), 40.0);
        let (saved, night) = relations.save();
        assert_eq!(Relations::restore(&saved, night).unwrap(), relations);

        let mut twice = saved.clone();
        twice.push(saved[0]);
        assert!(Relations::restore(&twice, night).is_err());
        let mut too_many = saved;
        too_many[0].points = -1.0;
        assert!(Relations::restore(&too_many, night).is_err());
    }

    #[test]
    fn a_chat_comes_back_from_a_save_as_it_was() {
        for phase in [Phase::Inviting, Phase::Talking, Phase::Done] {
            let chat = Chat { with: person(6), phase, leads: phase == Phase::Talking, since: 33.0 };
            assert_eq!(Chat::restore(&chat.save()).unwrap(), chat);
        }
    }
}

/// Relationships in a running world: chats had by wanderers who meet, the
/// families the spawn pass makes, colleagues, and the nights.
#[cfg(test)]
mod in_the_world {
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::biology::ProcessId;
    use crate::sim::clock::{Clock, DAY, TIME_SCALE};
    use crate::sim::job::OFFICE_HOURS;
    use crate::sim::testing::{needy_human, World};
    use crate::sim::{process_game_state, GameEntity, GameState, Input};

    /// A world hour, in ticks of `run`.
    const HOUR_OF_TICKS: u32 = 1800;

    /// Tick `state` at 60Hz, and say what the log said, each line with the
    /// tick it was said on.
    fn run(state: &mut GameState, ticks: u32) -> Vec<(u64, String)> {
        let input = Input::new();
        let mut lines = Vec::new();
        for _ in 0..ticks {
            process_game_state(state, 1.0 / 60.0, &input);
            let tick = state.tick();
            lines.extend(state.log.drain().into_iter().map(|line| (tick, line)));
        }
        lines
    }

    /// Nothing for `uid` to do but wander: every need held where it is.
    fn only_wandering(state: &mut GameState, uid: Uid) {
        for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun, ProcessId::Energy] {
            assert!(state.set_process(uid, process, false));
        }
    }

    /// `count` people scattered over an open yard, only wandering.
    fn yard(count: i32, seed: u64) -> (GameState, Vec<Uid>) {
        let mut state = GameState::new(Map::new(Size::new(40, 40), FLOOR), seed);
        let crowd: Vec<Uid> = (0..count)
            .map(|i| state.spawn(EntityType::Human, Point::new(3 + (i % 5) * 8, 3 + (i / 5) * 8)))
            .collect();
        for &uid in &crowd {
            only_wandering(&mut state, uid);
        }
        (state, crowd)
    }

    /// Who chatted with whom, in the order the chats ended: `(who, with)`.
    fn chats(lines: &[(u64, String)]) -> Vec<(u64, String, String)> {
        lines
            .iter()
            .filter_map(|(tick, line)| {
                let (who, rest) = line.split_once(" chatted with ")?;
                // A uid reads `human:...`, so the colon after it is the one
                // with a space.
                let (with, _) = rest.split_once(": ")?;
                Some((*tick, who.to_string(), with.to_string()))
            })
            .collect()
    }

    #[test]
    fn wanderers_who_meet_chat_and_each_thinks_better_of_the_other() {
        let (mut state, crowd) = yard(15, 11);
        let lines = run(&mut state, 3 * HOUR_OF_TICKS);
        let had = chats(&lines);
        assert!(had.len() >= 4, "{} chats in three hours: {had:?}", had.len());
        // Both sides of a chat have their say.
        let mutual = had.iter().filter(|(_, who, with)| had.iter().any(|(_, a, b)| a == with && b == who)).count();
        assert!(mutual >= 4, "{mutual} of {} chats had by both sides", had.len());
        // ...and each thinks better of the other for it: strangers before,
        // a chat or two's worth after.
        let raised = crowd
            .iter()
            .flat_map(|&uid| state.acquaintances(uid))
            .filter(|known| known.chatted.is_finite())
            .inspect(|known| assert!(known.level >= state.society().rules().least_chat_gain, "{known:?}"))
            .count();
        assert!(raised >= 4, "{raised} relationships raised");
    }

    #[test]
    fn nobody_chats_with_the_same_person_twice_within_an_hour() {
        let (mut state, _) = yard(15, 23);
        let had = chats(&run(&mut state, 5 * HOUR_OF_TICKS));
        assert!(had.len() >= 6, "{} chats", had.len());
        for (i, (tick, who, with)) in had.iter().enumerate() {
            if let Some((again, ..)) = had[i + 1..].iter().find(|(_, a, b)| a == who && b == with) {
                assert!(
                    again - tick >= HOUR_OF_TICKS as u64,
                    "{who} chatted with {with} at tick {tick} and again at {again}"
                );
            }
        }
    }

    #[test]
    fn a_chat_is_the_same_chat_every_run() {
        let build = || yard(15, 5).0;
        let (mut a, mut b) = (build(), build());
        let (lines_a, lines_b) = (run(&mut a, HOUR_OF_TICKS), run(&mut b, HOUR_OF_TICKS));
        assert_eq!(chats(&lines_a), chats(&lines_b));
        let all = |state: &GameState| -> Vec<Vec<Acquaintance>> {
            state.entities().iter().map(|entity| state.acquaintances(entity.uid())).collect()
        };
        assert_eq!(all(&a), all(&b));
    }

    #[test]
    fn somebody_left_mid_chat_by_a_partner_who_leaves_the_world_gives_up_at_once() {
        let (mut state, _) = yard(15, 11);
        let input = Input::new();
        // Run until two have agreed a chat, and are at it.
        let (one, other) = loop {
            process_game_state(&mut state, 1.0 / 60.0, &input);
            assert!(state.tick() < 3 * HOUR_OF_TICKS as u64, "nobody agreed a chat");
            let agreed = state.entities().iter().find_map(|entity| {
                let chat = entity.chat().filter(|chat| chat.phase == Phase::Talking)?;
                let theirs = state.entities().get(chat.with)?.chat()?;
                (theirs.with == entity.uid() && theirs.phase == Phase::Talking).then_some((entity.uid(), chat.with))
            });
            if let Some(pair) = agreed {
                break pair;
            }
        };
        assert!(state.despawn(other));
        for _ in 0..3 {
            process_game_state(&mut state, 1.0 / 60.0, &input);
        }
        assert_eq!(state.entities().get(one).unwrap().chat(), None, "still chatting with somebody gone");
    }

    #[test]
    fn people_who_spawn_in_one_house_are_family_and_nobody_else_is() {
        let mut state = GameState::new(crate::sim::property::tests::house(), 3);
        let (mother, son) = (state.spawn(EntityType::Human, Point::new(4, 4)), state.spawn(EntityType::Human, Point::new(6, 5)));
        let stranger = state.spawn(EntityType::Human, Point::new(30, 30));
        let dog = state.spawn(EntityType::Dog, Point::new(5, 5));
        assert!(state.society().are_family(mother, son));
        assert!(!state.society().are_family(mother, stranger));
        assert!(state.society().person(dog).is_none(), "a dog keeps no relationships");
        assert_eq!(state.entities().get(son).unwrap().body().family(), state.properties().of(Point::new(4, 4)));

        let known = state.acquaintances(mother);
        assert_eq!(known.len(), 1, "{known:?}");
        assert_eq!((known[0].who, known[0].kind(), known[0].level), (son, "family", 30.0));
        assert!(state.acquaintances(stranger).is_empty());

        assert!(state.despawn(son));
        assert!(state.acquaintances(mother).is_empty(), "gone, and nothing remembered of them");
    }

    #[test]
    fn people_with_the_same_job_are_colleagues_until_one_is_let_go() {
        let mut map = Map::new(Size::new(24, 17), FLOOR);
        for desk in [Point::new(2, 2), Point::new(20, 12)] {
            crate::sim::testing::prop_at(&mut map, "computer", desk);
        }
        crate::map::utilities::serve_everything(&mut map, Point::new(0, 16), Point::new(23, 16));
        let mut state = GameState::new(map, 5);
        let (a, b) = (state.spawn(EntityType::Human, Point::new(4, 4)), state.spawn(EntityType::Human, Point::new(4, 4)));
        let jobless = state.spawn(EntityType::Human, Point::new(4, 4));
        assert_eq!(state.workplaces().worker(Point::new(2, 2), OFFICE_HOURS), Some(a));
        assert!(state.society().are_colleagues(a, b) && !state.society().are_colleagues(a, jobless));
        assert_eq!(state.society().level(a, b, 0.0), 10.0);
        assert_eq!(state.acquaintances(b).first().map(|known| known.kind()), Some("colleague"));

        assert!(state.dismiss(b));
        assert!(!state.society().are_colleagues(a, b));
        assert!(state.hire(b));
        assert!(state.society().are_colleagues(a, b), "hired back");
    }

    #[test]
    fn every_night_takes_a_point_off_what_a_person_thinks_of_everybody() {
        let mut world = World::new(Map::new(Size::new(9, 9), FLOOR));
        let mut human = needy_human(Point::new(4, 4), 0.0, 0.0, 0.0);
        let friend = Uid::new(EntityType::Human, 99);
        human.brain_mut().memory_mut().relations_mut().set_points(friend, 20.0);
        // A look's beat, at least once: the first notes the day.
        for _ in 0..8 {
            world.step(&mut human);
        }
        assert_eq!(human.relations().unwrap().points(friend), 20.0);
        world.clock = Clock::after_watching(2.0 * (DAY / TIME_SCALE) as f64);
        for _ in 0..8 {
            world.step(&mut human);
        }
        assert_eq!(human.relations().unwrap().points(friend), 18.0, "two nights gone by");
    }
}
