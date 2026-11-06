//! [`Training`]: the **soldiers' ground** the king picks out in the wasteland,
//! where the district's soldiers drill.
//!
//! **The king decides** (`Task::decree`, from the throne): once there are
//! soldiers to drill (a roll of them on the map, [`FeatureKind::Ranks`]) and no
//! ground yet, it finds one ([`choose_ground`]: [`FROM_WALLS`] out from the
//! district's wall — twenty to thirty cells — on open ground anybody can get
//! to, as near as that allows to the palace's street, where the soldiers are on the roll) and its centre is
//! marked ([`Training::found`]). It is decided once, and for good.
//!
//! **What is drilled there.** The commander stands in the middle, at
//! [`Training::center`]; each soldier walks in circles round it, a lap of
//! [`LAP`] waypoints at [`RADIUS`] cells ([`ring`]). Where a soldier is on its
//! lap is where it *is*: the next waypoint is worked out from its position
//! ([`next_waypoint`]), so a drill keeps nothing but the ground itself.
//!
//! Written only by the world step (`Effect::DecreeGround`), read by every
//! brain through `Think::training`, and saved.

use crate::map::{Bounds, Map, Point, Size};
use crate::save::{invalid, pb, Invalid};

use super::feature::FeatureKind;
use super::outposts::Outposts;

/// How many soldiers a district keeps: their places on the roll (no barracks: nothing is built for them).
pub const SOLDIERS: usize = 10;

/// How many commanders: one stands in the middle of the ground.
pub const COMMANDERS: usize = 1;

/// How far from the district's wall the whole of the ground lies, nearest and
/// furthest cell, in cells: twenty to thirty.
pub const FROM_WALLS: (i32, i32) = (20, 30);

/// How far from the commander the soldiers walk their circles, in cells.
pub const RADIUS: i32 = 4;

/// The waypoints of a lap.
pub const LAP: usize = 16;

/// The ground's footprint: a square of this radius round its centre — the
/// circle and a cell round it.
pub const FOOTPRINT_RADIUS: i32 = RADIUS + 1;

/// A lap, as offsets from the centre, **clockwise** on the screen: sixteen
/// cells on a circle of [`RADIUS`], none twice.
const LAP_OFFSETS: [(i32, i32); LAP] = [
    (4, 0),
    (4, 2),
    (3, 3),
    (2, 4),
    (0, 4),
    (-2, 4),
    (-3, 3),
    (-4, 2),
    (-4, 0),
    (-4, -2),
    (-3, -3),
    (-2, -4),
    (0, -4),
    (2, -4),
    (3, -3),
    (4, -2),
];

/// The cells a soldier walks round the ground at `center`, in lap order.
pub fn ring(center: Point) -> [Point; LAP] {
    LAP_OFFSETS.map(|(dx, dy)| center.offset(dx, dy))
}

/// The square of ground at `center`: the circle and the cell round it.
pub fn footprint(center: Point) -> Bounds {
    let side = 2 * FOOTPRINT_RADIUS + 1;
    Bounds::new(center.offset(-FOOTPRINT_RADIUS, -FOOTPRINT_RADIUS), Size::new(side, side))
}

/// Where a soldier on the ground at `center`, standing at `here`, walks next:
/// the waypoint after the one it is nearest — or, with the ring still some way
/// off (it has just come out), the waypoint of its own `slot` among the
/// `soldiers`, so that they come onto the lap spread round it rather than in
/// a heap where they came in.
pub fn next_waypoint(center: Point, here: Point, slot: usize, soldiers: usize) -> Point {
    ring(center)[next_index(center, here, slot, soldiers)]
}

/// [`next_waypoint`], as its place on the lap.
pub fn next_index(center: Point, here: Point, slot: usize, soldiers: usize) -> usize {
    let (nearest, gap) = ring(center)
        .iter()
        .enumerate()
        .map(|(index, &cell)| (index, here.manhattan_distance(cell)))
        .min_by_key(|&(index, gap)| (gap, index))
        .unwrap_or((0, 0));
    if gap > 2 {
        return slot * LAP / soldiers.max(1) % LAP;
    }
    (nearest + 1) % LAP
}

/// The soldiers' ground, once the king has picked it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Training {
    /// The middle of the ground, where the commander stands.
    ground: Option<Point>,
    /// The day it was decreed, 0 with none.
    decreed: u32,
    /// The day the king last looked for a place and found none; 0, never.
    fruitless: u32,
}

