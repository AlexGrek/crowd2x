//! A district made from a seed: [`generate`].
//!
//! Thirty-two houses with one or two people living in each, a bank laid out
//! like an office, three empty shops, and streets to walk between them. The
//! same seed always gives the same district — every choice is drawn from one
//! [`SmallRng`] in a fixed order — so a district somebody liked can be made
//! again from its number.
//!
//! What comes out is an ordinary [`Map`]: it is saved, opened in the editor
//! and played exactly like a painted one. Nothing downstream knows it was
//! generated.
//!
//! # Layout
//!
//! A grid of blocks with a [`STREET`] between every two and around the edge,
//! so every block faces a street on all four sides. A block is 2x2 **lots**,
//! and anything that is not a building is paving, so the yards and the gaps
//! between houses are as walkable as the street. Which blocks are what is
//! the seed's choice:
//!
//! * **the bank** — two neighbouring blocks merged across the street between
//!   them: a lobby inside the door lined with fridges, an office of
//!   computers in desk rows, and two restrooms walled off at each end;
//! * **the shops** — one block, three of its lots a shop with a wide door and
//!   nothing inside yet, the fourth left as a small paved square;
//! * **the houses** — every lot of every other block, less [`EMPTY_LOTS`]
//!   left paved, which leaves [`HOUSES`].
//!
//! A building's door is always in a wall facing one of its lot's *outer*
//! sides. Every paved cell in a lot lies in a row or a column the building
//! does not cover, and that row or column runs out to the lot's outer side,
//! which is street — so nothing generated can be sealed off, and a test walks
//! the whole map to make sure.
//!
//! # Roofs and lamps
//!
//! Every building is roofed over, walls and all, and the streets and yards
//! are open sky — so the sun lights the street and comes in by the doors
//! (`crate::lighting`). A house and a shop each get one warm [`HOUSE_LAMP`]
//! in the middle of the room; the bank gets [`OFFICE_LAMP`]s in a grid every
//! [`OFFICE_LAMP_SPACING`] cells. Both are laid out from the buildings once
//! they are built, drawing nothing from the seed, so adding them changed no
//! district: a seed is still the same streets, houses and people.
//!
//! # Power and water
//!
//! Laid last, like the lamps and for the same reason: from the buildings as
//! built, without the seed. A [`TRANSFORMER`] stands in one corner of the
//! district and a [`SEWER`] in the opposite one, and a power line and a sewer
//! main run down the middle of every street, under the paving, skipping
//! whatever a building stands on. Every building with something to power has
//! a distribution box just inside its wall nearest a street, a line out to
//! the main under the street, and wiring from the box to every lamp, fridge
//! and computer inside; every building with a toilet has a pipe from its
//! toilets out to the sewer main the same way. Underground, so it crosses
//! walls and furniture freely and is in nobody's way. See
//! [`super::utilities`] for what connects to what.
//!
//! # Residents
//!
//! A house has a bed for each of its residents, a fridge and a toilet — each
//! with two free cells beside it, so a crowd cannot jam one — and a
//! `"human"` spawner per resident on the cell beside its bed — beside that
//! bed and *not* beside the other one. Arriving hands a human the nearest bed
//! nobody owns (`sim::GameState::spawn`), and a spawner one step from its own
//! bed is two or more from any other, so everybody ends up sleeping at home
//! without the map having to say whose bed is whose.

use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::{RngExt, SeedableRng};

use super::grid::{power, water};
use super::utilities::{self, Utility};
use super::{GridLayer, Map, Object, ObjectKind, ObjectLayer, Point, Size, TerrainId, PIXELS_PER_CELL};

/// How many houses a district has.
pub const HOUSES: usize = 32;
/// How many shops.
pub const SHOPS: usize = 3;
/// How many people can live in one house.
pub const RESIDENTS: std::ops::RangeInclusive<u8> = 1..=2;

/// Cells across a street: room for two to pass with one to spare.
pub const STREET: i32 = 3;
const LOT_W: i32 = 8;
const LOT_H: i32 = 7;
const BLOCK_W: i32 = 2 * LOT_W;
const BLOCK_H: i32 = 2 * LOT_H;
const BLOCKS_X: i32 = 4;
const BLOCKS_Y: i32 = 3;
/// Lots in the residential blocks that are left paved: nine blocks of four
/// is 36 lots, and the district wants [`HOUSES`].
const EMPTY_LOTS: usize = 36 - HOUSES;

pub const WIDTH: i32 = BLOCKS_X * BLOCK_W + (BLOCKS_X + 1) * STREET;
pub const HEIGHT: i32 = BLOCKS_Y * BLOCK_H + (BLOCKS_Y + 1) * STREET;

