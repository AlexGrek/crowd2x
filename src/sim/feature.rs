//! What the props on a map are *for*: [`FEATURES`], and the [`Features`]
//! index of where they are.
//!
//! The same shape as `map::TERRAIN`: a static catalogue that binds a prop's
//! **name** — the one a map file stores and the editor's palette paints — to
//! what a brain can do with it. The art stays in the editor; what a fridge
//! looks like is none of this module's business, and that it holds food is
//! none of the editor's.
//!
//! A prop with no entry here is scenery, which today is every prop but the
//! fridge, the toilet, the computer and the beds. A prop with two entries is
//! two things at once: a fridge is food and water.

use rand::rngs::SmallRng;
use rand::RngExt;

use crate::map::{Map, ObjectLayer, Point};

/// What a brain can get out of a feature.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeatureKind {
    Food,
    Water,
    Toilet,
    /// Something to do: a computer, used from the chair beside it.
    Entertainment,
    /// Somewhere to sleep: a bed, slept *in* rather than beside.
    Bed,
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
}

impl FeatureKind {
    pub const fn access(self) -> Access {
        match self {
            FeatureKind::Food | FeatureKind::Water | FeatureKind::Entertainment => Access::Beside,
            // A bed is lain in, not stood next to, and one sleeper to a bed:
            // the toilet's arrangement, and for the same reason.
            FeatureKind::Toilet | FeatureKind::Bed => Access::Entered,
        }
    }
}

pub struct Feature {
    /// The prop name, exactly as the palette and the map file spell it.
    pub name: &'static str,
    pub kind: FeatureKind,
}

/// Every prop a brain can use.
///
/// A name may appear more than once, once per use: the fridge has drinks in
/// it as well as food, rather than there being a second prop to walk to.
///
/// A fridge never runs out: there is no depletion state, so there is nothing
/// about a fridge that can change during a tick, which is what lets the index
/// below be built once and read from every thread.
pub const FEATURES: &[Feature] = &[
    Feature {
        name: "fridge",
        kind: FeatureKind::Food,
    },
    Feature {
        name: "fridge",
        kind: FeatureKind::Water,
    },
    Feature {
        name: "toilet",
        kind: FeatureKind::Toilet,
    },
    Feature {
        name: "computer",
        kind: FeatureKind::Entertainment,
    },
    // The six beds the editor can place. Their art is 48x48 and has no
    // "somebody is in it" strip yet, so a bed does not show that it is being
    // slept in (see `game::props`); the sleeper is what shows it.
    Feature {
        name: "bed 1",
        kind: FeatureKind::Bed,
    },
    Feature {
        name: "bed 2",
        kind: FeatureKind::Bed,
    },
    Feature {
        name: "bed 3",
        kind: FeatureKind::Bed,
    },
    Feature {
        name: "bed 4",
        kind: FeatureKind::Bed,
    },
    Feature {
        name: "bed 5",
        kind: FeatureKind::Bed,
    },
    Feature {
        name: "bed 6",
        kind: FeatureKind::Bed,
    },
];

/// Everything a prop name is for — nothing, for scenery.
pub fn kinds_of(name: &str) -> impl Iterator<Item = FeatureKind> + '_ {
    FEATURES
        .iter()
        .filter(move |feature| feature.name == name)
        .map(|feature| feature.kind)
}

/// Where the things a brain can use are, by cell.
///
/// Built once in `GameState::new` by scanning the map's `Props` layer, so the
/// per-tick cost of finding one is a scan of the fridges — a handful — and
/// never of the crowd or the map.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Features {
    food: Vec<Point>,
    water: Vec<Point>,
    toilet: Vec<Point>,
    entertainment: Vec<Point>,
    bed: Vec<Point>,
    /// Cells whose kind is [`Access::Entered`] — gathered once, across every
    /// kind, so the move step can ask "can anyone at all step in here"
    /// without knowing what the feature is for. A handful of cells on any
    /// real map, so a linear scan is what asking it costs.
    entered: Vec<Point>,
}

