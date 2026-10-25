//! What the props on a map are *for*: [`FeatureKind`]'s catalogue, and the
//! [`Features`] index of where they are.
//!
//! The same shape as `map::TERRAIN`: a static catalogue that binds a prop's
//! **name** — the one a map file stores and the editor's palette paints — to
//! what a brain can do with it ([`FeatureDef::props`]). The art stays in the
//! editor; what a fridge looks like is none of this module's business, and
//! that it holds food is none of the editor's.
//!
//! A prop no kind names is scenery. A prop two kinds name is two things at
//! once: a fridge is food and water. A kind with a [`StoreDef`] is a store —
//! what it holds is [`crate::sim::bins::Bins`]' — so a new store is a row
//! here and nothing else in the simulation.
//!
//! **A feature that does not work is not one.** A fridge with no power, a
//! computer with no power and a toilet with no drain
//! ([`crate::map::utilities`]) are left out of the index, so a brain never
//! walks to one — the same as a map without it. Whether something is
//! connected cannot change during play, since nothing in a tick edits the
//! map.
//!
//! **A fridge is the one feature with state of its own** — its door and its
//! temperature, in [`crate::sim::fridge::Fridges`] rather than here. What
//! this module answers never changes during a tick: a name is food, or it
//! is not. Whether the fridge behind that name is open right now is a
//! different question, asked of a different index.

use rand::rngs::SmallRng;
use rand::RngExt;

use crate::map::utilities::Supply;
use crate::map::{Map, ObjectLayer, Point};

use super::catalogue::catalogue;
use super::item::ItemKind;
use super::property::Properties;

/// Everything about one kind of feature: its row in [`FeatureKind`]'s
/// catalogue.
pub struct FeatureDef {
    /// The props that are one — the names a map file stores and the editor's
    /// palette paints. A prop may be several kinds at once (a fridge is food
    /// and water, a computer a desk and something to play on) and a kind may
    /// be several props (six beds).
    pub props: &'static [&'static str],
    /// How it is used: from beside it, by entering it, or from in sight of
    /// it.
    pub access: Access,
    /// What it keeps, for a store ([`crate::sim::bins`]); `None` for
    /// anything else.
    pub store: Option<StoreDef>,
}

/// A feature things are kept in: one kind of item each, as many pieces as
/// are put in it ([`crate::sim::bins::Bins`]). Filled and emptied from
/// beside it, needing no power.
pub struct StoreDef {
    /// The one kind of thing it holds.
    pub holds: ItemKind,
    /// What a log line calls it: "the bin", "the scrap hopper".
    pub called: &'static str,
    /// The building whose stock it is, as the statistics menu lists it
    /// (`warehouse`, `factory`): every store with the same site is added up
    /// together.
    pub site: &'static str,
    /// The word the HUD counts what it holds under (`scrap  12 stored`),
    /// for the stores the HUD counts; `None` for the rest.
    pub hud: Option<&'static str>,
}

/// Used from a cell beside it, and nothing kept in it: most features.
const BESIDE: FeatureDef = FeatureDef { props: &[], access: Access::Beside, store: None };

