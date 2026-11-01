---
name: news
description: The news in crowd2x - stories (a person's death with its case - who, job, cause, where, when - bad news; every 100 of anything made, a production record, good news) filed in sim::news::News, the propagandist (a noble's job, the propaganda computer in the palace, noon to four every day, Paperwork::News, Task::write_news, Effect::Publish) who makes each into an article on everybody's phone for 24 hours, reading it on a phone (sim::news::read from Brain::react step 6a: one unread article a look, else a little fun), good news mending the mind and bad news wearing it down and adding +10 obedience for 12 hours (news::obedience, Memory::obedient_until), StoreDef::made, the archive that keeps every article for good and the game screen's news menu that pages through it (game::news), saving, and the tests. Maps every concept to the file that owns it. Use when changing the news, the propagandist, what is newsworthy, phones, obedience from bad news, or the news menu.
---

# The news

What is on everybody's phone. Plain world state (`Facilities::news`, `Think::news`,
`GameState::news()`), written only in the world step, read by brains in react, saved.
**Nothing is ever let go**: every story waits until written up, every article stays in the
archive for good, for the player to read in the news menu.

| Concept | Where | What |
| --- | --- | --- |
| `Story` | `sim/news.rs` | `Death(Death)` (bad) or `Record { item, total }` (good, `total` a multiple of `RECORD` = 100). Not `Copy` (a death owns strings). `headline()` for the log, the reader and the menu: `Ann Lee, a guard, died of thirst at 40, 22 on day 3 Wed  14:05` (the uid where there is no name), `a record: 200 steel made` |
| the case of a death | `news::Death` | `dead` (uid), `name` (`GameEntity::name`, a person's identity), `job` (what it did when it died), `cause` (`biology::health::cause_of_death` or `deathdog bites`, as a string), `at` (the cell it fell in), `when` (world seconds: `Effect::Died { cause, when }` carries the clock from the react that died, since `apply` has none). `news::time_of(seconds)` formats a time |
| filing a death | `sim/effect.rs` `died` | every **human** death, before the job is let go: `news.report(Story::Death(..))`. A dog's or a deathdog's is not news |
| production records | `sim/effect.rs` `Effect::Deposit` arm, `News::made` | pieces into a store whose `StoreDef::made` is true (`sim/feature.rs`: the collectors' scrap bin, the steel pallet, the grain bin, the meal pallet, the fish bin, the outpost store) count per `ItemKind`; crossing the next 100 files a record (one for a jump past two). Stores things are only carried on to (rack, hopper, superfridge) say `made: false`, so nothing is counted twice. A new store needs the column |
| waiting | `News::stories` | oldest first, **however many**: with nobody to write them up they wait, and the backlog goes up oldest first once somebody does |
| the propagandist | `sim/job.rs` `JobKind::Propagandist` (22) | workplace `FeatureKind::PropagandaComputer` (35, `"propaganda computer"`, beside, **needs power**), `GoalId::Work`, `GUARD_COMMUTE`, `PROPAGANDA_HOURS` (12-16 every day: four hours), `noble: true` (a noble's while any lives), uniform `"uniform propagandist"` (crimson tunic, black armband, gold star), wage 60, `paperwork: Some(Paperwork::News)` |
| writing | `brain/goals/work.rs`, `brain/tasks/write_news.rs` | `WorkGoal::plan` queues `Task::write_news(desk, NEWS_SECONDS)` (30 world min, a `Work` stretch at the computer) while `news.waiting() > 0`, otherwise an hour's `"write copy"` stretch (`STRETCHES`). Over → `Effect::Publish { when }` |
| publishing | `News::publish` | the oldest story becomes an `Article { id, story, published }` (ids from 1, in order), logged `important`: `the news (bad): Ann Lee, a guard, died of ...`, and **kept in `News::articles` for good** (the archive) |
| on the phones | `News::current(now)`, `News::unread(read, now)` | articles younger than `SHELF_LIFE` (one world day). The archive is in id and time order, so the first one on the phones and past `read` is found by halving (`partition_point`), however long the archive grows |
| reading | `sim::news::read`, called from `Brain::react` step 6a | when `Task::phone` ends in success (the look seen through — `PhoneGoal`, `StayBusyRoutine`, the `brain-engineer` skill): the oldest current article numbered past `Memory::news_read` is read — `Event::News { good }` (`biology/mind.rs`: `GOOD_NEWS` +3, `BAD_NEWS` -3 mental health; **no fun**), `news_read` set, logged `"{uid} read the news: ..."`; bad news also sets `Memory::obedient_until` = now + `BAD_NEWS_LASTS` (12 h). Nothing unread → `Event::Phoned` (`PHONE_FUN` fun). One article a look |
| obedience | `news::obedience(body, memory, clock)`, `GameEntity::obedience(clock)` | `Body::obedience()` + `BAD_NEWS_OBEDIENCE` (10) while shaken, capped at 100. `sim::sound::take_in` (a guard's GET BACK IN!, obeyed over 60) reads it; the debug menu shows `75 / 100 (bad news, own 65)` |
| the palace | `map/district/palace.rs` | `PROPAGANDA_COMPUTER` in the far wall `ACCOUNTANT_OFFSET` cells left of the throne (the accountant's is right). Map rows: `map/props.rs` (blocking), `map/utilities.rs` (power), `editor/props.rs` (the computer's art), `lighting/scene.rs` (screen glow) |

## The news menu (`game/news.rs`)

`news`, last in the world's second corner row (after `find`), opens an **opaque** full-screen
modal (`NewsMenu { root, page }` so `leave` closes it on `esc`; its own `Scope`; `newer`,
`older`, `close` as nav buttons). Title `the news`, a status line (`12 published   2 on the
phones   1 waiting for the propagandist   page 1 of 2`, or `nothing published yet ...`) and
`PAGE` (8) lines, **newest first**: `day 3 Wed  13:30  bad   <headline>` in red, good news in
green, wrapped at 92% of the canvas. Rewritten every frame it is open, written only on a change,
so a new article appears at the top while it is open. `describe(news, now, page)` makes the text
(unit-tested), `pages(news)` counts them. QA `qa/news.json`.

## Saving

`pb::World.news` (tag 30, `pb::News`: stories, articles, `made` by item name, `next`),
`News::save`/`restore` (absent = no news; refuses an article numbered past `next` or out of
order in id or time, a record not of a hundred, an unknown item, a story both or neither, a
death without its cause, place or time, or in no such job). `pb::Story` keeps a death's case:
`cause` (4), `at` (5), `name` (6), `job` (7, the catalogue number), `when` (8). The whole
archive is saved. `pb::Memory.news_read`
(15) and `obedient_until` (16). `Effect::Publish` never outlives a tick.

## Tests

`sim/news/tests.rs`: records per item and per hundred; publish order, shelf life, read order,
the archive kept weeks on; 500 stories all waiting; a death's headline with and without a name;
save round trip and refusals; a death filed with its whole case (name, job, cause, cell, time);
the bin counts and the
hopper does not; reading — bad news (mind, obedience for 12 h, no fun), good news, then fun;
obedience capped; the propagandist off duty writes nothing and on duty writes both stories up
then works a plain hour. `brain/tasks/write_news.rs` (`Rig`): the effect only when over and only
with a story; a dark computer. `brain/tasks/phone.rs`: the task alone gives no fun.
`map::district::palace`: one propaganda computer, powered, off the carpet.
`sim::tests::everybody_in_a_district_moves_into_a_bed_of_their_own_house`: one propagandist in
the palace. `save::tests::the_news_saved_while_it_is_written_and_read_carries_on_exactly`
(seen failing with the news not restored). `game/news.rs`: pages newest first, colours, the
last page past the end, nothing published. QA `qa/news.json`: a powered newsroom (a crate on
the box, so no electrician), the propagandist with thirst and hunger off, a jobless resident
who dies of thirst for real overnight, the menu after day 3 reading `1 published   0 on the
phones` and `died of thirst at`, `esc` closing it in the game.
