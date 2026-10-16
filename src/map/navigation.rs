//! Static connected components and their cheapest terrain cost.
//!
//! Potentially open doors connect components. Keys remain the search's job.
//!
//! Worked out over [`Map::navigation_area`]: the site, and on an endless map
//! a margin of wasteland round it. The world past that margin is not labelled
//! cell by cell — it has no end to label — so every component that reaches the
//! margin's edge is one component, **the outside**, and so is every cell past
//! it. That can only ever say two cells are joined when they are not (a lake
//! island out in the wasteland), never the reverse, which is the direction a
//! filter in front of a search is allowed to be wrong in: the search, bounded
//! by its window, finds out.
use super::{MIN_WALK_COST, Map, Point, wasteland};

#[derive(Clone, Debug)]
pub struct Navigation {
    components: Vec<u32>,
    minimum: Vec<u32>,
    area: super::Bounds,
    /// The component every cell past the area is in; `0` (none) on a map
    /// with nothing past its edge.
    outside: u32,
}
impl Navigation {
    pub(super) fn build(map: &Map) -> Self {
        let area = map.navigation_area();
        let mut result = Self {
            components: vec![0; area.area()],
            minimum: vec![MIN_WALK_COST as u32],
            area,
            outside: 0,
        };
        // Whether each component touches the area's edge.
        let mut on_edge = vec![false];
        let mut pending = Vec::new();
        for cell in area.points() {
            let index = area.index_of(cell).unwrap();
            if result.components[index] != 0 || !map.is_passable(cell) {
                continue;
            }
            let component = result.minimum.len() as u32;
            let mut minimum = u32::MAX;
            let mut edge = false;
            result.components[index] = component;
            pending.push(cell);
            while let Some(here) = pending.pop() {
                minimum = minimum.min(map.walk_cost(here) as u32);
                edge |= area.on_edge(here);
                for next in here.cardinal_neighbours() {
                    let Some(index) = area.index_of(next) else {
                        continue;
                    };
                    if result.components[index] == 0 && map.is_passable(next) {
                        result.components[index] = component;
                        pending.push(next);
                    }
                }
            }
            result.minimum.push(minimum.max(MIN_WALK_COST as u32));
            on_edge.push(edge);
        }
        if map.is_endless() {
            result.join_the_outside(&on_edge);
        }
        result
    }

    /// Fold every component touching the area's edge into one, the outside,
    /// which also holds everything past the edge.
    fn join_the_outside(&mut self, on_edge: &[bool]) {
        let outside = self.minimum.len() as u32;
        let mut minimum = wasteland::MIN_COST as u32;
        for (component, &edge) in on_edge.iter().enumerate() {
            if edge {
                minimum = minimum.min(self.minimum[component]);
            }
        }
        for component in &mut self.components {
            if *component != 0 && on_edge[*component as usize] {
                *component = outside;
            }
        }
        self.minimum.push(minimum);
        self.outside = outside;
    }

    fn component(&self, cell: Point) -> u32 {
        self.area.index_of(cell).map_or(self.outside, |i| self.components[i])
    }
    /// A blocked start may escape through any passable neighbour, including
    /// when leaving an entered feature such as a bed or toilet.
    pub fn may_reach(&self, start: Point, goal: Point) -> bool {
        if start == goal {
            return true;
        }
        let to = self.component(goal);
        if to == 0 {
            return false;
        }
        let from = self.component(start);
        if from != 0 {
            from == to
        } else {
            start
                .cardinal_neighbours()
                .iter()
                .any(|&p| self.component(p) == to)
        }
    }
    pub fn minimum_cost(&self, goal: Point) -> u32 {
        self.minimum[self.component(goal) as usize]
    }
    /// Whether `cell` is open wasteland: in the component joined to
    /// everything past the area's edge, not a pocket a lake has walled in.
    /// Every cell past the area counts, as it does for [`Self::may_reach`].
    /// Never on a map with nothing past its edge.
    pub fn is_outside(&self, cell: Point) -> bool {
        self.outside != 0 && self.component(cell) == self.outside
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FLOOR, NAV_MARGIN, Size, TerrainId, WALL};
    #[test]
    fn adding_and_removing_furniture_invalidates_connectivity() {
        let mut map = Map::new(Size::new(5, 1), FLOOR);
        let (start, goal) = (Point::ORIGIN, Point::new(4, 0));
        assert!(map.navigation().may_reach(start, goal));
        crate::sim::testing::prop_at(&mut map, "fridge", Point::new(2, 0));
        assert!(!map.navigation().may_reach(start, goal));
        let prop = map.objects(crate::map::ObjectLayer::Props)[0].clone();
        map.remove_object(crate::map::ObjectLayer::Props, &prop);
        assert!(map.navigation().may_reach(start, goal));
    }

