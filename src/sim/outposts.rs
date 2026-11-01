//! [`Outposts`]: the buildings the king decrees out in the wasteland and the
//! builders put up — the **building plan** every builder reads, which tiles
//! of it are whose to work on, and how far each has got.
//!
//! **The king decides** (`Task::decree`, from the throne): when there is steel
//! enough on the warehouse's racks for a whole outpost ([`OUTPOST_STEEL`]) and
//! none is going up, it finds a site ([`choose_site`]: [`FROM_WALLS`] out from
//! the district's wall, as near as that allows to the biggest heap of scrap
//! anybody can get to) and puts a flag up in its middle
//! ([`Outposts::found`]). The plan is [`LAYOUT`], a tile at a time: twenty
//! walls, a doorway of two doors facing the district, a fridge, a local
//! warehouse ([`Part::Store`]) and two beds.
//!
//! **A builder takes a tile on** ([`Outposts::claim`]), one tile one builder,
//! never two; fetches the steel it takes from the warehouse (a wall, the
//! fridge and the store a piece each, the doors and beds none); and, with
//! nobody standing in the cell, **lays a building tile** there with all of it
//! at once ([`Outposts::lay`]). The building tile is in the way from then on
//! (a doorway's never is), and is worked on an hour at the most at a time
//! until it has had [`TILE_WORK`] — two hours — when it is what it was the
//! building of ([`Outposts::construct`]): a wall is wall terrain, the rest
//! props — a [`Fixture`] the world puts into its indexes, so the fridge is
//! somewhere to eat, the beds somewhere to sleep, the doors doors.
//!
//! The map keeps *that* a cell is built on (`Map::build`), because
//! passability is the map's; this keeps the rest, and builds the map again
//! when a game is loaded, as `sim::fields` replants it. Written only by the
//! world step (`effect::apply`: `Effect::Decree`, `Claim`, `Lay`,
//! `Construct`), read by every brain through `Think::outposts`, and saved.

use crate::map::{Bounds, Map, Point, Size, Structure};
use crate::save::{invalid, pb, Invalid};

use super::clock::{HOUR, MINUTE};
use super::feature::FeatureKind;
use super::uid::Uid;

/// How many builders a district keeps: their places in the builders' yard
/// beside the warehouse's steel rack (`FeatureKind::BuildersYard`).
pub const BUILDERS: usize = 4;

/// How wide an outpost is, in cells.
pub const WIDTH: i32 = 6;

/// How tall an outpost is, in cells.
pub const HEIGHT: i32 = 7;

/// How far from the district's wall every cell of an outpost is, in cells,
/// as a king walks: no nearer than the first, no further than the second.
pub const FROM_WALLS: (i32, i32) = (50, 100);

/// How long a tile is worked on, all told, before it is built: two hours of
/// world time, in **world** seconds.
pub const TILE_WORK: f32 = 2.0 * HOUR;

/// The longest a builder works on a tile at a stretch, in **world** seconds:
/// an hour, then a rest ([`REST`]).
pub const STRETCH: f32 = HOUR;

/// How long a builder rests after a stretch, in **world** seconds — long
/// enough to eat or drink what it carries if it wants to.
pub const REST: f32 = 15.0 * MINUTE;

/// The plan, a character a cell, **top row first**, its doorway in the
/// bottom row: `W` a wall, `D` a door, `R` the fridge, `S` the local
/// warehouse, `B` a bed, `.` nothing to build. Turned upside down for an
/// outpost the district is below ([`Outpost::flipped`]), so the doorway
/// always faces the district. Every piece of furniture keeps two free cells
/// beside it, and the inside stays joined up whatever is built first.
pub const LAYOUT: [&str; HEIGHT as usize] = [
    "WWWWWW",
    "WB..BW",
    "W....W",
    "W....W",
    "W....W",
    "WR..SW",
    "WWDDWW",
];

/// Where the flag stands, from the outpost's bottom-left cell: the middle,
/// on a cell with nothing to build.
pub const CENTRE: (i32, i32) = (3, 3);

/// How many tiles a plan has: everything on [`LAYOUT`] but its `.`s.
pub const TILES: usize = count(false);

/// How much steel a whole outpost takes: a piece for every wall, the fridge
/// and the store. What the warehouse's racks have to hold before the king
/// decrees one.
pub const OUTPOST_STEEL: u32 = count(true) as u32;

/// Every tile on [`LAYOUT`] — or, `steel`, the steel they take — at compile
/// time.
const fn count(steel: bool) -> usize {
    let mut total = 0;
    let mut row = 0;
    while row < LAYOUT.len() {
        let bytes = LAYOUT[row].as_bytes();
        let mut col = 0;
        while col < bytes.len() {
            if let Some(part) = Part::of(bytes[col]) {
                total += if steel { part.steel() as usize } else { 1 };
            }
            col += 1;
        }
        row += 1;
    }
    total
}

const _: () = {
    let mut row = 0;
    while row < LAYOUT.len() {
        assert!(LAYOUT[row].len() == WIDTH as usize, "every row of the plan is an outpost wide");
        row += 1;
    }
    assert!(LAYOUT[(HEIGHT - 1 - CENTRE.1) as usize].as_bytes()[CENTRE.0 as usize] == b'.', "the flag stands on nothing to build");
};

/// What a tile of the plan is the building of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Wall,
    Door,
    Fridge,
    /// The outpost's local warehouse: a store of scrap metal
    /// (`FeatureKind::OutpostStore`).
    Store,
    Bed,
}