/// Everything outside a building.
const PAVING: &str = "floor";
/// What lights a house or a shop.
pub const HOUSE_LAMP: &str = "ceiling lamp";
/// What lights the bank, in rows.
pub const OFFICE_LAMP: &str = "tube lamp";
/// Cells between two of the bank's lamps, each way: close enough that their
/// light overlaps (a tube lamp reaches five).
pub const OFFICE_LAMP_SPACING: i32 = 5;
const HOUSE_WALL: &str = "wall red";
const HOUSE_FLOORS: [&str; 2] = ["wood", "wood cracked"];
const BANK_WALL: &str = "wall purple";
const BANK_FLOOR: &str = "floor white";
const RESTROOM_FLOOR: &str = "tiles blue";
const SHOP_WALL: &str = "wall brown";
const SHOP_FLOORS: [&str; 3] = ["floor red", "tiles yellow", "floor colorful"];
const WINDOW: &str = "window";
/// Cells between the windows along a wall: one every third cell.
const WINDOW_SPACING: i32 = 3;
const BEDS: [&str; 6] = ["bed 1", "bed 2", "bed 3", "bed 4", "bed 5", "bed 6"];
const FRIDGE: &str = "fridge";
const TOILET: &str = "toilet";
const COMPUTER: &str = "computer";
/// The door of a shop, the bank and its restrooms: anybody may open it.
const DOOR: &str = "door";
/// A house's door, which only the people living there may open: the
/// simulation reads the room behind it as their property (`sim::property`).
const HOUSE_DOOR: &str = "house door";
/// How wide every doorway is, in leaves: one person coming in and one going
/// out of a one-cell door is a standoff neither of them can resolve.
pub const DOOR_WIDTH: i32 = 2;
/// What powers the district, in its bottom-right corner.
pub const TRANSFORMER: &str = "transformer";
/// What drains it, in its top-left corner.
pub const SEWER: &str = "sewer";
/// How far a building's line or pipe may run to reach the main under a
/// street. A lot is never farther than this from one of its streets.
const LONGEST_SPUR: i32 = 12;
/// What a resident's spawner spawns: an `EntityType` name.
pub const RESIDENT: &str = "human";

/// Every tile name the generator paints with, so a test can check each one
/// is still in the catalogue rather than finding out from a panic.
const TILES: &[&str] = &[
    PAVING,
    HOUSE_WALL,
    HOUSE_FLOORS[0],
    HOUSE_FLOORS[1],
    BANK_WALL,
    BANK_FLOOR,
    RESTROOM_FLOOR,
    SHOP_WALL,
    SHOP_FLOORS[0],
    SHOP_FLOORS[1],
    SHOP_FLOORS[2],
    WINDOW,
];

/// A rectangle of cells: `x`, `y` is its lowest corner.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub const fn right(self) -> i32 {
        self.x + self.w - 1
    }

    pub const fn top(self) -> i32 {
        self.y + self.h - 1
    }

    pub const fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.top()
    }

    /// The rectangle one cell in from every side: inside a building's walls.
    pub const fn interior(self) -> Rect {
        Rect::new(self.x + 1, self.y + 1, self.w - 2, self.h - 2)
    }

    /// Whether `p` is on the outermost ring of this rectangle.
    pub const fn on_edge(self, p: Point) -> bool {
        self.contains(p) && (p.x == self.x || p.x == self.right() || p.y == self.y || p.y == self.top())
    }

    /// Every cell, row by row from the bottom.
    pub fn cells(self) -> impl Iterator<Item = Point> {
        (self.y..self.y + self.h).flat_map(move |y| (self.x..self.x + self.w).map(move |x| Point::new(x, y)))
    }
}

/// A house, as it was built.
#[derive(Clone, Debug)]
pub struct House {
    /// The outside of its walls.
    pub walls: Rect,
    /// One per resident, in the order their spawners were written.
    pub beds: Vec<Point>,
    pub residents: u8,
}

/// A generated district: the map, and where everything in it went.
///
/// The layout is kept beside the map so a test can make claims about each
/// building without working it back out of the tiles.
pub struct District {
    pub seed: u64,
    pub map: Map,
    pub houses: Vec<House>,
    pub bank: Rect,
    pub shops: Vec<Rect>,
}

impl District {
    /// Everybody who lives here: one spawner each.
    pub fn residents(&self) -> usize {
        self.houses.iter().map(|house| house.residents as usize).sum()
    }
}

/// Which way a wall faces.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Side {
    West,
    East,
    South,
    North,
}

impl Side {
    /// One step out through a wall on this side.
    const fn outward(self) -> Point {
        match self {
            Side::West => Point::new(-1, 0),
            Side::East => Point::new(1, 0),
            Side::South => Point::new(0, -1),
            Side::North => Point::new(0, 1),
        }
    }
}

/// A plot in a block, and the two sides of it that face a street.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Lot {
    origin: Point,
    outer: [Side; 2],
}

fn block_origin(bx: i32, by: i32) -> Point {
    Point::new(
        STREET + bx * (BLOCK_W + STREET),
        STREET + by * (BLOCK_H + STREET),
    )
}

fn lots_of(bx: i32, by: i32) -> [Lot; 4] {
    let origin = block_origin(bx, by);
    let lot = |lx: i32, ly: i32| Lot {
        origin: origin.offset(lx * LOT_W, ly * LOT_H),
        outer: [
            if lx == 0 { Side::West } else { Side::East },
            if ly == 0 { Side::South } else { Side::North },
        ],
    };
    [lot(0, 0), lot(1, 0), lot(0, 1), lot(1, 1)]
}

/// A building of `w` x `h` somewhere inside `lot`.
fn place_in(rng: &mut SmallRng, lot: Lot, w: i32, h: i32) -> Rect {
    let ox = rng.random_range(0..=LOT_W - w);
    let oy = rng.random_range(0..=LOT_H - h);
    Rect::new(lot.origin.x + ox, lot.origin.y + oy, w, h)
}