    #[test]
    fn components_reject_sealed_rooms_and_refresh_after_an_edit() {
        let mut map = Map::new(Size::new(7, 5), FLOOR);
        for y in 0..5 {
            map.set_terrain(Point::new(3, y), WALL);
        }
        assert!(
            !map.navigation()
                .may_reach(Point::new(1, 2), Point::new(5, 2))
        );
        map.set_terrain(Point::new(3, 2), FLOOR);
        assert!(
            map.navigation()
                .may_reach(Point::new(1, 2), Point::new(5, 2))
        );
    }
    #[test]
    fn the_minimum_cost_tracks_terrain_edits_and_blocked_starts_can_escape() {
        let road = TerrainId::from_name("road").unwrap();
        let mut map = Map::new(Size::new(5, 5), road);
        assert_eq!(map.navigation().minimum_cost(Point::ORIGIN), 5);
        map.set_terrain(Point::new(2, 2), FLOOR);
        assert_eq!(map.navigation().minimum_cost(Point::ORIGIN), 1);
        map.set_terrain(Point::new(2, 2), WALL);
        assert!(map.navigation().may_reach(Point::new(2, 2), Point::ORIGIN));
    }

    #[test]
    fn on_an_endless_map_the_two_sides_of_a_wall_meet_round_it() {
        // A wall down the middle of the site parts it on a bounded map; with
        // the wasteland round it, walking out and round joins the halves.
        let mut map = Map::new(Size::new(7, 5), FLOOR);
        for y in 0..5 {
            map.set_terrain(Point::new(3, y), WALL);
        }
        let (west, east) = (Point::new(1, 2), Point::new(5, 2));
        assert!(!map.navigation().may_reach(west, east));
        let map = map.with_wasteland(1);
        assert!(map.navigation().may_reach(west, east));
        // ...and the wasteland past the margin is the same place.
        let far = Point::new(-NAV_MARGIN * 3, 40);
        assert!(map.navigation().may_reach(west, far));
        assert!(map.navigation().minimum_cost(far) >= MIN_WALK_COST as u32);
    }

    #[test]
    fn a_room_walled_in_is_still_sealed_on_an_endless_map() {
        let mut map = Map::new(Size::new(7, 7), FLOOR);
        for cell in crate::map::Bounds::from_corners(Point::new(1, 1), Point::new(5, 5)).points() {
            if crate::map::Bounds::from_corners(Point::new(1, 1), Point::new(5, 5)).on_edge(cell) {
                map.set_terrain(cell, WALL);
            }
        }
        assert!(!map.navigation().is_outside(Point::new(0, 0)), "a bounded map has no outside");
        let map = map.with_wasteland(2);
        assert!(!map.navigation().may_reach(Point::new(3, 3), Point::new(0, 0)));
        assert!(!map.navigation().may_reach(Point::new(0, 0), Point::new(3, 3)));
        assert!(map.navigation().may_reach(Point::new(0, 0), Point::new(-50, -50)));
        // The room is a pocket; the yard round it is the open world.
        assert!(!map.navigation().is_outside(Point::new(3, 3)));
        assert!(map.navigation().is_outside(Point::new(0, 0)));
        assert!(map.navigation().is_outside(Point::new(-NAV_MARGIN * 3, 40)));
    }
}
