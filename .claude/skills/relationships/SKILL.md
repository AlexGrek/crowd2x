---
name: relationships
description: Relationships in crowd2x - what each person thinks of the people it knows (points per person in its own memory, a point lost to everybody each night), families (everybody who spawned in one house, floor 30) and colleagues (the same kind of job, floor 10) kept in sim::relationships::Society, chats started by wanderers who see each other (a chance by level, 10/24/40%), agreed with no brain reading another through the per-tick Chats index, walked to and talked by ChatGoal and the Talk task, never repeated within an hour, the relationships.yaml rules and the QA `relationships` step that changes them, saving it all, and the unit panel's relationship tab. Maps every concept to the file and function that owns it. Use when changing relationships, families, colleagues, chats, their chances or other rules, relationships.yaml, the relationship tab, or when people stop to talk (or never do).
---

# Relationships

People get on. Each person keeps **points** for everybody it knows; the **level** of a
relationship is those points or its **floor**, whichever is higher. Family never falls below
30, colleagues below 10. **Wanderers who see each other may chat**, and each chat adds 5-10 to
what each side thinks of the other. Every night takes a point off everybody's points. Every
number is a **rule** in `relationships.yaml`.

Plain Rust in `src/sim/` like the rest of the simulation; the unit panel draws it.

## Where everything is