/// `width` cells of the wall on `side`, never a corner.
fn door(rng: &mut SmallRng, walls: Rect, side: Side, width: i32) -> Vec<Point> {
    let along = match side {
        Side::South | Side::North => walls.w,
        Side::West | Side::East => walls.h,
    };
    let start = rng.random_range(1..=along - 1 - width);
    (start..start + width)
        .map(|i| match side {
            Side::South => Point::new(walls.x + i, walls.y),
            Side::North => Point::new(walls.x + i, walls.top()),
            Side::West => Point::new(walls.x, walls.y + i),
            Side::East => Point::new(walls.right(), walls.y + i),
        })
        .collect()
}

fn tile(name: &str) -> TerrainId {
    TerrainId::from_name(name).unwrap_or_else(|| panic!("district: no tile called {name:?}"))
}

/// Walls round the edge of `walls`, `floor` inside.
fn build(map: &mut Map, walls: Rect, wall: &str, floor: &str) {
    let (wall, floor) = (tile(wall), tile(floor));
    for cell in walls.cells() {
        map.set_terrain(cell, if walls.on_edge(cell) { wall } else { floor });
    }
}

/// An object in the middle of `cell`, which is where a click in the editor
/// would put it.
fn object(name: &str, cell: Point) -> Object {
    Object {
        at: Point::new(
            cell.x * PIXELS_PER_CELL + PIXELS_PER_CELL / 2,
            cell.y * PIXELS_PER_CELL + PIXELS_PER_CELL / 2,
        ),
        kind: ObjectKind::new(name),
    }
}

/// Build a district from `seed`.
pub fn generate(seed: u64) -> District {
    let mut rng = SmallRng::seed_from_u64(seed);
    let mut map = Map::new(Size::new(WIDTH, HEIGHT), tile(PAVING));
    // Props wait until the terrain is painted: painting a cell asks every
    // prop placed so far whether it stands there.
    let mut props: Vec<Object> = Vec::new();
    let mut spawners: Vec<Object> = Vec::new();

    // Which blocks are what.
    let bank_x = rng.random_range(0..BLOCKS_X - 1);
    let bank_y = rng.random_range(0..BLOCKS_Y);
    let mut blocks: Vec<(i32, i32)> = (0..BLOCKS_Y)
        .flat_map(|by| (0..BLOCKS_X).map(move |bx| (bx, by)))
        .filter(|&(bx, by)| !(by == bank_y && (bx == bank_x || bx == bank_x + 1)))
        .collect();
    let (shop_x, shop_y) = blocks.remove(rng.random_range(0..blocks.len()));

    let bank = bank(&mut rng, &mut map, &mut props, bank_x, bank_y);
    let shops = shops(&mut rng, &mut map, &mut props, shop_x, shop_y);

    let mut lots: Vec<Lot> = blocks.iter().flat_map(|&(bx, by)| lots_of(bx, by)).collect();
    lots.shuffle(&mut rng);
    lots.truncate(lots.len() - EMPTY_LOTS);
    // Back into street order, so the spawners — and so the order people
    // arrive in — read the map from the bottom left rather than at random.
    lots.sort_by_key(|lot| (lot.origin.y, lot.origin.x));
    let houses: Vec<House> = lots
        .into_iter()
        .map(|lot| house(&mut rng, &mut map, &mut props, &mut spawners, lot))
        .collect();

    for prop in props {
        map.add_object(ObjectLayer::Props, prop);
    }
    for spawner in spawners {
        map.add_object(ObjectLayer::Spawners, spawner);
    }
    roof_and_light(&mut map, &houses, bank, &shops);
    let buildings: Vec<Rect> =
        houses.iter().map(|house| house.walls).chain(shops.iter().copied()).chain([bank]).collect();
    for &walls in &buildings {
        glaze(&mut map, walls);
    }
    connect(&mut map, &buildings);

    District {
        seed,
        map,
        houses,
        bank,
        shops,
    }
}

/// A roof over every building, and the lamps under them. Last, from the
/// buildings as built, and without the seed — see the module docs.
fn roof_and_light(map: &mut Map, houses: &[House], bank: Rect, shops: &[Rect]) {
    let rooms = houses.iter().map(|house| house.walls).chain(shops.iter().copied());
    for walls in rooms.clone().chain([bank]) {
        for cell in walls.cells() {
            map.set_ceiling(cell, true);
        }
    }
    let hang = |map: &mut Map, name: &str, cell: Point| {
        // Not inside a wall: a lamp there would light nothing but the wall.
        if map.sight().is_passable(cell) {
            map.add_object(ObjectLayer::Lamps, object(name, cell));
        }
    };
    for walls in rooms {
        let inside = walls.interior();
        hang(map, HOUSE_LAMP, Point::new(inside.x + inside.w / 2, inside.y + inside.h / 2));
    }
    let inside = bank.interior();
    let offset = Point::new(
        (inside.w - 1) % OFFICE_LAMP_SPACING / 2,
        (inside.h - 1) % OFFICE_LAMP_SPACING / 2,
    );
    for y in (inside.y + offset.y..=inside.top()).step_by(OFFICE_LAMP_SPACING as usize) {
        for x in (inside.x + offset.x..=inside.right()).step_by(OFFICE_LAMP_SPACING as usize) {
            hang(map, OFFICE_LAMP, Point::new(x, y));
        }
    }
}

