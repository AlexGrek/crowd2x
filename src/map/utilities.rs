//! What the networks under the floor connect: [`Supply`].
//!
//! Two utilities, each a source standing on the street, a network laid in a
//! [`GridLayer`] under the floor, and the things at the other end that do
//! not work without it:
//!
//! * **Power.** A `"transformer"` puts high voltage into the **power line**
//!   under it; the line runs to **distribution boxes**, and a box that the
//!   line reaches gives out low voltage along the **wiring** — and only
//!   wiring is what a lamp, a fridge or a computer can plug into. A consumer
//!   standing on a power line gets nothing (it would not survive it), and a
//!   box that no line reaches is a box of dead switches.
//! * **Water.** A `"sewer"` drains the **pipes** under it, and a toilet with a
//!   pipe under it that reaches a sewer flushes.
//!
//! **Everything connects through its own cell** — the one its centre falls
//! in, [`Object::cell`]'s rule, which is also the cell it blocks. A fridge is
//! plugged in when the wiring runs under it; a transformer feeds the line it
//! stands on. That is what "exists on both levels" means for a transformer or
//! a sewer: a prop in the street that blocks a cell, and a node in its
//! network in that same cell. A box and a pipe are only in the network.
//!
//! **A box can be switched off** — in the game, from its panel
//! (`sim::GameState::switch_box`). An off box still passes the line through
//! it, since it is a node on the line, but gives nothing out: the wiring it
//! fed goes dead, and whatever was plugged into that wiring stops working.
//! Which boxes are off is the simulation's state, not the map's, so it is
//! handed in ([`Supply::with_boxes_off`]) rather than read off the map.
//!
//! This is plain Rust like the rest of `map/`, and derived like
//! passability: [`Supply::from_map`] floods both networks once, and the
//! simulation keeps the answer, since nothing in a tick changes the map.
//! Whether the result *matters* is the reader's business — `sim::feature`
//! does not index a fridge with no power or a toilet with no drain, and
//! `crate::lighting` leaves a lamp with no power dark.

use super::grid::{power, water};
use super::{GridLayer, Map, Object, ObjectKind, ObjectLayer, Point, Size, PIXELS_PER_CELL};

/// What a network carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Utility {
    Power,
    Water,
}

/// A network a cell can be live on. Power has two, since a power line and
/// wiring are not the same thing to plug into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Network {
    /// High voltage, fed by a transformer.
    Line,
    /// Low voltage, fed by a box on a live line.
    Wiring,
    /// Drained by a sewer.
    Pipes,
}

/// What feeds a network, by the prop name a map stores.
pub const SOURCES: &[(&str, Utility)] = &[("transformer", Utility::Power), ("sewer", Utility::Water)];

/// What does not work without a network, by the name a map stores — a prop's
/// or a lamp's. A name may appear once: nothing needs two utilities yet.
pub const CONSUMERS: &[(&str, Utility)] = &[
    ("fridge", Utility::Power),
    ("computer", Utility::Power),
    ("ceiling lamp", Utility::Power),
    ("tube lamp", Utility::Power),
    ("toilet", Utility::Water),
];

/// The utility something called `name` cannot work without, if any.
pub fn needs(name: &str) -> Option<Utility> {
    CONSUMERS.iter().find(|(known, _)| *known == name).map(|&(_, utility)| utility)
}

/// The utility something called `name` feeds, if it is a source.
pub fn source_of(name: &str) -> Option<Utility> {
    SOURCES.iter().find(|(known, _)| *known == name).map(|&(_, utility)| utility)
}

/// The object layers a consumer can be on: furniture, and what hangs from
/// the ceiling.
const CONSUMER_LAYERS: [ObjectLayer; 2] = [ObjectLayer::Props, ObjectLayer::Lamps];

/// Which cells are live on which network.
#[derive(Clone, PartialEq, Debug)]
pub struct Supply {
    size: Size,
    /// One per cell, row-major, per [`Network`] in declaration order.
    live: [Vec<bool>; 3],
    /// Answers yes everywhere, whatever is painted: [`Supply::everywhere`].
    everywhere: bool,
}

