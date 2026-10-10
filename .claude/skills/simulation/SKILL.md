---
name: simulation
description: The crowd2x simulation core (src/sim/) - the GameState contract, process_game_state, Uids, the spawn and processing passes (think, move, react, world), FrozenEntities, occupancy, Commands and freezing, inventories and items, the world clock and TIME_SCALE, the entity arena, determinism and the lock-free log. Use before writing anything simulation-shaped, adding an entity kind, Intent, Command or Effect, changing movement or collisions, items or carrying, or time. For brains and bodies use brain-engineer.
---

# The simulation

The game runs here, and **Bevy is only a renderer**: it draws sprites and UI, reads the
devices, runs the shaders, and does not decide anything. Same rule as the map, one level
up and over the state that changes — **plain Rust, no `bevy::` imports**, testable with
`cargo test` and no `App`.

Four things define the shape, and they are the contract:

- **One super-object.** `GameState` is the whole game: the `map`, the `entities`, and a
  `log`. Not a set of resources, not a plugin — one value you can construct in a test.
- **One function.** `process_game_state(&mut GameState, dt, &Input)`. That is the entire
  entry point. The name says `-> GameState`; taking `&mut` is the honest implementation of
  it, because returning a fresh state would copy the map, the log and the id allocator
  sixty times a second to express a change that touches only the entities. The semantics
  that were actually wanted — a tick is a pure function of the previous state — come from
  the pass split, not from the signature.
- **Entities are addressed by `Uid`, never by Bevy's `Entity`.** 64 bits: the top byte is
  the `EntityType`, the low 56 are random and unique. The type in the id means a log line
  or a debugger says *what* something is with no lookup; the randomness means an id can be
  written to a file and still mean the same thing after a reload, which an `Entity` index
  cannot. "Guaranteed unique" is a retry against the live set, not a hope about 56 bits.
- **An actor is not an entity.** Entities are for things that draw. `game/actors.rs`
  spawns sprites *from* the simulation; the simulation has never heard of them.

A tick is **two passes**, and which one may write the entity table is the design:

| | writes the entity table | allocates | parallelisable |
| --- | --- | --- | --- |
| `spawn_pass(&mut GameState, &Input)` | **yes** — the only thing that does | yes | no |
| `process_pass(&mut GameState, dt)` | **no** | no | that is what it is for |

The **spawn pass** drains `Input`'s spawns and despawns. It runs first, so an entity
spawned this tick thinks this tick, and it is the only pass that may resize the table.

The **processing pass** advances every entity and adds or removes none. It is handed a
`FrozenEntities` — the table with its membership frozen, offering no `insert` and no
`remove` — so **that is a guarantee the compiler holds, not a rule to remember.** Inside
it are four steps:

1. **think** — read-only over the entities and the map, each returning an `Intent` into a
   slot-indexed buffer.
2. **move** — single-threaded, slots ascending, draining that buffer. **An entity does not
   decide where it ends up; the world does.** A step is granted only if the destination
   cell is passable terrain *and* unoccupied, and otherwise the move is **cancelled
   outright** — not trimmed, not slid along the obstacle — and what stopped it goes into a
   second slot-indexed buffer as a `MoveOutcome`, carrying the blocking entity's `Uid` when
   the obstacle was one. This is the one step that cannot be parallelised, and that is
   what it is for: two entities stepping into the same empty cell on the same tick is the
   race it exists to settle, and slot order settles it the same way on every run.
3. **react** — the second thinking round, once the whole crowd has moved. Each entity is
   handed its own outcome and revises its plan, writing only to itself. Answering a
   collision inside the move loop would mean reacting to a world that is halfway through
   the tick, with the later slots not moved yet. It may also hand back an `Effect` — a
   request for a change outside itself — `Effect::Fridge { at, open }`, or `Effect::Door { at, key }` (hold a door open; the `doors` skill) — into a
   third slot-indexed buffer, since **writing only to itself** rules out flipping a fridge's
   door directly: that is a fact about the world, read by every entity beside it, not about
   the one entity that opened it.