/// Windows in the outer walls of `walls`: one every [`WINDOW_SPACING`] cells,
/// never at a corner, never beside a door or another window (a post of wall
/// is left between), and only where the cell inside is open floor, so a
/// partition meeting the wall does not get one. The wall tile is replaced and
/// the ceiling left alone: the lightmap lets the sun in through the glass.
/// Last, from the buildings as built, and without the seed — see the module
/// docs.
fn glaze(map: &mut Map, walls: Rect) {
    let window = tile(WINDOW);
    let solid = |map: &Map, cell: Point| {
        map.terrain(cell).is_some_and(|t| !t.is_passable() && !t.is_see_through())
    };
    let mut panes = Vec::new();
    for (side, along) in [(Side::South, walls.w), (Side::North, walls.w), (Side::West, walls.h), (Side::East, walls.h)] {
        let cell_at = |i: i32| match side {
            Side::South => Point::new(walls.x + i, walls.y),
            Side::North => Point::new(walls.x + i, walls.top()),
            Side::West => Point::new(walls.x, walls.y + i),
            Side::East => Point::new(walls.right(), walls.y + i),
        };
        let inward = side.outward();
        for i in (WINDOW_SPACING / 2 + 1..along - 1).step_by(WINDOW_SPACING as usize) {
            let cell = cell_at(i);
            let room = cell.offset(-inward.x, -inward.y);
            if solid(map, cell)
                && solid(map, cell_at(i - 1))
                && solid(map, cell_at(i + 1))
                && map.sight().is_passable(room)
            {
                panes.push(cell);
            }
        }
    }
    for cell in panes {
        map.set_terrain(cell, window);
    }
}

/// The transformer, the sewer, the mains along the streets and every
/// building's connection to them. Last, from the buildings as built, and
/// without the seed — see the module docs.
fn connect(map: &mut Map, buildings: &[Rect]) {
    let inside_any = |cell: Point| buildings.iter().any(|walls| walls.contains(cell));

    // The mains: down the middle of every street, both networks side by side
    // in their own layers, broken wherever a building stands across a street
    // (the bank does) — every street is in a ring of them, so a break is
    // walked round.
    let mains: Vec<Point> = (0..=BLOCKS_Y)
        .flat_map(|k| {
            let y = k * (BLOCK_H + STREET) + STREET / 2;
            (STREET / 2..WIDTH - STREET / 2).map(move |x| Point::new(x, y))
        })
        .chain((0..=BLOCKS_X).flat_map(|k| {
            let x = k * (BLOCK_W + STREET) + STREET / 2;
            (STREET / 2..HEIGHT - STREET / 2).map(move |y| Point::new(x, y))
        }))
        .filter(|&cell| !inside_any(cell))
        .collect();
    for &cell in &mains {
        map.set_grid(GridLayer::Power, cell, power::LINE);
        map.set_grid(GridLayer::Water, cell, water::PIPE);
    }

    // The sources, in two corners of the street round the edge, each on its
    // network and joined to the main.
    let transformer = Point::new(WIDTH - 1, 0);
    let sewer = Point::new(0, HEIGHT - 1);
    map.add_object(ObjectLayer::Props, object(TRANSFORMER, transformer));
    map.add_object(ObjectLayer::Props, object(SEWER, sewer));
    utilities::lay(map, GridLayer::Power, power::LINE, transformer, Point::new(WIDTH - 1 - STREET / 2, STREET / 2));
    utilities::lay(map, GridLayer::Water, water::PIPE, sewer, Point::new(STREET / 2, HEIGHT - 1 - STREET / 2));

    for &walls in buildings {
        let others = |cell: Point| buildings.iter().any(|&other| other != walls && other.contains(cell));
        let wants = |utility: Utility| -> Vec<Point> {
            [ObjectLayer::Props, ObjectLayer::Lamps]
                .into_iter()
                .flat_map(|layer| map.objects(layer).iter())
                .filter(|object| {
                    walls.contains(object.cell()) && utilities::needs(object.kind.as_str()) == Some(utility)
                })
                .map(Object::cell)
                .collect()
        };
        let (powered, drained) = (wants(Utility::Power), wants(Utility::Water));
        if !powered.is_empty() {
            let entry = service_entry(map, GridLayer::Power, power::LINE, walls, &others);
            map.set_grid(GridLayer::Power, entry, power::BOX);
            for cell in powered {
                utilities::lay_over_empty(map, GridLayer::Power, power::WIRING, entry, cell);
            }
        }
        if !drained.is_empty() {
            let entry = service_entry(map, GridLayer::Water, water::PIPE, walls, &others);
            map.set_grid(GridLayer::Water, entry, water::PIPE);
            for cell in drained {
                utilities::lay(map, GridLayer::Water, water::PIPE, entry, cell);
            }
        }
    }
}

/// Where a building takes a network in: the inside cell in the middle of
/// whichever wall is nearest a `main` cell straight out from it, with that
/// stretch laid — through the wall, across the yard, to the main — without
/// crossing another building. Returns the inside cell, for the caller to put
/// a box or a pipe in.
fn service_entry(map: &mut Map, layer: GridLayer, main: u8, walls: Rect, others: &dyn Fn(Point) -> bool) -> Point {
    let inside = walls.interior();
    let middle = Point::new(inside.x + inside.w / 2, inside.y + inside.h / 2);
    let sides = [
        (Side::West, Point::new(inside.x, middle.y)),
        (Side::East, Point::new(inside.right(), middle.y)),
        (Side::South, Point::new(middle.x, inside.y)),
        (Side::North, Point::new(middle.x, inside.top())),
    ];
    let spur = |side: Side, entry: Point| -> Option<Vec<Point>> {
        let step = side.outward();
        let mut cells = Vec::new();
        let mut cell = entry.offset(step.x, step.y);
        for _ in 0..LONGEST_SPUR {
            if map.grid(layer, cell) == main {
                return Some(cells);
            }
            if !map.contains(cell) || others(cell) {
                return None;
            }
            cells.push(cell);
            cell = cell.offset(step.x, step.y);
        }
        None
    };
    let (entry, cells) = sides
        .into_iter()
        .filter_map(|(side, entry)| Some((entry, spur(side, entry)?)))
        .min_by_key(|(entry, cells)| (cells.len(), *entry))
        .expect("district: a building with no street within reach");
    for cell in cells {
        map.set_grid(layer, cell, main);
    }
    entry
}

