//! A district made from a seed: [`generate`].
//!
//! Ninety-two houses with one or two people living in each, a bank laid out
//! like an office, three empty shops, a warehouse where the scrap collectors
//! store what they bring in from the wasteland, and streets to walk between
//! them. The same seed always gives the same district — every choice is
//! drawn from one [`SmallRng`] in a fixed order — so a district somebody
//! liked can be made again from its number.
//!
//! What comes out is an ordinary [`Map`]: it is saved, opened in the editor
//! and played exactly like a painted one. Nothing downstream knows it was
//! generated.
//!
//! # Layout
//!
//! A grid of blocks with a [`STREET`] between every two and around the edge,
//! so every block faces a street on all four sides. A street is a
//! [`PAVEMENT`] each side of three cells of [`ROAD`]; the pavement wraps
//! round every block, corners included, and runs along the edge of the map,
//! so the crowd — whose routes are cheapest by the ground's walk cost
//! (`sim::path`) — walks the pavement and crosses the road straight over. A
//! block is 2x2 **lots**, and anything in a block that is not a building is
//! pavement too, so the yards and the gaps between houses are as walkable as
//! the street. Which blocks are what is the seed's choice:
//!
//! * **the bank** — two neighbouring blocks merged across the street between
//!   them: a lobby inside the door lined with fridges, an office of
//!   computers in desk rows, and two restrooms walled off at each end;
//! * **the shops** — one block, three of its lots a shop with a wide door and
//!   nothing inside yet, the fourth left as a small park;
//! * **the mandatory lot buildings** — one lot each, taken before any house:
//!   so far only the **warehouse** ([`warehouse`]), on the lot facing the
//!   street round the edge of the district that is nearest a heap of the
//!   district's own wasteland ([`heaps_round`]), so its collectors' trips are
//!   short, with a [`SCRAP_BIN`] in each corner;
//! * **the houses** — every lot of every other block, less [`EMPTY_LOTS`]
//!   left as parks, which leaves [`HOUSES`].
//!
//! A **park** is a whole lot of [`GRASS`]: walkable, dearer than the
//! pavement, so it is cut across when that saves enough and walked round
//! when it does not.
//!
//! A building's door is always in a wall facing one of its lot's *outer*
//! sides. Every open cell in a lot lies in a row or a column the building
//! does not cover, and that row or column runs out to the lot's outer side,
//! which is street — so nothing generated can be sealed off, and a test walks
//! the whole map to make sure.
//!
//! # Roofs and lamps
//!
//! Every building is roofed over, walls and all, and the streets and yards
//! are open sky — so the sun lights the street and comes in by the doors
//! (`crate::lighting`). A house, a shop and the warehouse each get one warm
//! [`HOUSE_LAMP`] in the middle of the room; the bank gets [`OFFICE_LAMP`]s in a grid every
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
//! # Street lights
//!
//! Laid after the power, the same way: from the map as built, without the
//! seed. [`STREET_LAMP`]s stand along both sides of every street, staggered
//! across it, on a few long circuits — a side of a street running across,
//! wired end to end, with the sides of the streets running up hanging off it
//! a block at a time. Where a circuit crosses a power line the crossing cell
//! is a junction box, which carries the line on and joins the wiring. One
//! street light controller per circuit switches all of it by daylight
//! ([`utilities::CONTROLLER`]).
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
use super::{Bounds, GridLayer, Map, Object, ObjectKind, ObjectLayer, Point, Size, TerrainId, Wasteland, NAV_MARGIN, PIXELS_PER_CELL};

/// How many houses a district has: about 86% of the residential lots left
/// once the mandatory buildings have theirs.
pub const HOUSES: usize = 92;
/// How many shops.
pub const SHOPS: usize = 3;
/// How many people can live in one house.
pub const RESIDENTS: std::ops::RangeInclusive<u8> = 1..=2;

/// Cells across a street: a pavement each side of three cells of road.
pub const STREET: i32 = 5;
/// Cells from a street's edge to the lane its lamps and their wiring stand
/// in: the road's outer lane, just off the pavement, so a post never stands
/// in the one cell of pavement a walker has.
const CURB: i32 = 1;
const LOT_W: i32 = 8;
const LOT_H: i32 = 7;
const BLOCK_W: i32 = 2 * LOT_W;
const BLOCK_H: i32 = 2 * LOT_H;
const BLOCKS_X: i32 = 6;
const BLOCKS_Y: i32 = 5;
/// Lots in the blocks that are neither the bank's (two) nor the shops' (one):
/// four to a block.
const RESIDENTIAL_LOTS: usize = 4 * (BLOCKS_X * BLOCKS_Y - 3) as usize;
/// The buildings that must be in every district and take a lot each before
/// any house is built ([`generate`]'s mandatory step): the warehouse.
const LOT_BUILDINGS: usize = 1;
/// The residential lots left as parks: what neither a mandatory building nor
/// a house took.
const EMPTY_LOTS: usize = RESIDENTIAL_LOTS - LOT_BUILDINGS - HOUSES;