4. **world** — single-threaded, slots ascending, like move, and for the same reason: two
   units opening and closing one fridge on the same tick is a race, and this is what settles
   it the same way on every run. Every `Effect` is applied (`fridges.set_open`), the slot
   reset to `Effect::None`, and then `Fridges::advance` moves every fridge's temperature on
   by the tick's worth of world time — see `sim::world_step` and "Features" in `brain-engineer`'s `architecture.md`.

**Passability is at the tile level, on the centre cell**, and it is two layers asked in
order: `map`'s `PassabilityMap` for the terrain and the furniture, then `sim/occupancy.rs`
for the crowd —
`Occupancy` is the dynamic layer the passability map's docs always said belonged
elsewhere. One entity to a cell, `claim` naming the occupant when it refuses. **Movement
therefore never creates an overlap; spawning can** — an entity is placed exactly where it
was asked for, so two can share a cell, and `release` only ever clears a cell whose
occupant is the entity leaving it, which is what keeps that contained.

**Freezing is the one thing done to an entity from outside it.** `Command::Freeze` rides
the same queue as a spawn and is applied in the spawn pass, so a freeze asked for this tick
is in force before anything thinks; the think step then hands a frozen entity an
`Intent::Idle` instead of asking it, and the react step skips it too — its brain would
find nothing running and ask for work every tick, a route searched for nothing each time.
That costs a bool per entity and means a frozen entity is not deciding things nobody will
carry out; no time passes for it, its biology included. It keeps its cell and its goal, so
letting it go again carries on rather than starting over. `GameEntity::debug_fields` is the
other half of that pair — what a kind would tell a debugger about itself, allocating
freely because it is asked about the one entity somebody has selected and never in a tick.

### What a unit carries (`sim/inventory.rs`, `sim/item.rs`)

An `ItemKind` says **what it is made of** — `nutrition`, `hydration`, and a `mass` in
kilograms and a `volume` in litres — and never what that does to a body or to whoever is
carrying it. `Inventory` is a unit's **hand plus what it has stowed**, inline and `Copy` on
`Human`, and the two slots are counted differently on purpose:

|  | the hand | stowed |
| --- | --- | --- |
| **mass** (kg) | counted | counted |
| **volume** (litres) | **not** counted | counted |

**Weight is carried wherever it is**, so a full hand leaves less that can be stowed;
**space is about packing things away**, and a hand is not a shelf. Both limits genuinely
bind, which is why there are two of them: food is bulky for its weight and water is dense,
so a person runs out of *space* at twelve meals and out of *strength* at twenty glasses
(`Capacity::HUMAN` is 10kg and 12L, per unit, so a bag or a build can differ later).

Two rules hold the rest together:

- **The limits govern stowing and never the hand.** `stow` refuses what will not fit;
  `set_hand` refuses nothing. A hand that could be refused would deadlock a hungry unit
  whose pockets were full, and nothing in the brain could say why —
  `a_human_loaded_to_its_limits_can_still_pick_food_up_and_eat_it` is that guarantee.
- **It is a count per kind, not a list of things.** `ItemKind` is fieldless, so two
  portions of food are the same item in every way the simulation can tell apart: one `u8`
  per kind, no allocation in a tick, and no slot count imposing a third limit nobody asked
  for. An item that needs state of its own is a change to `ItemKind` first.

