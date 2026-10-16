//! The wasteland: the endless empty earth round every map.
//!
//! A map is a finite *site* — the cells somebody authored, or the district
//! generator laid out — and outside it the world does not stop. It goes on in
//! every direction as bare post-apocalyptic ground: mostly dirt, some of it
//! stony or cracked, patches of scrub and meadow where something still grows,
//! and lakes with a margin of mud. Nothing is built out there and nothing
//! stands in it; it is terrain only.
//!
//! **It is a pure function of a seed and a cell.** [`Wasteland::tile`] is
//! the whole of it: no state, no history, nothing that depends on what was
//! asked first. So it needs storing nowhere — a map file holds the seed — and
//! the chunks ([`super::chunk`]) it is cached in can be loaded and dropped by
//! whatever is looking at them without that changing a single cell, which is
//! what lets the camera stream the world in and the simulation stay
//! deterministic.
//!
//! The ground is a few fields of value noise (a hashed lattice, smoothly
//! interpolated, at several scales), all integer hashing and `f32` adds and
//! multiplies — no `sin`, no `exp`, nothing a platform rounds differently —
//! so a seed is the same world on every machine:
//!
//! * **wet** decides water: a lake where it is highest, mud on the shore;
//! * **green** decides growth: meadow where it is highest, scrub round it —
//!   and it is raised near water, so lakes come with green banks;
//! * **dry** cracks the dirt in patches, and a per-cell hash scatters stones.
//!
//! **The site keeps a clearing.** Water is pushed down near the site and
//! gone within [`CLEARING`] cells of it, so whatever leads off a map's edge
//! — a district's outer pavement, a door in an outer wall — opens onto dry
//! ground and never into a lake.

use super::coords::{Bounds, Point};
use super::terrain::{TerrainId, DIRT, DIRT_CRACKED, DIRT_STONES, MEADOW, MUD, SCRUB, WATER};

/// The seed of a map that never named one: every map saved before the
/// wasteland existed, and a new one from the browser.
pub const DEFAULT_SEED: u64 = 0;

/// No water within this many cells of the site.
pub const CLEARING: i32 = 12;

/// Over this many cells beyond [`CLEARING`] the lakes come back to full
/// strength, so the clearing has no hard rim.
const CLEARING_FADE: i32 = 24;

/// Every tile the wasteland lays, for whatever needs to know them all — the
/// editor's palette has to draw each one.
pub const TILES: [TerrainId; 7] = [DIRT, DIRT_STONES, DIRT_CRACKED, SCRUB, MEADOW, MUD, WATER];

/// What walking the cheapest wasteland ground costs: bare dirt.
pub const MIN_COST: u8 = 2;

// Thresholds on the fields, each in [0, 1), set from the fields' measured
// quantiles over a big patch of several seeds: water above the 91st
// percentile of wetness and mud for three more, meadow and scrub a fifth of
// the ground, and the dry patches of cracked earth the top seventh of the
// rest. `the_ground_is_mostly_dirt_with_some_green_and_some_water` holds them
// to that.
const WATER_ABOVE: f32 = 0.69;
const MUD_ABOVE: f32 = 0.665;
const MEADOW_ABOVE: f32 = 0.705;
const SCRUB_ABOVE: f32 = 0.64;
const CRACKED_ABOVE: f32 = 0.75;
/// One cell of plain dirt in this many is stony.
const STONES_ONE_IN: u64 = 7;

// Salts, so the fields of one seed are unrelated to each other.
const WET: u64 = 0x5745_5400_0000_0001;
const GREEN: u64 = 0x4752_4545_4e00_0002;
const DRY: u64 = 0x4452_5900_0000_0003;
const STONES: u64 = 0x5354_4f4e_4500_0004;

/// The ground round one map.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Wasteland {
    seed: u64,
}

impl Wasteland {
    pub const fn new(seed: u64) -> Wasteland {
        Wasteland { seed }
    }

    pub const fn seed(self) -> u64 {
        self.seed
    }