impl Training {
    pub const fn new() -> Training {
        Training { ground: None, decreed: 0, fruitless: 0 }
    }

    /// The middle of the ground, if the king has picked it.
    pub const fn center(&self) -> Option<Point> {
        self.ground
    }

    /// The day the ground was decreed.
    pub const fn decreed(&self) -> u32 {
        self.decreed
    }

    /// Whether the king still has a ground to pick on `day`: it has not got
    /// one, and has not looked in vain today.
    pub const fn wants_a_ground(&self, day: u32) -> bool {
        self.ground.is_none() && self.fruitless != day
    }

    /// Mark the ground at `center`, if the king has none and it is clear
    /// ([`is_clear`]).
    pub fn found(&mut self, map: &Map, center: Point, outposts: &Outposts, day: u32) -> bool {
        if self.ground.is_some() || !is_clear(map, center, outposts) {
            return false;
        }
        self.ground = Some(center);
        self.decreed = day;
        true
    }

    /// The king found nowhere on `day`.
    pub fn found_nowhere(&mut self, day: u32) {
        self.fruitless = day;
    }

    // --- saving ---

    pub(crate) fn save(&self) -> pb::Training {
        let Training { ground, decreed, fruitless } = *self;
        pb::Training { center: ground.map(Into::into), decreed, fruitless }
    }

    /// The ground as saved: none in a save from before there were soldiers.
    /// Refuses one that no ground could be at (the map and the outposts say).
    pub(crate) fn restore(saved: Option<&pb::Training>, map: &Map, outposts: &Outposts) -> Result<Training, Invalid> {
        let Some(saved) = saved else {
            return Ok(Training::new());
        };
        let ground = match saved.center {
            Some(_) => Some(pb::cell(saved.center, "training ground")?),
            None => None,
        };
        if let Some(center) = ground
            && map.is_endless()
            && !is_clear(map, center, outposts)
        {
            return invalid(format!("no room for a training ground at {}, {}", center.x, center.y));
        }
        Ok(Training { ground, decreed: saved.decreed, fruitless: saved.fruitless })
    }
}

// --- the king's choice ---

/// Whether the king, on duty, picks the ground now (`WorkGoal`, at the
/// throne): it has none, has not looked in vain today, and there are soldiers
/// to drill on it (a roll of them).
pub fn ground_due(think: &super::Think<'_>) -> bool {
    think.training.wants_a_ground(think.clock.day()) && think.features.count(FeatureKind::Command) > 0
}

/// The nearest and the furthest any cell of `bounds` is from `site`, as a
/// king walks.
fn distances(site: Bounds, bounds: Bounds) -> (i32, i32) {
    let (low, high) = (bounds.origin, bounds.max());
    let gap_x = (site.origin.x - high.x).max(low.x - site.max().x).max(0);
    let gap_y = (site.origin.y - high.y).max(low.y - site.max().y).max(0);
    let far = [low, high, Point::new(low.x, high.y), Point::new(high.x, low.y)]
        .into_iter()
        .map(|corner| site.chebyshev_distance(corner))
        .max()
        .unwrap_or(0);
    (gap_x.max(gap_y), far)
}

/// Whether a ground centred on `center` can be there: every cell of its
/// footprint [`FROM_WALLS`] from the site, and open ground anybody can get to
/// — walkable now, nothing built or sown or heaped in it, not good soil
/// (which the farmers want), not a pocket a lake has shut in — and clear of
/// every outpost.
pub fn is_clear(map: &Map, center: Point, outposts: &Outposts) -> bool {
    let ground = footprint(center);
    let (near, far) = distances(map.site(), ground);
    if near < FROM_WALLS.0 || far > FROM_WALLS.1 {
        return false;
    }
    if outposts.iter().any(|outpost| outpost.footprint().grown(1).overlaps(ground)) {
        return false;
    }
    let navigation = map.navigation();
    ground.points().all(|cell| {
        map.can_build(cell) && !map.terrain(cell).is_some_and(|tile| tile.is_fertile()) && navigation.is_outside(cell)
    })
}

