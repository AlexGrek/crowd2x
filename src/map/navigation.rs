//! Static connected components and their cheapest terrain cost.
//!
//! Potentially open doors connect components. Keys remain the search's job.
use super::{MIN_WALK_COST, Map, Point};

#[derive(Clone, Debug)]
pub struct Navigation {
    components: Vec<u32>,
    minimum: Vec<u32>,
    size: super::Size,
}
impl Navigation {
    pub(super) fn build(map: &Map) -> Self {
        let size = map.size();
        let mut result = Self {
            components: vec![0; size.area()],
            minimum: vec![MIN_WALK_COST as u32],
            size,
        };
        let mut pending = Vec::new();
        for cell in size.points() {
            let index = size.index_of(cell).unwrap();
            if result.components[index] != 0 || !map.is_passable(cell) {
                continue;
            }
            let component = result.minimum.len() as u32;
            let mut minimum = u32::MAX;
            result.components[index] = component;
            pending.push(cell);
            while let Some(here) = pending.pop() {
                minimum = minimum.min(map.walk_cost(here) as u32);
                for next in here.cardinal_neighbours() {
                    let Some(index) = size.index_of(next) else {
                        continue;
                    };
                    if result.components[index] == 0 && map.is_passable(next) {
                        result.components[index] = component;
                        pending.push(next);
                    }
                }
            }
            result.minimum.push(minimum.max(MIN_WALK_COST as u32));
        }
        result
    }
    fn component(&self, cell: Point) -> u32 {
        self.size.index_of(cell).map_or(0, |i| self.components[i])
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
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FLOOR, Size, TerrainId, WALL};
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
}