impl Supply {
    /// Flood both networks from their sources.
    ///
    /// Three breadth-first fills over the grid, each touching a cell at most
    /// once, so the cost is the map's area and never what stands on it: the
    /// line from every transformer standing on it, the wiring from every box
    /// the line reached, the pipes from every sewer standing on one.
    pub fn from_map(map: &Map) -> Supply {
        Supply::with_boxes_off(map, &[])
    }

    /// [`Supply::from_map`] with the boxes in `off` switched off: they carry
    /// the line through and feed no wiring, and wiring does not pass through
    /// them either.
    pub fn with_boxes_off(map: &Map, off: &[Point]) -> Supply {
        let size = map.size();
        let sources = |utility: Utility| -> Vec<Point> {
            map.objects(ObjectLayer::Props)
                .iter()
                .filter(|object| source_of(object.kind.as_str()) == Some(utility))
                .map(Object::cell)
                .collect()
        };
        let power_at = |cell: Point| map.grid(GridLayer::Power, cell);

        let line = flood(size, sources(Utility::Power), |cell| {
            matches!(power_at(cell), power::LINE | power::BOX)
        });
        let switched_on = |cell: Point| !off.contains(&cell);
        let boxes: Vec<Point> = size
            .points()
            .filter(|&cell| power_at(cell) == power::BOX && line[size.index_of(cell).unwrap()] && switched_on(cell))
            .collect();
        let wiring = flood(size, boxes, |cell| match power_at(cell) {
            power::WIRING => true,
            power::BOX => switched_on(cell),
            _ => false,
        });
        let pipes = flood(size, sources(Utility::Water), |cell| {
            map.grid(GridLayer::Water, cell) == water::PIPE
        });

        Supply {
            size,
            live: [line, wiring, pipes],
            everywhere: false,
        }
    }

    /// A supply that reaches every cell of every network, whatever is or is
    /// not painted.
    ///
    /// For a world whose wiring is not what is under test — a test about
    /// whether a unit eats, not whether its fridge is plugged in — and
    /// nothing else: a map that is played is always asked
    /// [`Supply::from_map`].
    pub fn everywhere(size: Size) -> Supply {
        Supply {
            size,
            live: [Vec::new(), Vec::new(), Vec::new()],
            everywhere: true,
        }
    }

    /// Whether `cell` is live on `network`. Off the map is dead.
    pub fn is_live(&self, network: Network, cell: Point) -> bool {
        match self.size.index_of(cell) {
            Some(index) => self.everywhere || self.live[network as usize][index],
            None => false,
        }
    }

    /// Whether something called `name` standing in `cell` has what it needs
    /// to work — always, for something that needs nothing.
    pub fn serves(&self, name: &str, cell: Point) -> bool {
        match needs(name) {
            None => true,
            Some(Utility::Power) => self.is_live(Network::Wiring, cell),
            Some(Utility::Water) => self.is_live(Network::Pipes, cell),
        }
    }

    /// Every consumer on the map, its cell and whether it is served — for a
    /// readout and a test, not a tick.
    pub fn consumers<'a>(&'a self, map: &'a Map) -> impl Iterator<Item = (&'a Object, Utility, bool)> + 'a {
        CONSUMER_LAYERS.into_iter().flat_map(move |layer| {
            map.objects(layer).iter().filter_map(move |object| {
                let utility = needs(object.kind.as_str())?;
                Some((object, utility, self.serves(object.kind.as_str(), object.cell())))
            })
        })
    }
}

