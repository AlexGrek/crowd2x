//! What every simulated thing is: [`GameEntity`], and the [`Body`] each one
//! carries.
//!
//! # Positions are in cells, not pixels
//!
//! [`Body::position`] is world-space in **cell units** — `(3.5, 2.5)` is the
//! middle of cell `(3, 2)`. The simulation already addresses everything in
//! cells ([`Point`], [`crate::map::PassabilityMap`]), and how many screen
//! pixels a cell is drawn at is a fact about the art, which this module has no
//! business knowing. `src/map/` draws the same line — "art is not in here" —
//! and the Bevy adapter multiplies by the tile size on its way to a
//! `Transform`.
//!
//! [`GameEntity::center_position`] is the cell the entity is standing in,
//! which is `position.floor()`. It is derived rather than stored so the two
//! can never disagree about where something is.

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};

use super::kinds::Facing;
use super::property::PropertyId;
use super::uid::{EntityType, Uid};
use super::{Effect, Intent};

/// The state every entity has, whatever else it has.
///
/// Kept as one embedded struct rather than as trait methods over private
/// fields so that the apply phase can write a position through
/// [`GameEntity::body_mut`] without every kind reimplementing a setter.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Body {
    uid: Uid,
    /// World position in cell units. See the module docs.
    position: (f32, f32),
    /// Held still: the tick decides nothing for it and it moves nowhere.
    ///
    /// A debugging handle rather than a state of the world — the unit panel
    /// freezes whoever is being inspected so they stop wandering off the
    /// screen mid-inspection. On [`Body`] rather than on one kind because
    /// "stop moving" means the same thing for everything that has a position,
    /// and [`super::think_step`] is the one place that reads it.
    frozen: bool,
    /// The property whose locked doors this body may open and whose
    /// furniture it may use — its key, handed over with a bed by the spawn
    /// pass ([`super::property`]). `None` for a body that owns nowhere, which
    /// may still go anywhere that is nobody's.
    home: Option<PropertyId>,
    /// Dead: it thinks, moves and ages no more, and lies where it fell. Like
    /// `frozen`, read by [`super::think_step`] and [`super::react_step`];
    /// unlike it, there is no letting go. Set by the kind itself when its
    /// health runs out (`Human::react`), and settled with the world by
    /// [`super::Effect::Died`].
    dead: bool,
    /// Running for its life, flat out (`walker::PANIC_SPEED`): set by its own
    /// brain while it flees (`Walker::panic`), and only while it has
    /// somewhere to run. On the body, beside the position the move step
    /// already has in cache, because that step reads it of every mover to
    /// say who is seen panicking ([`super::panics`]).
    panicking: bool,
    /// The house it spawned in, if it spawned in one: everybody born into the
    /// same house is its family ([`super::relationships`]). Set once, by the
    /// spawn pass; unlike `home`, never handed over with a bed.
    family: Option<PropertyId>,
    /// Surface contaminants carried by this unit, independent of its needs.
    dirt: super::dirt::Dirt,
}

impl Body {
    pub fn new(uid: Uid, position: (f32, f32)) -> Body {
        Body {
            uid,
            position,
            frozen: false,
            home: None,
            dead: false,
            panicking: false,
            family: None,
            dirt: super::dirt::Dirt::CLEAN,
        }
    }

    /// A body standing in the middle of `cell`.
    pub fn at_cell(uid: Uid, cell: Point) -> Body {
        Body::new(uid, (cell.x as f32 + 0.5, cell.y as f32 + 0.5))
    }

    pub fn uid(&self) -> Uid {
        self.uid
    }

    pub fn position(&self) -> (f32, f32) {
        self.position
    }

    /// The cell this body is standing in.
    ///
    /// `floor`, not `round`: a cell owns the half-open range from its own
    /// coordinate to the next, so every position in cell 3 answers 3 —
    /// including 3.0 exactly. Rounding would give cell 3 the right-hand half of
    /// cell 2 and make the boundary land in the middle of a tile.
    pub fn center_position(&self) -> Point {
        Point::new(
            self.position.0.floor() as i32,
            self.position.1.floor() as i32,
        )
    }