impl Features {
    pub fn from_map(map: &Map) -> Features {
        let mut features = Features::default();
        for object in map.objects(ObjectLayer::Props) {
            let cell = object.cell();
            for kind in kinds_of(object.kind.as_str()) {
                match kind {
                    FeatureKind::Food => features.food.push(cell),
                    FeatureKind::Water => features.water.push(cell),
                    FeatureKind::Toilet => features.toilet.push(cell),
                    FeatureKind::Entertainment => features.entertainment.push(cell),
                    FeatureKind::Bed => features.bed.push(cell),
                }
                if kind.access() == Access::Entered && !features.entered.contains(&cell) {
                    features.entered.push(cell);
                }
            }
        }
        // Sorted, so nothing that picks among them depends on the order the
        // props were painted in.
        for cells in [&mut features.food, &mut features.water, &mut features.toilet, &mut features.entertainment, &mut features.bed] {
            cells.sort_unstable();
        }
        features
    }

    /// Whether `cell` is a feature that must be entered to use, and so may
    /// be stepped onto — by whoever currently holds it, via
    /// [`crate::sim::occupancy::Occupancy`] — despite being impassable to
    /// ordinary movement. See [`Access::Entered`].
    pub fn is_enterable(&self, cell: Point) -> bool {
        self.entered.contains(&cell)
    }

    fn cells(&self, kind: FeatureKind) -> &[Point] {
        match kind {
            FeatureKind::Food => &self.food,
            FeatureKind::Water => &self.water,
            FeatureKind::Toilet => &self.toilet,
            FeatureKind::Entertainment => &self.entertainment,
            FeatureKind::Bed => &self.bed,
        }
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
    fn a_fridge_on_the_map_is_food_and_water_in_the_index_and_is_not_a_bed() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        map.add_object(ObjectLayer::Props, prop("fridge", Point::new(3, 4)));
        map.add_object(ObjectLayer::Props, prop("crate", Point::new(6, 6)));

        let features = Features::from_map(&map);
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
            let features = Features::from_map(&map);
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
        let features = Features::from_map(&map);
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
        let features = Features::from_map(&map);
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
            Features::from_map(&map)
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

        let features = Features::from_map(&map);
        assert_eq!(features.nearest(FeatureKind::Toilet, Point::new(0, 0)), Some(Point::new(7, 2)));
        assert_eq!(features.count(FeatureKind::Food) + features.count(FeatureKind::Water), 0);
    }

    #[test]
    fn the_nearest_fridge_is_the_nearest_one() {
        let mut map = Map::new(Size::new(20, 20), FLOOR);
        for cell in [Point::new(1, 1), Point::new(18, 18), Point::new(9, 8)] {
            map.add_object(ObjectLayer::Props, prop("fridge", cell));
        }
        let features = Features::from_map(&map);
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
            Features::from_map(&forwards).nearest(FeatureKind::Food, from),
            Features::from_map(&backwards).nearest(FeatureKind::Food, from),
        );
    }

    #[test]
    fn a_world_with_no_fridge_has_nowhere_to_eat() {
        let map = Map::new(Size::new(4, 4), FLOOR);
        assert_eq!(Features::from_map(&map).nearest(FeatureKind::Food, Point::new(1, 1)), None);
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
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        let computer = Point::new(4, 8);
        map.add_object(ObjectLayer::Props, prop("computer", computer));

        let features = Features::from_map(&map);
        assert_eq!(features.count(FeatureKind::Entertainment), 1);
        assert_eq!(features.nearest(FeatureKind::Entertainment, Point::new(0, 0)), Some(computer));
        assert_eq!(FeatureKind::Entertainment.access(), Access::Beside);
        assert!(!features.is_enterable(computer));
        assert_eq!(features.count(FeatureKind::Food), 0, "a computer is not lunch");
    }

    #[test]
    fn a_toilets_cell_is_enterable_and_a_fridges_is_not() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        let toilet = Point::new(7, 2);
        let fridge = Point::new(3, 4);
        map.add_object(ObjectLayer::Props, prop("toilet", toilet));
        map.add_object(ObjectLayer::Props, prop("fridge", fridge));

        let features = Features::from_map(&map);
        assert!(features.is_enterable(toilet));
        assert!(!features.is_enterable(fridge));
        assert!(!features.is_enterable(Point::new(0, 0)), "a plain empty cell");
    }
}