catalogue! {
    /// What a brain can get out of a feature.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum FeatureKind: FeatureDef {
        Food = 0 => FeatureDef { props: &["fridge"], ..BESIDE },
        Water = 1 => FeatureDef { props: &["fridge"], ..BESIDE },
        /// A toilet is sat on, one at a time: entered, not stood beside.
        Toilet = 2 => FeatureDef { props: &["toilet"], access: Access::Entered, ..BESIDE },
        /// Something to do: a home computer, used from the chair beside it.
        Entertainment = 3 => FeatureDef { props: &["computer"], ..BESIDE },
        /// Something to do from across the room: a TV, watched from anywhere
        /// within [`VIEWING_DISTANCE`] that is in sight of it ([`can_watch`]).
        /// The other way to be amused, so `goals::play` goes for whichever of the
        /// two is nearer; needs power, like the computer, and is anybody's
        /// outside a home.
        Television = 4 => FeatureDef { props: &["tv"], access: Access::Watched, ..BESIDE },
        /// Somewhere to sleep: a bed, slept *in* rather than beside, and one
        /// sleeper to a bed — the toilet's arrangement, and for the same
        /// reason. Every bed the editor can place; their art is 48x48 and has
        /// no "somebody is in it" strip yet, so the sleeper is what shows it.
        Bed = 5 => FeatureDef {
            props: &["bed 1", "bed 2", "bed 3", "bed 4", "bed 5", "bed 6"],
            access: Access::Entered,
            ..BESIDE
        },
        /// Somewhere to work: a computer again, worked at from the chair beside
        /// it. Which one is somebody's own desk is a job's business
        /// ([`crate::sim::job`]); this only says what counts as a desk.
        Workstation = 6 => FeatureDef { props: &["computer"], ..BESIDE },
        /// Somewhere to store scrap metal: a warehouse's bin, filled from beside
        /// it. A scrap collector's workplace, the way a computer is an office
        /// worker's. Needs no power.
        ScrapBin = 7 => FeatureDef {
            props: &["scrap bin"],
            store: Some(StoreDef { holds: ItemKind::Scrap, called: "bin", site: "warehouse", hud: Some("scrap") }),
            ..BESIDE
        },
        /// Somewhere to keep watch: a guard post by a gate in the district's
        /// wall, stood at from beside it. A guard's workplace; needs no power.
        GuardPost = 8 => FeatureDef { props: &["guard post"], ..BESIDE },
        /// Somewhere to put the dead: a crematorium, loaded from beside it. An
        /// undertaker's workplace; needs no power.
        Crematorium = 9 => FeatureDef { props: &["crematorium"], ..BESIDE },
        /// Somewhere to keep steel: the warehouse's rack, which a courier fills
        /// with what the steel factory makes.
        SteelRack = 10 => FeatureDef {
            props: &["steel rack"],
            store: Some(StoreDef { holds: ItemKind::Steel, called: "steel rack", site: "warehouse", hud: Some("steel") }),
            ..BESIDE
        },
        /// The steel factory's store of scrap metal to be smelted: filled by a
        /// courier from the warehouse's bins, emptied by a steelworker. A
        /// courier's workplace.
        ScrapHopper = 11 => FeatureDef {
            props: &["scrap hopper"],
            store: Some(StoreDef { holds: ItemKind::Scrap, called: "scrap hopper", site: "factory", hud: None }),
            ..BESIDE
        },
        /// The steel factory's store of steel it has made: filled by a
        /// steelworker, emptied by a courier taking it to the warehouse.
        SteelPallet = 12 => FeatureDef {
            props: &["steel pallet"],
            store: Some(StoreDef { holds: ItemKind::Steel, called: "steel pallet", site: "factory", hud: None }),
            ..BESIDE
        },
        /// Where scrap metal is smelted into steel: a steelworker's workplace,
        /// worked from beside it. **Needs power** (`map::utilities`).
        Furnace = 13 => FeatureDef { props: &["furnace"], ..BESIDE },
        /// The farm's store of wheat: a farmer's workplace, one farmer to a
        /// bin, and what its field is sown for (`sim::fields`) — the harvest
        /// goes in it.
        GrainBin = 14 => FeatureDef {
            props: &["grain bin"],
            store: Some(StoreDef { holds: ItemKind::Wheat, called: "grain bin", site: "farm", hud: Some("wheat") }),
            ..BESIDE
        },
        /// Where a power plant is run from: its control panel, stood at from
        /// beside it. A power plant worker's workplace; needs no power.
        ControlPanel = 15 => FeatureDef { props: &["control panel"], ..BESIDE },
        /// Where a king holds court: seated on the palace's throne, one king
        /// at a time. The king's workplace; needs no power.
        Throne = 16 => FeatureDef { props: &["throne"], access: Access::Entered, ..BESIDE },
        DistributionBox = 17 => FeatureDef { props: &[], ..BESIDE },
        FoodCooker = 18 => FeatureDef { props: &["food cooker"], ..BESIDE },
        WheatHopper = 19 => FeatureDef {
            props: &["wheat hopper"],
            store: Some(StoreDef { holds: ItemKind::Wheat, called: "wheat hopper", site: "food factory", hud: None }),
            ..BESIDE
        },
        MealPallet = 20 => FeatureDef {
            props: &["meal pallet"],
            store: Some(StoreDef { holds: ItemKind::Meal, called: "meal pallet", site: "food factory", hud: None }),
            ..BESIDE
        },
        MealRack = 21 => FeatureDef {
            props: &["meal rack"],
            store: Some(StoreDef { holds: ItemKind::Meal, called: "meal rack", site: "warehouse", hud: Some("meals") }),
            ..BESIDE
        },
        /// An arcade cabinet in a playzone: a quick go at something, played from
        /// beside it — the quickest fun there is — and **only while the
        /// playzone is run** ([`crate::sim::playzone::Playzones::runs`]): needs
        /// power, like a computer, and an administrator at the playzone's
        /// computer. Anybody's.
        Arcade = 22 => FeatureDef { props: &["arcade cabinet"], ..BESIDE },
        /// Where a playzone is run from: its central computer, worked from
        /// beside it. The administrator's workplace; **needs power**. While an
        /// administrator is on duty at it, the playzone's cabinets work.
        PlayzoneComputer = 23 => FeatureDef { props: &["playzone computer"], ..BESIDE },
        /// Where the district's books are kept: the accountant's computer in
        /// the palace, worked from beside it. **Needs power**, and is nothing
        /// to play on.
        AccountantComputer = 24 => FeatureDef { props: &["accountant computer"], ..BESIDE },
        HospitalBed = 25 => FeatureDef { props: &["hospital bed"], access: Access::Entered, ..BESIDE },
        NurseStation = 26 => FeatureDef { props: &["nurse station"], ..BESIDE },
    }
}