    /// Move to a world position, in cell units.
    pub fn set_position(&mut self, position: (f32, f32)) {
        self.position = position;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn set_frozen(&mut self, frozen: bool) {
        self.frozen = frozen;
    }

    pub fn is_dead(&self) -> bool {
        self.dead
    }

    pub fn set_dead(&mut self) {
        self.dead = true;
        self.panicking = false;
    }

    /// Whether it is running for its life — see [`Body`].
    pub fn is_panicking(&self) -> bool {
        self.panicking
    }

    pub fn set_panicking(&mut self, panicking: bool) {
        self.panicking = panicking;
    }

    /// The property it holds the key to, if any.
    pub fn home(&self) -> Option<PropertyId> {
        self.home
    }

    pub fn set_home(&mut self, home: Option<PropertyId>) {
        self.home = home;
    }

    /// The house it spawned in, whose other arrivals are its family.
    pub fn family(&self) -> Option<PropertyId> {
        self.family
    }

    pub fn set_family(&mut self, family: Option<PropertyId>) {
        self.family = family;
    }

    pub fn dirt(&self) -> super::dirt::Dirt { self.dirt }

    pub fn set_dirt(&mut self, value: super::dirt::Dirt) -> bool {
        if !value.is_valid() { return false; }
        self.dirt = value;
        true
    }

    pub(crate) fn dirt_mut(&mut self) -> &mut super::dirt::Dirt { &mut self.dirt }

    // --- saving ---

    pub(crate) fn save(&self) -> pb::Body {
        let Body { uid, position: (x, y), frozen, home, dead, panicking, family, dirt } = *self;
        pb::Body {
            uid: uid.raw(),
            x,
            y,
            frozen,
            home: home.map(|home| home.get() as u32),
            dead,
            panicking,
            family: family.map(|family| family.get() as u32),
            dirt: Some(dirt.save()),
        }
    }

    pub(crate) fn restore(saved: &pb::Body) -> Result<Body, Invalid> {
        let Some(uid) = Uid::from_raw(saved.uid) else {
            return invalid(format!("{:#x} is not an entity id", saved.uid));
        };
        if !(saved.x.is_finite() && saved.y.is_finite()) {
            return invalid(format!("{uid} stands at {}, {}", saved.x, saved.y));
        }
        let property = |number: Option<u32>, what: &str| match number {
            None => Ok(None),
            Some(number) => match u16::try_from(number).ok().and_then(PropertyId::new) {
                Some(property) => Ok(Some(property)),
                None => invalid(format!("{uid}'s {what} is property {number}")),
            },
        };
        let home = property(saved.home, "home")?;
        let family = property(saved.family, "family")?;
        Ok(Body {
            uid,
            position: (saved.x, saved.y),
            frozen: saved.frozen,
            home,
            dead: saved.dead,
            panicking: saved.panicking,
            family,
            dirt: saved.dirt.as_ref().map(super::dirt::Dirt::restore).transpose()?.unwrap_or_default(),
        })
    }
}

/// The cell a world position in cell units falls in.
///
/// [`Body::center_position`]'s rule, for a position that is not on a body yet
/// — a step that has been decided but not taken. One definition, so a move
/// cannot disagree with the entity it is moving about which cell it is
/// entering.
pub fn cell_of((x, y): (f32, f32)) -> Point {
    Point::new(x.floor() as i32, y.floor() as i32)
}

/// Anything the simulation ticks.
///
/// `Send + Sync` because the think phase is meant to run across threads; it is
/// stated here rather than left to be discovered when someone first tries.
pub trait GameEntity: Send + Sync {
    fn body(&self) -> &Body;
    fn body_mut(&mut self) -> &mut Body;