/// The bank, across two blocks and the street between them.
///
/// Laid out in a frame of its own — `(0, 0)` the inside corner by the door —
/// and flipped into place, so which long side the door is on is a coin toss
/// rather than a second layout. Symmetric end to end: restrooms at both ends
/// and fridges all along the lobby.
///
/// **Everything a desk needs is nearer than the houses outside.** A brain
/// goes to the nearest fridge or toilet as the crow flies, walls and all, so
/// a bank with one restroom at one end sent everybody at the other end
/// across the street into the nearest house — and a two-person house with a
/// dozen strangers in it is a room nobody can move in.
fn bank(rng: &mut SmallRng, map: &mut Map, props: &mut Vec<Object>, bx: i32, by: i32) -> Rect {
    let area = block_origin(bx, by);
    let walls = Rect::new(area.x + 1, area.y + 1, 2 * BLOCK_W + STREET - 2, BLOCK_H - 2);
    let inside = walls.interior();
    let door_south = rng.random_bool(0.5);
    let at = |x: i32, y: i32| {
        Point::new(
            inside.x + x,
            if door_south { inside.y + y } else { inside.top() - y },
        )
    };

    build(map, walls, BANK_WALL, BANK_FLOOR);

    // The front door, two wide, in the middle.
    let (wide, high) = (inside.w, inside.h);
    let floor = tile(BANK_FLOOR);
    let door = [wide / 2 - 1, wide / 2];
    for x in door {
        map.set_terrain(at(x, -1), floor);
        props.push(object(DOOR, at(x, -1)));
    }

    // Two restrooms at each end: a partition wall across the bank, and one
    // along the restrooms to split them, each with a doorway into the hall
    // as wide as every other door, and a door in it.
    const PARTITION: i32 = 5;
    let split = high / 2 - 1;
    let (wall, tiles) = (tile(BANK_WALL), tile(RESTROOM_FLOOR));
    for end in [0, wide - 1] {
        // `x` counted from this end's outside wall.
        let from_end = |x: i32| if end == 0 { x } else { end - x };
        for y in 0..high {
            for x in 0..PARTITION {
                map.set_terrain(at(from_end(x), y), if y == split { wall } else { tiles });
            }
            map.set_terrain(at(from_end(PARTITION), y), wall);
        }
        for y in [1, 2, high - 4, high - 3] {
            map.set_terrain(at(from_end(PARTITION), y), tiles);
            props.push(object(DOOR, at(from_end(PARTITION), y)));
        }
        for y in [0, 2, split + 2, split + 4] {
            props.push(object(TOILET, at(from_end(0), y)));
        }
    }

    // The hall between the partitions, a free aisle along each of them.
    let (first, last) = (PARTITION + 1, wide - 2 - PARTITION);

    // Fridges along the front wall, every few cells either side of the door.
    for x in (first + 2..=last - 2).step_by(3) {
        if door.iter().all(|&d| (x - d).abs() > 1) {
            props.push(object(FRIDGE, at(x, 0)));
        }
    }

    // The office: desks of one or two computers in rows, two-cell aisles
    // between every desk and every row, the first rows clear for a lobby.
    let desk = rng.random_range(1..=2);
    let first_row = rng.random_range(3..=4);
    for y in (first_row..high).step_by(3) {
        let mut x = first + 2;
        while x + desk - 1 <= last - 1 {
            for seat in 0..desk {
                props.push(object(COMPUTER, at(x + seat, y)));
            }
            x += desk + 2;
        }
    }

    walls
}

/// Three empty shops in one block; the fourth lot is a paved square.
fn shops(rng: &mut SmallRng, map: &mut Map, props: &mut Vec<Object>, bx: i32, by: i32) -> Vec<Rect> {
    let mut lots = lots_of(bx, by).to_vec();
    lots.remove(rng.random_range(0..lots.len()));
    lots.into_iter()
        .map(|lot| {
            let walls = place_in(rng, lot, LOT_W - 1, LOT_H - 1);
            let floor = SHOP_FLOORS[rng.random_range(0..SHOP_FLOORS.len())];
            build(map, walls, SHOP_WALL, floor);
            let side = lot.outer[rng.random_range(0..2)];
            for cell in door(rng, walls, side, DOOR_WIDTH) {
                map.set_terrain(cell, tile(floor));
                props.push(object(DOOR, cell));
            }
            walls
        })
        .collect()
}