/// Where the king puts the ground: the clear spot ([`is_clear`]) nearest, as
/// the crow flies, to `near` — a cell of the soldiers' roll — a
/// lower cell on a tie. `None` with no wasteland or nowhere clear.
///
/// A scan of the band round the district, once a game.
pub fn choose_ground(map: &Map, outposts: &Outposts, near: Point) -> Option<Point> {
    if !map.is_endless() {
        return None;
    }
    let band = map.site().grown(FROM_WALLS.1 - FOOTPRINT_RADIUS + 1);
    band.points()
        .filter(|&center| is_clear(map, center, outposts))
        .min_by_key(|&center| (center.manhattan_distance(near), center))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::FLOOR;

    fn wasteland(seed: u64) -> Map {
        Map::new(Size::new(40, 30), FLOOR).with_wasteland(seed)
    }

    #[test]
    fn a_lap_is_sixteen_different_cells_on_a_circle_round_the_commander() {
        let centre = Point::new(100, 100);
        let lap = ring(centre);
        for (index, cell) in lap.iter().enumerate() {
            assert!(!lap[..index].contains(cell), "{cell:?} twice");
            let (dx, dy) = ((cell.x - centre.x) as f64, (cell.y - centre.y) as f64);
            let radius = (dx * dx + dy * dy).sqrt();
            assert!((radius - RADIUS as f64).abs() < 0.8, "{cell:?} is {radius} from the middle");
            assert!(footprint(centre).contains(*cell));
            let next = lap[(index + 1) % LAP];
            assert!(cell.chebyshev_distance(next) <= 2, "{cell:?} to {next:?} is too long a step");
        }
        assert!(!lap.contains(&centre), "nobody drills where the commander stands");
    }

    #[test]
    fn a_soldier_on_the_lap_walks_on_to_the_next_waypoint_and_one_far_off_comes_onto_its_own() {
        let centre = Point::new(50, 50);
        let lap = ring(centre);
        assert_eq!(next_waypoint(centre, lap[3], 0, SOLDIERS), lap[4]);
        assert_eq!(next_waypoint(centre, lap[15], 0, SOLDIERS), lap[0], "the lap closes");
        assert_eq!(next_waypoint(centre, centre.offset(0, 30), 5, SOLDIERS), lap[8], "slot 5 of 10 is half way round");
        let slots: Vec<Point> = (0..SOLDIERS).map(|slot| next_waypoint(centre, centre.offset(30, 30), slot, SOLDIERS)).collect();
        for (index, cell) in slots.iter().enumerate() {
            assert!(!slots[..index].contains(cell), "two soldiers sent to {cell:?}");
        }
    }

    #[test]
    fn the_king_picks_a_ground_twenty_to_thirty_cells_from_the_wall_nearest_the_roll() {
        for seed in [3, 7, 11] {
            let map = wasteland(seed);
            let near = Point::new(20, 5);
            let center = choose_ground(&map, &Outposts::new(), near).unwrap_or_else(|| panic!("seed {seed}: nowhere"));
            let (nearest, furthest) = distances(map.site(), footprint(center));
            assert!(nearest >= FROM_WALLS.0 && furthest <= FROM_WALLS.1, "seed {seed}: {nearest}..{furthest}");
            assert!(is_clear(&map, center, &Outposts::new()));
            assert!(center.y < 0, "seed {seed}: nearest the roll, above the site: {center:?}");
        }
    }

    #[test]
    fn a_ground_is_picked_once_and_a_day_found_in_vain_is_not_looked_at_again() {
        let map = wasteland(3);
        let mut training = Training::new();
        assert!(training.wants_a_ground(1));
        training.found_nowhere(1);
        assert!(!training.wants_a_ground(1) && training.wants_a_ground(2));
        let center = choose_ground(&map, &Outposts::new(), Point::new(20, 5)).unwrap();
        assert!(!training.found(&map, Point::new(20, 15), &Outposts::new(), 2), "not 20 cells out");
        assert!(training.found(&map, center, &Outposts::new(), 2));
        assert_eq!((training.center(), training.decreed()), (Some(center), 2));
        assert!(!training.wants_a_ground(3));
        assert!(!training.found(&map, center, &Outposts::new(), 3), "one ground");
    }

    #[test]
    fn a_ground_saved_is_a_ground_restored_and_a_made_up_one_is_refused() {
        let map = wasteland(3);
        let mut training = Training::new();
        assert_eq!(Training::restore(None, &map, &Outposts::new()).unwrap(), Training::new());
        let center = choose_ground(&map, &Outposts::new(), Point::new(20, 5)).unwrap();
        assert!(training.found(&map, center, &Outposts::new(), 4));
        training.found_nowhere(2);
        let saved = training.save();
        assert_eq!(Training::restore(Some(&saved), &map, &Outposts::new()).unwrap(), training);
        let broken = pb::Training { center: Some(Point::new(20, 15).into()), ..saved };
        assert!(Training::restore(Some(&broken), &map, &Outposts::new()).is_err());
    }
}