/// Everything plugged into the wiring the box in `at` feeds — whether or not
/// the box is on, or the line reaches it: what switching it would switch.
/// Wiring that reaches another box is shared with it, and counted for both.
/// For a panel, not a tick: it floods the map.
pub fn fed_by(map: &Map, at: Point) -> Vec<&Object> {
    if map.grid(GridLayer::Power, at) != power::BOX {
        return Vec::new();
    }
    let size = map.size();
    let wiring = flood(size, vec![at], |cell| matches!(map.grid(GridLayer::Power, cell), power::WIRING | power::BOX));
    CONSUMER_LAYERS
        .into_iter()
        .flat_map(|layer| map.objects(layer))
        .filter(|object| {
            needs(object.kind.as_str()) == Some(Utility::Power)
                && size.index_of(object.cell()).is_some_and(|index| wiring[index])
        })
        .collect()
}

/// Every cell reachable from `seeds` by 4-neighbour steps through cells
/// `conducts` says yes to. A seed is only a start if it conducts itself.
fn flood(size: Size, seeds: Vec<Point>, conducts: impl Fn(Point) -> bool) -> Vec<bool> {
    let mut live = vec![false; size.area()];
    let mut queue: Vec<Point> = Vec::new();
    for seed in seeds {
        if let Some(index) = size.index_of(seed)
            && !live[index]
            && conducts(seed)
        {
            live[index] = true;
            queue.push(seed);
        }
    }
    while let Some(cell) = queue.pop() {
        for next in cell.cardinal_neighbours() {
            if let Some(index) = size.index_of(next)
                && !live[index]
                && conducts(next)
            {
                live[index] = true;
                queue.push(next);
            }
        }
    }
    live
}

/// The cells of an L from `from` to `to`: along the row first, then up or
/// down the column, both ends included, each cell once.
pub fn l_path(from: Point, to: Point) -> impl Iterator<Item = Point> {
    let dx = (to.x - from.x).signum();
    let dy = (to.y - from.y).signum();
    let along = (0..=(to.x - from.x).abs()).map(move |i| Point::new(from.x + i * dx, from.y));
    let up = (1..=(to.y - from.y).abs()).map(move |i| Point::new(to.x, from.y + i * dy));
    along.chain(up)
}

/// Lay `value` on `layer` along [`l_path`]. Cells already holding something
/// are overwritten, so the caller decides what may be crossed.
pub fn lay(map: &mut Map, layer: GridLayer, value: u8, from: Point, to: Point) {
    for cell in l_path(from, to) {
        map.set_grid(layer, cell, value);
    }
}

/// Like [`lay`], but leaves cells that already hold something alone: wiring
/// run to a second consumer follows the first one's where they overlap, and
/// never paints over the box it started from.
pub fn lay_over_empty(map: &mut Map, layer: GridLayer, value: u8, from: Point, to: Point) {
    for cell in l_path(from, to) {
        if map.grid(layer, cell) == 0 {
            map.set_grid(layer, cell, value);
        }
    }
}

