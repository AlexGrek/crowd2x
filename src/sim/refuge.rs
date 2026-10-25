//! [`WaysIn`]: where on the edge of the site there is a way in, and the
//! cell a few steps inside it that somebody running for safety runs to.
//!
//! **Inside the site is safe**: a deathdog never sets foot on it
//! (`sim::deathdogs`), so anybody outside who panics runs for the nearest way
//! in (`brain::goals::FleeGoal`). On a walled district the ways in are its
//! gates — the edge is the wall but for them — and on an open map, all of
//! its edge. Each is taken a few cells in ([`REFUGE_DEPTH`]) while the cells
//! stay passable, so somebody who gets there is clear of the gateway, not
//! standing in it with a deathdog an arm's length away.
//!
//! Derived from the map once, in `Facilities::new`, and derived again on
//! load: **not saved**.

use crate::map::{Map, Point};

/// How many cells in from the edge a refuge is taken, at most.
pub const REFUGE_DEPTH: i32 = 4;

/// How deep inside the site somebody has to be to have stopped running
/// ([`depth`]): off the edge's ring of cells.
pub const SAFE_DEPTH: i32 = 2;

/// One way in: a passable cell on the site's edge, and the refuge inside it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WayIn {
    pub edge: Point,
    pub refuge: Point,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WaysIn {
    /// In the order the edge is walked: bottom row, top row, left column,
    /// right column — the order ties go in.
    ways: Vec<WayIn>,
}

impl WaysIn {
    /// Every passable cell on `map`'s edge, each with its refuge.
    pub fn from_map(map: &Map) -> WaysIn {
        let site = map.site();
        let max = site.max();
        let mut ways = Vec::new();
        let mut add = |edge: Point, inward: (i32, i32)| {
            if !map.is_passable(edge) {
                return;
            }
            let mut refuge = edge;
            for _ in 0..REFUGE_DEPTH {
                let next = refuge.offset(inward.0, inward.1);
                if !site.contains(next) || !map.is_passable(next) {
                    break;
                }
                refuge = next;
            }
            ways.push(WayIn { edge, refuge });
        };
        for x in site.origin.x..=max.x {
            add(Point::new(x, site.origin.y), (0, 1));
            if max.y != site.origin.y {
                add(Point::new(x, max.y), (0, -1));
            }
        }
        for y in site.origin.y + 1..max.y {
            add(Point::new(site.origin.x, y), (1, 0));
            if max.x != site.origin.x {
                add(Point::new(max.x, y), (-1, 0));
            }
        }
        WaysIn { ways }
    }

    /// The refuge behind the way in nearest `here`, by the steps to its edge
    /// cell; ties to the first listed. `None` on a site with no way in. A
    /// walk of every way in — a few dozen on a district — asked once a flight
    /// is planned, never per tick.
    pub fn nearest(&self, here: Point) -> Option<Point> {
        self.ways
            .iter()
            .min_by_key(|way| way.edge.manhattan_distance(here))
            .map(|way| way.refuge)
    }

    /// Every way in, in order.
    pub fn iter(&self) -> impl Iterator<Item = &WayIn> + '_ {
        self.ways.iter()
    }

    pub fn len(&self) -> usize {
        self.ways.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ways.is_empty()
    }
}

/// How far inside the site `cell` is: 0 outside it, 1 on its edge, 2 one
/// cell in from that, and so on.
pub fn depth(map: &Map, cell: Point) -> i32 {
    let site = map.site();
    if !site.contains(cell) {
        return 0;
    }
    let max = site.max();
    1 + (cell.x - site.origin.x).min(cell.y - site.origin.y).min(max.x - cell.x).min(max.y - cell.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Size, FLOOR, WALL};

    /// A walled yard with one gap in the bottom wall: one way in, taken in
    /// as far as the yard lets it.
    #[test]
    fn a_walled_site_s_only_ways_in_are_its_gaps_and_each_refuge_is_inside() {
        let mut map = Map::new(Size::new(12, 10), FLOOR);
        for cell in map.site().points().collect::<Vec<_>>() {
            if map.site().on_edge(cell) {
                map.set_terrain(cell, WALL);
            }
        }
        map.set_terrain(Point::new(5, 0), FLOOR);
        map.set_terrain(Point::new(5, 3), WALL);
        let ways = WaysIn::from_map(&map);
        assert_eq!(ways.iter().copied().collect::<Vec<_>>(), [WayIn { edge: Point::new(5, 0), refuge: Point::new(5, 2) }]);
        assert_eq!(ways.nearest(Point::new(-20, -9)), Some(Point::new(5, 2)));
        assert_eq!(depth(&map, Point::new(5, 2)), 3);
    }

    #[test]
    fn on_an_open_map_every_edge_cell_is_a_way_in_and_depth_counts_in_from_the_edge() {
        let map = Map::new(Size::new(10, 10), FLOOR);
        let ways = WaysIn::from_map(&map);
        assert_eq!(ways.len(), 36);
        assert_eq!(ways.nearest(Point::new(4, -6)), Some(Point::new(4, REFUGE_DEPTH)));
        assert_eq!(ways.nearest(Point::new(15, 6)), Some(Point::new(9 - REFUGE_DEPTH, 6)));
        assert_eq!(depth(&map, Point::new(-1, 3)), 0);
        assert_eq!(depth(&map, Point::new(0, 3)), 1);
        assert_eq!(depth(&map, Point::new(4, 5)), 5);
    }
}