    /// **Step 1, think.** Decide what to do, reading only. The returned
    /// [`Intent`] is what the move step will *try* to carry out — see
    /// [`super::process_pass`].
    ///
    /// Nothing about the world may be mutated here, which is what makes the
    /// whole phase parallelisable without a lock. Nor is what this asks for
    /// granted: the world is arbitrated in the move step, and an entity finds
    /// out what became of its intent in [`GameEntity::react`].
    fn think(&self, ctx: &Think<'_>) -> Intent;

    /// **Step 2, move.** Carry out what [`GameEntity::think`] decided.
    ///
    /// Called only for a move the world allowed — a cancelled one leaves the
    /// entity untouched, so this never has to undo anything.
    ///
    /// The default writes the position and nothing else. A kind overrides this
    /// to keep its own state in step — a dog turning to face the way it walks
    /// — rather than that consequence being encoded in the intent, which
    /// describes what an entity *wants*.
    fn apply(&mut self, intent: &Intent) {
        if let Intent::Move { to } = intent {
            self.body_mut().set_position(*to);
        }
    }

    /// **Step 3, react.** Take in what became of this tick's move, once every
    /// entity has taken theirs.
    ///
    /// This is the second thinking round, and the only place a collision is
    /// answered: [`super::MoveOutcome::Blocked`] carries the id of whatever
    /// was in the way, and an entity decides here what that means for its
    /// plan. It runs after the whole crowd has moved, so what it sees is the
    /// world as it ended the tick rather than as it was partway through.
    ///
    /// **It may write only to itself.** No position (the move already
    /// happened, and the occupancy layer has been settled around it), and
    /// nothing outside this entity — which is what leaves the phase
    /// parallelisable in the same way think is, entity by entity.
    ///
    /// The one thing it may *ask* for beyond itself is an [`Effect`], applied
    /// afterwards and sequentially by [`super::world_step`] — a fridge's door
    /// is a fact about the world, not about the entity that opened it, so
    /// flipping it here directly would be writing outside itself under
    /// another name.
    ///
    /// The default does nothing and asks for nothing: a kind with no plan has
    /// nothing to revise.
    fn react(&mut self, ctx: &Think<'_>, outcome: super::MoveOutcome) -> Effect {
        let _ = (ctx, outcome);
        Effect::default()
    }

    /// The part of it that walks, for a kind that walks — what the react
    /// step meters a tick's route searching through ([`super::react_step`]).
    /// `None`, the default, for a kind that plans no routes.
    fn walker(&self) -> Option<&super::walker::Walker> {
        None
    }

    fn walker_mut(&mut self) -> Option<&mut super::walker::Walker> {
        None
    }

    /// Which way it is oriented, if that means anything for this kind.
    ///
    /// On the trait rather than reached for with a downcast: the renderer asks
    /// every entity this, and a `dyn GameEntity` that has to be guessed at
    /// before it can be drawn is not an abstraction, it is a cast with extra
    /// steps.
    fn facing(&self) -> Option<Facing> {
        None
    }

    /// The goal in charge of this kind's mind right now, for a label drawn
    /// above it — `None` for a kind with no brain.
    ///
    /// On the trait rather than reached for with a downcast, for the reason
    /// [`GameEntity::facing`] is above. Unlike [`GameEntity::debug_fields`]
    /// this is cheap and allocates nothing — [`super::brain::Brain::top_goal`]
    /// is a field read — which is what lets the renderer ask it every frame
    /// for every actor rather than only for the one selected.
    fn current_goal(&self) -> Option<super::brain::GoalId> {
        None
    }

    /// Whether this entity is held still.
    ///
    /// Read once per entity per tick by [`super::think_step`], which hands a
    /// frozen entity an [`super::Intent::Idle`] instead of asking it to think:
    /// nothing it decided could be carried out, so deciding it is work with
    /// nowhere to go. Overriding this is not the way to make a kind that never
    /// moves — return [`super::Intent::Idle`] from `think` for that. This is
    /// the flag something *outside* the entity sets.
    fn is_frozen(&self) -> bool {
        self.body().is_frozen()
    }

    fn set_frozen(&mut self, frozen: bool) {
        self.body_mut().set_frozen(frozen);
    }

    /// Dead, and so neither asked to think nor to react — see [`Body`].
    fn is_dead(&self) -> bool {
        self.body().is_dead()
    }

    /// This kind's body — its stats and the processes that change them — or
    /// `None` for a kind that has none. On the trait for the reason
    /// [`GameEntity::facing`] is: a test, the debug view and
    /// `Command::SetProcess` all reach a unit through `dyn GameEntity`.
    fn biology(&self) -> Option<&super::biology::Biology> {
        None
    }

    fn biology_mut(&mut self) -> Option<&mut super::biology::Biology> {
        None
    }

    /// What this kind is carrying — its hand and what it has stowed — or
    /// `None` for a kind that carries nothing at all. On the trait for the
    /// reason [`GameEntity::biology`] is: the unit panel, a test and
    /// `Command::GiveItem` all reach a unit through `dyn GameEntity`.
    fn inventory(&self) -> Option<&super::inventory::Inventory> {
        None
    }

    fn inventory_mut(&mut self) -> Option<&mut super::inventory::Inventory> {
        None
    }

    /// Lasting aptitudes, when this kind has them. The unit panel reads them
    /// through the trait, and explicit commands update them through the same
    /// simulation queue as other changes.
    fn talents(&self) -> Option<&super::talents::Talents> {
        None
    }

    fn talents_mut(&mut self) -> Option<&mut super::talents::Talents> {
        None
    }

    /// The outfit currently worn, separate from the stable appearance seed.
    /// `None` means this kind does not wear clothing; `Clothing::None` is a
    /// person who has taken their outfit off.
    fn clothing(&self) -> Option<super::clothing::Clothing> {
        None
    }

    fn clothing_mut(&mut self) -> Option<&mut super::clothing::Clothing> {
        None
    }

    /// What it has on at `clock`: its job's uniform while the job is due
    /// (`Job::is_due` — from setting off for the shift until it ends), or
    /// always for a job whose [`Bearing`](super::job::Bearing) says so (the
    /// king's robes and crown); its own [`clothing`](GameEntity::clothing)
    /// otherwise. Worked out, never stored, so nothing about it needs saving
    /// and a loaded world dresses the same; the clothing menu changes the own
    /// clothes, which a worker is back in after the shift, and a king when
    /// it is let go. `None` for a kind that wears nothing.
    fn worn(&self, clock: super::clock::Clock) -> Option<super::clothing::Clothing> {
        let own = self.clothing()?;
        Some(match self.job() {
            Some(job) if job.kind.bearing().always_in_uniform || job.is_due(clock) => job.kind.uniform(),
            _ => own,
        })
    }

    /// What this kind would tell a debugger about itself, beyond the body.
    ///
    /// Name and value, in the order they should be read. The universal facts —
    /// the id, the cell, whether it is frozen — are the same for everything
    /// and are read off the body by whoever is displaying them; this is for
    /// what only a `Human` or a `Dog` knows, like where it thinks it is going.
    ///
    /// It allocates, which nothing in a tick may do. That is fine, because
    /// nothing in a tick calls it: it is asked once a frame about the one
    /// entity somebody has selected.
    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }

    /// What this kind's mind is doing, for the brains menu: the goal in
    /// charge, the priority list, the task, the action. Empty for a kind with
    /// no brain.
    ///
    /// Beside [`GameEntity::debug_fields`] rather than folded into it, because
    /// the two are two menus; and on the trait rather than reached for with a
    /// downcast to `Human`, for the reason [`GameEntity::facing`] is. Same
    /// allocation licence: asked about one selected entity, never in a tick.
    fn brain_fields(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }

    /// Where this kind is planning to walk, current step first — for a debug
    /// visualization to draw over the map. Empty for a kind that does not walk
    /// one, and for one with no plan right now.
    ///
    /// It allocates, for the same reason [`GameEntity::debug_fields`] does:
    /// nothing in a tick calls it, only a frame drawing the one entity that is
    /// selected.
    fn planned_path(&self) -> Vec<Point> {
        Vec::new()
    }

    /// Fraction complete of whatever timed action is running right now, for
    /// the reverse progress bar drawn over a unit doing something other than
    /// walking or waiting — see [`super::brain::Action::progress`]. `None`
    /// for a kind with no brain, and for one with nothing running that a bar
    /// is shown for.
    ///
    /// On the trait rather than reached for with a downcast, for the reason
    /// [`GameEntity::facing`] is.
    fn action_progress(&self) -> Option<f32> {
        None
    }

    /// The cell of whatever this entity is using right now — the computer it
    /// is sitting at, the fridge it is reaching into — and `None` for one
    /// that is walking, waiting or idle. See
    /// [`super::brain::Action::interacting_with`].
    ///
    /// What a prop that shows whether it is in use reads (`game::props`): the
    /// fact belongs to the *unit*, since a feature has no state of its own —
    /// which is what lets the feature index be built once and read from every
    /// thread. On the trait, and cheap, for the reason
    /// [`GameEntity::current_goal`] is.
    fn interacting_with(&self) -> Option<Point> {
        None
    }

    /// Make `bed` this entity's own, and with it the key to `property`, the
    /// one the bed stands in (`None` for a bed that is nobody's property) —
    /// and say whether it took them.
    ///
    /// Called by the spawn pass, the only place a bed is handed out, for an
    /// entity that has just arrived. `false` — the default — for a kind that
    /// does not sleep: a dog has no use for a bed, and must not be holding one
    /// that a person could have had.
    fn set_home(&mut self, bed: Point, property: Option<PropertyId>) -> bool {
        let _ = (bed, property);
        false
    }