/// How a feature is used: from a cell beside it, or by entering its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    /// From a cell beside it, any number of units queuing up one after
    /// another — the fridge, and everything before the toilet. The prop's
    /// own cell stays off limits to everyone, as it always has
    /// (`goals::stand_beside` never offers it).
    Beside,
    /// From inside its own cell, one unit at a time. The cell is impassable
    /// to ordinary movement — nobody routes through it and nobody walks
    /// through it — but the move step lets exactly one unit *in*, once a
    /// task asks for it by entering directly
    /// (`crate::sim::brain::action::Action::enter`) rather than by an
    /// ordinary route, which a search would refuse.
    ///
    /// [`crate::sim::occupancy::Occupancy`] is the taken/free state: the
    /// unit standing in the cell holds the claim, and the cell is free again
    /// the instant it leaves — the same bookkeeping every other cell already
    /// gets from ordinary movement, asked of one more kind of cell.
    Entered,
    /// From any cell near enough to it and in sight of it — a TV, watched
    /// ([`can_watch`]). The prop's own cell is off limits, as it is to
    /// [`Access::Beside`], and any number of units may watch at once.
    Watched,
}

/// How far from a TV it can be watched, in cells, as the crow flies:
/// `dx² + dy² <= 3²` — three cells straight out, or two across and two along.
pub const VIEWING_DISTANCE: i32 = 3;

/// Whether `cell` is near enough to `screen` to watch it: within
/// [`VIEWING_DISTANCE`], and not on it. Distance only; [`can_watch`] adds the
/// line of sight.
pub fn within_viewing_distance(cell: Point, screen: Point) -> bool {
    let (dx, dy) = (cell.x - screen.x, cell.y - screen.y);
    (dx, dy) != (0, 0) && dx * dx + dy * dy <= VIEWING_DISTANCE * VIEWING_DISTANCE
}

/// Whether a unit standing in `cell` can watch the TV in `screen`: within
/// [`VIEWING_DISTANCE`] of it, with nothing but see-through terrain between
/// the two ([`in_sight`](crate::sim::brain::perception::in_sight)) — a wall
/// in the way hides the picture, furniture does not.
pub fn can_watch(map: &Map, cell: Point, screen: Point) -> bool {
    within_viewing_distance(cell, screen) && crate::sim::brain::perception::in_sight(map, cell, screen)
}

/// Every offset from a TV within [`VIEWING_DISTANCE`] of it, nearest first
/// (then by row, then by column), so whatever picks a seat from them picks
/// the same one however it is asked. Worked out at compile time: 28 cells.
pub const VIEWING_OFFSETS: [(i8, i8); 28] = viewing_offsets();

const fn viewing_offsets() -> [(i8, i8); 28] {
    let mut out = [(0i8, 0i8); 28];
    let mut len = 0;
    let mut d2 = 1;
    while d2 <= VIEWING_DISTANCE * VIEWING_DISTANCE {
        let mut dy = -VIEWING_DISTANCE;
        while dy <= VIEWING_DISTANCE {
            let mut dx = -VIEWING_DISTANCE;
            while dx <= VIEWING_DISTANCE {
                if dx * dx + dy * dy == d2 {
                    out[len] = (dx as i8, dy as i8);
                    len += 1;
                }
                dx += 1;
            }
            dy += 1;
        }
        d2 += 1;
    }
    assert!(len == 28, "every cell in reach, once");
    out
}

impl FeatureKind {
    pub const fn access(self) -> Access {
        self.def().access
    }

    /// What it keeps, if it is a store.
    pub const fn store(self) -> Option<&'static StoreDef> {
        self.def().store.as_ref()
    }

    /// The prop that is one: the first its row names.
    pub const fn prop(self) -> &'static str {
        if matches!(self, Self::DistributionBox) { "distribution box" } else { self.def().props[0] }
    }
}

/// Everything a prop name is for — nothing, for scenery — in the order the
/// kinds are listed.
///
/// A fridge never runs out of food or drink — there is no depletion state —
/// but it does have a door and a temperature
/// ([`crate::sim::fridge::Fridges`]), and a store holds what is put in it
/// ([`crate::sim::bins::Bins`]). What does not change is this: which prop
/// names are food, water, a toilet, a go on a computer or a bed to sleep in is
/// fixed, which is what lets the index be built once, at map load, and read
/// from every thread.
pub fn kinds_of(name: &str) -> impl Iterator<Item = FeatureKind> + '_ {
    FeatureKind::ALL.into_iter().filter(move |kind| kind.def().props.contains(&name))
}