pub const WIDTH: i32 = BLOCKS_X * BLOCK_W + (BLOCKS_X + 1) * STREET;
pub const HEIGHT: i32 = BLOCKS_Y * BLOCK_H + (BLOCKS_Y + 1) * STREET;

/// Everything outside a building that is not road or park: both sides of
/// every street, and the yards.
pub const PAVEMENT: &str = "pavement";
/// The middle of every street.
pub const ROAD: &str = "road";
/// A park: an empty lot.
pub const GRASS: &str = "grass";
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
/// Where a scrap collector stores what it brings in: one in each corner of
/// the warehouse, and one collector to a bin (`sim::job`).
pub const SCRAP_BIN: &str = "scrap bin";
/// How many bins the warehouse has, and so how many collectors a district
/// can employ.
pub const WAREHOUSE_BINS: usize = 4;
const WAREHOUSE_WALL: &str = "wall brown";
const WAREHOUSE_FLOOR: &str = "floor diagonal";
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
/// What lights a street at night.
pub const STREET_LAMP: &str = "street lamp";
/// Cells between two street lamps along one side of a street: close enough
/// that their pools meet (a street lamp reaches six and a half).
pub const STREET_LAMP_SPACING: i32 = 8;
/// The shortest stretch of a street's side worth a controller and a lamp.
const SHORTEST_LIT_RUN: usize = 6;
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
    PAVEMENT,
    ROAD,
    GRASS,
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
    WAREHOUSE_WALL,
    WAREHOUSE_FLOOR,
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
    /// Where the scrap collectors store their scrap: on a lot facing the
    /// edge street, a [`SCRAP_BIN`] in each corner.
    pub warehouse: Rect,
    /// The lots left as grass: the residential lots without a house, and the
    /// shop block's fourth.
    pub parks: Vec<Rect>,
}

impl District {
    /// Everybody who lives here: one spawner each.
    pub fn residents(&self) -> usize {
        self.houses.iter().map(|house| house.residents as usize).sum()
    }