impl Part {
    /// What a character of [`LAYOUT`] is the building of, if anything.
    const fn of(byte: u8) -> Option<Part> {
        match byte {
            b'W' => Some(Part::Wall),
            b'D' => Some(Part::Door),
            b'R' => Some(Part::Fridge),
            b'S' => Some(Part::Store),
            b'B' => Some(Part::Bed),
            _ => None,
        }
    }

    /// The steel it takes, all laid at once with its building tile.
    pub const fn steel(self) -> u32 {
        match self {
            Part::Wall | Part::Fridge | Part::Store => 1,
            Part::Door | Part::Bed => 0,
        }
    }

    /// What it is called in the log.
    pub const fn name(self) -> &'static str {
        match self {
            Part::Wall => "wall",
            Part::Door => "door",
            Part::Fridge => "fridge",
            Part::Store => "local warehouse",
            Part::Bed => "bed",
        }
    }

    /// The prop it is once built — what the world's indexes know it by — or
    /// `None` for a wall, which is terrain.
    pub const fn prop(self) -> Option<&'static str> {
        match self {
            Part::Wall => None,
            Part::Door => Some("door"),
            Part::Fridge => Some("fridge"),
            Part::Store => Some("outpost store"),
            Part::Bed => Some("bed 1"),
        }
    }

    /// What stands in its cell on the map once it is built: a wall is wall
    /// terrain, furniture in the way, a door in nobody's.
    pub const fn structure(self) -> Option<Structure> {
        match self {
            Part::Wall => Some(Structure::Wall),
            Part::Fridge | Part::Store | Part::Bed => Some(Structure::Obstacle),
            Part::Door => None,
        }
    }

    /// Whether its building tile is in the way: every one but a doorway's,
    /// which the inside is got to through while everything else goes up.
    pub const fn blocks_while_laid(self) -> bool {
        !matches!(self, Part::Door)
    }

    /// The order tiles are taken on in: the walls, then the fridge and the
    /// store, then the beds, the doors last.
    pub const fn phase(self) -> u8 {
        match self {
            Part::Wall => 0,
            Part::Fridge | Part::Store => 1,
            Part::Bed => 2,
            Part::Door => 3,
        }
    }
}

/// How far a tile has got.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Stage {
    /// On the plan, nothing there yet.
    Planned,
    /// Its building tile is down, its steel in it, and it has been worked on
    /// for `progress` world seconds of the [`TILE_WORK`] it takes.
    Laid { progress: f32 },
    /// Built.
    Built,
}

/// One cell of the plan.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Tile {
    pub cell: Point,
    pub part: Part,
    pub stage: Stage,
    /// The builder working on it, if anybody is: one at a time.
    pub claim: Option<Uid>,
}

impl Tile {
    /// The steel still to be brought for it: its part's while it is only on
    /// the plan, none once its building tile is down.
    pub const fn steel_wanted(&self) -> u32 {
        match self.stage {
            Stage::Planned => self.part.steel(),
            _ => 0,
        }
    }

    /// How much of the work it takes has been done, `0..=1`.
    pub fn done(&self) -> f32 {
        match self.stage {
            Stage::Planned => 0.0,
            Stage::Laid { progress } => (progress / TILE_WORK).clamp(0.0, 1.0),
            Stage::Built => 1.0,
        }
    }
}

/// A built piece of furniture the world's indexes take in: its prop, and
/// where it stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fixture {
    pub prop: &'static str,
    pub cell: Point,
}

/// What finishing a tile came to ([`Outposts::construct`]).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Completed {
    pub part: Part,
    /// The furniture to put into the world's indexes, if it was furniture.
    pub fixture: Option<Fixture>,
    /// How many of the outpost's tiles are built now.
    pub built: usize,
    /// Whether that was the last.
    pub outpost_done: bool,
}

/// One outpost: its flag, which way it faces, the day it was decreed, and its
/// plan, tile by tile in [`LAYOUT`]'s order.
#[derive(Clone, PartialEq, Debug)]
pub struct Outpost {
    center: Point,
    flipped: bool,
    decreed: u32,
    tiles: [Tile; TILES],
}

impl Outpost {
    /// An outpost with its flag at `center`, nothing built — its doorway in
    /// the top row if `flipped`, the bottom row otherwise.
    pub fn new(center: Point, flipped: bool, decreed: u32) -> Outpost {
        let origin = center.offset(-CENTRE.0, -CENTRE.1);
        let mut tiles = [Tile { cell: origin, part: Part::Wall, stage: Stage::Planned, claim: None }; TILES];
        let mut next = 0;
        for (row, line) in LAYOUT.iter().enumerate() {
            for (col, byte) in line.bytes().enumerate() {
                let Some(part) = Part::of(byte) else { continue };
                let y = if flipped { row as i32 } else { HEIGHT - 1 - row as i32 };
                tiles[next] = Tile { cell: origin.offset(col as i32, y), part, stage: Stage::Planned, claim: None };
                next += 1;
            }
        }
        Outpost { center, flipped, decreed, tiles }
    }

    /// Where its flag stands: its middle.
    pub fn center(&self) -> Point {
        self.center
    }

    /// Whether it is upside down, its doorway in the top row: the district is
    /// above it.
    pub fn flipped(&self) -> bool {
        self.flipped
    }

