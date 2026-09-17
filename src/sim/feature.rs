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
//! fridge and the toilet. A prop with two entries is two things at once: a
//! fridge is food and water.

use crate::map::{Map, ObjectLayer, Point};

/// What a brain can get out of a feature.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeatureKind {
    Food,
    Water,
    Toilet,
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
}

impl Features {
    pub fn from_map(map: &Map) -> Features {
        let mut features = Features::default();
        for object in map.objects(ObjectLayer::Props) {
            for kind in kinds_of(object.kind.as_str()) {
                match kind {
                    FeatureKind::Food => features.food.push(object.cell()),
                    FeatureKind::Water => features.water.push(object.cell()),
                    FeatureKind::Toilet => features.toilet.push(object.cell()),
                }
            }
        }
        features
    }

    fn cells(&self, kind: FeatureKind) -> &[Point] {
        match kind {
            FeatureKind::Food => &self.food,
            FeatureKind::Water => &self.water,
            FeatureKind::Toilet => &self.toilet,
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
    fn a_fridge_on_the_map_is_food_and_water_in_the_index_and_a_bed_is_nothing() {
        let mut map = Map::new(Size::new(10, 10), FLOOR);
        map.add_object(ObjectLayer::Props, prop("fridge", Point::new(3, 4)));
        map.add_object(ObjectLayer::Props, prop("bed 1", Point::new(6, 6)));

        let features = Features::from_map(&map);
        for kind in [FeatureKind::Food, FeatureKind::Water] {
            assert_eq!(features.count(kind), 1, "{kind:?}");
            assert_eq!(features.nearest(kind, Point::new(0, 0)), Some(Point::new(3, 4)), "{kind:?}");
        }
        assert_eq!(features.count(FeatureKind::Toilet), 0, "a fridge is not a toilet");
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
}
