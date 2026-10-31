---
name: pets
description: Pet dogs in crowd2x - a Dog with an owner (Memory::companion on both), which stays within range of the owner and roams freely there (FollowGoal, Heel routine), cannot feed itself and is fed dog food bought from a fridge for $0.50 by its owner (DogCareGoal, Keep routine, Task::Feed, Effect::Feed, Feedings), has a belly (sim::pets::Gut: hunger, digesting, bowel) and relieves itself on the street (DefecateGoal, Task::Defecate, Effect::Defecate, dirt::DROPPING), is found by its owner and the owner by it through the per-tick Bonds index, adopted by one resident in eight of a district (GameState::adopt_dog / adopt_pets), saved, and tested. Maps every concept to the file that owns it. Use when changing dogs that belong to somebody, feeding them, their droppings, how close they stay, dog food, or adding a later pet behaviour.
---

# Pet dogs

A pet is a plain `Dog` (`sim/kinds.rs`) with an **owner**: a person who has it as its
**companion**. It stays near the owner, cannot feed itself, and leaves droppings on the street.
What it does *beyond* that (tricks, walkies, guarding, dying of hunger) is for later: nothing
happens to a dog left hungry yet but that it stays at its owner's heel and begs.

## The model

| Concept | Where | What |
| --- | --- | --- |
| companion | `brain/memory.rs` `Memory::companion` | a person's dog, or a dog's owner. Inline, saved (`pb::Memory.companion`, tag 10). `GameEntity::companion` / `set_companion`, `Brain::companion` / `set_companion` |
| `Gut` | `sim/pets.rs` | a dog's whole body: `hunger` (0-100, rises `100 / HOURS_TO_STARVING` = 16 h), `digesting` (a meal, still to pass), `bowel`. `fed()` (`MEAL` = 60 off hunger, into `digesting`), `advance(world dt)` (a meal passes into the bowel over `DIGEST_HOURS` = 3), `relieved()`. `HUNGRY` = 50 (its owner is moved to feed it, about every eight hours: three meals a day), `MUST_GO` = 50 (a meal reaches it 2.5 h after). Saved (`pb::Dog.gut`, tag 5; absent = a full belly). A dog is born 0-40 hungry |
| `Bond`, `Bonds` | `sim/pets.rs` | **who is bonded to whom this tick**, sorted by uid, rebuilt by the think step from `GameEntity::bond(ctx)` like `Chats` (`GameState::bonders`, `Think::bonds`; never saved): a person with a dog, and a dog with an owner, each saying where it stands; a dog adds its hunger and bowel. `get(uid)`, `partner(uid)` (the other side, if it is bonded back). **A dog's bond counts a meal served and not yet felt** (`Dog::bond`), so the owner does not set off for a second dinner on the tick after the first. The dead publish none; a frozen unit still does |
| `Feeding`, `Feedings` | `sim/pets.rs` | a meal served: `Effect::Feed` files it in the world step, the dog feels it in **its own react** next tick (`Dog::react`: `gut.fed()`), as a bite is felt (`sim::wounds`). Cleared at the next world step, held between ticks so **saved** (`pb::World.feedings`, tag 22), `Think::feedings` |
| the dog's mind | `brain/mod.rs` `Brain::dog()` | routines `stay_busy` + `Routine::heel()` (`HeelRoutine`), executors `WanderGoal`, `FollowGoal`, `DefecateGoal` (no catalogue executor: no person carries them, `Brain::dog` builds them, like a deathdog's). A dog with no companion only wanders, as it always did |
| `HeelRoutine` | `brain/routines.rs` | with a companion: `GoalId::Follow` at `FOLLOWING` 0.2; with `bowel >= MUST_GO`: `GoalId::Defecate` at `SQUATTING` 0.3 |
| `FollowGoal` | `brain/goals/follow.rs` | within `ROAM` (4) cells of its owner a dog **roams freely**: a spot of its own within range (a bounded few tries, `may_reach`, no route, **never a doorway**: a dog standing a while in one is an owner who cannot get by), walks to it, now and then stands a while (`PAUSE`); past it, it **comes**, to a spot within `CLOSE` (2) of the owner. **A hungry dog keeps within `CLOSE`.** The owner moves, so `keep_up` (brain step 3b) cuts a walk whose spot the owner has left behind, and a pause when the owner walks off. Its owner not being in the world (dead, gone) is a dog that forgets it and wanders. State saved as `pb::Executor::Wander` |
| `DefecateGoal`, `Task::Defecate`, `Effect::Defecate` | `brain/goals/defecate.rs`, `brain/tasks/defecate.rs`, `sim/effect.rs` | **only out of doors**: ground with no ceiling over it, no door in it and nobody's property. Where it stands if that will do, else a spot within `RADIUS` (6) cells, else `Blocked` (held off, so it goes on following its owner). The effect: `facilities.dirt.soil(at, dirt::DROPPING)` (0.6 earth, 0.2 wet: far over what a cleaner takes notice of, `dirt` skill) and `GameEntity::defecate()` empties the bowel. Task saved by name `"defecate"`, goal as `pb::Executor::Wander` |
| `KeepRoutine` | `brain/routines.rs` | a person with a dog whose bond says `hunger >= HUNGRY`: `GoalId::DogCare` at `DOG_CARE` 0.55 - over wandering, a chat and an order, under a night in bed (0.6), every need committed to and a shift |
| `DogCareGoal` | `brain/goals/dog_care.rs` | what eating does for a person, with a different dinner: walk beside the nearest fridge it may use, open it, **take dog food (`ItemKind::DogFood` = 8) out of it - which costs what a fridge charges, $0.50 (`economy.yaml` `dog food`, `economy::SOLD`)** via the existing `Effect::Buy` - close it; then, the dog not being a place, **only with the food in hand** it asks where the dog is now (its own bond), walks beside it and feeds it. A dog out of reach is gone after again, up to `MAX_CHASES` (6) a visit. Any other thing in hand is finished first; a body in hand is seen to first. Saved as `pb::Executor::DogCare` (tag 6: seek, chases) |
| `Task::Feed`, `Effect::Feed` | `brain/tasks/feed.rs`, `sim/effect.rs` | dog food in the hand and the dog (found through the owner's own bond, so the task holds no uid) within `FEED_REACH` (2) **every tick**; finishing takes the food out of the hand and asks `Effect::Feed { dog }`, granted to the dog's owner within reach: a `Feeding`, and the log line `"{owner} fed {dog}"`. Saved by name `"feed"` |
| adoption | `sim/mod.rs` `GameState::adopt_dog`, `adopt_pets` | `adopt_dog(owner)`: a new dog in a free cell within three of a living person who has none, bonded both ways, **with the owner's key** (`Body::home`: it goes where its owner may, and opens its locked doors). `spawn_from_spawners` then gives **one resident in `pets::ONE_IN` (8) who has a home** a dog, by the random half of its id, **after every arrival** (so no human's roll depends on a dog) and **only in a town** (`feature::TOWN_HOMES` homes or more, as the cleaners' district is), so a small test or QA map is never changed. Pets are the last arrivals: **arrival numbers of people do not move** (`qa` `select`) |

## Where the numbers are

`sim/pets.rs` (`HOURS_TO_STARVING`, `MEAL`, `HUNGRY`, `DIGEST_HOURS`, `MUST_GO`, `FEED_REACH`,
`ONE_IN`), `goals/follow.rs` (`ROAM`, `CLOSE`, `PAUSE`), `goals/defecate.rs` (`RADIUS`),
`routines.rs` (`DOG_CARE`, `FOLLOWING`, `SQUATTING`), `dirt.rs` (`DROPPING`), `economy.yaml`
(`dog food`).

## Seeing it

The unit panel's debug menu shows a dog's `owner`, `hunger` and `bowel` and a person's `dog`
(`Dog::debug_fields`, `Human::debug_fields`); the brains menu shows `stay with the owner`,
`following to` / `relieving at` and `look after the dog` / `dog care`. A hungry dog hovers
within two cells of its owner; the street keeps a brown spot where one went.

## Saved (`savegame` skill)

`pb::Dog.gut` (5), `pb::Memory.companion` (10), `pb::World.feedings` (22), `pb::Executor::DogCare`
(6), the tasks `"defecate"` and `"feed"` by name, `GoalId::DogCare` = 26, `Follow` = 27,
`Defecate` = 28, `ItemKind::DogFood` = 8 (by name). The routines `heel` and `keep` keep nothing.
Saves from before read as no pets.

## Tests

- `sim/pets.rs`: the gut (hunger, digesting, bowel; saved), bonds and meals found by who.
- `brain/goals/follow.rs` (`testing::World`, `pets::rig`): roams within range and never beyond, never stops in a doorway (seen failing without the check),
  comes from far away, **keeps up with an owner who walks away** (it stood out its pause while the
  owner walked off, 14 cells: seen failing), a hungry dog keeps close, forgets an owner who is
  gone, saved.
- `brain/goals/defecate.rs`: fouls the street where it stands (leaving more than a cleaner's 0.15),
  goes out of a roofed room to do it, held off with no open ground, saved.
- `brain/goals/dog_care.rs`: buys dog food for fifty cents and feeds a hungry dog, **no second dinner on
  the tick after** (seen failing without the bond counting the meal), a full dog is not fed, no fridge
  is held off, saved. `brain/tasks/feed.rs`, `brain/tasks/defecate.rs`.
- `sim::tests`: adoption bonds both ways and refuses where it cannot; **a district gives one in eight
  a dog beside them with their key**; a hungry dog is fed and hours later fouls the street, in a
  `GameState` with a powered fridge.
- `save::tests::a_pet_dog_saved_with_a_meal_inside_it_carries_on_exactly` (seen failing without the
  gut, and without the meals served restored).
- QA `qa/pets.json`: the first pet of a district's debug and brains menus.