| Concept | Owner |
| --- | --- |
| The rules (every number), their defaults, YAML parsing, `change` in play, save | `sim/relationships.rs`: `Rules`, `Rule` (name, range), `DEFAULTS_YAML = include_str!("../../relationships.yaml")` |
| One person's points and last chat per person, the nightly loss | `Relations` (sorted `Vec<Relation>` + `night`), boxed in `brain::Memory::relations` |
| The chat a person is in | `relationships::Chat { with, phase, leads, since }`, inline in `Memory::chat` |
| Family (spawn house) and job per person, the rules | `Society` (`BTreeMap<Uid, Person>` + `Rules`), `Facilities::society`, `Think::society`, `GameState::society()` |
| Who is chatting with whom this tick | `Chats` (two sorted lists of `Chatter`), `Facilities::chats`, `Think::chats` |
| Asking somebody just seen; taking an invitation up; keeping watch | `brain/social.rs`: `notice` (in attention's `take_in`), `tend` (every tick) |
| Wanting the chat once agreed | `routines.rs`: `ChatRoutine` ("keep company"), `CHATTING = 0.15` |
| The walk over, the talk, the points | `brain/goals/chat.rs`: `ChatGoal` (`GoalId::Chat` = 23, 💬) |
| One talk | `brain/tasks/talk.rs`: `Talk` (`TimedTask` "talk": the other's cell, seconds) |
| A new game's rules from the file | `game/relationships.rs`: `load_rules` (`CROWD2X_RELATIONSHIPS` or `relationships.yaml`), called in `actors::build_world` |
| Changing a rule in play | `Command::SetRelationshipRule { rule, value }` → `GameState::set_relationship_rule` (clamped, logged `relationships: <name> is <value>`) |
| The tab | `game/unitpanel.rs`: `Menu::Relationship`, `relationship_block`, from `GameState::acquaintances(uid)` → `relationships::acquaintances` |

## The level, the floors, the nights

- `Society::level(a, b, points) = max(points, floor(a, b))`; `floor` is the family floor if
  `a` and `b` spawned in one house (`Body::family`, set by `GameState::spawn` from
  `Properties::of(spawn cell)`; a district's residents spawn in their house), the colleague
  floor if both have the same `JobKind`, the higher of the two for both, 0 otherwise. Points
  never go below 0 or above `most`.
- **Floors are applied when read**, never stored: `Relations` keeps raw points, the nightly
  loss takes them towards 0, and the level stays at the floor. A chat raises the points to
  `level + gain`, floor included.
- **The nightly loss** (`Relations::pass_nights`) runs on the unit's `Priority::Med` beat (every
  7th tick, step 0), comparing `Clock::day()` with the night last taken: a unit frozen over
  three nights loses three points at once; the first call only notes the day. Somebody down to
  0 and not chatted with lately is dropped from the list (a floor comes back with nothing
  stored).
- `Society` is kept by the spawn pass: `join` in `GameState::spawn` (people only),
  `set_job` in `give_a_job`, `dismiss` and `effect::died`, `leave` in `despawn`. **Not saved**:
  `GameState::restore` rebuilds it with `Society::of(entities, rules)`; its rules are saved.

## A chat, step by step

No brain reads another, so a chat is agreed through the crowd's `Chats`, which the **think
step gathers every tick** (`GameEntity::chatter`, into the slot buffer `GameState::chatters`,
then `Chats::rebuild`, before react) — like `Panics`, never saved.

1. **Asking** (`social::notice`): a person whose top goal was `Wander` last tick, in no chat,
   notices a new face (attention's `take_in`, so one roll per encounter, not per look) — a
   person not already in a chat, whose cell it may use (`Think::may_use`) and reach
   (`Navigation::may_reach`, O(1)), not chatted with within `minutes between chats` — and rolls
   `Rules::chance(level)` (stranger / acquaintance over 1 / friend over 45) on
   `tick_rng(me, tick ^ other)`. Success: `Chat { Inviting, leads: true }`. It keeps wandering:
   the routine wants nothing for an invitation.
2. **Taking it up** (`social::tend`): a wanderer in no chat finds an invitation addressed to it
   (`Chats::invitations`, lowest uid first, not from somebody chatted with lately) and
   remembers `Chat { Talking, leads: false }`. `ChatRoutine` raises `Chat` over wandering; the
   walk halts.
3. **Agreed**: the asker sees the other `Talking` with it and becomes `Talking`, leading. Two who
   asked each other at once both see the other `Inviting` and agree; the lower uid leads.
4. **The walk and the talk** (`ChatGoal`): the leader `move_beside`s the other's cell (from
   `Chats`), retrying a body in the way `PATIENCE` times; the other waits (`LISTEN_SECONDS`).
   Within a step (Chebyshev 1) of a partner who has agreed, each queues its own `Talk` for
   `minutes of talk`. `Talk` checks every tick that its partner — found by uid through its own
   entry in `Chats`, so a partner still taking its last step does not break it — is in the chat
   (talking or done) and within a step.
5. **Done**: a talk that ends raises the points (`Relations::chatted`, gain from
   `Rules::chat_gain`), records `chatted` (the hour rule), logs
   `"{me} chatted with {other}: before -> after"` (`log.related`), and marks the side `Done`. It
   stays (waiting) until the other is done too, then both forget the chat on the same tick.

**What ends a chat early** (`social::tend` and `ChatGoal::deprioritized`): the partner gone from
`Chats` (busy, frozen, dead, despawned — a despawned unit's slot is cleared in `despawn`, and
holes are written `None`); something else in charge (an invitation by somebody no longer
wandering; an agreed chat whose goal was not top on any tick after the one it was agreed — a
need, or the goal held off); no answer within `minutes to answer`; no end within `minutes to
meet`; the other not finishing within `minutes to wait for the other`; the leader finding no way
over (`Blocked`).

## The rules (`relationships.yaml`)

Flat, one number per rule, every rule named and nothing else (`Rules::from_yaml` refuses a
missing or unknown name, a value out of `Rule::range`, a floor over `most`, the least gain over
the most). The checked-in file is the defaults (`Rules::default`), embedded at build time; a new
game reads the file (`CROWD2X_RELATIONSHIPS` names another) and falls back on the defaults with a
logged warning. **A saved game keeps its own rules** (`pb::World.relationship_rules`, tag 20,
by name; a rule not named is its default, an unknown one refused).

| Rule | Default |
| --- | --- |
| `most` | 100 |
| `family floor` / `colleague floor` | 30 / 10 |
| `nightly loss` | 1 |
| `stranger chance` / `acquaintance chance` / `friend chance` | 0.10 / 0.24 / 0.40 |
| `acquaintance over` / `friend over` | 1 / 45 |
| `least chat gain` / `most chat gain` | 5 / 10 |
| `minutes of talk` | 15 |
| `minutes between chats` | 60 |
| `minutes to answer` / `minutes to meet` / `minutes to wait for the other` | 2 / 60 / 5 |

A new rule: a `Rules` field, a `Rule` variant with its name and range (and `get`/`set` arms),
its line in `relationships.yaml` (the embedded-defaults test fails without it), and a reader.

**QA**: `{"relationships": {"stranger chance": 1.0}}` sends `Command::SetRelationshipRule` per
name (an unknown name fails the step), in force from the next tick; the log says
`relationships: stranger chance is 1`. `qa/relationships.json` sets every chance to 1 so
strangers in a yard are sure to chat.

## Saving

`pb::Memory.chat` (5), `.relations` (6, `pb::Relation { who, points, chatted? }`), `.night` (7);
`pb::Body.family` (8); `pb::World.relationship_rules` (20); `ChatGoal` is a `pb::Executor::Seek`
(retries); `Talk` is a `TimedTask` named `"talk"`. Older saves: nobody known, no chat, no
families, default rules. `Chats` and `Society` are rebuilt, not saved; the save tests'
fingerprint includes `GameState::society()`.

## The tab

`relationship` on the unit bar (the button row wraps at `BUTTON_ROW` canvas pixels so it
never runs off the canvas): the chat it is in (`asking for a chat:`, `chatting with`, `saying
goodbye to`), a summary (`family N  colleagues N  friends N  acquaintances N`), then up to
`RELATIONSHIP_ROWS` lines `name  level  kind`, best first, `(dead)` / `(gone)` for people no
longer about. Live: rewritten every frame. The brains menu shows `chat` and `knows N`.

## Tests

- `sim::relationships::tests` — rules (defaults, file refusals, change in range, save),
  chance thresholds, chat gain and the hour, nights, floors, the tab's listing, `Chats`.
- `sim::relationships::in_the_world` — a yard of wanderers chats (both sides, levels raised),
  nobody chats with the same person twice within an hour, determinism, a partner despawned
  mid-chat is given up on at once, families from spawn houses, colleagues until let go, the
  nightly loss through a brain.
- `sim::brain::social::tests` — every way `tend` keeps or drops a chat, the hour, invitations
  never made to somebody out of reach.
- `sim::brain::tasks::talk::tests`; `save::tests::a_crowd_saved_mid_chat_carries_on_exactly`;
  `determinism_survives_brains_and_biology_in_the_parallel_rounds` requires chats.
- `qa/relationships.json` — the tab: a family at 30, strangers who chatted.

Mutation-checked when written: the hour rule, the despawn slot clear, restoring a chat and
rebuilding the society each fail a test when broken.