    /// The ground at `cell`, round a site covering `site`.
    ///
    /// Defined for every cell, the site's own included — what would be there
    /// had nobody built anything — though a map only asks it about cells off
    /// the site.
    pub fn tile(self, cell: Point, site: Bounds) -> TerrainId {
        let (x, y) = (cell.x, cell.y);
        let away = site.chebyshev_distance(cell);
        // Lakes are pushed down near the site: gone inside the clearing,
        // fading back in over the band beyond it.
        let held_back = if away <= CLEARING {
            1.0
        } else if away <= CLEARING + CLEARING_FADE {
            (CLEARING + CLEARING_FADE - away) as f32 / CLEARING_FADE as f32 * 0.25
        } else {
            0.0
        };
        let wet = fbm(self.seed ^ WET, x, y, [61, 23, 9]) - held_back;
        if wet > WATER_ABOVE {
            return WATER;
        }
        if wet > MUD_ABOVE {
            return MUD;
        }
        // Green near water: a lake's banks grow over.
        let green = fbm(self.seed ^ GREEN, x, y, [47, 17, 7]) + (wet - 0.5).max(0.0) * 0.5;
        if green > MEADOW_ABOVE {
            return MEADOW;
        }
        if green > SCRUB_ABOVE {
            return SCRUB;
        }
        if value(self.seed ^ DRY, x, y, 13) > CRACKED_ABOVE {
            return DIRT_CRACKED;
        }
        if hash(self.seed ^ STONES, x, y) % STONES_ONE_IN == 0 {
            return DIRT_STONES;
        }
        DIRT
    }
}

/// splitmix64's finaliser: every bit of the input moves every bit of the
/// output.
const fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A lattice point's hash.
#[inline]
const fn hash(seed: u64, x: i32, y: i32) -> u64 {
    mix(seed ^ mix((x as u32 as u64) | ((y as u32 as u64) << 32)))
}

/// A hash as a number in `[0, 1)`: its top 24 bits, which an `f32` holds
/// exactly.
#[inline]
fn unit(hash: u64) -> f32 {
    (hash >> 40) as f32 * (1.0 / (1u64 << 24) as f32)
}

/// Smoothstep: eases into and out of each lattice point, so the field has no
/// creases along the lattice lines.
#[inline]
fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Value noise in `[0, 1)` at a cell, on a lattice `scale` cells apart.
#[inline]
fn value(seed: u64, x: i32, y: i32, scale: i32) -> f32 {
    let (gx, gy) = (x.div_euclid(scale), y.div_euclid(scale));
    let tx = ease(x.rem_euclid(scale) as f32 / scale as f32);
    let ty = ease(y.rem_euclid(scale) as f32 / scale as f32);
    let a = unit(hash(seed, gx, gy));
    let b = unit(hash(seed, gx + 1, gy));
    let c = unit(hash(seed, gx, gy + 1));
    let d = unit(hash(seed, gx + 1, gy + 1));
    let bottom = a + (b - a) * tx;
    let top = c + (d - c) * tx;
    bottom + (top - bottom) * ty
}