    /// Take on an order to salvage the heap of scrap in `heap`, and say
    /// whether it was taken. From outside the world ([`Command::Salvage`]),
    /// applied by the spawn pass. `false` — the default — for a kind with
    /// nothing to carry scrap away in.
    ///
    /// [`Command::Salvage`]: super::Command::Salvage
    fn order_salvage(&mut self, heap: Point) -> bool {
        let _ = heap;
        false
    }

    /// Take on `job`, its desk handed out with it, and say whether it was
    /// taken. Called by the spawn pass only — for an arrival
    /// (`GameState::give_a_job`) and for [`Command::Hire`] — which is what
    /// keeps one desk to one worker. `false` — the default — for a kind that
    /// cannot work: a dog must not be holding a desk a person could have had.
    ///
    /// [`Command::Hire`]: super::Command::Hire
    fn hire(&mut self, job: super::job::Job) -> bool {
        let _ = job;
        false
    }

    /// Lose its job, and say what it was. From the spawn pass only
    /// ([`Command::Dismiss`]), which gives the desk back.
    ///
    /// [`Command::Dismiss`]: super::Command::Dismiss
    fn dismiss(&mut self) -> Option<super::job::Job> {
        None
    }

    /// What it does for a living, if anything — for the job menu, and for
    /// whatever hires and lets go. `None` for a kind that cannot work and for
    /// somebody with no job alike; [`GameEntity::hire`] is what tells them
    /// apart.
    fn job(&self) -> Option<super::job::Job> {
        None
    }

    /// Whether it is seeing to its needs before a trip out for its job — for
    /// the job menu, which says so rather than calling it a break.
    fn is_on_sick_leave(&self, clock: super::clock::Clock) -> bool {
        let _ = clock; false
    }

    fn is_getting_ready(&self) -> bool {
        false
    }

    /// The chat it is in, as the rest of the crowd sees it this tick — what
    /// the think step gathers into [`Chats`](super::relationships::Chats).
    /// Asked of every unit every tick, from inside the think step's walk of
    /// the crowd, so it reads only what is inline. `None` — the default — for
    /// a kind that never chats.
    fn chatter(&self) -> Option<super::relationships::Chatter> {
        None
    }

    /// What it thinks of everybody it knows ([`super::relationships`]), for
    /// the relationship tab. `None` for a kind that knows nobody.
    fn relations(&self) -> Option<&super::relationships::Relations> {
        None
    }

    /// The chat it is in, if any, as it remembers it.
    fn chat(&self) -> Option<super::relationships::Chat> {
        None
    }

    /// The small talk it is in, as the rest of the crowd sees it this tick —
    /// what the think step gathers into
    /// [`Smalltalks`](super::relationships::Smalltalks), like
    /// [`GameEntity::chatter`]. `None` for a kind that never talks.
    fn smalltalker(&self) -> Option<super::relationships::Smalltalker> {
        None
    }

    /// The small talk it is in, if any, as it remembers it: what the game
    /// draws sparks for once answered.
    fn smalltalk(&self) -> Option<super::relationships::Smalltalk> {
        None
    }

    /// Who it is bonded to, as the rest of the crowd sees it this tick — what
    /// the think step gathers into [`Bonds`](super::pets::Bonds): a person's
    /// pet dog, or a dog's owner, where it stands, and a dog's hunger. Asked
    /// of every unit every tick, so it reads only what is inline. `None` —
    /// the default — for a kind that is bonded to nobody. Handed `ctx` because
    /// a dog's hunger counts the meal served and not yet felt.
    fn bond(&self, ctx: &Think<'_>) -> Option<super::pets::Bond> {
        let _ = ctx;
        None
    }

    /// Who it keeps company with: a person's pet dog, or a dog's owner.
    fn companion(&self) -> Option<super::uid::Uid> {
        None
    }

    /// Keep company with `companion` from now on (a pet dog and its owner,
    /// each told of the other by the spawn pass), and say whether it did.
    /// `false` — the default — for a kind that keeps none.
    fn set_companion(&mut self, companion: super::uid::Uid) -> bool {
        let _ = companion;
        false
    }

    /// A test's way to make a dog this hungry, with nothing eaten.
    #[cfg(test)]
    fn starve_to(&mut self, hunger: f32) {
        let _ = hunger;
    }

    /// Empty the bowel, and say whether there was one (`Effect::Defecate`).
    /// `false` — the default — for a kind that has none.
    fn defecate(&mut self) -> bool {
        false
    }