    /// The day the king decreed it.
    pub fn decreed(&self) -> u32 {
        self.decreed
    }

    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }

    /// The cells it covers.
    pub fn footprint(&self) -> Bounds {
        footprint(self.center)
    }

    /// How many of its tiles are built.
    pub fn built(&self) -> usize {
        self.tiles.iter().filter(|tile| tile.stage == Stage::Built).count()
    }

    pub fn is_complete(&self) -> bool {
        self.built() == TILES
    }

    /// The tile in `cell`, if the plan has one there.
    pub fn tile(&self, cell: Point) -> Option<&Tile> {
        self.tiles.iter().find(|tile| tile.cell == cell)
    }

    fn tile_mut(&mut self, cell: Point) -> Option<&mut Tile> {
        self.tiles.iter_mut().find(|tile| tile.cell == cell)
    }
}

/// The cells an outpost with its flag at `center` covers.
pub fn footprint(center: Point) -> Bounds {
    Bounds::new(center.offset(-CENTRE.0, -CENTRE.1), Size::new(WIDTH, HEIGHT))
}

/// Every outpost decreed, and the plan of the one going up.
///
/// **One goes up at a time**: the last decreed, until every tile of it is
/// built. See the [module docs](self).
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Outposts {
    /// In the order decreed.
    outposts: Vec<Outpost>,
    /// The last day the king looked for a site and found none: not looked for
    /// again that day.
    fruitless: u32,
}

impl Outposts {
    pub fn new() -> Outposts {
        Outposts::default()
    }

    /// Every outpost, in the order decreed.
    pub fn iter(&self) -> impl Iterator<Item = &Outpost> {
        self.outposts.iter()
    }

    pub fn len(&self) -> usize {
        self.outposts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.outposts.is_empty()
    }

    /// The outpost going up, if one is.
    pub fn under_way(&self) -> Option<&Outpost> {
        self.outposts.last().filter(|outpost| !outpost.is_complete())
    }

    fn under_way_mut(&mut self) -> Option<&mut Outpost> {
        self.outposts.last_mut().filter(|outpost| !outpost.is_complete())
    }

    /// How many outposts are built, every tile.
    pub fn complete(&self) -> usize {
        self.outposts.iter().filter(|outpost| outpost.is_complete()).count()
    }

    /// Whether the king has a site to look for on `day`: none is going up,
    /// and it has not looked in vain already today.
    pub fn wants_a_decree(&self, day: u32) -> bool {
        self.under_way().is_none() && self.fruitless != day
    }

    /// The tile of the outpost going up in `cell`, if any.
    pub fn tile(&self, cell: Point) -> Option<&Tile> {
        self.under_way()?.tile(cell)
    }

    /// The tile `uid` has taken on, if any.
    pub fn claim_of(&self, uid: Uid) -> Option<&Tile> {
        self.under_way()?.tiles.iter().find(|tile| tile.claim == Some(uid))
    }

    /// Whether there is building for `uid` to do: a tile of its own, or one
    /// nobody has taken on.
    pub fn has_work_for(&self, uid: Uid) -> bool {
        self.under_way()
            .is_some_and(|outpost| outpost.tiles.iter().any(|tile| tile.stage != Stage::Built && tile.claim.is_none_or(|by| by == uid)))
    }

    /// The steel the outpost going up still wants brought: every tile's on
    /// the plan, and of those, the ones nobody has taken on.
    pub fn steel_wanted(&self) -> (u32, u32) {
        self.under_way().map_or((0, 0), |outpost| {
            outpost.tiles.iter().fold((0, 0), |(all, free), tile| {
                let wanted = tile.steel_wanted();
                (all + wanted, free + if tile.claim.is_none() { wanted } else { 0 })
            })
        })
    }

    /// The tile a builder at `here` carrying `steel` should take on next, of
    /// those nobody has: out at the outpost (`away`), one its pocket can
    /// supply if there is one — no walk back to the warehouse — and then, as
    /// anywhere, by [`Part::phase`], the nearest, the lower cell on a tie.
    pub fn pick(&self, here: Point, steel: u32, away: bool) -> Option<Point> {
        self.under_way()?
            .tiles
            .iter()
            .filter(|tile| tile.claim.is_none() && tile.stage != Stage::Built)
            .min_by_key(|tile| (away && tile.steel_wanted() > steel, tile.part.phase(), tile.cell.manhattan_distance(here), tile.cell))
            .map(|tile| tile.cell)
    }