/// The kind of store a prop called `name` is, if it is one.
pub fn store_of(name: &str) -> Option<FeatureKind> {
    kinds_of(name).find(|kind| kind.store().is_some())
}

/// Where the things a brain can use are, by cell.
///
/// Built once in `GameState::new` by scanning the map's `Props` layer, so the
/// per-tick cost of finding one is a scan of the fridges — a handful — and
/// never of the crowd or the map.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Features {
    /// Every working feature of each kind, sorted, at the kind's number.
    cells: [Vec<Point>; FeatureKind::COUNT],
    /// Cells with passable terrain and exactly one blocking prop, a working
    /// [`Access::Entered`] feature. Entry bypasses that prop alone, never a
    /// wall, the map boundary, or another piece of furniture stacked on it.
    entered: Vec<Point>,
}

impl Features {
    /// Every prop on `map` a brain can use, *and that works*: one that needs
    /// power or a drain `supply` does not give it is left out. Computers
    /// outside a home provide work only, never entertainment.
    #[cfg(test)]
    pub fn from_map(map: &Map, supply: &Supply) -> Features {
        Self::with_properties(map, supply, &Properties::from_map(map))
    }

    /// Reuse the world's property index, including when its power changes.
    pub(super) fn with_properties(map: &Map, supply: &Supply, properties: &Properties) -> Features {
        let mut features = Features::default();
        // Built once, not searched per feature or per move. Two means two or
        // more: all we need to know is whether the feature is the sole blocker.
        let mut blockers = vec![0u8; map.size().area()];
        for object in map.objects(ObjectLayer::Props) {
            if !object.kind.prop_passability().is_passable()
                && let Some(index) = map.size().index_of(object.cell())
            {
                blockers[index] = (blockers[index] + 1).min(2);
            }
        }
        for object in map.objects(ObjectLayer::Props) {
            let cell = object.cell();
            if !supply.serves(object.kind.as_str(), cell) {
                continue;
            }
            for kind in kinds_of(object.kind.as_str()) {
                // Computers in public buildings are office equipment, even
                // when nobody has been hired at them or the shift is over.
                // Only a home's computer is available for gaming; choosing
                // it still requires that home's key.
                if object.kind.as_str() == "computer"
                    && kind == FeatureKind::Entertainment
                    && properties.of(cell).is_none()
                {
                    continue;
                }
                if kind.access() == Access::Entered {
                    let can_enter = map.size().index_of(cell).is_some_and(|index| {
                        map.terrain(cell).is_some_and(|tile| tile.is_passable()) && blockers[index] == 1
                    });
                    if !can_enter {
                        continue;
                    }
                }
                features.cells[kind as usize].push(cell);
                if kind.access() == Access::Entered && !features.entered.contains(&cell) {
                    features.entered.push(cell);
                }
            }
        }
        for cell in map.size().points() {
            // One public, accessible service box is the crew's base: three
            // electricians (one per watch), rather than three at every house.
            if map.grid(crate::map::GridLayer::Power, cell) == crate::map::grid::power::BOX
                && map.is_passable(cell) && properties.of(cell).is_none()
                && cell.cardinal_neighbours().iter().any(|&side| map.is_passable(side))
            {
                features.cells[FeatureKind::DistributionBox as usize].push(cell);
                break;
            }
        }
        // Sorted, so nothing that picks among them depends on the order the
        // props were painted in.
        for cells in &mut features.cells {
            cells.sort_unstable();
        }
        features
    }

    /// Whether `cell` is a feature that must be entered to use, and so may
    /// be stepped onto — by whoever currently holds it, via
    /// [`crate::sim::occupancy::Occupancy`] — despite being impassable to
    /// ordinary movement. This exempts only the feature itself: construction
    /// excludes walls, off-map cells and any additional blocking prop.
    /// See [`Access::Entered`].
    pub fn is_enterable(&self, cell: Point) -> bool {
        self.entered.contains(&cell)
    }

    fn cells(&self, kind: FeatureKind) -> &[Point] {
        &self.cells[kind as usize]
    }

    /// Every working feature of `kind`, sorted by cell.
    pub fn cells_of(&self, kind: FeatureKind) -> &[Point] {
        self.cells(kind)
    }

    pub fn count(&self, kind: FeatureKind) -> usize {
        self.cells(kind).len()
    }