    /// This entity's own name, for a kind that has one.
    ///
    /// `None` rather than a made-up default: a dog is not almost a person,
    /// and a fallback name would say otherwise. The unit panel falls back to
    /// [`EntityType::name`] itself for a kind that answers `None`.
    fn display_name(&self) -> Option<String> {
        None
    }

    /// Everything this entity is, for a save (`crate::save`) — the kind's
    /// own message, which `kinds::restore` turns back into the same entity.
    ///
    /// No default, on purpose: a kind that could be left out of a save would
    /// come back from a load as something else, or not at all.
    fn save(&self) -> pb::Kind;

    /// Stable randomness for the renderer to build an appearance from.
    ///
    /// The simulation does not know what a human looks like and should not
    /// start to — but it does own the one thing choosing an appearance needs,
    /// which is a number that is the same every time for this entity and
    /// different for the next one. The id's random half already is that.
    fn appearance_seed(&self) -> u64 {
        self.uid().body()
    }

    // --- derived, from the body ---

    fn uid(&self) -> Uid {
        self.body().uid()
    }

    /// What this is. Read from the id, so an entity cannot report a type its
    /// own id disagrees with.
    ///
    /// `None` only for an id whose tag this build does not know, which cannot
    /// happen for an entity it constructed itself.
    fn kind(&self) -> Option<EntityType> {
        self.uid().kind()
    }

    fn position(&self) -> (f32, f32) {
        self.body().position()
    }

    fn center_position(&self) -> Point {
        self.body().center_position()
    }
}

/// What an entity is allowed to see while it thinks: the world, read-only.
///
/// A struct rather than a pile of arguments so that adding something for
/// entities to look at — a spatial index, the other entities, the time of day
/// — does not change every `think` signature in the game.
pub struct Think<'a> {
    pub map: &'a crate::map::Map,
    /// Who is standing where. **What this says depends on when it is read**,
    /// and the two rounds read it at different moments on purpose: think sees
    /// the crowd as it ended the previous tick, react sees it as it ended
    /// this one. The near stage of pathfinding wants the second, which is why
    /// it is a reaction and not a decision — see
    /// [`crate::sim::walker::Walker`].
    pub occupancy: &'a super::occupancy::Occupancy,
    pub log: &'a super::log::Log,
    /// Where the props a brain can use are — fridges, so far. Built once with
    /// the world, so reading it is a scan of a handful of cells.
    pub features: &'a super::feature::Features,
    pub hospital: &'a super::hospital::Hospital,
    /// Every fridge's door and temperature, as of the *start* of this tick —
    /// unlike `features`, this changes tick to tick, through [`Effect`] and
    /// [`super::world_step`]. A task reads it to decide whether a door is
    /// open; nothing here may write it directly.
    pub fridges: &'a super::fridge::Fridges,
    /// Every door's leaf and lock, as of the start of this tick — changed
    /// only by [`super::world_step`], like `fridges`. A walker reads it to
    /// know whether the doorway ahead is open, and a route which doors it
    /// may plan through.
    pub doors: &'a super::door::Doors,
    /// Which cells are whose property. Built once with the world: what a
    /// goal asks before choosing somewhere to go ([`Think::may_use`]).
    pub properties: &'a super::property::Properties,
    /// Who lies dead where — what an undertaker looks through for the
    /// nearest body. As of the start of this tick, changed only by
    /// [`super::world_step`].
    pub corpses: &'a super::corpses::Corpses,
    /// What every store holds — the warehouse's bins and rack, the steel
    /// factory's hoppers and pallets — as of the start of this tick, changed
    /// only by [`super::world_step`]. What a courier or a steelworker looks
    /// through for somewhere with something to take.
    pub bins: &'a super::bins::Bins,
    /// Distribution box faults, changed by manual switches and repairs.
    pub electrical: &'a super::electrical::Electrical,
    /// Every crop in the fields, as of the start of this tick, changed only
    /// by [`super::world_step`]: what a farmer looks through for what wants
    /// doing in its field ([`super::fields`]).
    pub fields: &'a super::fields::Fields,
    pub flowers: &'a super::flowers::Flowers,
    pub dirt: &'a super::dirt::DirtLayer,
    /// Which arcade cabinets work, as of the start of this tick, changed only
    /// by [`super::world_step`]: whether a playzone has an administrator at
    /// its computer ([`super::playzone`]).
    pub playzones: &'a super::playzone::Playzones,
    /// The district's money, as of the start of this tick, changed only by
    /// [`super::world_step`]: whether anybody is owed, which day's books
    /// want closing, and what is in them ([`super::economy`]).
    pub economy: &'a super::economy::Economy,
    /// Who was seen running in a panic this tick, sorted: rebuilt by the
    /// move step before react reads it ([`super::panics`]). What lets panic
    /// spread by sight with no scan of the crowd.
    pub panics: &'a super::panics::Panics,
    /// The bites dealt at the end of the last tick, each read only by the
    /// unit bitten, in its own react ([`super::wounds`]).
    pub wounds: &'a super::wounds::Wounds,
    /// Where on the site's edge there is a way in, and how far in from it
    /// is safe: where somebody fleeing runs to ([`super::refuge`]).
    pub ways_in: &'a super::refuge::WaysIn,
    /// Everybody's family and job, as of the start of this tick, changed only
    /// by the spawn pass: what a wanderer asks about somebody it has just
    /// seen, for the floor of what it thinks of them
    /// ([`super::relationships::Society`]).
    pub society: &'a super::relationships::Society,
    /// Who is chatting with whom, gathered by this tick's think step: who is
    /// asking somebody for a chat, and where a partner stands
    /// ([`super::relationships::Chats`]).
    pub chats: &'a super::relationships::Chats,
    /// Who is making small talk with whom, gathered by this tick's think
    /// step ([`super::relationships::Smalltalks`]).
    pub smalltalks: &'a super::relationships::Smalltalks,
    /// Who is bonded to whom — a person to its pet dog — gathered by this
    /// tick's think step: where an owner is for its dog to stay by, and how
    /// hungry the dog is for its owner to feed it ([`super::pets::Bonds`]).
    pub bonds: &'a super::pets::Bonds,
    /// The dog foods given at the end of the last tick, each felt only by the
    /// dog fed, in its own react ([`super::pets::Feedings`]).
    pub feedings: &'a super::pets::Feedings,
    /// **Watched** seconds since the previous tick: what a body *moves* by.
    ///
    /// A walk, an action being stood through, a pause to let somebody past —
    /// everything measured in front of the player is measured with this. What
    /// the world's clock governs is [`Think::game_dt`] instead, which is a
    /// hundred and twenty times as much; [`crate::sim::clock`] says why the
    /// two are different numbers.
    pub dt: f32,
    /// Which tick this is. Deterministic, so it is usable as an RNG seed
    /// alongside an entity's own id.
    pub tick: u64,
    /// What time it is in the world, as of this tick: what a routine asks to
    /// know whether it is night. A reading of the one world clock
    /// ([`crate::sim::GameState::clock`]), taken once per pass and shared by
    /// every entity, so the whole crowd agrees what time it is.
    pub clock: super::clock::Clock,
}