/// A house with its residents' beds, a fridge, a toilet and their spawners.
fn house(
    rng: &mut SmallRng,
    map: &mut Map,
    props: &mut Vec<Object>,
    spawners: &mut Vec<Object>,
    lot: Lot,
) -> House {
    let residents = rng.random_range(RESIDENTS);
    let wide = rng.random_range(LOT_W - 1..=LOT_W);
    let high = rng.random_range(LOT_H - 1..=LOT_H);
    let walls = place_in(rng, lot, wide, high);
    let floor = HOUSE_FLOORS[rng.random_range(0..HOUSE_FLOORS.len())];
    build(map, walls, HOUSE_WALL, floor);
    // Locked to the people who live here: the room behind it is theirs.
    let side = lot.outer[rng.random_range(0..2)];
    let door = door(rng, walls, side, DOOR_WIDTH);
    for &cell in &door {
        map.set_terrain(cell, tile(floor));
        props.push(object(HOUSE_DOOR, cell));
    }
    let entry: Vec<Point> = door.iter().map(|&cell| cell - side.outward()).collect();

    let furnished = furnish(rng, walls.interior(), &entry, residents as usize);
    for (index, &bed) in furnished.beds.iter().enumerate() {
        let art = BEDS[rng.random_range(0..BEDS.len())];
        props.push(object(art, bed));
        spawners.push(object(RESIDENT, furnished.spawners[index]));
    }
    props.push(object(FRIDGE, furnished.fridge));
    props.push(object(TOILET, furnished.toilet));

    House {
        walls,
        beds: furnished.beds,
        residents,
    }
}

/// Free cells a piece of house furniture keeps beside it.
///
/// One is enough to reach it and not enough to live with: whoever is in the
/// toilet or the bed cannot get out past somebody waiting in its only
/// doorway, and the two wait for each other for good.
const ROOM_AROUND: usize = 2;

/// Where the furniture went in one house, and where its residents start.
struct Furnishing {
    beds: Vec<Point>,
    fridge: Point,
    toilet: Point,
    /// One per bed, beside it.
    spawners: Vec<Point>,
}

/// Put the furniture against the walls of `inside`, keeping the cells inside
/// the door clear.
///
/// Rolled at random until an arrangement is livable — [`arrange`] says what
/// that means — and if the dice keep refusing, every arrangement is tried in
/// order, so a house is always furnished and still the same one for the same
/// seed.
fn furnish(rng: &mut SmallRng, inside: Rect, entry: &[Point], residents: usize) -> Furnishing {
    let mut spots: Vec<Point> = inside.cells().filter(|&cell| inside.on_edge(cell) && !entry.contains(&cell)).collect();
    let pieces = residents + 2;
    for _ in 0..64 {
        spots.shuffle(rng);
        if let Some(furnishing) = arrange(inside, entry, &spots[..pieces], residents) {
            return furnishing;
        }
    }
    let mut chosen = Vec::with_capacity(pieces);
    exhaustively(inside, entry, &spots, residents, pieces, &mut chosen)
        .expect("district: a house with no livable arrangement")
}

fn exhaustively(
    inside: Rect,
    entry: &[Point],
    spots: &[Point],
    residents: usize,
    pieces: usize,
    chosen: &mut Vec<Point>,
) -> Option<Furnishing> {
    if chosen.len() == pieces {
        return arrange(inside, entry, chosen, residents);
    }
    for &spot in spots {
        if chosen.contains(&spot) {
            continue;
        }
        chosen.push(spot);
        if let Some(furnishing) = exhaustively(inside, entry, spots, residents, pieces, chosen) {
            return Some(furnishing);
        }
        chosen.pop();
    }
    None
}