    /// The feature of `kind` closest to `from`, as the crow flies along the
    /// grid.
    ///
    /// Manhattan distance, not route length: finding out how far a route is
    /// costs a search per fridge, and a fridge that turns out to be walled off
    /// is the goal's to discover when the route to it is asked for. Ties go to
    /// the lower cell, so the answer does not depend on the order the props
    /// were placed in.
    pub fn nearest(&self, kind: FeatureKind, from: Point) -> Option<Point> {
        self.cells(kind)
            .iter()
            .copied()
            .min_by_key(|&cell| (from.manhattan_distance(cell), cell))
    }

    /// Whether there is a feature of `kind` in `cell`.
    pub fn has(&self, kind: FeatureKind, cell: Point) -> bool {
        self.cells(kind).contains(&cell)
    }

    /// Like [`Features::nearest`], but only among the features `keep` says yes
    /// to — the nearest bed that is not owned, say. `None` when there are none
    /// left, rather than falling back to one that was refused. The caller says
    /// what it is asking for, so this knows nothing about the crowd.
    pub fn nearest_where(&self, kind: FeatureKind, from: Point, keep: impl Fn(Point) -> bool) -> Option<Point> {
        self.cells(kind)
            .iter()
            .copied()
            .filter(|&cell| keep(cell))
            .min_by_key(|&cell| (from.manhattan_distance(cell), cell))
    }

    /// One of the features `keep` says yes to, chosen at random from `rng`.
    ///
    /// **Uniform over the ones kept, and independent of the order props were
    /// placed in** — the cells are kept sorted (see [`Features::from_map`]), so
    /// the same dice pick the same bed however the map was painted. Two passes
    /// over a handful of cells and no allocation, since a goal asks it in a
    /// tick.
    pub fn pick_where(&self, kind: FeatureKind, rng: &mut SmallRng, keep: impl Fn(Point) -> bool) -> Option<Point> {
        let cells = self.cells(kind);
        let count = cells.iter().filter(|&&cell| keep(cell)).count();
        if count == 0 {
            return None;
        }
        let nth = rng.random_range(0..count);
        cells.iter().copied().filter(|&cell| keep(cell)).nth(nth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Object, ObjectKind, Size, FLOOR, PIXELS_PER_CELL};

    /// The index of a map whose wiring is not what these tests are about.
    fn features_of(map: &Map) -> Features {
        Features::from_map(map, &Supply::everywhere(map.size()))
    }

    #[test]
    fn a_fridge_with_no_power_and_a_toilet_with_no_drain_are_not_somewhere_to_go() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        for (name, cell) in [("fridge", Point::new(3, 4)), ("toilet", Point::new(7, 2)), ("computer", Point::new(5, 8)), ("bed 1", Point::new(1, 1))] {
            map.add_object(ObjectLayer::Props, prop(name, cell));
        }
        let features = Features::from_map(&map, &Supply::from_map(&map));
        for kind in [FeatureKind::Food, FeatureKind::Water, FeatureKind::Toilet, FeatureKind::Entertainment, FeatureKind::Workstation] {
            assert_eq!(features.count(kind), 0, "{kind:?} works without being connected");
        }
        assert!(!features.is_enterable(Point::new(7, 2)), "a broken toilet is not entered either");
        assert_eq!(features.count(FeatureKind::Bed), 1, "a bed needs nothing");

        crate::map::utilities::serve_everything(&mut map, Point::new(0, 9), Point::new(9, 9));
        let features = Features::from_map(&map, &Supply::from_map(&map));
        for kind in [FeatureKind::Food, FeatureKind::Water, FeatureKind::Toilet, FeatureKind::Workstation] {
            assert_eq!(features.count(kind), 1, "{kind:?}, once connected");
        }
        assert_eq!(features.count(FeatureKind::Entertainment), 0, "an office computer is work-only");
    }

    fn prop(name: &str, cell: Point) -> Object {
        // In the middle of the cell, where the editor would have put it.
        Object {
            at: Point::new(
                cell.x * PIXELS_PER_CELL + PIXELS_PER_CELL / 2,
                cell.y * PIXELS_PER_CELL + PIXELS_PER_CELL / 2,
            ),
            kind: ObjectKind::new(name),
        }
    }

