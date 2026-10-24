//! A district made from a seed: [`generate`].
//!
//! A hundred and twenty-four houses with one or two people living in each, a
//! palace in the middle for the king, a bank laid out like an office, three
//! empty shops, a warehouse where the scrap collectors store what they bring
//! in from the wasteland, a power plant that powers all of it, and streets to
//! walk between them, the main street wider than the rest. The same seed always gives the same district — every choice is
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
//! (`sim::path`) — walks the pavement and crosses the road straight over.
//! One street is wider: the **main street** ([`MAIN_STREET`]), two cells of
//! pavement each side of five of road, running across the district from a
//! gate in one side wall to a gate in the other, along the front of the
//! palace. A block is 2x2 **lots**, and anything in a block that is not a
//! building is pavement too, so the yards and the gaps between houses are as
//! walkable as the street. The block in the middle is always the palace's;
//! which of the rest are what is the seed's choice:
//!
//! * **the [`palace`]** — the whole of the middle block, its door onto the
//!   main street, the king's throne across the hall from it;
//! * **the bank** — two neighbouring blocks merged across the street between
//!   them: two opposite exits joined by a clear aisle, a lobby lined with
//!   fridges, an office of computers in desk rows, and two restrooms walled
//!   off at each end;
//! * **the shops** — one block, three of its lots a shop with a wide door and
//!   nothing inside yet, the fourth left as a small park;
//! * **the lot buildings** every district must have — a lot each, taken
//!   before any house, one module each, listed in [`LOT_BUILDINGS`]: the
//!   [`warehouse`] on the edge lot nearest the scrap, a [`restaurant`] in each
//!   quarter, the [`crematorium`] on the edge, the [`steel_factory`] beside
//!   the warehouse, the [`farm`] on the edge lot nearest the good soil, the
//!   [`power_plant`] on the edge. **A new one is a module with a [`LotBuilding`] in it and
//!   a line in that list**; everything else — the lot it takes from the
//!   houses, its roof, lamp, windows, power, its place in
//!   [`District::buildings`] — follows from the list;
//! * **the houses** — every lot of every other block, less [`EMPTY_LOTS`]
//!   left as parks — room for lot buildings to come — which leaves
//!   [`HOUSES`].
//!
//! **A wall round the whole of it** ([`PERIMETER_WALL`]) takes the place of
//! the pavement along the map's edge, so the street round the edge reads
//! wall, three cells of road, pavement. It is broken only by [`GATES`]
//! gates, [`GATES_PER_SIDE`] a side, each a two-leaf [`DOOR`] where a street
//! running out to the edge meets it, with a [`GUARD_POST`] set into the
//! wall two cells along — the guards' workplaces (`sim::job`), three watches
//! to a post. Laid from the geometry alone, drawing nothing from the seed.
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
//! (`crate::lighting`). A house, a shop and a lot building each get one warm
//! [`HOUSE_LAMP`] in the middle of the room; the bank gets [`OFFICE_LAMP`]s in a grid every
//! [`OFFICE_LAMP_SPACING`] cells, and the palace's hall a grid of warm ones.
//! All are laid out from the buildings once
//! they are built, drawing nothing from the seed, so adding them changed no
//! district: a seed is still the same streets, houses and people.
//!
//! # Power and water
//!
//! Laid last, like the lamps and for the same reason: from the buildings as
//! built, without the seed. The [`power_plant`]'s generators are the
//! district's one source of power, a [`SEWER`] in a corner drains it, and a
//! power line and a sewer main run down the middle of every street, under the
//! paving, skipping whatever a building stands on. The plant's line runs from
//! under each generator into its walls and round them to where it goes out
//! to the main, as every building's does ([`feed_in`]). Every building with something to power has
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

pub mod crematorium;
pub mod farm;
pub mod food_factory;
pub mod playzone;
pub mod palace;
pub mod power_plant;
pub mod restaurant;
pub mod steel_factory;
pub mod warehouse;

use super::grid::{power, water};
use super::utilities::{self, Utility};
use super::{is_plot, Bounds, GridLayer, Map, Object, ObjectKind, ObjectLayer, Point, Size, TerrainId, Wasteland, NAV_MARGIN, PIXELS_PER_CELL};

/// How many houses a district has: four in five of the residential lots
/// left once the lot buildings have theirs. The rest are parks — and room for
/// lot buildings still to come.
pub const HOUSES: usize = 124;
/// How many shops.
pub const SHOPS: usize = 3;
/// How many people can live in one house.
pub const RESIDENTS: std::ops::RangeInclusive<u8> = 1..=2;

/// Cells across a street: a pavement each side of three cells of road.
pub const STREET: i32 = 5;
/// Cells across the **main street**, the one street wider than the rest:
/// two cells of pavement each side of five of road. It runs across the
/// district from gate to gate along the front of the palace ([`MAIN`]).
pub const MAIN_STREET: i32 = 9;
/// Cells of pavement each side of the main street.
const MAIN_PAVEMENT: i32 = 2;
const LOT_W: i32 = 8;
const LOT_H: i32 = 7;
const BLOCK_W: i32 = 2 * LOT_W;
const BLOCK_H: i32 = 2 * LOT_H;
const BLOCKS_X: i32 = 9;
const BLOCKS_Y: i32 = 5;
/// Which of the streets running across is the main street, counted from the
/// bottom (the street round the bottom edge is 0): the one below the middle
/// row of blocks, which the palace's door opens onto.
const MAIN: i32 = BLOCKS_Y / 2;
const _: () = assert!(BLOCKS_X % 2 == 1 && BLOCKS_Y % 2 == 1, "a district has a block in its middle, for the palace");
/// Lots in the blocks that are neither the bank's (two), the shops' (one)
/// nor the palace's (one): four to a block.
const RESIDENTIAL_LOTS: usize = 4 * (BLOCKS_X * BLOCKS_Y - 4) as usize;
/// The residential lots left as parks: what neither a lot building nor a
/// house took.
const EMPTY_LOTS: usize = RESIDENTIAL_LOTS - lots_built() - HOUSES;

pub const WIDTH: i32 = up(BLOCKS_X).end();
pub const HEIGHT: i32 = across(BLOCKS_Y).end();

/// A street, across its width: where it starts, how wide it is, and how much
/// of that is pavement each side — the rest is road.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Street {
    start: i32,
    width: i32,
    pavement: i32,
}

impl Street {
    /// The first cell past it.
    const fn end(self) -> i32 {
        self.start + self.width
    }

    /// Whether `at`, counted across it, is in it.
    const fn contains(self, at: i32) -> bool {
        at >= self.start && at < self.end()
    }

    /// Cells of road across it.
    const fn road(self) -> i32 {
        self.width - 2 * self.pavement
    }