    /// The outside of every building's walls: the houses, the shops, the
    /// warehouse and the bank.
    pub fn buildings(&self) -> Vec<Rect> {
        self.houses
            .iter()
            .map(|house| house.walls)
            .chain(self.shops.iter().copied())
            .chain([self.warehouse, self.bank])
            .collect()
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

impl Lot {
    /// Whether its `side` faces the street round the edge of the district,
    /// with only the wasteland beyond it.
    const fn faces_the_edge(self, side: Side) -> bool {
        match side {
            Side::West => self.origin.x == STREET,
            Side::East => self.origin.x + LOT_W == WIDTH - STREET,
            Side::South => self.origin.y == STREET,
            Side::North => self.origin.y + LOT_H == HEIGHT - STREET,
        }
    }

    /// Its outer sides that face the edge street: none for a lot inside the
    /// district, two for the corner of a corner block.
    fn edge_sides(self) -> impl Iterator<Item = Side> {
        self.outer.into_iter().filter(move |&side| self.faces_the_edge(side))
    }
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
    door_clear_of_corners(rng, walls, side, width, 1)
}

/// `width` cells of the wall on `side`, at least `clear` cells from either
/// corner — `2` keeps the cells inside it off the room's own corners.
fn door_clear_of_corners(rng: &mut SmallRng, walls: Rect, side: Side, width: i32, clear: i32) -> Vec<Point> {
    let along = match side {
        Side::South | Side::North => walls.w,
        Side::West | Side::East => walls.h,
    };
    let start = rng.random_range(clear..=along - clear - width);
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
    let mut map = Map::new(Size::new(WIDTH, HEIGHT), tile(ROAD));
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

    // The ground goes down first, buildings on top of it.
    let bank_area = {
        let origin = block_origin(bank_x, bank_y);
        Rect::new(origin.x, origin.y, 2 * BLOCK_W + STREET, BLOCK_H)
    };
    let block_areas: Vec<Rect> = blocks
        .iter()
        .chain([(shop_x, shop_y)].iter())
        .map(|&(bx, by)| {
            let origin = block_origin(bx, by);
            Rect::new(origin.x, origin.y, BLOCK_W, BLOCK_H)
        })
        .chain([bank_area])
        .collect();
    pave(&mut map, &block_areas);

    let bank = bank(&mut rng, &mut map, &mut props, bank_x, bank_y);
    let (shops, spare) = shops(&mut rng, &mut map, &mut props, shop_x, shop_y);

    let mut lots: Vec<Lot> = blocks.iter().flat_map(|&(bx, by)| lots_of(bx, by)).collect();
    lots.shuffle(&mut rng);
    // The buildings every district must have, a lot each, before a house is
    // given one: the next one is another lot taken here and a builder. The
    // warehouse goes on a lot facing the street round the edge, the side the
    // scrap is on — the one nearest a heap, ties to the first in the
    // shuffled order. The wasteland is the seed's as much as the district
    // is, so this draws nothing and is the same every time.
    let heaps = heaps_round(seed);
    let to_scrap = |lot: &Lot| {
        let middle = lot.origin.offset(LOT_W / 2, LOT_H / 2);
        heaps.iter().map(|&heap| heap.manhattan_distance(middle)).min().unwrap_or(i32::MAX)
    };
    let warehouse_lot = take_lot(&mut lots, |lot| lot.edge_sides().next().is_some(), to_scrap);
    let warehouse = warehouse(&mut rng, &mut map, &mut props, warehouse_lot);
    let empty = lots.split_off(lots.len() - EMPTY_LOTS);
    // Back into street order, so the spawners — and so the order people
    // arrive in — read the map from the bottom left rather than at random.
    lots.sort_by_key(|lot| (lot.origin.y, lot.origin.x));
    let houses: Vec<House> = lots
        .into_iter()
        .map(|lot| house(&mut rng, &mut map, &mut props, &mut spawners, lot))
        .collect();

    let mut parks: Vec<Rect> = empty.iter().chain([&spare]).map(|lot| Rect::new(lot.origin.x, lot.origin.y, LOT_W, LOT_H)).collect();
    parks.sort_by_key(|park| (park.y, park.x));
    let grass = tile(GRASS);
    for cell in parks.iter().flat_map(|park| park.cells()) {
        map.set_terrain(cell, grass);
    }

    for prop in props {
        map.add_object(ObjectLayer::Props, prop);
    }
    for spawner in spawners {
        map.add_object(ObjectLayer::Spawners, spawner);
    }
    let rooms: Vec<Rect> =
        houses.iter().map(|house| house.walls).chain(shops.iter().copied()).chain([warehouse]).collect();
    roof_and_light(&mut map, &rooms, bank);
    let buildings: Vec<Rect> = rooms.iter().copied().chain([bank]).collect();
    for &walls in &buildings {
        glaze(&mut map, walls);
    }
    connect(&mut map, &buildings);
    light_streets(&mut map, &buildings);
    // Last, and from the seed without drawing on it: everything above was
    // laid out on a bounded map, so the wasteland round the district changes
    // nothing in it, and each district has a wasteland of its own.
    map.set_wasteland(Some(crate::map::Wasteland::new(seed)));

    District {
        seed,
        map,
        houses,
        bank,
        shops,
        warehouse,
        parks,
    }
}

/// Of the `lots` that `wanted` says yes to, the one `rank` puts lowest —
/// the first of them, on a tie — taken out of them. There is always one:
/// every district has lots on its edge.
fn take_lot(lots: &mut Vec<Lot>, wanted: impl Fn(&Lot) -> bool, rank: impl Fn(&Lot) -> i32) -> Lot {
    let at = (0..lots.len())
        .filter(|&i| wanted(&lots[i]))
        .min_by_key(|&i| rank(&lots[i]))
        .expect("district: no lot for a building it must have");
    lots.remove(at)
}

/// Every heap of scrap the wasteland of `seed` lays within the navigation
/// area round a district's site ([`NAV_MARGIN`]) — the heaps its scrap
/// collectors go to (`sim::brain::goals::CollectGoal`). Worked out from the
/// generator cell by cell, before the map has a wasteland of its own.
fn heaps_round(seed: u64) -> Vec<Point> {
    let site = Bounds::from(Size::new(WIDTH, HEIGHT));
    let wasteland = Wasteland::new(seed);
    site.grown(NAV_MARGIN)
        .points()
        .filter(|&cell| !site.contains(cell) && wasteland.scrap(cell, site).is_some())
        .collect()
}

/// The ground before anything is built on it: [`PAVEMENT`] over every block,
/// one cell round it and along the map's edge, and [`ROAD`] — what the map
/// starts as — everywhere else. So each street reads pavement, three cells
/// of road, pavement, and where two meet the pavements wrap round the
/// corners and the road runs on through. `blocks` counts the bank's two
/// blocks as one, street between and all, since the bank stands across it.
fn pave(map: &mut Map, blocks: &[Rect]) {
    let pavement = tile(PAVEMENT);
    let near_a_block = |cell: Point| {
        blocks.iter().any(|block| {
            cell.x >= block.x - 1 && cell.x <= block.right() + 1 && cell.y >= block.y - 1 && cell.y <= block.top() + 1
        })
    };
    for cell in map.size().points().collect::<Vec<_>>() {
        let edge = cell.x == 0 || cell.y == 0 || cell.x == WIDTH - 1 || cell.y == HEIGHT - 1;
        if edge || near_a_block(cell) {
            map.set_terrain(cell, pavement);
        }
    }
}

/// A roof over every building, and the lamps under them: one in the middle
/// of each of the `rooms` (the houses, the shops, the warehouse) and a grid
/// in the bank. Last, from the buildings as built, and without the seed —
/// see the module docs.
fn roof_and_light(map: &mut Map, rooms: &[Rect], bank: Rect) {
    for &walls in rooms.iter().chain([&bank]) {
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

/// Street lamps along both sides of every street, on a few long circuits,
/// one [`utilities::CONTROLLER`] each. Last, after the mains and every
/// building's spur, from the map as built and without the seed — see the
/// module docs.
///
/// A street is a pavement, three cells of road and a pavement, with the
/// main under the middle of the road, so its two **sides** — the road's
/// outer lanes, [`CURB`] in from either edge, either side of the main — are
/// where lamps go: off the pavement, which is one cell wide and the crowd's
/// ([`can_stand`] keeps every post and cabinet off it).
///
/// * **Rows** (the sides of the streets running across) are wired end to
///   end. Where a power line crosses one — a main at a crossroads, a
///   building's spur — the crossing cell becomes a **junction**: a
///   distribution box with no cabinet, which still carries the line through
///   (a box is a node on the line) and joins the wiring either side of it. A
///   row is cut only by what cannot carry anything: a building, a prop.
/// * **Columns** (the sides of the streets running up) are wired a block at
///   a time and hang off the row below: the wiring starts beside that row's
///   and stops one cell short of the row above, so a column joins one row's
///   circuit and never ties two together.
///
/// So a circuit is about one side of one street with the columns hanging off
/// it — a handful in a district — and [`place_controllers`] puts a single
/// controller on each, which switches every box on it.
fn light_streets(map: &mut Map, buildings: &[Rect]) {
    let inside_any = |cell: Point| buildings.iter().any(|walls| walls.contains(cell));
    let doors: Vec<Point> = map
        .objects(ObjectLayer::Props)
        .iter()
        .filter(|prop| [DOOR, HOUSE_DOOR].contains(&prop.kind.as_str()))
        .map(Object::cell)
        .collect();
    let streets_x: Vec<i32> = (0..=BLOCKS_X).map(|k| k * (BLOCK_W + STREET)).collect();
    let streets_y: Vec<i32> = (0..=BLOCKS_Y).map(|k| k * (BLOCK_H + STREET)).collect();

    // Each stretch with the offset its lamps are spaced from: the far side of
    // a street is staggered half a spacing against the near one, so two
    // lamps never face each other across it.
    let mut stretches: Vec<(Vec<Point>, i32)> = Vec::new();
    let (near, far) = (CURB, STREET - 1 - CURB);
    for &y0 in &streets_y {
        for (y, stagger) in [(y0 + near, 0), (y0 + far, STREET_LAMP_SPACING / 2)] {
            stretches.push(((0..WIDTH).map(|x| Point::new(x, y)).collect(), stagger));
        }
    }
    for &x0 in &streets_x {
        for (x, stagger) in [(x0 + near, 0), (x0 + far, STREET_LAMP_SPACING / 2)] {
            // A block's height at a time, from beside the far row of the
            // street below to one short of the near row of the street above.
            for &y0 in &streets_y[..streets_y.len() - 1] {
                let (from, to) = (y0 + far + 1, y0 + STREET + BLOCK_H + near - 1);
                stretches.push(((from..to).map(|y| Point::new(x, y)).collect(), stagger));
            }
        }
    }

    for (stretch, stagger) in stretches {
        let carries = |map: &Map, cell: Point| {
            map.contains(cell)
                && !inside_any(cell)
                && match map.grid(GridLayer::Power, cell) {
                    power::NONE => map.is_passable(cell),
                    // A line crossing: a junction, unless something stands on
                    // it (the transformer).
                    power::LINE => map.is_passable(cell),
                    // Street wiring laid by a stretch before.
                    _ => true,
                }
        };
        let runs: Vec<Vec<Point>> = stretch
            .split(|&cell| !carries(map, cell))
            .filter(|run| run.len() >= SHORTEST_LIT_RUN)
            .map(<[Point]>::to_vec)
            .collect();
        for run in runs {
            wire_run(map, &run, stagger, &doors);
        }
    }
    place_controllers(map, &doors);
}

/// Wiring the length of `run` — a junction box wherever a line crosses it —
/// and lamps along it every [`STREET_LAMP_SPACING`] cells, counted from
/// `stagger`, each nudged to the nearest cell a post can stand in.
fn wire_run(map: &mut Map, run: &[Point], stagger: i32, doors: &[Point]) {
    let along = |cell: Point| if run[0].y == run[run.len() - 1].y { cell.x } else { cell.y };
    let plain = |map: &Map, at: usize| map.grid(GridLayer::Power, run[at]) == power::NONE;
    let lamp_sites: Vec<usize> = (0..run.len())
        .filter(|&at| (along(run[at]) + stagger) % STREET_LAMP_SPACING == STREET_LAMP_SPACING / 2)
        .collect();
    for want in lamp_sites {
        let found = (0..=2)
            .flat_map(|d| [want.checked_sub(d), Some(want + d)])
            .flatten()
            .find(|&at| at < run.len() && plain(map, at) && can_stand(map, run[at], doors) && no_lamp_near(map, run[at]));
        if let Some(at) = found {
            map.add_object(ObjectLayer::Props, object(STREET_LAMP, run[at]));
        }
    }
    for &cell in run {
        let wired = match map.grid(GridLayer::Power, cell) {
            power::NONE => power::WIRING,
            power::LINE => power::BOX,
            other => other,
        };
        map.set_grid(GridLayer::Power, cell, wired);
    }
}

/// One controller on every street-lighting circuit: on a wiring cell beside
/// a line, as near the middle of the circuit as a cabinet can stand, with a
/// box put under it. A circuit with no such cell, or no lamp, is taken up
/// again — lamps, wiring and junctions — since nothing could switch it.
fn place_controllers(map: &mut Map, doors: &[Point]) {
    let size = map.size();
    let mut seen = vec![false; size.area()];
    let outdoor_wiring: Vec<Point> = size
        .points()
        .filter(|&cell| map.grid(GridLayer::Power, cell) == power::WIRING && !map.has_ceiling(cell))
        .collect();
    for start in outdoor_wiring {
        if seen[size.index_of(start).unwrap()] {
            continue;
        }
        let circuit = utilities::circuit(map, start);
        for &cell in &circuit {
            seen[size.index_of(cell).unwrap()] = true;
        }
        let is_lamp = |map: &Map, cell: Point| {
            map.objects(ObjectLayer::Props).iter().any(|o| o.kind.as_str() == STREET_LAMP && o.cell() == cell)
        };
        let has_lamp = circuit.iter().any(|&cell| is_lamp(map, cell));
        let middle = circuit[circuit.len() / 2];
        let beside_line = |cell: Point| {
            cell.cardinal_neighbours().into_iter().any(|next| map.grid(GridLayer::Power, next) == power::LINE)
        };
        let site = circuit
            .iter()
            .copied()
            .filter(|&cell| {
                map.grid(GridLayer::Power, cell) == power::WIRING && beside_line(cell) && can_stand(map, cell, doors)
            })
            .min_by_key(|&cell| ((cell.x - middle.x).pow(2) + (cell.y - middle.y).pow(2), cell));
        match (has_lamp, site) {
            (true, Some(site)) => {
                map.set_grid(GridLayer::Power, site, power::BOX);
                map.add_object(ObjectLayer::Props, object(utilities::CONTROLLER, site));
            }
            _ => {
                for cell in circuit {
                    if is_lamp(map, cell) {
                        map.remove_object(ObjectLayer::Props, &object(STREET_LAMP, cell));
                    }
                    let bare = if map.grid(GridLayer::Power, cell) == power::BOX { power::LINE } else { power::NONE };
                    map.set_grid(GridLayer::Power, cell, bare);
                }
            }
        }
    }
}

/// Whether something solid may stand in `cell`: nothing is there, it is not
/// the pavement, no door is beside it, and it cuts nobody off
/// ([`blocks_nothing`]).
fn can_stand(map: &Map, cell: Point, doors: &[Point]) -> bool {
    map.is_passable(cell)
        && map.terrain(cell) != Some(tile(PAVEMENT))
        && doors.iter().all(|door| (door.x - cell.x).abs() > 1 || (door.y - cell.y).abs() > 1)
        && blocks_nothing(map, cell)
}

/// Whether no street lamp stands within two cells of `cell` — where a
/// column's lamps meet a row's at a crossroads.
fn no_lamp_near(map: &Map, cell: Point) -> bool {
    map.objects(ObjectLayer::Props)
        .iter()
        .filter(|o| o.kind.as_str() == STREET_LAMP)
        .all(|o| (o.cell().x - cell.x).abs() > 2 || (o.cell().y - cell.y).abs() > 2)
}

/// Whether something solid in `cell` would leave every way round it open:
/// the walkable cells beside it still join up through the corners between
/// them, so nobody who could walk through it is cut off by it. Local, and so
/// stricter than it has to be — it never looks for a long way round — which
/// is the safe side for a lamp post.
fn blocks_nothing(map: &Map, cell: Point) -> bool {
    // The eight cells round it, in order round the ring, cardinals at the
    // even places.
    const RING: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
    let open = RING.map(|(dx, dy)| map.is_passable(cell.offset(dx, dy)));
    if open.iter().all(|&o| o) {
        return true;
    }
    // Two cardinals in a row round the ring are joined when the corner
    // between them is open too. Count the open cardinals not joined to the
    // one before: each is a separate stretch, and more than one is two
    // sides only `cell` was joining.
    let starts = (0..8)
        .step_by(2)
        .filter(|&i| open[i] && !(open[(i + 7) % 8] && open[(i + 6) % 8]))
        .count();
    starts <= 1
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

/// Three empty shops in one block, and the fourth lot, which is left as a
/// park.
fn shops(rng: &mut SmallRng, map: &mut Map, props: &mut Vec<Object>, bx: i32, by: i32) -> (Vec<Rect>, Lot) {
    let mut lots = lots_of(bx, by).to_vec();
    let spare = lots.remove(rng.random_range(0..lots.len()));
    let shops = lots
        .into_iter()
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
        .collect();
    (shops, spare)
}

/// The warehouse, on `lot`: a room as big as a shop, a plain door — anybody
/// may come in, it is nobody's property — in a wall facing the edge street,
/// and a [`SCRAP_BIN`] in each of its [`WAREHOUSE_BINS`] corners. The door is
/// kept two cells off the corners, so no bin stands in front of it, and each
/// bin has the two cells beside it free to be used from.
fn warehouse(rng: &mut SmallRng, map: &mut Map, props: &mut Vec<Object>, lot: Lot) -> Rect {
    let walls = place_in(rng, lot, LOT_W - 1, LOT_H - 1);
    build(map, walls, WAREHOUSE_WALL, WAREHOUSE_FLOOR);
    let sides: Vec<Side> = lot.edge_sides().collect();
    let side = sides[rng.random_range(0..sides.len())];
    for cell in door_clear_of_corners(rng, walls, side, DOOR_WIDTH, 2) {
        map.set_terrain(cell, tile(WAREHOUSE_FLOOR));
        props.push(object(DOOR, cell));
    }
    let inside = walls.interior();
    let corners = [
        Point::new(inside.x, inside.y),
        Point::new(inside.right(), inside.y),
        Point::new(inside.x, inside.top()),
        Point::new(inside.right(), inside.top()),
    ];
    for corner in corners {
        props.push(object(SCRAP_BIN, corner));
    }
    walls
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
            let buildings = district.buildings();
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
            let buildings = district.buildings();
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
            assert_eq!(in_room(district.warehouse, HOUSE_LAMP), 1, "seed {seed}: the warehouse");
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
        for name in BEDS
            .iter()
            .chain(&[FRIDGE, TOILET, COMPUTER, SCRAP_BIN, TRANSFORMER, SEWER, DOOR, HOUSE_DOOR, STREET_LAMP, utilities::CONTROLLER])
        {
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
            let buildings = district.buildings();
            // Outdoors, wiring is the street lights' and nobody else's: every
            // outdoor circuit has a controlled box in it and nothing indoors.
            let controlled = utilities::controlled_boxes(map);
            let indoors = |cell: Point| buildings.iter().any(|walls| walls.interior().contains(cell));
            for cell in map.size().points() {
                match map.grid(GridLayer::Power, cell) {
                    power::LINE => assert!(!indoors(cell), "seed {seed}: a line indoors at {cell:?}"),
                    power::WIRING | power::BOX if !indoors(cell) => {
                        let circuit = utilities::circuit(map, cell);
                        assert!(
                            circuit.iter().any(|at| controlled.contains(at)),
                            "seed {seed}: wiring outdoors at {cell:?} that no street light controller switches"
                        );
                        assert!(
                            circuit.iter().all(|&at| !indoors(at)),
                            "seed {seed}: the street lights at {cell:?} are wired into a building"
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn the_street_lamps_have_power_at_night_only_and_everything_else_all_day() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let day = utilities::Supply::with_boxes_off(map, &utilities::controlled_boxes(map));
            let night = utilities::Supply::from_map(map);
            let mut lamps = 0;
            for (object, _, served) in day.consumers(map) {
                if object.kind.as_str() == STREET_LAMP {
                    lamps += 1;
                    assert!(!served, "seed {seed}: the street lamp at {:?} is on by day", object.cell());
                    assert!(night.serves(STREET_LAMP, object.cell()));
                } else {
                    assert!(
                        served,
                        "seed {seed}: the {} at {:?} has power only at night",
                        object.kind.as_str(),
                        object.cell()
                    );
                }
            }
            assert!(lamps >= 30, "seed {seed}: {lamps} street lamps");
            // A few long circuits, one cabinet each: about one per side of a
            // street running across, not one per stretch between two cables.
            let cabinets = props_in(map, Rect::new(0, 0, WIDTH, HEIGHT), utilities::CONTROLLER);
            assert!((1..=2 * (BLOCKS_Y as usize + 1) + 2).contains(&cabinets), "seed {seed}: {cabinets} controllers");
        }
    }

    /// Most of the street is in a lamp's reach: a cell of the main under the
    /// middle of a street is nearer a lamp than its radius.
    #[test]
    fn the_streets_are_lit() {
        let radius = crate::lighting::scene::emission(STREET_LAMP).expect("a street lamp gives off light").radius;
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let lamps: Vec<Point> = map
                .objects(ObjectLayer::Props)
                .iter()
                .filter(|o| o.kind.as_str() == STREET_LAMP)
                .map(Object::cell)
                .collect();
            let mains: Vec<Point> =
                map.size().points().filter(|&cell| map.grid(GridLayer::Power, cell) == power::LINE).collect();
            let lit = mains
                .iter()
                .filter(|cell| {
                    lamps
                        .iter()
                        .any(|lamp| (((lamp.x - cell.x).pow(2) + (lamp.y - cell.y).pow(2)) as f32).sqrt() < radius)
                })
                .count();
            assert!(lit * 10 >= mains.len() * 9, "seed {seed}: {lit} of {} cells of main lit", mains.len());
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

            // One warehouse, its bins and nothing else in it, open to
            // anybody, on a lot facing the street round the edge.
            let warehouse = district.warehouse;
            let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| warehouse.interior().contains(prop.cell()));
            assert!(inside.clone().all(|prop| prop.kind.as_str() == SCRAP_BIN), "seed {seed}: the warehouse holds bins only");
            assert_eq!(inside.count(), WAREHOUSE_BINS, "seed {seed}");
            assert_eq!(props_in(map, Rect::new(0, 0, WIDTH, HEIGHT), SCRAP_BIN), WAREHOUSE_BINS, "seed {seed}: bins nowhere else");
            let properties = crate::sim::Properties::from_map(map);
            for cell in warehouse.cells() {
                assert_eq!(properties.of(cell), None, "seed {seed}: the warehouse is nobody's property");
            }
            let near_the_edge = warehouse.x - STREET <= 1
                || WIDTH - STREET - 1 - warehouse.right() <= 1
                || warehouse.y - STREET <= 1
                || HEIGHT - STREET - 1 - warehouse.top() <= 1;
            assert!(near_the_edge, "seed {seed}: the warehouse at {warehouse:?} is not on the edge");

            // Of the lots on the edge to be had — not the bank's or the
            // shops' blocks — the one nearest the scrap.
            let heaps = heaps_round(seed);
            let to_scrap = |lot: &Lot| {
                let middle = lot.origin.offset(LOT_W / 2, LOT_H / 2);
                heaps.iter().map(|&heap| heap.manhattan_distance(middle)).min().unwrap()
            };
            let taken = |bx: i32, by: i32| {
                let origin = block_origin(bx, by);
                let block = Rect::new(origin.x, origin.y, BLOCK_W, BLOCK_H);
                let bank = district.bank;
                [Point::new(bank.x, bank.y), Point::new(bank.right(), bank.top()), Point::new(district.shops[0].x, district.shops[0].y)]
                    .into_iter()
                    .any(|cell| block.contains(cell))
            };
            let edge_lots: Vec<Lot> = (0..BLOCKS_Y)
                .flat_map(|by| (0..BLOCKS_X).map(move |bx| (bx, by)))
                .filter(|&(bx, by)| !taken(bx, by))
                .flat_map(|(bx, by)| lots_of(bx, by))
                .filter(|lot| lot.edge_sides().next().is_some())
                .collect();
            let ours = edge_lots
                .iter()
                .find(|lot| Rect::new(lot.origin.x, lot.origin.y, LOT_W, LOT_H).contains(Point::new(warehouse.x, warehouse.y)))
                .expect("the warehouse is on an edge lot");
            let nearest = edge_lots.iter().map(to_scrap).min().unwrap();
            assert_eq!(to_scrap(ours), nearest, "seed {seed}: the warehouse is not on the edge lot nearest the scrap");
        }
    }

    /// The warehouse's door faces the edge street, so a collector steps out
    /// of it towards the wasteland.
    #[test]
    fn the_warehouse_door_opens_onto_the_street_round_the_edge() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let warehouse = district.warehouse;
            let doors: Vec<Point> = doors_of(map).into_iter().filter(|&(cell, _)| warehouse.contains(cell)).map(|(cell, _)| cell).collect();
            assert_eq!(doors.len(), DOOR_WIDTH as usize, "seed {seed}");
            for door in doors {
                let outside = door.cardinal_neighbours().into_iter().find(|&cell| !warehouse.contains(cell)).unwrap();
                let edge_street = outside.x < STREET || outside.x >= WIDTH - STREET || outside.y < STREET || outside.y >= HEIGHT - STREET;
                let yard = !edge_street && map.terrain(outside) == Some(tile(PAVEMENT));
                // A building is placed somewhere in its lot, so the door may
                // open onto the cell of yard between the lot's edge and it.
                assert!(edge_street || yard, "seed {seed}: the warehouse door at {door:?} opens onto {outside:?}");
                let toward_edge = |cell: Point| {
                    cell.x.min(cell.y).min(WIDTH - 1 - cell.x).min(HEIGHT - 1 - cell.y)
                };
                assert!(toward_edge(outside) < toward_edge(door), "seed {seed}: the door at {door:?} faces inwards");
            }
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
                .chain([(district.warehouse, DOOR), (district.bank, DOOR)])
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

    // --- the ground ---

    /// The tiles across a street at `along`, from one edge to the other.
    fn cross_section(map: &Map, start: Point, across: Point) -> Vec<TerrainId> {
        (0..STREET).map(|i| map.terrain(start.offset(across.x * i, across.y * i)).unwrap()).collect()
    }

    #[test]
    fn every_street_is_a_pavement_each_side_of_three_cells_of_road() {
        let (pavement, road) = (tile(PAVEMENT), tile(ROAD));
        let street = [pavement, road, road, road, pavement];
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            // Across each street halfway along every block beside it, away
            // from the crossroads — except where the bank stands across one.
            for k in 0..=BLOCKS_Y {
                let y0 = k * (BLOCK_H + STREET);
                for bx in 0..BLOCKS_X {
                    let x = block_origin(bx, 0).x + BLOCK_W / 2;
                    assert_eq!(cross_section(map, Point::new(x, y0), Point::new(0, 1)), street, "seed {seed}: across at x {x}");
                }
            }
            for k in 0..=BLOCKS_X {
                let x0 = k * (BLOCK_W + STREET);
                for by in 0..BLOCKS_Y {
                    let y = block_origin(0, by).y + BLOCK_H / 2;
                    if district.bank.contains(Point::new(x0 + STREET / 2, y)) {
                        continue;
                    }
                    assert_eq!(cross_section(map, Point::new(x0, y), Point::new(1, 0)), street, "seed {seed}: across at y {y}");
                }
            }
        }
    }

    #[test]
    fn the_empty_lots_are_parks_and_nothing_else_is_grass() {
        let grass = tile(GRASS);
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            assert_eq!(district.parks.len(), EMPTY_LOTS + 1, "seed {seed}: the empty lots and the shops' fourth");
            for park in &district.parks {
                assert_eq!((park.w, park.h), (LOT_W, LOT_H), "seed {seed}: a park is a whole lot");
            }
            for cell in map.size().points() {
                let in_a_park = district.parks.iter().any(|park| park.contains(cell));
                assert_eq!(map.terrain(cell) == Some(grass), in_a_park, "seed {seed}: {cell:?}");
            }
            for walls in district.buildings() {
                assert!(district.parks.iter().all(|park| walls.cells().all(|cell| !park.contains(cell))), "seed {seed}: a building in a park");
            }
        }
    }

    #[test]
    fn nothing_stands_on_the_pavement_but_the_transformer_and_the_sewer() {
        // A lamp post on a pavement one cell wide sends everybody walking it
        // out into the road and back.
        let pavement = tile(PAVEMENT);
        for seed in SEEDS {
            let map = generate(seed).map;
            for prop in map.objects(ObjectLayer::Props) {
                if map.terrain(prop.cell()) == Some(pavement) {
                    assert!([TRANSFORMER, SEWER].contains(&prop.kind.as_str()), "seed {seed}: a {} on the pavement at {:?}", prop.kind.as_str(), prop.cell());
                }
            }
        }
    }

    /// The cell outside each house's door: where a resident steps out to.
    fn front_steps(district: &District) -> Vec<Point> {
        let map = &district.map;
        district
            .houses
            .iter()
            .map(|house| {
                doors_of(map)
                    .into_iter()
                    .filter(|&(cell, _)| house.walls.contains(cell))
                    .flat_map(|(cell, _)| cell.cardinal_neighbours())
                    .find(|&cell| !house.walls.contains(cell) && map.is_passable(cell))
                    .expect("a door opens onto somewhere")
            })
            .collect()
    }

    #[test]
    fn people_keep_to_the_pavement_and_cross_the_road_straight_over() {
        // Routes the way a walker's far stage asks for them, between the
        // front doors of houses across the district from each other.
        let (pavement, road) = (tile(PAVEMENT), tile(ROAD));
        for seed in 0..8 {
            let district = generate(seed);
            let map = &district.map;
            let steps = front_steps(&district);
            let (mut on_pavement, mut on_road) = (0usize, 0usize);
            for (i, &from) in steps.iter().enumerate() {
                let to = steps[(i + steps.len() / 2) % steps.len()];
                let route = crate::sim::find_path(map.size(), from, to, 1 << 20, |cell| {
                    map.is_passable(cell).then(|| map.walk_cost(cell) as u32)
                })
                .unwrap_or_else(|| panic!("seed {seed}: no way from {from:?} to {to:?}"));
                let mut run = 0;
                for &cell in &route {
                    let ground = map.terrain(cell);
                    if ground == Some(road) {
                        on_road += 1;
                        run += 1;
                        assert!(run <= STREET - 2, "seed {seed}: walked down the road at {cell:?}, {from:?} to {to:?}");
                    } else {
                        run = 0;
                        on_pavement += usize::from(ground == Some(pavement));
                    }
                }
            }
            let share = on_pavement as f32 / (on_pavement + on_road) as f32;
            assert!(share >= 0.8, "seed {seed}: only {:.0}% of the way out of doors on the pavement", share * 100.0);
        }
    }
}

