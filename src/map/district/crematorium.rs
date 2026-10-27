//! The **crematorium**: on a lot on the edge of the district, with
//! [`CREMATORIA`] furnaces the undertakers bring the dead to (`sim::job`).

use rand::rngs::SmallRng;
use rand::RngExt;

use super::{object, public_room, Lot, LotBuilding, Rect, Side};
use crate::map::{Map, Object};

/// What [`super::District::the`] calls it.
pub const NAME: &str = "crematorium";
/// Where the dead are brought: an undertaker's workplace, three watches to
/// each (`sim::job`).
pub const CREMATORIUM: &str = "crematorium";
/// How many crematoria the crematorium has, and so how many undertakers a
/// district takes: three to each.
pub const CREMATORIA: usize = 2;
const WALL: &str = "wall purple";
const FLOOR: &str = "floor white";

/// One, on the first edge lot left in the shuffled order.
pub const SPEC: LotBuilding = LotBuilding {
    name: NAME,
    count: 1,
    whole_block: false,
    wanted: |lot| lot.edge_sides().next().is_some(),
    rank: |_, _, _| 0,
    build: crematorium,
    tiles: &[WALL, FLOOR],
    props: &[CREMATORIUM],
};

/// The crematorium, on `lot`: a room as big as a shop with a plain door —
/// nobody's property — on a side facing the edge street, and a
/// [`CREMATORIUM`] in each of the [`CREMATORIA`] corners facing the door.
fn crematorium(rng: &mut SmallRng, map: &mut Map, props: &mut Vec<Object>, lot: Lot) -> Rect {
    let sides: Vec<Side> = lot.edge_sides().collect();
    let side = sides[rng.random_range(0..sides.len())];
    let room = public_room(rng, map, props, lot, side, WALL, FLOOR);
    for across in [0, room.wide - 1] {
        props.push(object(CREMATORIUM, room.at(across, room.deep - 1)));
    }
    room.walls
}

#[cfg(test)]
mod tests {
    use super::super::tests::{props_in, SEEDS};
    use super::super::*;
    use super::*;
    use crate::map::ObjectLayer;

    /// The crematorium: its furnaces and nothing else, nobody's, on the
    /// edge, and none anywhere else.
    #[test]
    fn a_district_has_one_crematorium_on_its_edge() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let crematorium = district.the(NAME);
            let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| crematorium.interior().contains(prop.cell()));
            assert!(inside.clone().all(|prop| prop.kind.as_str() == CREMATORIUM), "seed {seed}: the crematorium holds crematoria only");
            assert_eq!(inside.count(), CREMATORIA, "seed {seed}");
            assert_eq!(props_in(map, Rect::new(0, 0, WIDTH, HEIGHT), CREMATORIUM), CREMATORIA, "seed {seed}: none elsewhere");
            let properties = crate::sim::Properties::from_map(map);
            for cell in crematorium.cells() {
                assert_eq!(properties.of(cell), None, "seed {seed}: the crematorium is nobody's property");
            }
            let on_the_edge = crematorium.x - STREET <= 1
                || WIDTH - STREET - 1 - crematorium.right() <= 1
                || crematorium.y - STREET <= 1
                || HEIGHT - STREET - 1 - crematorium.top() <= 1;
            assert!(on_the_edge, "seed {seed}: the crematorium at {crematorium:?} is not on the edge");
        }
    }
}