    /// Its middle cell, which its mains run under.
    const fn middle(self) -> i32 {
        self.start + self.width / 2
    }

    /// Its two **sides**, where its lamps and their wiring stand: the road's
    /// outer lanes, just off the pavement, so a post never stands in the
    /// pavement a walker has.
    const fn sides(self) -> (i32, i32) {
        (self.start + self.pavement, self.end() - 1 - self.pavement)
    }
}

/// The `k`th street running across, from the bottom: 0 is the street round
/// the bottom edge, [`BLOCKS_Y`] the one round the top, [`MAIN`] the main
/// street.
const fn across(k: i32) -> Street {
    let past_the_main = if k > MAIN { MAIN_STREET - STREET } else { 0 };
    let (width, pavement) = if k == MAIN { (MAIN_STREET, MAIN_PAVEMENT) } else { (STREET, 1) };
    Street { start: k * (BLOCK_H + STREET) + past_the_main, width, pavement }
}

/// The `k`th street running up, from the left: 0 is the street round the
/// left edge, [`BLOCKS_X`] the one round the right.
const fn up(k: i32) -> Street {
    Street { start: k * (BLOCK_W + STREET), width: STREET, pavement: 1 }
}

/// The street running across that `y` is in, if it is in one.
fn across_at(y: i32) -> Option<Street> {
    (0..=BLOCKS_Y).map(across).find(|street| street.contains(y))
}

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
/// The wall round the district, in place of the pavement on the map's edge.
pub const PERIMETER_WALL: &str = "wall brown big";
/// How many gates each side of the perimeter wall has.
pub const GATES_PER_SIDE: i32 = 2;
/// How many gates the perimeter wall has, all four sides.
pub const GATES: usize = 4 * GATES_PER_SIDE as usize;
/// A sentry box set into the perimeter wall beside each gate: where its
/// guards keep watch (`sim::job::JobKind::Guard`).
pub const GUARD_POST: &str = "guard post";
/// Cells between the windows along a wall: one every third cell.
const WINDOW_SPACING: i32 = 3;
const BEDS: [&str; 6] = ["bed 1", "bed 2", "bed 3", "bed 4", "bed 5", "bed 6"];
const FRIDGE: &str = "fridge";
const TOILET: &str = "toilet";
const COMPUTER: &str = "computer";
/// What some houses have to pass the time in front of (`sim::feature`'s
/// `Television`), watched from across the room.
pub const TV: &str = "tv";
/// The share of houses with a [`TV`] in them.
const HOUSES_WITH_A_TV: f64 = 0.5;
/// Mixed into the seed for the stream that says which houses have a TV and
/// where it stands, so that choosing draws nothing from the district's own
/// and every seed's streets, houses and people are what they were without.
const TV_STREAM: u64 = 0x7e1e_7151_0a5e_7000;
/// The door of a shop, the bank and its restrooms: anybody may open it.
const DOOR: &str = "door";
/// A house's door, which only the people living there may open: the
/// simulation reads the room behind it as their property (`sim::property`).
const HOUSE_DOOR: &str = "house door";
/// How wide every doorway is, in leaves: one person coming in and one going
/// out of a one-cell door is a standoff neither of them can resolve.
pub const DOOR_WIDTH: i32 = 2;
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

/// Every tile name the generator paints with — beside each lot building's own
/// ([`LotBuilding::tiles`]) — so a test can check each one is still in the
/// catalogue rather than finding out from a panic.
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
    WINDOW,
    PERIMETER_WALL,
];

/// A building every district must have, on a lot of its own, taken before
/// any house is built: what [`LOT_BUILDINGS`] lists, one module each.
pub struct LotBuilding {
    /// What [`District::the`] and [`District::all`] call it.
    pub name: &'static str,
    /// How many of it a district has, a lot each.
    pub count: usize,
    /// Which lots it may stand on.
    wanted: fn(&Lot) -> bool,
    /// How good a lot is for the `index`th of it, lowest best — ties to the
    /// first in the shuffled order — given where the scrap is and what has
    /// been built already ([`Siting`]).
    rank: fn(&Siting<'_>, usize, &Lot) -> i32,
    /// Build it on the lot: walls, floor, door and props; the walls back.
    /// May draw from the district's RNG, which is why the order of
    /// [`LOT_BUILDINGS`] is the order of every seed's draws.
    build: fn(&mut SmallRng, &mut Map, &mut Vec<Object>, Lot) -> Rect,
    /// Every tile it paints with, for the catalogue test.
    pub tiles: &'static [&'static str],
    /// Every prop it places but its doors, for the catalogue test.
    pub props: &'static [&'static str],
}

/// What a [`LotBuilding`] is sited by: the heaps of scrap round the district,
/// the good soil, the gates out to them, and the lot buildings already built
/// — in [`LOT_BUILDINGS`] order — so one can stand near another.
pub struct Siting<'a> {
    pub heaps: &'a [Point],
    /// Every plot of good soil round the district ([`soil_round`]).
    pub soil: &'a [Point],
    /// The gates in the perimeter wall ([`gate_layout`]): the only ways out
    /// to the heaps and the soil.
    pub gates: &'a [Gate],
    pub built: &'a [(&'static str, Rect)],
}

impl Siting<'_> {
    /// How far a walk it is from `from`, in the district, to `to`, out in
    /// the wasteland: round by the gate that makes it shortest
    /// ([`Gate::walk`]).
    pub fn walk_out(&self, from: Point, to: Point) -> i32 {
        self.gates.iter().map(|gate| gate.walk(from, to)).min().unwrap_or(i32::MAX)
    }

    /// The walls of the lot building called `name`, which has to have been
    /// built already: earlier in [`LOT_BUILDINGS`].
    pub fn the(&self, name: &str) -> Rect {
        self.built.iter().find(|(built, _)| *built == name).map(|&(_, walls)| walls).expect("built before")
    }
}

/// Every lot building, **in the order they are built**: each takes its lots
/// and draws from the RNG in turn, so a new one goes on the end — which keeps
/// every seed's bank, shops and lot buildings as they were, and moves only its
/// houses and parks — and moving one changes every district.
pub const LOT_BUILDINGS: &[&LotBuilding] =
    &[&warehouse::SPEC, &restaurant::SPEC, &crematorium::SPEC, &steel_factory::SPEC, &farm::SPEC, &power_plant::SPEC, &food_factory::SPEC, &playzone::SPEC];

/// How many lots the lot buildings take.
const fn lots_built() -> usize {
    let mut lots = 0;
    let mut i = 0;
    while i < LOT_BUILDINGS.len() {
        lots += LOT_BUILDINGS[i].count;
        i += 1;
    }
    lots
}

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
    /// Its [`TV`], against an inside wall, if it has one.
    pub tv: Option<Point>,
}