    #[test]
    fn entering_a_feature_bypasses_only_its_own_prop() {
        use crate::map::{WALL, VOID};
        let cell = Point::new(2, 2);
        for (name, kind) in [("bed 1", FeatureKind::Bed), ("toilet", FeatureKind::Toilet), ("throne", FeatureKind::Throne)] {
            for terrain in [WALL, VOID] {
                let mut map = Map::new(Size::new(5, 5), FLOOR);
                map.set_terrain(cell, terrain);
                map.add_object(ObjectLayer::Props, prop(name, cell));
                let features = features_of(&map);
                assert!(!features.is_enterable(cell), "{name} on {terrain:?}");
                assert_eq!(features.count(kind), 0, "an unusable feature is not a goal");
            }
            for obstacle in ["crate", "bed 2", "toilet", "unknown prop"] {
                let mut map = Map::new(Size::new(5, 5), FLOOR);
                map.add_object(ObjectLayer::Props, prop(name, cell));
                map.add_object(ObjectLayer::Props, prop(obstacle, cell));
                let features = features_of(&map);
                assert!(!features.is_enterable(cell), "{name} stacked with {obstacle}");
                assert_eq!(features.count(kind), 0);
            }
            let mut map = Map::new(Size::new(5, 5), FLOOR);
            map.add_object(ObjectLayer::Props, prop(name, cell));
            assert!(features_of(&map).is_enterable(cell), "{name} alone on floor");
        }
    }

    #[test]
    fn a_fridge_on_the_map_is_food_and_water_in_the_index_and_is_not_a_bed() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        map.add_object(ObjectLayer::Props, prop("fridge", Point::new(3, 4)));
        map.add_object(ObjectLayer::Props, prop("crate", Point::new(6, 6)));