impl<'a> Think<'a> {
    /// The world as `facilities` has it, at tick `tick` of `dt` watched
    /// seconds, `clock` o'clock: **the one way a `Think` is made**, so a new
    /// part of the world a brain reads is a field here and a line below, and
    /// nothing that builds one — the process pass, the tests' worlds — has to
    /// change.
    pub(crate) fn new(
        map: &'a crate::map::Map,
        occupancy: &'a super::occupancy::Occupancy,
        log: &'a super::log::Log,
        facilities: &'a super::facilities::Facilities,
        dt: f32,
        tick: u64,
        clock: super::clock::Clock,
    ) -> Think<'a> {
        Think {
            map,
            occupancy,
            log,
            features: &facilities.features,
            hospital: &facilities.hospital,
            fridges: &facilities.fridges,
            doors: &facilities.doors,
            properties: &facilities.properties,
            corpses: &facilities.corpses,
            bins: &facilities.bins,
            fields: &facilities.fields,
            flowers: &facilities.flowers,
            dirt: &facilities.dirt,
            playzones: &facilities.playzones,
            economy: &facilities.economy,
            panics: &facilities.panics,
            wounds: &facilities.wounds,
            ways_in: &facilities.ways_in,
            society: &facilities.society,
            chats: &facilities.chats,
            smalltalks: &facilities.smalltalks,
            bonds: &facilities.bonds,
            feedings: &facilities.feedings,
            electrical: &facilities.electrical,
            dt,
            tick,
            clock,
        }
    }
}