/// A way through the perimeter wall.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gate {
    /// Its two [`DOOR`] leaves, side by side in the wall.
    pub leaves: [Point; 2],
    /// Its [`GUARD_POST`], in the wall two cells along from the nearer leaf
    /// with wall between, so a guard standing at it stands inside the
    /// district and out of the gateway.
    pub post: Point,
    /// One step out through it: off the map, into the wasteland.
    pub outward: Point,
}

impl Gate {
    /// How far a walk it is along the grid from `from`, in the district, out
    /// through this gate to `to`, out in the wasteland: to the road inside its
    /// first leaf, through, and on. The district's streets are a grid, so
    /// this is near enough what a route there comes to.
    pub fn walk(&self, from: Point, to: Point) -> i32 {
        let (leaf, out) = (self.leaves[0], self.outward);
        from.manhattan_distance(leaf.offset(-out.x, -out.y)) + 2 + leaf.offset(out.x, out.y).manhattan_distance(to)
    }
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
    /// The [`palace`]: the whole of the block in the middle.
    pub palace: Rect,
    /// Every lot building ([`LOT_BUILDINGS`]) by name, in the order they were
    /// built: the warehouse, the restaurants, the crematorium, the steel
    /// factory, the farm, the power plant. Ask for one with [`District::the`]
    /// or [`District::all`].
    pub lot_buildings: Vec<(&'static str, Rect)>,
    /// The lots left as grass: the residential lots without a house, and the
    /// shop block's fourth.
    pub parks: Vec<Rect>,
    /// The ways through the perimeter wall, each with its guard post.
    pub gates: Vec<Gate>,
}

impl District {
    /// Everybody who lives here: one spawner each.
    pub fn residents(&self) -> usize {
        self.houses.iter().map(|house| house.residents as usize).sum()
    }

    /// The outside of every building's walls: the houses, the shops, every
    /// lot building, the bank and the palace.
    pub fn buildings(&self) -> Vec<Rect> {
        self.houses
            .iter()
            .map(|house| house.walls)
            .chain(self.shops.iter().copied())
            .chain(self.lot_buildings.iter().map(|&(_, walls)| walls))
            .chain([self.bank, self.palace])
            .collect()
    }

    /// The walls of the one lot building called `name` (a module's `NAME`:
    /// [`warehouse::NAME`]...).
    pub fn the(&self, name: &str) -> Rect {
        let mut found = self.all(name);
        let walls = found.next().unwrap_or_else(|| panic!("no {name} in the district"));
        assert!(found.next().is_none(), "more than one {name} in the district");
        walls
    }

    /// The walls of every lot building called `name`, in the order built.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = Rect> + 'a {
        self.lot_buildings.iter().filter(move |(each, _)| *each == name).map(|&(_, walls)| walls)
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
            Side::West => self.origin.x == up(0).end(),
            Side::East => self.origin.x + LOT_W == up(BLOCKS_X).start,
            Side::South => self.origin.y == across(0).end(),
            Side::North => self.origin.y + LOT_H == across(BLOCKS_Y).start,
        }
    }

    /// Its outer sides that face the edge street: none for a lot inside the
    /// district, two for the corner of a corner block.
    fn edge_sides(self) -> impl Iterator<Item = Side> {
        self.outer.into_iter().filter(move |&side| self.faces_the_edge(side))
    }
}

/// The lowest corner of block `bx`, `by`: just past the streets left of it
/// and below it.
const fn block_origin(bx: i32, by: i32) -> Point {
    Point::new(up(bx).end(), across(by).end())
}