    /// The cells a builder can work on the tile in `cell` from: the four
    /// beside it that no tile of the plan is in — never a cell something is
    /// still to be built in, or a doorway. Whether they can be walked is the
    /// map's to say.
    pub fn stand_cells(&self, cell: Point) -> impl Iterator<Item = Point> + '_ {
        cell.cardinal_neighbours().into_iter().filter(move |&side| self.tile(side).is_none())
    }

    /// Every piece of furniture built, in every outpost, for the world's
    /// indexes to take in.
    pub fn fixtures(&self) -> impl Iterator<Item = Fixture> + '_ {
        self.outposts.iter().flat_map(|outpost| outpost.tiles.iter()).filter_map(|tile| {
            let prop = tile.part.prop()?;
            (tile.stage == Stage::Built).then_some(Fixture { prop, cell: tile.cell })
        })
    }

    // --- the world step's ---

    /// Put an outpost's flag up at `center`, decreed on `day`: its plan, with
    /// nothing built and nobody on it, its doorway facing the site. Refused —
    /// `false` — while one is going up, or where the footprint is not clear
    /// (`is_clear`, which the king asked already; the world asks again).
    pub fn found(&mut self, map: &Map, center: Point, day: u32) -> bool {
        if self.under_way().is_some() || !is_clear(map, center, self) {
            return false;
        }
        let flipped = center.y < map.site().origin.y;
        self.outposts.push(Outpost::new(center, flipped, day));
        true
    }

    /// The king looked on `day` and found nowhere: not again today.
    pub fn found_nowhere(&mut self, day: u32) {
        self.fruitless = day;
    }

    /// Give the tile in `cell` to `uid` to work on, while nobody has it and
    /// it is not built — and `uid` has no other. `false`, nothing changed,
    /// otherwise.
    pub fn claim(&mut self, cell: Point, uid: Uid) -> bool {
        if self.claim_of(uid).is_some() {
            return false;
        }
        let Some(tile) = self.under_way_mut().and_then(|outpost| outpost.tile_mut(cell)) else {
            return false;
        };
        if tile.claim.is_some() || tile.stage == Stage::Built {
            return false;
        }
        tile.claim = Some(uid);
        true
    }

    /// Take back whatever tile `uid` had — it died, was let go, went to
    /// hospital — for another builder to take on as it stands. Whether it had
    /// one.
    pub fn release(&mut self, uid: Uid) -> bool {
        let Some(tile) = self.under_way_mut().and_then(|outpost| outpost.tiles.iter_mut().find(|tile| tile.claim == Some(uid))) else {
            return false;
        };
        tile.claim = None;
        true
    }

    /// Lay the building tile in `cell` for `uid`, whose tile it is: in the
    /// way from now on (a doorway's never is). What it is the building of,
    /// or why not. Whether anybody stands there, or the steel was brought,
    /// is the caller's to have asked.
    pub fn lay(&mut self, map: &mut Map, cell: Point, uid: Uid) -> Result<Part, &'static str> {
        let Some(tile) = self.under_way_mut().and_then(|outpost| outpost.tile_mut(cell)) else {
            return Err("nothing is to be built there");
        };
        if tile.claim != Some(uid) {
            return Err("it is somebody else's to build");
        }
        if tile.stage != Stage::Planned {
            return Err("a building tile is down there already");
        }
        if !map.can_build(cell) {
            return Err("the ground there is not clear");
        }
        if tile.part.blocks_while_laid() && !map.build(cell, Structure::Obstacle) {
            return Err("the ground there is not clear");
        }
        tile.stage = Stage::Laid { progress: 0.0 };
        Ok(tile.part)
    }

    /// `seconds` of world time worked by `uid` on its building tile in
    /// `cell`. Once it has had [`TILE_WORK`] it is built — a wall wall
    /// terrain, furniture in the way — and given up; what that came to, or
    /// `None` while it is still going up (or was not `uid`'s to work on).
    pub fn construct(&mut self, map: &mut Map, cell: Point, uid: Uid, seconds: f32) -> Option<Completed> {
        let outpost = self.under_way_mut()?;
        let tile = outpost.tile_mut(cell)?;
        let Stage::Laid { progress } = tile.stage else {
            return None;
        };
        if tile.claim != Some(uid) || !(seconds.is_finite() && seconds > 0.0) {
            return None;
        }
        let progress = progress + seconds;
        if progress < TILE_WORK {
            tile.stage = Stage::Laid { progress };
            return None;
        }
        tile.stage = Stage::Built;
        tile.claim = None;
        let part = tile.part;
        if let Some(structure) = part.structure() {
            let built = map.build(cell, structure);
            debug_assert!(built, "a laid tile is built over by what it was the building of");
        }
        let fixture = part.prop().map(|prop| Fixture { prop, cell });
        let built = outpost.built();
        Some(Completed { part, fixture, built, outpost_done: built == TILES })
    }

    // --- saving ---

    pub(crate) fn save(&self) -> pb::Outposts {
        let Outposts { outposts, fruitless } = self;
        pb::Outposts {
            outposts: outposts
                .iter()
                .map(|outpost| {
                    let Outpost { center, flipped, decreed, tiles } = outpost;
                    pb::Outpost {
                        center: Some((*center).into()),
                        flipped: *flipped,
                        decreed: *decreed,
                        tiles: tiles
                            .iter()
                            .map(|&Tile { cell: _, part: _, stage, claim }| {
                                let (stage, progress) = match stage {
                                    Stage::Planned => (pb::TileStage::Planned, 0.0),
                                    Stage::Laid { progress } => (pb::TileStage::Laid, progress),
                                    Stage::Built => (pb::TileStage::Built, 0.0),
                                };
                                pb::OutpostTile { stage: stage as i32, progress, claim: claim.map(Uid::raw) }
                            })
                            .collect(),
                    }
                })
                .collect(),
            fruitless: *fruitless,
        }
    }

    /// The outposts as saved — none in a save from before there were any —
    /// **built on `map` again**: a save keeps the ground as it was found, and
    /// what stands on it is this. Refuses a plan with the wrong number of
    /// tiles, work that is no amount of time, a builder on two tiles or on a
    /// tile already built, more than the last outpost unfinished, and
    /// anything the map will not have built where it is.
    pub(crate) fn restore(saved: Option<&pb::Outposts>, map: &mut Map) -> Result<Outposts, Invalid> {
        let Some(saved) = saved else {
            return Ok(Outposts::new());
        };
        let mut outposts = Vec::with_capacity(saved.outposts.len());
        let mut claimed: Vec<Uid> = Vec::new();
        for (index, kept) in saved.outposts.iter().enumerate() {
            let center = pb::cell(kept.center, "outpost")?;
            let mut outpost = Outpost::new(center, kept.flipped, kept.decreed);
            if kept.tiles.len() != TILES {
                return invalid(format!("an outpost at {}, {} of {} tiles, not {TILES}", center.x, center.y, kept.tiles.len()));
            }
            for (tile, kept) in outpost.tiles.iter_mut().zip(&kept.tiles) {
                let at = format!("the outpost's {} at {}, {}", tile.part.name(), tile.cell.x, tile.cell.y);
                tile.stage = match pb::known::<pb::TileStage>(kept.stage, "outpost tile")? {
                    pb::TileStage::Planned => Stage::Planned,
                    pb::TileStage::Laid if kept.progress.is_finite() && (0.0..TILE_WORK).contains(&kept.progress) => {
                        Stage::Laid { progress: kept.progress }
                    }
                    pb::TileStage::Laid => return invalid(format!("{at} worked on for {} seconds", kept.progress)),
                    pb::TileStage::Built => Stage::Built,
                };
                if let Some(raw) = kept.claim {
                    let Some(uid) = Uid::from_raw(raw) else {
                        return invalid(format!("{at} taken on by nobody"));
                    };
                    if tile.stage == Stage::Built || claimed.contains(&uid) {
                        return invalid(format!("{at} taken on by {uid}, who cannot have it"));
                    }
                    claimed.push(uid);
                    tile.claim = Some(uid);
                }
                let structure = match tile.stage {
                    Stage::Planned => None,
                    Stage::Laid { .. } => tile.part.blocks_while_laid().then_some(Structure::Obstacle),
                    Stage::Built => tile.part.structure(),
                };
                if let Some(structure) = structure
                    && !map.build(tile.cell, structure)
                {
                    return invalid(format!("{at} cannot stand there"));
                }
            }
            if !outpost.is_complete() && index + 1 != saved.outposts.len() {
                return invalid(format!("the outpost at {}, {} unfinished, and a later one decreed", center.x, center.y));
            }
            outposts.push(outpost);
        }
        if outposts.last().is_none_or(Outpost::is_complete) && !claimed.is_empty() {
            return invalid("a builder on a tile of no outpost going up");
        }
        Ok(Outposts { outposts, fruitless: saved.fruitless })
    }
}