/// Beds, then the fridge, then the toilet, in the cells given — if the house
/// is still livable with them there: every free cell reachable from the door,
/// [`ROOM_AROUND`] free cells beside every piece of furniture, and a free
/// cell beside each bed that is beside no other, for its resident to start
/// on.
fn arrange(inside: Rect, entry: &[Point], cells: &[Point], residents: usize) -> Option<Furnishing> {
    let free = |cell: Point| inside.contains(cell) && !cells.contains(&cell);

    let mut reached = entry.to_vec();
    let mut next = 0;
    while next < reached.len() {
        for neighbour in reached[next].cardinal_neighbours() {
            if free(neighbour) && !reached.contains(&neighbour) {
                reached.push(neighbour);
            }
        }
        next += 1;
    }
    if reached.len() != inside.cells().filter(|&cell| free(cell)).count() {
        return None;
    }
    let room = |cell: &Point| cell.cardinal_neighbours().into_iter().filter(|&n| free(n)).count();
    if !cells.iter().all(|cell| room(cell) >= ROOM_AROUND) {
        return None;
    }

    let beds = &cells[..residents];
    let mut spawners: Vec<Point> = Vec::with_capacity(residents);
    for &bed in beds {
        let spot = bed.cardinal_neighbours().into_iter().find(|&cell| {
            free(cell)
                && !spawners.contains(&cell)
                && beds
                    .iter()
                    .all(|&other| other == bed || other.manhattan_distance(cell) > 1)
        })?;
        spawners.push(spot);
    }

    Some(Furnishing {
        beds: beds.to_vec(),
        fridge: cells[residents],
        toilet: cells[residents + 1],
        spawners,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEEDS: std::ops::Range<u64> = 0..64;

    fn props_in(map: &Map, rect: Rect, name: &str) -> usize {
        map.objects(ObjectLayer::Props)
            .iter()
            .filter(|prop| prop.kind.as_str() == name && rect.contains(prop.cell()))
            .count()
    }

    fn reachable(map: &Map) -> Vec<bool> {
        let size = map.size();
        let mut seen = vec![false; size.area()];
        let start = Point::new(0, 0);
        assert!(map.is_passable(start), "the corner of the map is street");
        let mut queue = vec![start];
        seen[size.index_of(start).unwrap()] = true;
        while let Some(cell) = queue.pop() {
            for neighbour in cell.cardinal_neighbours() {
                if let Some(index) = size.index_of(neighbour)
                    && !seen[index]
                    && map.is_passable(neighbour)
                {
                    seen[index] = true;
                    queue.push(neighbour);
                }
            }
        }
        seen
    }

    #[test]
    fn every_building_has_windows_in_its_outer_walls_and_nowhere_else() {
        let window = tile(WINDOW);
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let buildings: Vec<Rect> = district
                .houses
                .iter()
                .map(|house| house.walls)
                .chain(district.shops.iter().copied())
                .chain([district.bank])
                .collect();
            for cell in map.size().points() {
                if map.terrain(cell) == Some(window) {
                    assert!(
                        buildings.iter().any(|walls| walls.on_edge(cell)),
                        "seed {seed}: a window at {cell:?} is not in a wall"
                    );
                    assert!(!map.is_passable(cell) && map.sight().is_passable(cell));
                }
            }
            for walls in &buildings {
                let count = walls.cells().filter(|&cell| map.terrain(cell) == Some(window)).count();
                assert!(count >= 2, "seed {seed}: {walls:?} has {count} windows");
            }
            // Glazing took no way in away: everything is still reachable.
            let seen = reachable(map);
            for cell in map.size().points() {
                if map.is_passable(cell) {
                    assert!(seen[map.size().index_of(cell).unwrap()], "seed {seed}: {cell:?}");
                }
            }
        }
    }

    #[test]
    fn every_building_is_roofed_and_the_streets_are_open_sky() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let buildings: Vec<Rect> = district
                .houses
                .iter()
                .map(|house| house.walls)
                .chain(district.shops.iter().copied())
                .chain([district.bank])
                .collect();
            for cell in map.size().points() {
                let inside = buildings.iter().any(|walls| walls.contains(cell));
                assert_eq!(map.has_ceiling(cell), inside, "seed {seed}, {cell:?}");
            }
        }
    }

    #[test]
    fn every_house_and_shop_has_a_lamp_and_the_bank_a_grid_of_them() {
        for seed in SEEDS {
            let district = generate(seed);
            let lamps = district.map.objects(ObjectLayer::Lamps);
            let in_room = |rect: Rect, name: &str| {
                lamps.iter().filter(|lamp| lamp.kind.as_str() == name && rect.interior().contains(lamp.cell())).count()
            };
            for house in &district.houses {
                assert_eq!(in_room(house.walls, HOUSE_LAMP), 1, "seed {seed}, house at {:?}", house.walls);
            }
            for &shop in &district.shops {
                assert_eq!(in_room(shop, HOUSE_LAMP), 1, "seed {seed}, shop at {shop:?}");
            }
            assert!(in_room(district.bank, OFFICE_LAMP) >= 12, "seed {seed}: {}", in_room(district.bank, OFFICE_LAMP));
            for lamp in lamps {
                assert!(district.map.sight().is_passable(lamp.cell()), "seed {seed}: a lamp in a wall");
                assert!(crate::lighting::scene::emission(lamp.kind.as_str()).is_some());
            }
        }
    }

    #[test]
    fn every_tile_and_prop_the_generator_uses_exists() {
        for name in TILES {
            assert!(TerrainId::from_name(name).is_some(), "{name}");
        }
        for name in BEDS.iter().chain(&[FRIDGE, TOILET, COMPUTER, TRANSFORMER, SEWER, DOOR, HOUSE_DOOR]) {
            assert!(ObjectKind::new(*name).prop().is_some(), "{name}");
        }
    }

    #[test]
    fn everything_that_needs_power_or_water_in_a_district_has_it() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let supply = utilities::Supply::from_map(map);
            let mut consumers = 0;
            for (object, utility, served) in supply.consumers(map) {
                assert!(served, "seed {seed}: the {} at {:?} has no {utility:?}", object.kind.as_str(), object.cell());
                consumers += 1;
            }
            // Every house's fridge, toilet and lamp at least.
            assert!(consumers >= 3 * HOUSES, "seed {seed}: {consumers}");
        }
    }

    #[test]
    fn a_power_line_never_runs_inside_a_building_and_wiring_never_outside_one() {
        // The voltages meet only in a box, and a box is just inside a wall:
        // anything else is a line through somebody's kitchen or a cable
        // across the street.
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let buildings: Vec<Rect> = district
                .houses
                .iter()
                .map(|house| house.walls)
                .chain(district.shops.iter().copied())
                .chain([district.bank])
                .collect();
            for cell in map.size().points() {
                let indoors = buildings.iter().any(|walls| walls.interior().contains(cell));
                match map.grid(GridLayer::Power, cell) {
                    power::LINE => assert!(!indoors, "seed {seed}: a line indoors at {cell:?}"),
                    power::WIRING | power::BOX => assert!(indoors, "seed {seed}: wiring outdoors at {cell:?}"),
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn the_same_seed_builds_the_same_district() {
        let json = |seed| generate(seed).map.to_json().unwrap();
        assert_eq!(json(7), json(7));
        assert_ne!(json(7), json(8));
    }

    #[test]
    fn a_district_survives_being_saved() {
        let district = generate(3);
        let json = district.map.to_json().unwrap();
        let loaded = Map::from_json(&json).unwrap();
        assert_eq!(loaded.to_json().unwrap(), json);
    }

    #[test]
    fn a_district_has_what_it_was_asked_for() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            assert_eq!(map.size(), Size::new(WIDTH, HEIGHT));
            assert_eq!(district.houses.len(), HOUSES, "seed {seed}");
            assert_eq!(district.shops.len(), SHOPS, "seed {seed}");
            assert_eq!(map.objects(ObjectLayer::Spawners).len(), district.residents(), "seed {seed}");

            for house in &district.houses {
                assert!(RESIDENTS.contains(&house.residents), "seed {seed}");
                let beds: usize = BEDS.iter().map(|bed| props_in(map, house.walls, bed)).sum();
                assert_eq!(beds, house.residents as usize, "seed {seed} {house:?}");
                assert_eq!(props_in(map, house.walls, FRIDGE), 1, "seed {seed} {house:?}");
                assert_eq!(props_in(map, house.walls, TOILET), 1, "seed {seed} {house:?}");
            }
            for shop in &district.shops {
                let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| shop.interior().contains(prop.cell()));
                assert_eq!(inside.count(), 0, "seed {seed}: a shop is empty for now");
            }
            assert!(props_in(map, district.bank, COMPUTER) >= 12, "seed {seed}");
            assert!(props_in(map, district.bank, TOILET) >= 2, "seed {seed}");
        }
    }

    #[test]
    fn nothing_in_a_district_is_sealed_off() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let seen = reachable(map);
            for cell in map.size().points() {
                if map.is_passable(cell) {
                    assert!(seen[map.size().index_of(cell).unwrap()], "seed {seed}: {cell:?} is walled in");
                }
            }
            // Everything is used from a cell beside it or entered from one,
            // so either way a reachable neighbour is what makes it usable.
            for prop in map.objects(ObjectLayer::Props) {
                let usable = prop.cell().cardinal_neighbours().into_iter().any(|cell| {
                    map.size().index_of(cell).is_some_and(|index| seen[index])
                });
                assert!(usable, "seed {seed}: nobody can get to the {} at {:?}", prop.kind.as_str(), prop.cell());
            }
        }
    }

    fn doors_of(map: &Map) -> Vec<(Point, &str)> {
        map.objects(ObjectLayer::Props)
            .iter()
            .filter(|prop| [DOOR, HOUSE_DOOR].contains(&prop.kind.as_str()))
            .map(|prop| (prop.cell(), prop.kind.as_str()))
            .collect()
    }

    #[test]
    fn every_doorway_is_a_door_at_least_two_leaves_wide() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let doors = doors_of(map);
            let door_at = |cell: Point| doors.iter().find(|(at, _)| *at == cell).map(|&(_, name)| name);
            let wall = |cell: Point| map.terrain(cell).is_some_and(|t| !t.is_passable());

            // A doorway is no narrower than two: every leaf has another of
            // the same kind beside it, in line with the wall.
            for &(cell, name) in &doors {
                assert!(map.is_passable(cell), "seed {seed}: a door in a wall at {cell:?}");
                let partner = cell.cardinal_neighbours().into_iter().any(|beside| door_at(beside) == Some(name));
                assert!(partner, "seed {seed}: the {name} at {cell:?} is one leaf wide");
            }

            let buildings: Vec<(Rect, &str)> = district
                .houses
                .iter()
                .map(|house| (house.walls, HOUSE_DOOR))
                .chain(district.shops.iter().map(|&shop| (shop, DOOR)))
                .chain([(district.bank, DOOR)])
                .collect();
            for &(walls, kind) in &buildings {
                // Every way through an outer wall has a door in it, of the
                // building's kind: a house's locks, anybody else's does not.
                for cell in walls.cells().filter(|&cell| walls.on_edge(cell) && map.is_passable(cell)) {
                    assert_eq!(door_at(cell), Some(kind), "seed {seed}: an open doorway at {cell:?}");
                }
                // And no gap one cell wide between two walls, outer or inner,
                // is left anywhere inside, door or no door.
                for cell in walls.cells().filter(|&cell| map.is_passable(cell)) {
                    for (a, b) in [(Point::new(-1, 0), Point::new(1, 0)), (Point::new(0, -1), Point::new(0, 1))] {
                        let squeezed = wall(cell.offset(a.x, a.y)) && wall(cell.offset(b.x, b.y));
                        assert!(!squeezed, "seed {seed}: a one-cell doorway at {cell:?}");
                    }
                }
            }
            for house in &district.houses {
                let leaves = doors.iter().filter(|&&(cell, _)| house.walls.contains(cell)).count();
                assert_eq!(leaves, DOOR_WIDTH as usize, "seed {seed}: {house:?}");
            }
            // The front door, and one into each of the four restrooms.
            let bank = doors.iter().filter(|&&(cell, _)| district.bank.contains(cell)).count();
            assert_eq!(bank, 5 * DOOR_WIDTH as usize, "seed {seed}");
        }
    }

    #[test]
    fn every_resident_starts_beside_a_bed_in_their_own_house() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let spawners = map.objects(ObjectLayer::Spawners);
            let mut next = 0;
            for house in &district.houses {
                for &bed in &house.beds {
                    let spawner = &spawners[next];
                    next += 1;
                    let at = spawner.cell();
                    assert_eq!(spawner.kind.as_str(), RESIDENT);
                    assert!(map.is_passable(at), "seed {seed}: a spawner in {at:?}");
                    assert!(house.walls.interior().contains(at), "seed {seed}");
                    assert_eq!(at.manhattan_distance(bed), 1, "seed {seed}");
                    for other in &house.beds {
                        assert!(*other == bed || at.manhattan_distance(*other) > 1, "seed {seed}");
                    }
                }
            }
        }
    }
}
