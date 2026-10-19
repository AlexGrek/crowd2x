//! The **restaurants**, [`RESTAURANTS`] of them, one on the lot nearest the
//! middle of each quarter of the district so nobody is far from one: public,
//! with fridges anybody may eat and drink from and toilets anybody may use.

use rand::rngs::SmallRng;
use rand::RngExt;

use super::{object, public_room, Lot, LotBuilding, Rect, FRIDGE, HEIGHT, LOT_H, LOT_W, TOILET, WIDTH};
use crate::map::{Map, Object, Point};

/// What [`super::District::all`] calls them.
pub const NAME: &str = "restaurant";
/// How many restaurants: one to each quarter of the district.
pub const RESTAURANTS: usize = 4;
const WALL: &str = "wall red";
const FLOOR: &str = "tiles yellow";

/// The middle of each quarter, in the order the restaurants are placed.
const QUARTERS: [(i32, i32); RESTAURANTS] = [(1, 1), (3, 1), (1, 3), (3, 3)];

/// One to each quarter, on the lot whose middle is nearest the quarter's.
pub const SPEC: LotBuilding = LotBuilding {
    name: NAME,
    count: RESTAURANTS,
    wanted: |_| true,
    rank: |_, index, lot| {
        let (qx, qy) = QUARTERS[index];
        lot.origin.offset(LOT_W / 2, LOT_H / 2).manhattan_distance(Point::new(WIDTH * qx / 4, HEIGHT * qy / 4))
    },
    build: restaurant,
    tiles: &[WALL, FLOOR],
    props: &[FRIDGE, TOILET],
};

/// A restaurant, on `lot`: a room as big as a shop with a plain door —
/// nobody's property, so anybody may come in and use what is inside — on one
/// of the lot's street sides, kept two cells off the corners. Fridges along
/// the wall facing the door, and a toilet against each side wall a row in from
/// the door, so the row inside the door stays clear and everything has free
/// cells beside it to be used from. Laid out facing the door whichever side it
/// is on.
fn restaurant(rng: &mut SmallRng, map: &mut Map, props: &mut Vec<Object>, lot: Lot) -> Rect {
    let side = lot.outer[rng.random_range(0..2)];
    let room = public_room(rng, map, props, lot, side, WALL, FLOOR);
    let mut fridges = vec![0, room.wide - 1];
    if room.wide % 2 == 1 {
        fridges.push(room.wide / 2);
    }
    for across in fridges {
        props.push(object(FRIDGE, room.at(across, room.deep - 1)));
    }
    for across in [0, room.wide - 1] {
        props.push(object(TOILET, room.at(across, 1)));
    }
    room.walls
}

#[cfg(test)]
mod tests {
    use super::super::tests::{props_in, SEEDS};
    use super::super::*;
    use super::*;
    use crate::map::ObjectLayer;

    /// The restaurants: one to each quarter, fridges and toilets and nothing
    /// else, open to anybody.
    #[test]
    fn a_district_has_a_public_restaurant_in_each_quarter() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let restaurants: Vec<Rect> = district.all(NAME).collect();
            assert_eq!(restaurants.len(), RESTAURANTS, "seed {seed}");
            let properties = crate::sim::Properties::from_map(map);
            for (i, &restaurant) in restaurants.iter().enumerate() {
                assert!(props_in(map, restaurant, FRIDGE) >= 2, "seed {seed} {restaurant:?}");
                assert_eq!(props_in(map, restaurant, TOILET), 2, "seed {seed} {restaurant:?}");
                let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| restaurant.interior().contains(prop.cell()));
                assert!(inside.clone().all(|prop| [FRIDGE, TOILET].contains(&prop.kind.as_str())), "seed {seed} {restaurant:?}");
                for cell in restaurant.cells() {
                    assert_eq!(properties.of(cell), None, "seed {seed}: a restaurant is nobody's property");
                }
                let (qx, qy) = ((i % 2) as i32, (i / 2) as i32);
                let (mx, my) = (restaurant.x + restaurant.w / 2, restaurant.y + restaurant.h / 2);
                assert_eq!((mx * 2 / WIDTH, my * 2 / HEIGHT), (qx, qy), "seed {seed}: restaurant {i} at {restaurant:?} is out of its quarter");
            }
        }
    }
}