/// Three octaves of value noise, coarsest first, weighted so the coarse one
/// draws the shapes and the fine ones fray their edges. In `[0, 1)`.
#[inline]
fn fbm(seed: u64, x: i32, y: i32, scales: [i32; 3]) -> f32 {
    value(seed, x, y, scales[0]) * 0.6
        + value(mix(seed), x, y, scales[1]) * 0.28
        + value(mix(mix(seed)), x, y, scales[2]) * 0.12
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Size;

    fn site() -> Bounds {
        Bounds::from(Size::new(40, 30))
    }

    #[test]
    fn a_seed_is_the_same_world_whatever_is_asked_first() {
        let land = Wasteland::new(7);
        let cells: Vec<Point> = (0..400).map(|i| Point::new(i * 37 % 211 - 105, i * 53 % 197 - 98)).collect();
        let forwards: Vec<TerrainId> = cells.iter().map(|&c| land.tile(c, site())).collect();
        let backwards: Vec<TerrainId> = cells.iter().rev().map(|&c| land.tile(c, site())).collect();
        assert!(forwards.iter().eq(backwards.iter().rev()));
        assert_eq!(forwards, cells.iter().map(|&c| Wasteland::new(7).tile(c, site())).collect::<Vec<_>>());
    }

    #[test]
    fn different_seeds_are_different_worlds() {
        let cells = Bounds::new(Point::new(200, 200), Size::new(64, 64));
        let differ = cells
            .points()
            .filter(|&c| Wasteland::new(1).tile(c, site()) != Wasteland::new(2).tile(c, site()))
            .count();
        assert!(differ > cells.area() / 4, "only {differ} cells differ");
    }

    #[test]
    fn the_ground_is_mostly_dirt_with_some_green_and_some_water() {
        // Far from the site, where the clearing does not reach.
        let patch = Bounds::new(Point::new(-1500, 900), Size::new(384, 384));
        let mut counts = [0usize; TILES.len()];
        for seed in [0, 1, 7, 40500] {
            let land = Wasteland::new(seed);
            for cell in patch.points() {
                let tile = land.tile(cell, site());
                counts[TILES.iter().position(|&t| t == tile).expect("a wasteland tile")] += 1;
            }
        }
        let total: usize = counts.iter().sum();
        let share = |tiles: &[TerrainId]| {
            tiles
                .iter()
                .map(|t| counts[TILES.iter().position(|x| x == t).unwrap()])
                .sum::<usize>() as f32
                / total as f32
        };
        let dirt = share(&[DIRT, DIRT_STONES, DIRT_CRACKED]);
        let green = share(&[SCRUB, MEADOW]);
        let water = share(&[WATER]);
        let mud = share(&[MUD]);
        assert!(dirt > 0.6, "dirt {dirt}");
        assert!((0.1..0.3).contains(&green), "green {green}");
        assert!((0.04..0.15).contains(&water), "water {water}");
        assert!(mud > 0.01 && mud < water, "mud {mud}");
        for tile in TILES {
            assert!(share(&[tile]) > 0.0, "no {} at all", tile.name());
        }
    }

    #[test]
    fn there_is_no_water_in_the_clearing_round_the_site() {
        for seed in 0..16 {
            let land = Wasteland::new(seed);
            for cell in site().grown(CLEARING).points() {
                let tile = land.tile(cell, site());
                assert!(tile != WATER && tile != MUD, "seed {seed}: {} at {cell:?}", tile.name());
            }
        }
    }

    #[test]
    fn every_tile_is_ground_a_unit_could_stand_on_or_water() {
        for tile in TILES {
            if tile == WATER {
                assert!(!tile.is_passable());
            } else {
                assert!(tile.is_passable() && tile.walk_cost() >= MIN_COST, "{}", tile.name());
            }
        }
    }

    #[test]
    fn the_land_is_one_piece_and_the_lakes_are_not() {
        // Flood the dry land of a big square from its middle: nearly all of it
        // is one piece, so nobody walks out of a map into a dead end.
        let land = Wasteland::new(3);
        let patch = Bounds::new(Point::new(-200, -200), Size::new(400, 400));
        let walkable: Vec<bool> = patch.points().map(|c| land.tile(c, site()).is_passable()).collect();
        let start = Point::new(-1, -1);
        let mut seen = vec![false; patch.area()];
        let mut queue = vec![start];
        seen[patch.index_of(start).unwrap()] = true;
        let mut reached = 0usize;
        while let Some(cell) = queue.pop() {
            reached += 1;
            for next in cell.cardinal_neighbours() {
                if let Some(index) = patch.index_of(next)
                    && !seen[index]
                    && walkable[index]
                {
                    seen[index] = true;
                    queue.push(next);
                }
            }
        }
        let dry = walkable.iter().filter(|&&w| w).count();
        assert!(reached as f32 > dry as f32 * 0.97, "{reached} of {dry} dry cells joined up");
    }
}