// --- the king's choice ---

/// Whether the king, on duty, decrees an outpost now (`WorkGoal`, at the
/// throne): none is going up, it has not looked in vain today, there are
/// builders to build one (a builders' yard), and the warehouse's racks hold
/// the steel a whole one takes ([`OUTPOST_STEEL`]).
pub fn decree_due(think: &super::Think<'_>) -> bool {
    think.outposts.wants_a_decree(think.clock.day())
        && think.features.count(FeatureKind::BuildersYard) > 0
        && think.bins.total_in(FeatureKind::SteelRack) >= OUTPOST_STEEL
}

/// How far past [`FROM_WALLS`]' far end the king looks for scrap: a base just
/// beyond the band still draws an outpost to the band's edge nearest it.
const BASE_REACH: i32 = 32;

/// How big a block of the wasteland the scrap in it is counted by, in cells.
const BLOCK: i32 = 16;

/// How far from a base's middle a site is looked for.
const SEARCH: i32 = FROM_WALLS.1 + BASE_REACH;

/// Where the king puts the next outpost: its flag, the middle of a clear
/// footprint ([`is_clear`]) every cell of which is [`FROM_WALLS`] from the
/// district's wall, **as near as that allows to the biggest base of scrap
/// anybody can get to** — the block of the wasteland, counted with the eight
/// round it, with the most pieces left in heaps that are out in the open
/// (`Navigation::is_outside`), not in a pocket a lake has walled in. `None`
/// with no wasteland, no scrap within reach, or nowhere clear.
///
/// A scan of the heaps round the district and of the rings round the base,
/// with one count of blocks allocated: what a king does once an outpost,
/// never what anybody does every tick.
pub fn choose_site(map: &Map, outposts: &Outposts) -> Option<Point> {
    if !map.is_endless() {
        return None;
    }
    let site = map.site();
    let area = site.grown(FROM_WALLS.1 + BASE_REACH);
    let navigation = map.navigation();
    let (cols, rows) = ((area.size.width + BLOCK - 1) / BLOCK, (area.size.height + BLOCK - 1) / BLOCK);
    let mut pieces = vec![0u32; (cols * rows) as usize];
    map.heaps(area, |cell, heap| {
        if navigation.is_outside(cell) {
            let (col, row) = ((cell.x - area.origin.x) / BLOCK, (cell.y - area.origin.y) / BLOCK);
            pieces[(row * cols + col) as usize] += heap.pieces as u32;
        }
    });
    let at = |col: i32, row: i32| if (0..cols).contains(&col) && (0..rows).contains(&row) { pieces[(row * cols + col) as usize] } else { 0 };
    let middle = |col: i32, row: i32| area.origin.offset(col * BLOCK + BLOCK / 2, row * BLOCK + BLOCK / 2);
    let (base, most) = (0..rows)
        .flat_map(|row| (0..cols).map(move |col| (col, row)))
        .map(|(col, row)| {
            let around: u32 = (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (dx, dy))).map(|(dx, dy)| at(col + dx, row + dy)).sum();
            (middle(col, row), around)
        })
        .min_by_key(|&(cell, around)| (std::cmp::Reverse(around), site.chebyshev_distance(cell), cell))?;
    if most == 0 {
        return None;
    }
    // Ring by ring out from the base: the first ring with a clear site in it
    // has the nearest, as the crow flies, a lower cell on a tie.
    for radius in 0..=SEARCH {
        let ring = Bounds::new(base.offset(-radius, -radius), Size::new(2 * radius + 1, 2 * radius + 1));
        let best = ring
            .points()
            .filter(|&cell| radius == 0 || ring.on_edge(cell))
            .filter(|&center| is_clear(map, center, outposts))
            .min_by_key(|&center| (center.manhattan_distance(base), center));
        if best.is_some() {
            return best;
        }
    }
    None
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