/// Connect every consumer on the map, the shortest-to-write way: a
/// transformer in `transformer` with a box under it, wiring from it straight
/// to everything that needs power, a sewer in `sewer` and a pipe from it to
/// everything that needs a drain.
///
/// Not how a district is wired (`district` runs lines along the streets and a
/// box into every building); this is for a map whose wiring is beside the
/// point — a test fixture about eating — that still has to be wired to be
/// honest. Both sources are props and block their cells, so choose cells
/// nobody needed: a wall, the edge of the map.
pub fn serve_everything(map: &mut Map, transformer: Point, sewer: Point) {
    let centre = |cell: Point| {
        Point::new(
            cell.x * PIXELS_PER_CELL + PIXELS_PER_CELL / 2,
            cell.y * PIXELS_PER_CELL + PIXELS_PER_CELL / 2,
        )
    };
    map.add_object(ObjectLayer::Props, Object { at: centre(transformer), kind: ObjectKind::new("transformer") });
    map.add_object(ObjectLayer::Props, Object { at: centre(sewer), kind: ObjectKind::new("sewer") });
    map.set_grid(GridLayer::Power, transformer, power::BOX);
    map.set_grid(GridLayer::Water, sewer, water::PIPE);

    let consumers: Vec<(Point, Utility)> = CONSUMER_LAYERS
        .into_iter()
        .flat_map(|layer| map.objects(layer).iter())
        .filter_map(|object| Some((object.cell(), needs(object.kind.as_str())?)))
        .collect();
    for (cell, utility) in consumers {
        match utility {
            Utility::Power => lay_over_empty(map, GridLayer::Power, power::WIRING, transformer, cell),
            Utility::Water => lay_over_empty(map, GridLayer::Water, water::PIPE, sewer, cell),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FLOOR, WALL};

    fn at(name: &str, cell: Point) -> Object {
        Object {
            at: Point::new(cell.x * PIXELS_PER_CELL + 24, cell.y * PIXELS_PER_CELL + 24),
            kind: ObjectKind::new(name),
        }
    }

    /// A row of floor: a transformer at the left end, a box, a fridge at the
    /// right — and whatever `power` says is under each cell.
    fn street(power: &str) -> Map {
        let mut map = Map::new(Size::new(power.len() as i32, 1), FLOOR);
        for (x, c) in power.chars().enumerate() {
            map.set_grid(GridLayer::Power, Point::new(x as i32, 0), GridLayer::Power.value_of(c).unwrap());
        }
        map.add_object(ObjectLayer::Props, at("transformer", Point::new(0, 0)));
        map.add_object(ObjectLayer::Props, at("fridge", Point::new(power.len() as i32 - 1, 0)));
        map
    }

    fn fridge_has_power(power: &str) -> bool {
        let map = street(power);
        Supply::from_map(&map).serves("fridge", Point::new(power.len() as i32 - 1, 0))
    }

    #[test]
    fn a_fridge_is_powered_through_a_line_a_box_and_wiring_and_not_otherwise() {
        assert!(fridge_has_power("==B--"), "transformer, line, box, wiring");
        assert!(fridge_has_power("B----"), "a box right under the transformer");
        assert!(!fridge_has_power("====="), "a line straight into a fridge would burn it out");
        assert!(!fridge_has_power("-----"), "wiring with no box is not fed by a transformer");
        assert!(!fridge_has_power("==.--"), "a gap in the line");
        assert!(!fridge_has_power("==B-."), "nothing under the fridge itself");
        assert!(!fridge_has_power("....."), "nothing at all");
    }

    #[test]
    fn a_box_switched_off_passes_the_line_on_and_feeds_no_wiring() {
        let map = street("=B=B-");
        let fridge = Point::new(4, 0);
        assert!(Supply::from_map(&map).serves("fridge", fridge));
        // The far box is the one the fridge's wiring hangs off.
        let off = Supply::with_boxes_off(&map, &[Point::new(3, 0)]);
        assert!(!off.serves("fridge", fridge));
        assert!(off.is_live(Network::Line, Point::new(3, 0)), "the line still reaches it");
        // The near one off: the line runs on through it, so the far box is live.
        let near = Supply::with_boxes_off(&map, &[Point::new(1, 0)]);
        assert!(near.serves("fridge", fridge));
        assert!(!near.is_live(Network::Wiring, Point::new(1, 0)));
    }

    #[test]
    fn a_box_feeds_what_is_on_its_wiring_and_nothing_else() {
        let map = street("==B--");
        let fed: Vec<&str> = fed_by(&map, Point::new(2, 0)).iter().map(|o| o.kind.as_str()).collect();
        assert_eq!(fed, ["fridge"]);
        assert!(fed_by(&map, Point::new(1, 0)).is_empty(), "a line is not a box");
    }

    #[test]
    fn a_line_and_wiring_side_by_side_do_not_touch() {
        // A line running past wiring is not a box.
        assert!(!fridge_has_power("===--"));
    }

    #[test]
    fn a_transformer_feeds_only_what_is_under_it() {
        let mut map = street("==B--");
        // The transformer moves off the end of the line.
        map.remove_object(ObjectLayer::Props, &at("transformer", Point::new(0, 0)));
        map.set_grid(GridLayer::Power, Point::new(0, 0), power::NONE);
        map.add_object(ObjectLayer::Props, at("transformer", Point::new(0, 0)));
        assert!(!Supply::from_map(&map).serves("fridge", Point::new(4, 0)));
    }

    #[test]
    fn a_toilet_drains_through_pipes_to_a_sewer() {
        let mut map = Map::new(Size::new(5, 3), FLOOR);
        map.add_object(ObjectLayer::Props, at("sewer", Point::new(0, 0)));
        map.add_object(ObjectLayer::Props, at("toilet", Point::new(4, 2)));
        let toilet = Point::new(4, 2);
        assert!(!Supply::from_map(&map).serves("toilet", toilet));

        lay(&mut map, GridLayer::Water, water::PIPE, Point::new(0, 0), toilet);
        assert!(Supply::from_map(&map).serves("toilet", toilet));
        // Power is a different network: wiring is not a pipe.
        assert!(!Supply::from_map(&map).serves("fridge", toilet));

        map.set_grid(GridLayer::Water, Point::new(2, 0), water::NONE);
        assert!(!Supply::from_map(&map).serves("toilet", toilet), "a cut pipe");
    }

    #[test]
    fn pipes_and_wiring_run_under_walls() {
        let mut map = Map::new(Size::new(5, 1), WALL);
        map.add_object(ObjectLayer::Props, at("sewer", Point::new(0, 0)));
        lay(&mut map, GridLayer::Water, water::PIPE, Point::new(0, 0), Point::new(4, 0));
        assert!(Supply::from_map(&map).is_live(Network::Pipes, Point::new(4, 0)));
    }

    #[test]
    fn an_l_path_visits_each_cell_once_from_end_to_end() {
        let path: Vec<Point> = l_path(Point::new(3, 1), Point::new(1, 3)).collect();
        assert_eq!(
            path,
            [Point::new(3, 1), Point::new(2, 1), Point::new(1, 1), Point::new(1, 2), Point::new(1, 3)]
        );
        assert_eq!(l_path(Point::new(2, 2), Point::new(2, 2)).count(), 1);
    }

    #[test]
    fn what_needs_nothing_is_always_served() {
        let map = Map::new(Size::new(3, 3), FLOOR);
        let supply = Supply::from_map(&map);
        assert!(supply.serves("bed 1", Point::new(1, 1)));
        assert!(supply.serves("fire", Point::new(1, 1)), "a fire needs no plug");
        assert!(!supply.serves("tube lamp", Point::new(1, 1)));
        assert!(Supply::everywhere(map.size()).serves("tube lamp", Point::new(1, 1)));
        assert!(!Supply::everywhere(map.size()).serves("tube lamp", Point::new(9, 9)), "off the map");
    }

    #[test]
    fn serve_everything_serves_everything() {
        let mut map = Map::new(Size::new(12, 9), FLOOR);
        for (name, cell) in [
            ("fridge", Point::new(3, 4)),
            ("computer", Point::new(9, 7)),
            ("toilet", Point::new(10, 1)),
            ("fridge", Point::new(1, 8)),
        ] {
            map.add_object(ObjectLayer::Props, at(name, cell));
        }
        map.add_object(ObjectLayer::Lamps, at("ceiling lamp", Point::new(6, 6)));
        serve_everything(&mut map, Point::new(0, 0), Point::new(11, 0));

        let supply = Supply::from_map(&map);
        let consumers: Vec<_> = supply.consumers(&map).collect();
        assert_eq!(consumers.len(), 5);
        for (object, _, served) in consumers {
            assert!(served, "{object:?}");
        }
    }

    #[test]
    fn every_consumer_and_source_is_a_thing_a_map_can_hold() {
        for (name, _) in SOURCES {
            assert!(ObjectKind::new(*name).prop().is_some(), "{name} is not in map::PROPS");
        }
        for (name, _) in CONSUMERS {
            assert_eq!(CONSUMERS.iter().filter(|(n, _)| n == name).count(), 1, "{name} twice");
        }
    }
}