impl Think<'_> {
    /// **World** seconds since the previous tick: what a body *ages* by.
    ///
    /// The same slice of time as [`Think::dt`], read off the world's clock
    /// rather than the player's — so a tick at 64Hz is not a sixty-fourth of a
    /// second of getting hungry but nearly two seconds of it. Every rate a
    /// [`Process`](crate::sim::biology::Process) is written with is per one of
    /// these.
    ///
    /// Derived rather than carried: one `dt` arrives from the outside, and a
    /// second field would be a second thing that could be wrong.
    pub fn game_dt(&self) -> f32 {
        self.dt * super::clock::TIME_SCALE
    }

    /// Whether the *terrain* would let an entity stand in `cell`. The static
    /// half, and the only half the far stage of a path is planned against.
    pub fn is_passable(&self, cell: Point) -> bool {
        self.map.is_passable(cell)
    }

    /// Whether `uid` could stand in `cell` **right now**: passable terrain,
    /// and nobody else already there.
    ///
    /// Both layers, in the order the move step asks them in. This is what the
    /// near stage plans against, and it is only meaningful for the moment it
    /// is asked — the crowd moves.
    pub fn is_clear_for(&self, cell: Point, uid: Uid) -> bool {
        self.is_passable(cell) && self.occupancy.is_free_for(cell, uid)
    }

    /// Whether a body holding `key` may plan a route through `cell`: the
    /// terrain lets it, and no door there is locked to it. What both stages
    /// of a walk are planned against. A door that is merely shut is no
    /// obstacle here — it opens.
    pub fn is_passable_with(&self, cell: Point, key: Option<PropertyId>) -> bool {
        self.is_passable(cell) && self.doors.lets_through(cell, key)
    }

    /// What stepping into `cell` costs a body of `kind` holding `key`, or
    /// `None` where it cannot go — where [`Think::is_passable_with`] says so,
    /// and anywhere on the site for a kind that keeps off it
    /// ([`EntityType::keeps_off_the_site`]: a deathdog's route goes round the
    /// walls). The terrain's walk cost, so a route keeps to the pavement and
    /// crosses the road ([`crate::sim::path`]).
    pub fn step_cost_with(&self, cell: Point, key: Option<PropertyId>, kind: Option<EntityType>) -> Option<u32> {
        if kind.is_some_and(EntityType::keeps_off_the_site) && self.map.site().contains(cell) {
            return None;
        }
        self.is_passable_with(cell, key).then(|| self.map.walk_cost(cell) as u32)
    }

    /// Whether a body holding `key` may go to `cell` and use what is there:
    /// it is nobody's property, or its own. Asked of a feature before it is
    /// chosen, so nobody plans a meal from a stranger's fridge.
    pub fn may_use(&self, cell: Point, key: Option<PropertyId>) -> bool {
        self.properties.may_use(cell, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(x: f32, y: f32) -> Body {
        Body::new(Uid::new(EntityType::Human, 7), (x, y))
    }

    #[test]
    fn the_center_cell_is_the_one_the_position_is_inside() {
        assert_eq!(body(3.5, 2.5).center_position(), Point::new(3, 2));
        // The low edge belongs to the cell it opens, the high edge to the next.
        assert_eq!(body(3.0, 2.0).center_position(), Point::new(3, 2));
        assert_eq!(body(3.999, 2.999).center_position(), Point::new(3, 2));
        assert_eq!(body(4.0, 3.0).center_position(), Point::new(4, 3));
    }

    #[test]
    fn cells_left_of_the_origin_floor_away_from_zero() {
        // `as i32` truncates towards zero, which would give -0.5 the cell 0
        // and put two cells' worth of world into one. `floor` first is why.
        assert_eq!(body(-0.5, -0.5).center_position(), Point::new(-1, -1));
        assert_eq!(body(-1.5, -2.5).center_position(), Point::new(-2, -3));
    }

    #[test]
    fn at_cell_puts_a_body_in_the_middle_of_it() {
        let uid = Uid::new(EntityType::Dog, 3);
        let cell = Point::new(6, 4);
        let body = Body::at_cell(uid, cell);
        assert_eq!(body.position(), (6.5, 4.5));
        assert_eq!(body.center_position(), cell);
    }

    #[test]
    fn moving_a_body_moves_the_cell_it_reports() {
        let mut body = body(1.5, 1.5);
        body.set_position((9.25, 0.75));
        assert_eq!(body.center_position(), Point::new(9, 0));
    }
}