A task is the only thing that writes it (`TaskCtx::inventory`); a goal only reads it
(`GoalCtx::inventory`), which is what keeps food arriving in a hand when a `TakeItem`
*finishes* rather than when a goal hears that it did. **Nothing stows anything yet** — the
brain takes into a hand and consumes from it, so a unit walks past a fridge with food in
its pockets, and `Command::GiveItem` (a QA script's `give`) is the only way in. A goal that
fetches and puts away is what closes that, and it needs a task that unstows. The hand is
the half a viewer can see — `game/held.rs` draws what is in it — and `Command::Hold` (a QA
script's `hold`) fills or empties it from outside, since the brain only ever fills it for
the length of a meal.

### What a second is (`sim/clock.rs`)

**A second of watching is two minutes of world** (`TIME_SCALE`), so an hour goes by in
thirty seconds and a day in twelve minutes. `GameState::clock` is that time of day,
derived from `elapsed` rather than counted beside it, and the HUD's left corner shows it.

Which leaves a tick measuring two different things, and the unit a duration is written in
says which:

- **What is watched happening** is on the watched clock: `Think::dt`, the fixed timestep.
  A walk crosses cells at a pace a person can follow, and `WAIT_FOR_A_GAP` is half a
  second of standing aside for somebody. Scaling these would make a crowd teleport — a
  tick is two minutes of world, and nobody should cross a room between two frames.
- **What the world's clock governs** is on `Think::game_dt`, a hundred and twenty times as
  much: the biology, and nothing else so far. Getting hungry takes hours, and hours are
  what that clock counts.

A duration that is *watched* but means something in world time is written as
`watched(15.0 * MINUTE)` — a quarter of an hour of eating, seven and a half seconds of
watching it — so even those say what the world thinks is going on. The game speed
multiplies how many *ticks* happen, so it moves both clocks together and 4x is the same
world watched faster.

`Think` also carries the time of day, `Think::clock` — one reading of the world's clock per
pass, shared by the whole crowd — and `Clock::is_night` (22:00 to 06:00, across midnight) is
the one question a routine asks of it so far.


Freezing membership is what makes the rest work. The intent buffer is indexed by slot and
sized once, in the spawn pass; a `Vec` that grows mid-tick moves its contents and
invalidates every index into it. And structural mutation is the one thing that genuinely
cannot be parallelised — two threads moving different entities never conflict, two threads
pushing to one `Vec` always do — so the pass that wants to run across threads is the pass
that is not allowed to change the table. An entity that needs to remove itself does not get
a back door: it returns something the *next* spawn pass acts on.

**The intent buffer is the double buffer.** Think reads entities and writes intents, move
reads intents and writes entities; the two never touch the same memory in the same
direction. That is what makes the read phase safe to parallelise, and it costs no entity
clone. Determinism follows from it plus a per-entity per-tick seeded RNG: same seed and
same input, same world after a thousand ticks, and there is a test that says so.

**Lock-free means there is nothing to lock**, not that there are clever atomics. The one
thing many threads genuinely write is the `log` — a bounded `ArrayQueue<String>` whose
`push` takes `&self`, evicts the oldest when full rather than growing, and counts what it
dropped. The renderer (`game/logview.rs`) is the only reader and it drains.

The store is a **dict over a dense arena**: `get(uid)`, `insert`, `remove`, `iter`, backed
by a `Vec` of slots plus a `HashMap` index. Callers look entities up by id; the tick walks
the `Vec` in slot order and hashes nothing. Each insert is stamped with a serial, so `in_spawn_order` can
answer "the third entity to arrive" — slot order cannot, since a despawn leaves a hole the
next spawn fills. Removal leaves a **tombstone** rather than
`swap_remove`-ing, because a slot index has to stay stable — the intent buffer is indexed
by it, and reordering the tail on every despawn would make iteration order, and so the
simulation, non-deterministic.

Positions are in **cell units, not pixels**: `(3.5, 2.5)` is the middle of cell `(3, 2)`,
and `center_position()` is `position.floor()`, derived so the two cannot disagree. How
many screen pixels a cell is drawn at is a fact about the art, and `actors.rs` is where it
is applied. For the same reason nothing here knows what a human looks like — it supplies
an `appearance_seed`, and `characters/` decides which PNGs that means.