/// Whether an outpost with its flag at `center` can go there: every cell of
/// it [`FROM_WALLS`] from the site, and it and the ring round it open ground
/// anybody can get to — walkable now, no heap, crop or anything built in it,
/// not good soil (which the farmers want), not a pocket a lake has shut in —
/// and clear of every outpost already decreed, with a cell between.
pub fn is_clear(map: &Map, center: Point, outposts: &Outposts) -> bool {
    let footprint = footprint(center);
    let (near, far) = distances(map.site(), footprint);
    if near < FROM_WALLS.0 || far > FROM_WALLS.1 {
        return false;
    }
    let ground = footprint.grown(1);
    if outposts.iter().any(|outpost| outpost.footprint().grown(1).overlaps(ground)) {
        return false;
    }
    let navigation = map.navigation();
    ground.points().all(|cell| {
        map.can_build(cell) && !map.terrain(cell).is_some_and(|tile| tile.is_fertile()) && navigation.is_outside(cell)
    })
}

/// The first clear site for an outpost round `map`, scanning out along the
/// band's near edge above the site: for a test that wants an outpost and does
/// not care where.
#[cfg(test)]
pub(crate) fn first_clear_site(map: &Map) -> Point {
    let site = map.site();
    Bounds::new(Point::new(site.origin.x - 60, site.max().y + 1 + FROM_WALLS.0 + CENTRE.1), Size::new(site.size.width + 120, 40))
        .points()
        .find(|&center| is_clear(map, center, &Outposts::new()))
        .expect("somewhere clear in the band above the site")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FLOOR, WALL};
    use crate::sim::uid::EntityType;

    fn builder(n: u64) -> Uid {
        Uid::from_raw(((EntityType::Human as u64) << 56) | n).unwrap()
    }

    /// A small site in the wasteland of seed 3.
    fn site() -> Map {
        Map::new(Size::new(20, 14), FLOOR).with_wasteland(3)
    }

    fn clear_center(map: &Map) -> Point {
        first_clear_site(map)
    }

    #[test]
    fn the_plan_is_twenty_walls_a_doorway_a_fridge_a_store_and_two_beds_for_twenty_two_steel() {
        let outpost = Outpost::new(Point::new(0, 0), false, 1);
        let of = |part| outpost.tiles().iter().filter(|tile| tile.part == part).count();
        assert_eq!((of(Part::Wall), of(Part::Door), of(Part::Fridge), of(Part::Store), of(Part::Bed)), (20, 2, 1, 1, 2));
        assert_eq!(TILES, 26);
        assert_eq!(OUTPOST_STEEL, 22);
        assert_eq!(outpost.footprint(), Bounds::new(Point::new(-3, -3), Size::new(6, 7)));
        assert!(outpost.tile(Point::new(0, 0)).is_none(), "nothing is built under the flag");
        let doors: Vec<Point> = outpost.tiles().iter().filter(|tile| tile.part == Part::Door).map(|tile| tile.cell).collect();
        assert_eq!(doors, [Point::new(-1, -3), Point::new(0, -3)], "the doorway in the bottom row");
        let flipped = Outpost::new(Point::new(0, 0), true, 1);
        let doors: Vec<Point> = flipped.tiles().iter().filter(|tile| tile.part == Part::Door).map(|tile| tile.cell).collect();
        assert_eq!(doors, [Point::new(-1, 3), Point::new(0, 3)], "upside down, in the top row");
        assert!(outpost.tiles().iter().all(|tile| outpost.footprint().contains(tile.cell)));
    }

    #[test]
    fn its_doorway_faces_the_district() {
        let map = site();
        let above = clear_center(&map);
        let mut outposts = Outposts::new();
        assert!(outposts.found(&map, above, 1));
        assert!(!outposts.iter().next().unwrap().flipped(), "above the district, the doorway at the bottom");
        let below = Bounds::new(Point::new(-60, -FROM_WALLS.0 - 40), Size::new(140, 40))
            .points()
            .find(|&center| is_clear(&map, center, &Outposts::new()))
            .expect("somewhere clear in the band below the site");
        let mut outposts = Outposts::new();
        assert!(outposts.found(&map, below, 1));
        assert!(outposts.iter().next().unwrap().flipped(), "below it, at the top");
    }

    #[test]
    fn a_site_is_in_the_band_on_open_ground_and_clear_of_other_outposts() {
        let map = site();
        let center = clear_center(&map);
        let (near, far) = distances(map.site(), footprint(center));
        assert!(near >= FROM_WALLS.0 && far <= FROM_WALLS.1);
        assert!(!is_clear(&map, Point::new(10, 14 + 10), &Outposts::new()), "too near the wall");
        assert!(!is_clear(&map, Point::new(10, 14 + FROM_WALLS.1 + 10), &Outposts::new()), "too far");
        assert!(!is_clear(&map, Point::new(5, 5), &Outposts::new()), "on the site");
        let mut outposts = Outposts::new();
        assert!(outposts.found(&map, center, 1));
        assert!(!is_clear(&map, center.offset(WIDTH, 0), &outposts), "no outpost against another");
        assert!(!is_clear(&Map::new(Size::new(20, 14), FLOOR), center, &Outposts::new()), "nothing round a bare map");
    }

    #[test]
    fn the_king_puts_an_outpost_in_the_band_as_near_as_it_can_to_the_biggest_reachable_base_of_scrap() {
        for seed in [3, 7, 11] {
            let map = Map::new(Size::new(30, 20), FLOOR).with_wasteland(seed);
            let center = choose_site(&map, &Outposts::new()).unwrap_or_else(|| panic!("seed {seed}: a site"));
            assert!(is_clear(&map, center, &Outposts::new()), "seed {seed}");
            // Scrap counted round the site found: there is a fair amount
            // within reach of it, and none of its cells is a heap.
            let mut near = 0;
            map.heaps(footprint(center).grown(BLOCK * 2), |_, heap| near += heap.pieces as u32);
            assert!(near > 0, "seed {seed}: scrap round the outpost at {center:?}");
            // A second goes elsewhere, never on top of the first.
            let mut outposts = Outposts::new();
            assert!(outposts.found(&map, center, 1));
            outposts.outposts[0].tiles.iter_mut().for_each(|tile| tile.stage = Stage::Built);
            let next = choose_site(&map, &outposts).expect("a second site");
            assert!(!footprint(next).grown(1).overlaps(footprint(center).grown(1)), "seed {seed}");
        }
        assert_eq!(choose_site(&Map::new(Size::new(20, 14), FLOOR), &Outposts::new()), None, "no wasteland, no outpost");
    }

    /// A site with an outpost founded on it, and its plan.
    fn founded() -> (Map, Outposts, Point) {
        let mut map = site();
        let center = clear_center(&map);
        map.load_chunks(footprint(center).grown(2));
        let mut outposts = Outposts::new();
        assert!(outposts.found(&map, center, 1));
        (map, outposts, center)
    }

    #[test]
    fn one_outpost_goes_up_at_a_time_and_the_king_looks_once_a_day() {
        let (map, mut outposts, center) = founded();
        assert!(!outposts.wants_a_decree(1) && !outposts.wants_a_decree(2), "one going up");
        assert!(!outposts.found(&map, center.offset(30, 0), 1), "not while one is going up");
        let mut none = Outposts::new();
        assert!(none.wants_a_decree(3));
        none.found_nowhere(3);
        assert!(!none.wants_a_decree(3) && none.wants_a_decree(4));
    }

    #[test]
    fn a_tile_is_one_builders_at_a_time_and_a_builder_has_one_tile() {
        let (_, mut outposts, _) = founded();
        let (a, b) = (builder(1), builder(2));
        let wall = outposts.pick(Point::new(0, 0), 0, false).unwrap();
        assert_eq!(outposts.tile(wall).unwrap().part, Part::Wall, "walls first");
        assert!(outposts.claim(wall, a));
        assert!(!outposts.claim(wall, b), "taken");
        let other = outposts.pick(Point::new(0, 0), 0, false).unwrap();
        assert_ne!(other, wall, "nobody else is offered it");
        assert!(!outposts.claim(other, a), "one tile to a builder");
        assert!(outposts.claim(other, b));
        assert_eq!(outposts.claim_of(a).map(|tile| tile.cell), Some(wall));
        assert!(outposts.has_work_for(a) && outposts.has_work_for(builder(3)));
        assert!(outposts.release(a));
        assert_eq!(outposts.claim_of(a), None);
        assert!(outposts.claim(wall, builder(3)), "free again for anybody");
    }

    #[test]
    fn out_at_the_outpost_with_no_steel_a_builder_takes_on_what_needs_none() {
        let (_, outposts, center) = founded();
        let picked = outposts.pick(center, 0, true).unwrap();
        assert_eq!(outposts.tile(picked).unwrap().steel_wanted(), 0);
        let picked = outposts.pick(center, 1, true).unwrap();
        assert_eq!(outposts.tile(picked).unwrap().part, Part::Wall, "with steel, the walls");
    }

    #[test]
    fn a_tile_is_laid_once_in_the_way_and_built_after_two_hours_work_and_not_before() {
        let (mut map, mut outposts, _) = founded();
        let me = builder(1);
        let wall = outposts.pick(Point::new(0, 0), 0, false).unwrap();
        assert_eq!(outposts.lay(&mut map, wall, me), Err("it is somebody else's to build"));
        assert!(outposts.claim(wall, me));
        assert_eq!(outposts.construct(&mut map, wall, me, HOUR), None, "nothing to work on before it is laid");
        assert_eq!(outposts.lay(&mut map, wall, me), Ok(Part::Wall));
        assert!(!map.is_passable(wall) && map.is_see_through(wall), "a building tile is in the way");
        assert!(outposts.lay(&mut map, wall, me).is_err(), "laid once");
        assert_eq!(outposts.construct(&mut map, wall, builder(2), HOUR), None, "not somebody else's to work");
        assert_eq!(outposts.construct(&mut map, wall, me, HOUR), None);
        assert_eq!(outposts.construct(&mut map, wall, me, HOUR - 1.0), None, "a second short of two hours");
        let done = outposts.construct(&mut map, wall, me, 1.0).expect("built at two hours");
        assert_eq!((done.part, done.fixture, done.built, done.outpost_done), (Part::Wall, None, 1, false));
        assert_eq!(map.terrain(wall), Some(WALL));
        assert!(!map.is_passable(wall) && !map.is_see_through(wall));
        assert_eq!(outposts.claim_of(me), None, "given up once built");
        assert!(!outposts.claim(wall, me), "nothing left to do there");
    }

    #[test]
    fn a_doorway_is_never_in_the_way_and_furniture_is_a_fixture_once_built() {
        let (mut map, mut outposts, _) = founded();
        let me = builder(1);
        let outpost = outposts.under_way().unwrap().clone();
        for tile in outpost.tiles() {
            assert!(outposts.claim(tile.cell, me));
            assert_eq!(outposts.lay(&mut map, tile.cell, me), Ok(tile.part));
            assert_eq!(map.is_passable(tile.cell), tile.part == Part::Door, "{:?}", tile.part);
            let done = outposts.construct(&mut map, tile.cell, me, TILE_WORK).unwrap();
            assert_eq!(done.fixture.map(|fixture| fixture.prop), tile.part.prop());
            assert_eq!(map.is_passable(tile.cell), tile.part == Part::Door, "{:?} built", tile.part);
        }
        assert!(outposts.under_way().is_none() && outposts.complete() == 1);
        let mut fixtures: Vec<&str> = outposts.fixtures().map(|fixture| fixture.prop).collect();
        fixtures.sort_unstable();
        assert_eq!(fixtures, ["bed 1", "bed 1", "door", "door", "fridge", "outpost store"]);
        assert!(outposts.wants_a_decree(1), "and the next can be decreed");
        assert!(map.navigation().may_reach(outpost.center(), outpost.center().offset(0, 20)), "the inside is got to by the doorway");
    }

    #[test]
    fn a_building_tile_is_not_laid_where_the_ground_is_not_clear() {
        let (mut map, mut outposts, _) = founded();
        let me = builder(1);
        let wall = outposts.pick(Point::new(0, 0), 0, false).unwrap();
        assert!(outposts.claim(wall, me));
        assert!(map.build(wall, Structure::Obstacle));
        assert_eq!(outposts.lay(&mut map, wall, me), Err("the ground there is not clear"));
    }

    #[test]
    fn a_builder_works_beside_its_tile_never_in_another() {
        let (_, outposts, _) = founded();
        let outpost = outposts.under_way().unwrap();
        for tile in outpost.tiles() {
            let stands: Vec<Point> = outposts.stand_cells(tile.cell).collect();
            assert!(!stands.is_empty(), "{:?} at {:?}", tile.part, tile.cell);
            assert!(stands.iter().all(|&cell| outpost.tile(cell).is_none()));
        }
    }

    #[test]
    fn outposts_come_back_from_a_save_as_they_were_and_build_the_map_again() {
        let (mut map, mut outposts, _) = founded();
        let me = builder(1);
        let wall = outposts.pick(Point::new(0, 0), 0, false).unwrap();
        assert!(outposts.claim(wall, me));
        outposts.lay(&mut map, wall, me).unwrap();
        outposts.construct(&mut map, wall, me, 600.0);
        let saved = outposts.save();
        let mut fresh = site();
        let restored = Outposts::restore(Some(&saved), &mut fresh).unwrap();
        assert_eq!(restored, outposts);
        assert_eq!(fresh.built().collect::<Vec<_>>(), map.built().collect::<Vec<_>>());
        assert_eq!(Outposts::restore(None, &mut site()).unwrap(), Outposts::new());

        let mut short = saved.clone();
        short.outposts[0].tiles.pop();
        assert!(Outposts::restore(Some(&short), &mut site()).is_err(), "a tile missing");
        let mut overworked = saved.clone();
        let laid = overworked.outposts[0].tiles.iter_mut().find(|tile| tile.stage == pb::TileStage::Laid as i32).unwrap();
        laid.progress = TILE_WORK;
        assert!(Outposts::restore(Some(&overworked), &mut site()).is_err(), "work past what it takes");
        let mut twice = saved.clone();
        let other = twice.outposts[0].tiles.iter().position(|tile| tile.claim.is_none()).unwrap();
        twice.outposts[0].tiles[other].claim = Some(me.raw());
        assert!(Outposts::restore(Some(&twice), &mut site()).is_err(), "a builder on two tiles");
        let mut unknown = saved.clone();
        unknown.outposts[0].tiles[0].stage = 9;
        assert!(Outposts::restore(Some(&unknown), &mut site()).is_err(), "no such stage");
        let mut blocked = site();
        blocked.load_chunks(footprint(saved.outposts[0].center.unwrap().into()).grown(2));
        assert!(blocked.build(wall, Structure::Wall));
        assert!(Outposts::restore(Some(&saved), &mut blocked).is_err(), "something already standing there");
    }
}