/// The whole of block `bx`, `by`.
const fn block(bx: i32, by: i32) -> Rect {
    let origin = block_origin(bx, by);
    Rect::new(origin.x, origin.y, BLOCK_W, BLOCK_H)
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

    // Which blocks are what: the palace's is the one in the middle, always;
    // the bank's two, side by side, and the shops' one are the seed's.
    let (palace_x, palace_y) = palace::BLOCK;
    let banks: Vec<(i32, i32)> = (0..BLOCKS_Y)
        .flat_map(|by| (0..BLOCKS_X - 1).map(move |bx| (bx, by)))
        .filter(|&(bx, by)| !(by == palace_y && (bx == palace_x || bx + 1 == palace_x)))
        .collect();
    let (bank_x, bank_y) = banks[rng.random_range(0..banks.len())];
    let mut blocks: Vec<(i32, i32)> = (0..BLOCKS_Y)
        .flat_map(|by| (0..BLOCKS_X).map(move |bx| (bx, by)))
        .filter(|&(bx, by)| !(by == bank_y && (bx == bank_x || bx == bank_x + 1)) && (bx, by) != palace::BLOCK)
        .collect();
    let (shop_x, shop_y) = blocks.remove(rng.random_range(0..blocks.len()));

    // The ground goes down first, buildings on top of it.
    let bank_area = {
        let origin = block_origin(bank_x, bank_y);
        Rect::new(origin.x, origin.y, 2 * BLOCK_W + STREET, BLOCK_H)
    };
    let block_areas: Vec<Rect> = blocks
        .iter()
        .chain([(shop_x, shop_y), palace::BLOCK].iter())
        .map(|&(bx, by)| block(bx, by))
        .chain([bank_area])
        .collect();
    pave(&mut map, &block_areas);

    let palace = palace::palace(&mut map, &mut props);
    let bank = bank(&mut rng, &mut map, &mut props, bank_x, bank_y);
    let (shops, spare) = shops(&mut rng, &mut map, &mut props, shop_x, shop_y);

    let mut lots: Vec<Lot> = blocks.iter().flat_map(|&(bx, by)| lots_of(bx, by)).collect();
    lots.shuffle(&mut rng);
    // The buildings every district must have, a lot each, before a house is
    // given one ([`LOT_BUILDINGS`]), in their order: each takes the lot it
    // ranks best and builds on it.
    let (heaps, soil, ways_out) = (heaps_round(seed), soil_round(seed), gate_layout());
    let mut lot_buildings: Vec<(&'static str, Rect)> = Vec::new();
    for spec in LOT_BUILDINGS {
        for index in 0..spec.count {
            let siting = Siting { heaps: &heaps, soil: &soil, gates: &ways_out, built: &lot_buildings };
            let lot = take_lot(&mut lots, spec.wanted, |lot| (spec.rank)(&siting, index, lot));
            let walls = (spec.build)(&mut rng, &mut map, &mut props, lot);
            lot_buildings.push((spec.name, walls));
        }
    }
    let empty = lots.split_off(lots.len() - EMPTY_LOTS);
    // Back into street order, so the spawners — and so the order people
    // arrive in — read the map from the bottom left rather than at random.
    lots.sort_by_key(|lot| (lot.origin.y, lot.origin.x));
    let mut tvs = SmallRng::seed_from_u64(seed ^ TV_STREAM);
    let houses: Vec<House> = lots
        .into_iter()
        .map(|lot| house(&mut rng, &mut tvs, &mut map, &mut props, &mut spawners, lot))
        .collect();

    let mut parks: Vec<Rect> = empty.iter().chain([&spare]).map(|lot| Rect::new(lot.origin.x, lot.origin.y, LOT_W, LOT_H)).collect();
    parks.sort_by_key(|park| (park.y, park.x));
    let grass = tile(GRASS);
    for cell in parks.iter().flat_map(|park| park.cells()) {
        map.set_terrain(cell, grass);
    }

    let gates = gates(&mut map, &mut props);

    for prop in props {
        map.add_object(ObjectLayer::Props, prop);
    }
    for spawner in spawners {
        map.add_object(ObjectLayer::Spawners, spawner);
    }
    let rooms: Vec<Rect> = houses
        .iter()
        .map(|house| house.walls)
        .chain(shops.iter().copied())
        .chain(lot_buildings.iter().map(|&(_, walls)| walls))
        .collect();
    let halls = [(bank, OFFICE_LAMP, OFFICE_LAMP_SPACING), (palace, HOUSE_LAMP, palace::LAMP_SPACING)];
    roof_and_light(&mut map, &rooms, &halls);
    let buildings: Vec<Rect> = rooms.iter().copied().chain([bank, palace]).collect();
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
        palace,
        lot_buildings,
        parks,
        gates,
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

/// Every plot of good soil the wasteland of `seed` lays within the
/// navigation area round a district's site — where its farmers sow
/// (`sim::brain::goals::FarmGoal`): the cells [`Map::can_sow`] says yes to
/// before anything is sown — good soil, open all round, a heap or a lake
/// beside it ruling it out, and the site beside it too (its edge is the
/// perimeter wall). Worked out from the generator a plot at a time, before
/// the map has a wasteland of its own.
fn soil_round(seed: u64) -> Vec<Point> {
    let site = Bounds::from(Size::new(WIDTH, HEIGHT));
    let wasteland = Wasteland::new(seed);
    let open = |cell: Point| {
        let tile = wasteland.tile(cell, site);
        !site.contains(cell) && tile.is_passable() && wasteland.scrap_on(cell, site, tile).is_none()
    };
    site.grown(NAV_MARGIN)
        .points()
        .filter(|&cell| {
            is_plot(cell)
                && !site.contains(cell)
                && wasteland.tile(cell, site).is_fertile()
                && (-1..=1).all(|dy| (-1..=1).all(|dx| open(cell.offset(dx, dy))))
        })
        .collect()
}

/// The ground before anything is built on it: [`PAVEMENT`] over every block
/// and round it as far as the street beside it has pavement — a cell, or the
/// main street's [`MAIN_PAVEMENT`] — the [`PERIMETER_WALL`] along the map's
/// edge, and [`ROAD`] — what the map starts as — everywhere else. So each
/// street reads pavement, three cells of road, pavement — the street round
/// the edge wall, road, pavement; the main street two of pavement, five of
/// road, two of pavement — and where two meet the pavements wrap round the
/// corners and the road runs on through. `blocks` counts the bank's two
/// blocks as one, street between and all, since the bank stands across it.
fn pave(map: &mut Map, blocks: &[Rect]) {
    let (pavement, wall) = (tile(PAVEMENT), tile(PERIMETER_WALL));
    let reach = |y: i32| across_at(y).map_or(1, |street| street.pavement);
    let near_a_block = |cell: Point| {
        blocks.iter().any(|block| {
            cell.x >= block.x - 1
                && cell.x <= block.right() + 1
                && cell.y >= block.y - reach(block.y - 1)
                && cell.y <= block.top() + reach(block.top() + 1)
        })
    };
    for cell in map.size().points().collect::<Vec<_>>() {
        let edge = cell.x == 0 || cell.y == 0 || cell.x == WIDTH - 1 || cell.y == HEIGHT - 1;
        if edge {
            map.set_terrain(cell, wall);
        } else if near_a_block(cell) {
            map.set_terrain(cell, pavement);
        }
    }
}

/// The gates in the perimeter wall ([`gate_layout`]), built: two [`DOOR`]
/// leaves on [`ROAD`] each, and its [`GUARD_POST`].
fn gates(map: &mut Map, props: &mut Vec<Object>) -> Vec<Gate> {
    let gates = gate_layout();
    let road = tile(ROAD);
    for gate in &gates {
        for leaf in gate.leaves {
            map.set_terrain(leaf, road);
            props.push(object(DOOR, leaf));
        }
        props.push(object(GUARD_POST, gate.post));
    }
    gates
}

/// Where the gates in the perimeter wall go: [`GATES_PER_SIDE`] a side,
/// each where one of the streets running out to that side meets the wall —
/// the ones that split the side most evenly, and on the two sides the main
/// street runs out to, the main street in place of the nearest of them, so
/// it runs gate to gate — opening straight off its road. Two leaves in the
/// first two lanes of its road, and a guard post in the wall two cells short
/// of the nearer leaf, a cell of wall between (on the far side of the
/// street's pavement; of the main street's two cells of it, on the first): a
/// guard stands at it on the road inside, out of the gateway, and could only
/// get round to the outside of it through the gate. From the geometry alone,
/// drawing nothing from the seed — which is what lets the lot buildings be
/// sited by the way out ([`Siting::walk_out`]) before the wall is built.
fn gate_layout() -> Vec<Gate> {
    // The streets that split `blocks` blocks most evenly, by number.
    let streets = |blocks: i32| (1..=GATES_PER_SIDE).map(move |i| i * (blocks + 1) / (GATES_PER_SIDE + 1));
    let mut gates = Vec::new();
    for k in streets(BLOCKS_X) {
        let x = up(k).sides().0;
        for (y, outward) in [(0, Point::new(0, -1)), (HEIGHT - 1, Point::new(0, 1))] {
            let leaves = [Point::new(x, y), Point::new(x + 1, y)];
            gates.push(Gate { leaves, post: Point::new(x - 2, y), outward });
        }
    }
    let mut across_streets: Vec<i32> = streets(BLOCKS_Y).collect();
    if let Some(nearest) = across_streets.iter_mut().min_by_key(|k| (**k - MAIN).abs()) {
        *nearest = MAIN;
    }
    for k in across_streets {
        let y = across(k).sides().0;
        for (x, outward) in [(0, Point::new(-1, 0)), (WIDTH - 1, Point::new(1, 0))] {
            let leaves = [Point::new(x, y), Point::new(x, y + 1)];
            gates.push(Gate { leaves, post: Point::new(x, y - 2), outward });
        }
    }
    gates
}

/// A roof over every building, and the lamps under them: one in the middle
/// of each of the `rooms` (the houses, the shops, the lot buildings) and a
/// grid in each of the `halls`, too big for one — the bank's tube lamps, the
/// palace's ceiling lamps — of the lamp given, every so many cells each way.
/// Last, from the buildings as built, and without the seed — see the module
/// docs.
fn roof_and_light(map: &mut Map, rooms: &[Rect], halls: &[(Rect, &str, i32)]) {
    for &walls in rooms.iter().chain(halls.iter().map(|(walls, _, _)| walls)) {
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
    for &(walls, lamp, spacing) in halls {
        let inside = walls.interior();
        let offset = Point::new((inside.w - 1) % spacing / 2, (inside.h - 1) % spacing / 2);
        for y in (inside.y + offset.y..=inside.top()).step_by(spacing as usize) {
            for x in (inside.x + offset.x..=inside.right()).step_by(spacing as usize) {
                hang(map, lamp, Point::new(x, y));
            }
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

/// The sewer, the mains along the streets and every building's connection to
/// them — the power plant's too, which is where the power comes from
/// ([`feed_in`]). Last, from the buildings as built, and without the seed —
/// see the module docs.
fn connect(map: &mut Map, buildings: &[Rect]) {
    let inside_any = |cell: Point| buildings.iter().any(|walls| walls.contains(cell));

    // The mains: down the middle of every street, both networks side by side
    // in their own layers, broken wherever a building stands across a street
    // (the bank does) — every street is in a ring of them, so a break is
    // walked round.
    let mains: Vec<Point> = (0..=BLOCKS_Y)
        .flat_map(|k| {
            let y = across(k).middle();
            (up(0).middle()..=up(BLOCKS_X).middle()).map(move |x| Point::new(x, y))
        })
        .chain((0..=BLOCKS_X).flat_map(|k| {
            let x = up(k).middle();
            (across(0).middle()..=across(BLOCKS_Y).middle()).map(move |y| Point::new(x, y))
        }))
        .filter(|&cell| !inside_any(cell))
        .collect();
    for &cell in &mains {
        map.set_grid(GridLayer::Power, cell, power::LINE);
        map.set_grid(GridLayer::Water, cell, water::PIPE);
    }

    // The sewer, in a corner of the street round the edge, inside the
    // perimeter wall, joined to the main. The power comes from the power
    // plant, which is joined to it like any building, below.
    let sewer = Point::new(1, HEIGHT - 2);
    map.add_object(ObjectLayer::Props, object(SEWER, sewer));
    utilities::lay(map, GridLayer::Water, water::PIPE, sewer, Point::new(up(0).middle(), across(BLOCKS_Y).middle()));

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
        let sources: Vec<Point> = map
            .objects(ObjectLayer::Props)
            .iter()
            .filter(|object| walls.contains(object.cell()) && utilities::source_of(object.kind.as_str()) == Some(Utility::Power))
            .map(Object::cell)
            .collect();
        if !powered.is_empty() || !sources.is_empty() {
            let entry = service_entry(map, GridLayer::Power, power::LINE, walls, &others);
            feed_in(map, walls, &sources);
            if !powered.is_empty() {
                map.set_grid(GridLayer::Power, entry, power::BOX);
            }
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
/// A street is a pavement, three cells of road and a pavement — the main
/// street two, five and two — with the main under the middle of the road, so
/// its two **sides** ([`Street::sides`]) — the road's outer lanes, either
/// side of the main — are where lamps go: off the pavement, which is the
/// crowd's ([`can_stand`] keeps every post and cabinet off it).
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
    // Kept clear of, as doors are: a post in a gate's wall has one cell
    // inside to be stood at from, and a lamp there would send its guard
    // round through the gate to stand outside.
    let doors: Vec<Point> = map
        .objects(ObjectLayer::Props)
        .iter()
        .filter(|prop| [DOOR, HOUSE_DOOR, GUARD_POST].contains(&prop.kind.as_str()))
        .map(Object::cell)
        .collect();
    // Each stretch with the offset its lamps are spaced from: the far side of
    // a street is staggered half a spacing against the near one, so two
    // lamps never face each other across it.
    let mut stretches: Vec<(Vec<Point>, i32)> = Vec::new();
    for k in 0..=BLOCKS_Y {
        let (near, far) = across(k).sides();
        for (y, stagger) in [(near, 0), (far, STREET_LAMP_SPACING / 2)] {
            stretches.push(((0..WIDTH).map(|x| Point::new(x, y)).collect(), stagger));
        }
    }
    for k in 0..=BLOCKS_X {
        let (near, far) = up(k).sides();
        for (x, stagger) in [(near, 0), (far, STREET_LAMP_SPACING / 2)] {
            // A block's height at a time, from beside the far row of the
            // street below to one short of the near row of the street above.
            for by in 0..BLOCKS_Y {
                let (from, to) = (across(by).sides().1 + 1, across(by + 1).sides().0 - 1);
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
                    // it.
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

/// A power plant's own line: from under each of its `sources` (the
/// generators) into the wall beside it and round the walls — never across the
/// floor — to the line already in them, which is where the building's line
/// goes out to the main ([`service_entry`]) or an earlier generator's. So the
/// only line indoors is under the generators themselves, and the wiring laid
/// from the box to the lamps never meets it: wiring beside a line is not fed
/// by it, and a lamp on a line would be dead.
///
/// The walk round is the shortest along the walls, the way round decided by
/// the order the neighbours are tried in, so the same building is always
/// wired the same way.
fn feed_in(map: &mut Map, walls: Rect, sources: &[Point]) {
    let size = map.size();
    for &source in sources {
        // Breadth first from the generator over the ring of the walls, to the
        // first cell of it already on the line.
        let mut came_from: Vec<Option<Point>> = vec![None; size.area()];
        let mut queue = std::collections::VecDeque::from([source]);
        came_from[size.index_of(source).unwrap()] = Some(source);
        let mut reached = None;
        while let Some(cell) = queue.pop_front() {
            if cell != source && map.grid(GridLayer::Power, cell) == power::LINE {
                reached = Some(cell);
                break;
            }
            for next in cell.cardinal_neighbours() {
                if walls.on_edge(next)
                    && let Some(index) = size.index_of(next)
                    && came_from[index].is_none()
                {
                    came_from[index] = Some(cell);
                    queue.push_back(next);
                }
            }
        }
        let mut cell = reached.expect("district: a generator with no wall round to the plant's line");
        while cell != source {
            map.set_grid(GridLayer::Power, cell, power::LINE);
            cell = came_from[size.index_of(cell).unwrap()].unwrap();
        }
        map.set_grid(GridLayer::Power, source, power::LINE);
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
/// and flipped into place, so which long side the lobby is on is a coin toss
/// rather than a second layout. Two exits on opposite long sides share a
/// clear aisle. Symmetric end to end: restrooms at both ends and fridges all
/// along the lobby.
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

    // Two exits, each two wide, in the middle of opposite walls.
    let (wide, high) = (inside.w, inside.h);
    let floor = tile(BANK_FLOOR);
    let door = [wide / 2 - 1, wide / 2];
    for y in [-1, high] {
        for x in door {
            map.set_terrain(at(x, y), floor);
            props.push(object(DOOR, at(x, y)));
        }
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
    // between every desk and every row, the first rows clear for a lobby,
    // and a two-cell aisle kept clear between the exits.
    let desk = rng.random_range(1..=2);
    let first_row = rng.random_range(3..=4);
    for y in (first_row..high).step_by(3) {
        let mut x = first + 2;
        while x + desk - 1 <= last - 1 {
            for seat in 0..desk {
                if !door.contains(&(x + seat)) {
                    props.push(object(COMPUTER, at(x + seat, y)));
                }
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

/// A building of one room laid out from its door: its walls, and the room
/// inside measured `across` the door's wall and `deep` in from it.
struct Room {
    walls: Rect,
    side: Side,
    wide: i32,
    deep: i32,
}

impl Room {
    /// The cell `across` along the door's wall and `depth` in from it.
    fn at(&self, across: i32, depth: i32) -> Point {
        let inside = self.walls.interior();
        match self.side {
            Side::South => Point::new(inside.x + across, inside.y + depth),
            Side::North => Point::new(inside.x + across, inside.top() - depth),
            Side::West => Point::new(inside.x + depth, inside.y + across),
            Side::East => Point::new(inside.right() - depth, inside.y + across),
        }
    }
}

/// A room as big as a shop somewhere on `lot`, `wall` round `floor`, with a
/// plain door — nobody's property — in the wall on `side`, kept two cells
/// off the corners so the row inside it stays clear of what stands in them.
#[allow(clippy::too_many_arguments)]
fn public_room(rng: &mut SmallRng, map: &mut Map, props: &mut Vec<Object>, lot: Lot, side: Side, wall: &str, floor: &str) -> Room {
    let walls = place_in(rng, lot, LOT_W - 1, LOT_H - 1);
    build(map, walls, wall, floor);
    for cell in door_clear_of_corners(rng, walls, side, DOOR_WIDTH, 2) {
        map.set_terrain(cell, tile(floor));
        props.push(object(DOOR, cell));
    }
    let inside = walls.interior();
    let (wide, deep) = match side {
        Side::South | Side::North => (inside.w, inside.h),
        Side::West | Side::East => (inside.h, inside.w),
    };
    Room { walls, side, wide, deep }
}

/// A house with its residents' beds, a fridge, a toilet and their spawners,
/// and — a [`HOUSES_WITH_A_TV`] chance, drawn from `tvs` — a TV.
fn house(
    rng: &mut SmallRng,
    tvs: &mut SmallRng,
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
    let tv = if tvs.random_bool(HOUSES_WITH_A_TV) { place_tv(tvs, walls.interior(), &entry, &furnished) } else { None };
    if let Some(tv) = tv {
        props.push(object(TV, tv));
    }

    House {
        walls,
        beds: furnished.beds,
        residents,
        tv,
    }
}

/// Where a TV goes in a furnished house: a cell against an inside wall, off
/// the door and the residents' starting cells, where the house is still
/// [`livable`] with it there. Every cell of a house is in sight of every
/// other and its free cells beside the TV are in reach of it, so it can
/// always be watched from somewhere. `None` when no wall cell will do.
fn place_tv(tvs: &mut SmallRng, inside: Rect, entry: &[Point], furnished: &Furnishing) -> Option<Point> {
    let mut taken: Vec<Point> = furnished.beds.iter().copied().chain([furnished.fridge, furnished.toilet]).collect();
    let mut spots: Vec<Point> = inside
        .cells()
        .filter(|&cell| {
            inside.on_edge(cell) && !entry.contains(&cell) && !taken.contains(&cell) && !furnished.spawners.contains(&cell)
        })
        .collect();
    spots.shuffle(tvs);
    spots.into_iter().find(|&spot| {
        taken.push(spot);
        let fits = livable(inside, entry, &taken);
        taken.pop();
        fits
    })
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
    if !livable(inside, entry, cells) {
        return None;
    }
    let free = |cell: Point| inside.contains(cell) && !cells.contains(&cell);

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

/// Whether a house with furniture in `taken` can be lived in: every free cell
/// reachable from the door, and [`ROOM_AROUND`] free cells beside every piece.
fn livable(inside: Rect, entry: &[Point], taken: &[Point]) -> bool {
    let free = |cell: Point| inside.contains(cell) && !taken.contains(&cell);

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
        return false;
    }
    let room = |cell: &Point| cell.cardinal_neighbours().into_iter().filter(|&n| free(n)).count();
    taken.iter().all(|cell| room(cell) >= ROOM_AROUND)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) const SEEDS: std::ops::Range<u64> = 0..64;

    pub(super) fn props_in(map: &Map, rect: Rect, name: &str) -> usize {
        map.objects(ObjectLayer::Props)
            .iter()
            .filter(|prop| prop.kind.as_str() == name && rect.contains(prop.cell()))
            .count()
    }

    fn reachable(map: &Map) -> Vec<bool> {
        let size = map.size();
        let mut seen = vec![false; size.area()];
        let start = Point::new(1, 1);
        assert!(map.is_passable(start), "the corner of the street round the edge is road");
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
            for &(name, walls) in &district.lot_buildings {
                assert_eq!(in_room(walls, HOUSE_LAMP), 1, "seed {seed}: the {name} at {walls:?}");
            }
            assert!(in_room(district.bank, OFFICE_LAMP) >= 12, "seed {seed}: {}", in_room(district.bank, OFFICE_LAMP));
            assert!(in_room(district.palace, HOUSE_LAMP) >= 9, "seed {seed}: {}", in_room(district.palace, HOUSE_LAMP));
            for lamp in lamps {
                assert!(district.map.sight().is_passable(lamp.cell()), "seed {seed}: a lamp in a wall");
                assert!(crate::lighting::scene::emission(lamp.kind.as_str()).is_some());
            }
        }
    }

    #[test]
    fn every_tile_and_prop_the_generator_uses_exists() {
        for name in TILES.iter().chain(palace::TILES).chain(LOT_BUILDINGS.iter().flat_map(|spec| spec.tiles)) {
            assert!(TerrainId::from_name(name).is_some(), "{name}");
        }
        for name in BEDS
            .iter()
            .chain(&[FRIDGE, TOILET, COMPUTER, TV, GUARD_POST, SEWER, DOOR, HOUSE_DOOR, STREET_LAMP, utilities::CONTROLLER])
            .chain(palace::PROPS)
            .chain(LOT_BUILDINGS.iter().flat_map(|spec| spec.props))
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
        // across the street. The one line indoors is the one under each of
        // the power plant's generators, which feed it.
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let buildings = district.buildings();
            let generators: Vec<Point> = map
                .objects(ObjectLayer::Props)
                .iter()
                .filter(|prop| prop.kind.as_str() == power_plant::GENERATOR)
                .map(Object::cell)
                .collect();
            assert_eq!(generators.len(), power_plant::GENERATORS, "seed {seed}");
            // Outdoors, wiring is the street lights' and nobody else's: every
            // outdoor circuit has a controlled box in it and nothing indoors.
            let controlled = utilities::controlled_boxes(map);
            let indoors = |cell: Point| buildings.iter().any(|walls| walls.interior().contains(cell));
            for cell in map.size().points() {
                match map.grid(GridLayer::Power, cell) {
                    power::LINE => assert!(!indoors(cell) || generators.contains(&cell), "seed {seed}: a line indoors at {cell:?}"),
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
                assert_eq!(props_in(map, house.walls, TV), house.tv.iter().count(), "seed {seed} {house:?}");
            }
            for shop in &district.shops {
                let inside = map.objects(ObjectLayer::Props).iter().filter(|prop| shop.interior().contains(prop.cell()));
                assert_eq!(inside.count(), 0, "seed {seed}: a shop is empty for now");
            }
            assert!(props_in(map, district.bank, COMPUTER) >= 12, "seed {seed}");
            assert!(props_in(map, district.bank, TOILET) >= 2, "seed {seed}");

            // Every lot building, as many as it says, and each in the
            // places its own module's tests say.
            for spec in LOT_BUILDINGS {
                assert_eq!(district.all(spec.name).count(), spec.count, "seed {seed}: {}", spec.name);
            }
        }
    }

    /// The whole edge of the map is the perimeter wall but for the gates:
    /// two leaves of an ordinary door on road, [`GATES_PER_SIDE`] to a side,
    /// each with a way in behind it and the wasteland in front.
    #[test]
    fn the_district_is_walled_in_but_for_its_gates() {
        let (wall, road) = (tile(PERIMETER_WALL), tile(ROAD));
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            assert_eq!(district.gates.len(), GATES, "seed {seed}");
            let leaves: Vec<Point> = district.gates.iter().flat_map(|gate| gate.leaves).collect();
            let size = map.size();
            for cell in size.points().filter(|&c| c.x == 0 || c.y == 0 || c.x == WIDTH - 1 || c.y == HEIGHT - 1) {
                if leaves.contains(&cell) {
                    assert_eq!(map.terrain(cell), Some(road), "seed {seed}: a gate at {cell:?}");
                    assert!(map.is_passable(cell), "seed {seed}: a gate at {cell:?}");
                } else {
                    assert_eq!(map.terrain(cell), Some(wall), "seed {seed}: a gap in the wall at {cell:?}");
                    assert!(!map.is_passable(cell), "seed {seed}: {cell:?}");
                }
            }
            let doors = doors_of(map);
            let seen = reachable(map);
            for gate in &district.gates {
                for leaf in gate.leaves {
                    assert!(doors.contains(&(leaf, DOOR)), "seed {seed}: the gate at {leaf:?} is a door anybody may use");
                    assert!(!size.contains(leaf + gate.outward), "seed {seed}: {leaf:?} opens off the map");
                    let inside = leaf - gate.outward;
                    assert!(seen[size.index_of(inside).unwrap()], "seed {seed}: nothing leads to the gate at {leaf:?}");
                }
            }
            for outward in [Point::new(-1, 0), Point::new(1, 0), Point::new(0, -1), Point::new(0, 1)] {
                let on_side = district.gates.iter().filter(|gate| gate.outward == outward).count();
                assert_eq!(on_side, GATES_PER_SIDE as usize, "seed {seed}: gates facing {outward:?}");
            }
        }
    }

    /// Every gate has a guard post in the wall beside it, with a free cell
    /// inside to stand at it from, and that cell is not in the gateway.
    #[test]
    fn every_gate_has_a_guard_post_in_the_wall_with_room_inside_to_stand_at_it() {
        let wall = tile(PERIMETER_WALL);
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let posts: Vec<Point> = map
                .objects(ObjectLayer::Props)
                .iter()
                .filter(|prop| prop.kind.as_str() == GUARD_POST)
                .map(Object::cell)
                .collect();
            assert_eq!(posts.len(), GATES, "seed {seed}: a post to a gate and none elsewhere");
            for gate in &district.gates {
                assert!(posts.contains(&gate.post), "seed {seed}: {gate:?}");
                assert_eq!(map.terrain(gate.post), Some(wall), "seed {seed}: the post is set into the wall");
                let nearer = gate.leaves.iter().map(|&leaf| leaf.manhattan_distance(gate.post)).min().unwrap();
                assert_eq!(nearer, 2, "seed {seed}: two along from its gate, wall between");
                let stand = gate.post - gate.outward;
                assert!(map.is_passable(stand), "seed {seed}: nowhere to stand at the post at {:?}", gate.post);
                assert!(gate.leaves.iter().all(|&leaf| stand.manhattan_distance(leaf) > 1), "seed {seed}: in the gateway");
                let beside: Vec<Point> = gate.post.cardinal_neighbours().into_iter().filter(|&cell| map.size().contains(cell) && map.is_passable(cell)).collect();
                assert_eq!(beside, [stand], "seed {seed}: the only side of the post on the map is inside");
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

    pub(super) fn doors_of(map: &Map) -> Vec<(Point, &str)> {
        map.objects(ObjectLayer::Props)
            .iter()
            .filter(|prop| [DOOR, HOUSE_DOOR].contains(&prop.kind.as_str()))
            .map(|prop| (prop.cell(), prop.kind.as_str()))
            .collect()
    }

    #[test]
    fn the_office_has_two_opposite_exits_with_a_clear_aisle_between_them() {
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let walls = district.bank;
            let exits: Vec<Point> = doors_of(map)
                .into_iter()
                .filter(|&(cell, name)| name == DOOR && walls.on_edge(cell))
                .map(|(cell, _)| cell)
                .collect();
            assert_eq!(exits.len(), 2 * DOOR_WIDTH as usize, "seed {seed}: two double-width exits");
            let south: Vec<Point> = exits.iter().copied().filter(|cell| cell.y == walls.y).collect();
            assert_eq!(south.len(), DOOR_WIDTH as usize, "seed {seed}: a south exit");
            assert_eq!((south[0].x - south[1].x).abs(), 1, "seed {seed}: adjacent leaves");
            for leaf in south {
                assert!(exits.contains(&Point::new(leaf.x, walls.top())), "seed {seed}: an opposite north exit");
                for y in walls.y - 1..=walls.top() + 1 {
                    let cell = Point::new(leaf.x, y);
                    assert!(map.is_passable(cell), "seed {seed}: the exit aisle is blocked at {cell:?}");
                }
            }
        }
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
                .chain(district.lot_buildings.iter().map(|&(_, walls)| (walls, DOOR)))
                .chain([(district.bank, DOOR), (district.palace, DOOR)])
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
            // Two exits, and one doorway into each of the four restrooms.
            let bank = doors.iter().filter(|&&(cell, _)| district.bank.contains(cell)).count();
            assert_eq!(bank, 6 * DOOR_WIDTH as usize, "seed {seed}");
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

    #[test]
    fn about_half_the_houses_have_a_tv_against_a_wall_with_room_to_watch_it_and_nothing_else_does() {
        let (mut with, mut houses) = (0, 0);
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            let tvs = map.objects(ObjectLayer::Props).iter().filter(|prop| prop.kind.as_str() == TV).count();
            assert_eq!(tvs, district.houses.iter().filter(|house| house.tv.is_some()).count(), "seed {seed}: a TV outside a house");
            for house in &district.houses {
                houses += 1;
                let Some(tv) = house.tv else { continue };
                with += 1;
                let inside = house.walls.interior();
                assert!(inside.on_edge(tv), "seed {seed}: the TV at {tv:?} is not against a wall");
                let seats = tv.cardinal_neighbours().into_iter().filter(|&cell| inside.contains(cell) && map.is_passable(cell)).count();
                assert!(seats >= ROOM_AROUND, "seed {seed}: the TV at {tv:?} is boxed in");
            }
        }
        let share = with as f64 / houses as f64;
        assert!((0.4..=0.6).contains(&share), "{with} of {houses} houses have a TV");
    }

    #[test]
    fn the_tvs_draw_nothing_from_the_district_so_a_seed_builds_the_same_houses() {
        // Seed 7's resident count, read before there were any TVs — and
        // again once the farm, then the power plant, then the palace took a
        // lot of its own, which moved the houses, and once the district grew
        // to 9x5 blocks with a main street and the palace in the middle.
        // The food factory takes one more lot before houses are laid out.
        assert_eq!(generate(7).residents(), 177);
    }

    // --- the ground ---

    /// The tiles across `street`, from one edge to the other, starting at
    /// `start` and stepping `across`.
    fn cross_section(map: &Map, street: Street, start: Point, across: Point) -> Vec<TerrainId> {
        (0..street.width).map(|i| map.terrain(start.offset(across.x * i, across.y * i)).unwrap()).collect()
    }

    #[test]
    fn every_street_is_a_pavement_each_side_of_its_road() {
        let (pavement, road, wall) = (tile(PAVEMENT), tile(ROAD), tile(PERIMETER_WALL));
        // Pavement, road, pavement — three cells of road and one of pavement
        // but on the main street — and the street round the edge has the
        // perimeter wall for its outer pavement.
        let reads = |street: Street, k: i32, last: i32| {
            let mut cells = vec![pavement; street.pavement as usize];
            cells.extend(vec![road; street.road() as usize]);
            cells.extend(vec![pavement; street.pavement as usize]);
            if k == 0 {
                cells[0] = wall;
            }
            if k == last {
                cells[street.width as usize - 1] = wall;
            }
            cells
        };
        assert_eq!(reads(across(1), 1, BLOCKS_Y), [pavement, road, road, road, pavement]);
        for seed in SEEDS {
            let district = generate(seed);
            let map = &district.map;
            // Across each street halfway along every block beside it, away
            // from the crossroads — except where the bank stands across one.
            for k in 0..=BLOCKS_Y {
                let street = across(k);
                for bx in 0..BLOCKS_X {
                    let x = block_origin(bx, 0).x + BLOCK_W / 2;
                    let cells = cross_section(map, street, Point::new(x, street.start), Point::new(0, 1));
                    assert_eq!(cells, reads(street, k, BLOCKS_Y), "seed {seed}: across at x {x}");
                }
            }
            for k in 0..=BLOCKS_X {
                let street = up(k);
                for by in 0..BLOCKS_Y {
                    let y = block_origin(0, by).y + BLOCK_H / 2;
                    if district.bank.contains(Point::new(street.middle(), y)) {
                        continue;
                    }
                    let cells = cross_section(map, street, Point::new(street.start, y), Point::new(1, 0));
                    assert_eq!(cells, reads(street, k, BLOCKS_X), "seed {seed}: across at y {y}");
                }
            }
        }
    }

    /// The main street is wider than any other, road and pavement both, and
    /// runs across the district from a gate in the west wall to one in the
    /// east, along the front of the palace.
    #[test]
    fn the_main_street_is_the_widest_and_runs_gate_to_gate_along_the_palace() {
        let main = across(MAIN);
        for street in (0..=BLOCKS_Y).filter(|&k| k != MAIN).map(across).chain((0..=BLOCKS_X).map(up)) {
            assert!(main.road() > street.road() && main.pavement > street.pavement, "{main:?} against {street:?}");
        }
        for seed in 0..8 {
            let district = generate(seed);
            for outward in [Point::new(-1, 0), Point::new(1, 0)] {
                let at_its_end = |gate: &Gate| gate.outward == outward && gate.leaves.iter().all(|leaf| main.contains(leaf.y));
                assert!(district.gates.iter().any(at_its_end), "seed {seed}: the main street has no gate facing {outward:?}");
            }
            assert_eq!(district.palace.y, main.end(), "seed {seed}: the palace is not along the main street");
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
    fn nothing_stands_on_the_pavement_but_the_sewer() {
        // A lamp post on a pavement one cell wide sends everybody walking it
        // out into the road and back.
        let pavement = tile(PAVEMENT);
        for seed in SEEDS {
            let map = generate(seed).map;
            for prop in map.objects(ObjectLayer::Props) {
                if map.terrain(prop.cell()) == Some(pavement) {
                    assert_eq!(prop.kind.as_str(), SEWER, "seed {seed}: a {} on the pavement at {:?}", prop.kind.as_str(), prop.cell());
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
                        // Straight over is three cells of road, five over the
                        // main street.
                        let crossing = across_at(cell.y).map_or(STREET - 2, |street| street.road());
                        assert!(run <= crossing, "seed {seed}: walked down the road at {cell:?}, {from:?} to {to:?}");
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