        let features = features_of(&map);
        for kind in [FeatureKind::Food, FeatureKind::Water] {
            assert_eq!(features.count(kind), 1, "{kind:?}");
            assert_eq!(features.nearest(kind, Point::new(0, 0)), Some(Point::new(3, 4)), "{kind:?}");
        }
        assert_eq!(features.count(FeatureKind::Toilet), 0, "a fridge is not a toilet");
        assert_eq!(features.count(FeatureKind::Bed), 0, "nor a bed, and a crate is scenery");
    }

    #[test]
    fn every_bed_the_editor_can_place_is_somewhere_to_sleep_and_is_slept_in() {
        for number in 1..=6 {
            let name = format!("bed {number}");
            assert_eq!(kinds_of(&name).collect::<Vec<_>>(), [FeatureKind::Bed], "{name}");

            let mut map = Map::new(Size::new(10, 10), FLOOR);
            let cell = Point::new(5, 5);
            map.add_object(ObjectLayer::Props, prop(&name, cell));
            let features = features_of(&map);
            assert_eq!(features.nearest(FeatureKind::Bed, Point::new(0, 0)), Some(cell), "{name}");
            assert!(features.is_enterable(cell), "{name} is slept in, so it is entered");
        }
        assert_eq!(FeatureKind::Bed.access(), Access::Entered);
    }

    #[test]
    fn nearest_where_skips_what_it_is_told_to_and_says_none_when_nothing_is_left() {
        let mut map = Map::new(Size::new(20, 20), FLOOR);
        let (near, far) = (Point::new(2, 2), Point::new(15, 15));
        map.add_object(ObjectLayer::Props, prop("bed 1", near));
        map.add_object(ObjectLayer::Props, prop("bed 2", far));
        let features = features_of(&map);
        let from = Point::new(0, 0);

        assert_eq!(features.nearest_where(FeatureKind::Bed, from, |_| true), Some(near));
        assert_eq!(features.nearest_where(FeatureKind::Bed, from, |cell| cell != near), Some(far));
        assert_eq!(features.nearest_where(FeatureKind::Bed, from, |_| false), None, "not a refused one instead");
        assert_eq!(features.nearest_where(FeatureKind::Toilet, from, |_| true), None);
    }

    #[test]
    fn pick_where_only_picks_what_it_is_allowed_and_reaches_all_of_it() {
        use rand::SeedableRng;
        let mut map = Map::new(Size::new(20, 20), FLOOR);
        let beds = [Point::new(2, 2), Point::new(8, 2), Point::new(14, 2)];
        for (i, bed) in beds.iter().enumerate() {
            map.add_object(ObjectLayer::Props, prop(&format!("bed {}", i + 1), *bed));
        }
        let features = features_of(&map);
        let mut rng = SmallRng::seed_from_u64(1);

        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..200 {
            let bed = features.pick_where(FeatureKind::Bed, &mut rng, |cell| cell != beds[1]);
            seen.insert(bed.expect("two are allowed"));
        }
        assert_eq!(seen, [beds[0], beds[2]].into(), "both allowed ones, and never the refused one");
        assert_eq!(features.pick_where(FeatureKind::Bed, &mut rng, |_| false), None);
        assert_eq!(features.pick_where(FeatureKind::Toilet, &mut rng, |_| true), None);
    }

    #[test]
    fn the_same_dice_pick_the_same_bed_whatever_order_the_beds_were_painted_in() {
        use rand::SeedableRng;
        let beds = [Point::new(2, 2), Point::new(8, 5), Point::new(14, 3), Point::new(5, 9)];
        let build = |order: &[usize]| {
            let mut map = Map::new(Size::new(20, 20), FLOOR);
            for &i in order {
                map.add_object(ObjectLayer::Props, prop(&format!("bed {}", i + 1), beds[i]));
            }
            features_of(&map)
        };
        let (forwards, backwards) = (build(&[0, 1, 2, 3]), build(&[3, 2, 1, 0]));
        for seed in 0..50 {
            let pick = |features: &Features| {
                features.pick_where(FeatureKind::Bed, &mut SmallRng::seed_from_u64(seed), |_| true)
            };
            assert_eq!(pick(&forwards), pick(&backwards), "seed {seed}");
        }
    }

    #[test]
    fn a_toilet_on_the_map_is_a_toilet_in_the_index_and_nothing_else() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        map.add_object(ObjectLayer::Props, prop("toilet", Point::new(7, 2)));

        let features = features_of(&map);
        assert_eq!(features.nearest(FeatureKind::Toilet, Point::new(0, 0)), Some(Point::new(7, 2)));
        assert_eq!(features.count(FeatureKind::Food) + features.count(FeatureKind::Water), 0);
    }

    #[test]
    fn the_nearest_fridge_is_the_nearest_one() {
        let mut map = Map::new(Size::new(20, 20), FLOOR);
        for cell in [Point::new(1, 1), Point::new(18, 18), Point::new(9, 8)] {
            map.add_object(ObjectLayer::Props, prop("fridge", cell));
        }
        let features = features_of(&map);
        assert_eq!(features.nearest(FeatureKind::Food, Point::new(12, 12)), Some(Point::new(9, 8)));
    }

    #[test]
    fn two_fridges_equally_far_away_are_settled_the_same_way_whatever_order_they_were_placed_in() {
        let (a, b) = (Point::new(2, 5), Point::new(8, 5));
        let from = Point::new(5, 5);
        let mut forwards = Map::new(Size::new(10, 10), FLOOR);
        let mut backwards = forwards.clone();
        forwards.add_object(ObjectLayer::Props, prop("fridge", a));
        forwards.add_object(ObjectLayer::Props, prop("fridge", b));
        backwards.add_object(ObjectLayer::Props, prop("fridge", b));
        backwards.add_object(ObjectLayer::Props, prop("fridge", a));

        assert_eq!(
            features_of(&forwards).nearest(FeatureKind::Food, from),
            features_of(&backwards).nearest(FeatureKind::Food, from),
        );
    }

    #[test]
    fn a_world_with_no_fridge_has_nowhere_to_eat() {
        let map = Map::new(Size::new(4, 4), FLOOR);
        assert_eq!(features_of(&map).nearest(FeatureKind::Food, Point::new(1, 1)), None);
    }

    #[test]
    fn a_toilet_is_entered_and_a_fridge_is_touched() {
        assert_eq!(FeatureKind::Toilet.access(), Access::Entered);
        assert_eq!(FeatureKind::Food.access(), Access::Beside);
        assert_eq!(FeatureKind::Water.access(), Access::Beside);
    }

    /// A computer is furniture with a chair in front of it, not a cubicle:
    /// it is used from beside it, and its own cell stays walled off like any
    /// other prop's.
    #[test]
    fn a_computer_on_the_map_is_somewhere_to_have_a_go_at_something_and_is_used_from_beside_it() {
        let mut map = crate::sim::testing::home_map(Size::new(10, 10));
        let computer = Point::new(4, 8);
        map.add_object(ObjectLayer::Props, prop("computer", computer));

        let features = features_of(&map);
        assert_eq!(features.count(FeatureKind::Entertainment), 1);
        assert_eq!(features.nearest(FeatureKind::Entertainment, Point::new(0, 0)), Some(computer));
        assert_eq!(FeatureKind::Entertainment.access(), Access::Beside);
        assert!(!features.is_enterable(computer));
        assert_eq!(features.count(FeatureKind::Food), 0, "a computer is not lunch");
    }

    /// A TV is watched from across the room — near enough and in sight — and
    /// needs power like a computer, but is not a home's alone.
    #[test]
    fn a_tv_is_watched_from_up_to_three_cells_away_in_sight_of_it() {
        let mut map = Map::new(Size::new(12, 12), FLOOR);
        let tv = Point::new(6, 6);
        map.add_object(ObjectLayer::Props, prop("tv", tv));
        let features = features_of(&map);
        assert!(features.has(FeatureKind::Television, tv), "a public TV is anybody's");
        assert_eq!(features.count(FeatureKind::Entertainment), 0, "a TV is not a computer");
        assert_eq!(FeatureKind::Television.access(), Access::Watched);
        assert!(!features.is_enterable(tv));
        let unwired = Features::from_map(&map, &Supply::from_map(&map));
        assert_eq!(unwired.count(FeatureKind::Television), 0, "no picture without power");

        for (cell, watchable) in [
            (Point::new(6, 3), true),  // three straight out
            (Point::new(4, 4), true),  // two across, two along
            (Point::new(5, 6), true),  // right beside it
            (Point::new(6, 2), false), // four away
            (Point::new(3, 4), false), // three across and two along is too far
            (tv, false),               // on it
        ] {
            assert_eq!(can_watch(&map, cell, tv), watchable, "{cell:?}");
        }
        // A wall between them hides the picture...
        map.set_terrain(Point::new(6, 4), crate::map::WALL);
        assert!(!can_watch(&map, Point::new(6, 3), tv));
        // ...and furniture does not.
        map.add_object(ObjectLayer::Props, prop("crate", Point::new(5, 5)));
        assert!(can_watch(&map, Point::new(4, 4), tv));
    }

    #[test]
    fn the_viewing_offsets_are_every_cell_in_reach_once_and_nearest_first() {
        let mut seen = std::collections::BTreeSet::new();
        let mut last = 0;
        for (dx, dy) in VIEWING_OFFSETS {
            let d2 = i32::from(dx).pow(2) + i32::from(dy).pow(2);
            assert!(d2 >= last, "nearest first");
            last = d2;
            assert!(seen.insert((dx, dy)), "once");
            assert!(within_viewing_distance(Point::new(dx.into(), dy.into()), Point::ORIGIN));
        }
    }

    /// A public computer is office equipment, whether somebody works at it
    /// yet or not. It never appears among gaming targets.
    #[test]
    fn an_office_computer_is_a_workstation_and_never_entertainment() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        let computer = Point::new(4, 8);
        map.add_object(ObjectLayer::Props, prop("computer", computer));

        let features = features_of(&map);
        assert!(features.has(FeatureKind::Workstation, computer));
        assert!(!features.has(FeatureKind::Entertainment, computer));
        assert_eq!(features.nearest(FeatureKind::Entertainment, computer), None);
        assert_eq!(FeatureKind::Workstation.access(), Access::Beside);
        assert_eq!(kinds_of("computer").collect::<Vec<_>>(), [FeatureKind::Entertainment, FeatureKind::Workstation]);
        assert_eq!(features.count(FeatureKind::Workstation), 1);
        assert!(!features.has(FeatureKind::Workstation, Point::new(0, 0)));
    }

    /// A scrap bin is where a collector stores scrap, used from beside it,
    /// and it needs nothing plugged in to be one.
    #[test]
    fn a_scrap_bin_is_somewhere_to_store_scrap_from_beside_it_with_no_power() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        let bin = Point::new(2, 7);
        map.add_object(ObjectLayer::Props, prop("scrap bin", bin));

        let features = Features::from_map(&map, &Supply::from_map(&map));
        assert!(features.has(FeatureKind::ScrapBin, bin), "nothing wired, and still a bin");
        assert_eq!(FeatureKind::ScrapBin.access(), Access::Beside);
        assert!(!features.is_enterable(bin));
        assert_eq!(kinds_of("scrap bin").collect::<Vec<_>>(), [FeatureKind::ScrapBin]);
        assert_eq!(features.count(FeatureKind::Workstation), 0, "a bin is not a desk");
    }

    /// A guard post is stood at from beside it, needs nothing plugged in,
    /// and is one even set into a wall, which is where a district has them.
    #[test]
    fn a_guard_post_is_somewhere_to_keep_watch_from_beside_it_even_in_a_wall() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        let post = Point::new(0, 4);
        map.set_terrain(post, crate::map::WALL);
        map.add_object(ObjectLayer::Props, prop("guard post", post));

        let features = Features::from_map(&map, &Supply::from_map(&map));
        assert!(features.has(FeatureKind::GuardPost, post));
        assert_eq!(FeatureKind::GuardPost.access(), Access::Beside);
        assert!(!features.is_enterable(post));
        assert_eq!(kinds_of("guard post").collect::<Vec<_>>(), [FeatureKind::GuardPost]);
        assert_eq!(features.count(FeatureKind::Workstation), 0, "a post is not a desk");
    }

    #[test]
    fn a_toilets_cell_is_enterable_and_a_fridges_is_not() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        let toilet = Point::new(7, 2);
        let fridge = Point::new(3, 4);
        map.add_object(ObjectLayer::Props, prop("toilet", toilet));
        map.add_object(ObjectLayer::Props, prop("fridge", fridge));

        let features = features_of(&map);
        assert!(features.is_enterable(toilet));
        assert!(!features.is_enterable(fridge));
        assert!(!features.is_enterable(Point::new(0, 0)), "a plain empty cell");
    }
}
