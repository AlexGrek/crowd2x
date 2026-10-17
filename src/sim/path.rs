//! Deterministic, resumable orthogonal A* and reusable route storage.
//!
//! A search distinguishes exhausted work from an impossible route. Scratch
//! uses generation-stamped grid slots: beginning a small search never clears
//! the capacity retained by an earlier large one. Ordinary searches reuse one
//! finder per thread; only unfinished searches keep their finder with a walker.
//!
//! A search runs inside a **window**, the cells its scratch is laid over. On a
//! bounded map that is the map. On an endless one — the wasteland round the
//! site goes on for ever — it is the start and the goals with
//! [`SEARCH_MARGIN`] round them, joined with the map's navigation area when
//! they are near it, so a route about the site can swing out round anything
//! in the way and a route out in the wasteland costs what its own stretch of
//! ground costs. Unreachable is then "not inside the window", which is what
//! keeps a search for a cell on a lake island from flooding the world.

use crate::map::{Bounds, MIN_WALK_COST, Map, Point};

/// How far past the start and the goals a search on an endless map may go.
/// A chunk: any detour that wants more than that is round something the size
/// of a lake, and near the site the navigation area is bigger anyway.
pub const SEARCH_MARGIN: i32 = crate::map::CHUNK;

/// The cells a search from `start` to `goals` is allowed to cover.
pub fn window(map: &Map, start: Point, goals: &[Point]) -> Bounds {
    if !map.is_endless() {
        return map.site();
    }
    let span = Bounds::around(goals.iter().copied().chain([start]))
        .expect("the start at least")
        .grown(SEARCH_MARGIN);
    let area = map.navigation_area();
    if span.overlaps(area) { span.union(area) } else { span }
}
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Inline destination set: a single cell or up to four sides of a feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Destinations {
    cells: [Point; 4],
    len: u8,
}
impl Destinations {
    pub const fn one(cell: Point) -> Self {
        Self {
            cells: [cell; 4],
            len: 1,
        }
    }
    pub fn from_cells(cells: impl IntoIterator<Item = Point>) -> Self {
        let mut result = Self {
            cells: [Point::ORIGIN; 4],
            len: 0,
        };
        for cell in cells {
            if result.as_slice().contains(&cell) {
                continue;
            }
            assert!(result.len < 4, "at most four destinations");
            result.cells[result.len as usize] = cell;
            result.len += 1;
        }
        result
    }
    pub fn as_slice(&self) -> &[Point] {
        &self.cells[..self.len as usize]
    }
    pub fn first(&self) -> Option<Point> {
        self.as_slice().first().copied()
    }
}

/// The far route and a short replacement prefix. Repairing a prefix copies
/// only any unfinished previous detour, never the untouched far-route tail.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Path {
    steps: Vec<Point>,
    next: usize,
    patch: Vec<Point>,
    patch_next: usize,
}
impl Path {
    pub fn none() -> Self {
        Self::default()
    }
    pub fn new(steps: Vec<Point>) -> Self {
        Self {
            steps,
            ..Self::default()
        }
    }
    pub fn current(&self) -> Option<Point> {
        self.patch
            .get(self.patch_next)
            .or_else(|| self.steps.get(self.next))
            .copied()
    }
    pub fn destination(&self) -> Option<Point> {
        self.steps
            .get(self.next..)
            .and_then(|s| s.last())
            .or_else(|| self.patch.last())
            .copied()
    }
    pub fn remaining(&self) -> impl Iterator<Item = Point> + '_ {
        self.patch[self.patch_next..]
            .iter()
            .chain(self.steps[self.next..].iter())
            .copied()
    }
    pub fn len(&self) -> usize {
        self.patch.len() - self.patch_next + self.steps.len() - self.next
    }
    pub fn is_done(&self) -> bool {
        self.len() == 0
    }
    pub fn advance(&mut self) {
        if self.patch_next < self.patch.len() {
            self.patch_next += 1;
        } else if self.next < self.steps.len() {
            self.next += 1;
        }
    }
    pub fn clear(&mut self) {
        self.steps.clear();
        self.patch.clear();
        self.next = 0;
        self.patch_next = 0;
    }
    pub fn output(&mut self) -> &mut Vec<Point> {
        self.clear();
        &mut self.steps
    }
    pub fn single(&mut self, cell: Point) {
        self.output().push(cell);
    }
    pub fn splice(&mut self, count: usize, detour: &mut Vec<Point>) {
        let patched = count.min(self.patch.len() - self.patch_next);
        self.patch_next += patched;
        self.next = (self.next + count - patched).min(self.steps.len());
        detour.extend_from_slice(&self.patch[self.patch_next..]);
        std::mem::swap(&mut self.patch, detour);
        detour.clear();
        self.patch_next = 0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchStatus {
    Found(Point),
    Unreachable,
    BudgetExhausted,
}

/// What a [`search`] may spend this time round.
///
/// A budget delays a route and never changes it: an unfinished search
/// resumes exactly where it stopped, and one that had to be let go starts
/// again from nothing and finds the same route, A* being deterministic. So
/// however little it is given, the route found is the one an unlimited search
/// would have found — only later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allowance {
    /// Cells it may expand; what it does expand is taken off.
    pub cells: usize,
    /// Whether a search that runs out may keep its own scratch to carry on
    /// next tick. If not, its work is let go and it starts again when next
    /// asked: an unfinished search holds scratch the size of its window, and
    /// whoever hands out allowances bounds how many are held at once
    /// (`sim::react_step`).
    pub may_keep: bool,
}

impl Allowance {
    /// `cells`, and an unfinished search may be kept.
    pub const fn new(cells: usize) -> Allowance {
        Allowance { cells, may_keep: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    f: u64,
    h: u64,
    point: Point,
}
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .f
            .cmp(&self.f)
            .then_with(|| other.h.cmp(&self.h))
            .then_with(|| other.point.cmp(&self.point))
    }
}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
#[derive(Clone, Copy, Debug, Default)]
struct Visit {
    generation: u32,
    cost: u64,
    from: Point,
}

#[derive(Clone, Debug, Default)]
pub struct PathFinder {
    open: BinaryHeap<Candidate>,
    seen: Vec<Visit>,
    generation: u32,
    window: Option<Bounds>,
    start: Point,
    goals: Option<Destinations>,
    minimum: u32,
    revision: u64,
    #[cfg(test)]
    expanded: usize,
}
impl PathFinder {
    pub fn new() -> Self {
        Self::default()
    }
    fn guess(&self, point: Point) -> u64 {
        self.goals
            .unwrap()
            .as_slice()
            .iter()
            .map(|&goal| {
                (point.x.abs_diff(goal.x) as u64 + point.y.abs_diff(goal.y) as u64)
                    * self.minimum as u64
            })
            .min()
            .unwrap_or(0)
    }
    pub fn begin(&mut self, window: Bounds, start: Point, goals: Destinations, minimum: u32) {
        self.open.clear();
        self.window = Some(window);
        self.seen.resize(window.area(), Visit::default());
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.seen.fill(Visit::default());
            self.generation = 1;
        }
        self.start = start;
        self.goals = Some(goals);
        self.minimum = minimum.max(MIN_WALK_COST as u32);
        #[cfg(test)]
        {
            self.expanded = 0;
        }
        // Starts outside the window are not valid simulation positions.
        if goals.first().is_none() {
            return;
        }
        let Some(index) = window.index_of(start) else {
            return;
        };
        self.seen[index] = Visit {
            generation: self.generation,
            cost: 0,
            from: start,
        };
        let h = self.guess(start);
        self.open.push(Candidate {
            f: h,
            h,
            point: start,
        });
    }
    /// Continue the same search, expanding at most `budget` cells and taking
    /// what it expands off it. The passability/cost function must remain
    /// unchanged between slices. Output is filled only on success.
    pub fn advance(
        &mut self,
        budget: &mut usize,
        cost: impl Fn(Point) -> Option<u32>,
        output: &mut Vec<Point>,
    ) -> SearchStatus {
        let size = self.window.expect("begin before advance");
        let goals = self.goals.unwrap();
        while let Some(candidate) = self.open.peek() {
            let point = candidate.point;
            let visit = self.seen[size.index_of(point).unwrap()];
            if candidate.f - candidate.h != visit.cost {
                self.open.pop();
                continue;
            }
            if goals.as_slice().contains(&point) && (point == self.start || cost(point).is_some()) {
                self.open.pop();
                output.clear();
                let mut here = point;
                while here != self.start {
                    output.push(here);
                    here = self.seen[size.index_of(here).unwrap()].from;
                }
                output.reverse();
                return SearchStatus::Found(point);
            }
            if *budget == 0 {
                return SearchStatus::BudgetExhausted;
            }
            self.open.pop();
            *budget -= 1;
            #[cfg(test)]
            {
                self.expanded += 1;
            }
            for next in point.cardinal_neighbours() {
                let Some(index) = size.index_of(next) else {
                    continue;
                };
                let Some(step) = cost(next) else { continue };
                let total = visit.cost + step.max(MIN_WALK_COST as u32) as u64;
                let seen = self.seen[index];
                if seen.generation == self.generation && seen.cost <= total {
                    continue;
                }
                self.seen[index] = Visit {
                    generation: self.generation,
                    cost: total,
                    from: point,
                };
                let h = self.guess(next);
                self.open.push(Candidate {
                    f: total + h,
                    h,
                    point: next,
                });
            }
        }
        SearchStatus::Unreachable
    }

    #[cfg(test)]
    fn find(
        &mut self,
        window: impl Into<Bounds>,
        start: Point,
        goal: Point,
        limit: usize,
        cost: impl Fn(Point) -> Option<u32>,
    ) -> Option<Vec<Point>> {
        if start != goal && cost(goal).is_none() {
            return None;
        }
        self.begin(window.into(), start, Destinations::one(goal), MIN_WALK_COST as u32);
        let mut output = Vec::new();
        matches!(
            self.advance(&mut { limit }, cost, &mut output),
            SearchStatus::Found(_)
        )
        .then_some(output)
    }
}
thread_local! {
    static SCRATCH: std::cell::RefCell<PathFinder> = std::cell::RefCell::new(PathFinder::new());
}

/// Start or resume a static route, borrowing ordinary search scratch from
/// this thread, spending no more than `allowance` lets it. A budget-exhausted
/// search alone takes ownership of its finder, if the allowance lets it keep
/// one; completion returns its buffers to the thread, including after
/// migration.
///
/// The cheap answers come first and cost nothing: a goal the map's components
/// say is unreachable, or one that cannot be stood on, is refused before any
/// cell is expanded — so with nothing left to spend, an impossible route is
/// still `Unreachable` at once and a possible one waits (`BudgetExhausted`,
/// nothing started and nothing kept).
pub fn search(
    pending: &mut Option<Box<PathFinder>>,
    map: &Map,
    start: Point,
    goals: Destinations,
    allowance: &mut Allowance,
    cost: impl Fn(Point) -> Option<u32>,
    output: &mut Vec<Point>,
) -> SearchStatus {
    if pending.as_ref().is_some_and(|p| {
        p.revision != map.navigation_revision()
            || p.start != start
            || p.window != Some(window(map, start, goals.as_slice()))
    }) {
        recycle(pending);
    }
    if let Some(finder) = pending {
        let status = finder.advance(&mut allowance.cells, &cost, output);
        if status != SearchStatus::BudgetExhausted {
            recycle(pending);
        }
        return status;
    }
    let goals = Destinations::from_cells(goals.as_slice().iter().copied().filter(|&goal| {
        map.navigation().may_reach(start, goal) && (start == goal || cost(goal).is_some())
    }));
    if goals.first().is_none() {
        return SearchStatus::Unreachable;
    }
    if allowance.cells == 0 && !goals.as_slice().contains(&start) {
        return SearchStatus::BudgetExhausted;
    }
    let minimum = goals
        .as_slice()
        .iter()
        .map(|&g| map.navigation().minimum_cost(g))
        .min()
        .unwrap();
    SCRATCH.with(|slot| {
        let mut finder = slot.borrow_mut();
        finder.begin(window(map, start, goals.as_slice()), start, goals, minimum);
        finder.revision = map.navigation_revision();
        let status = finder.advance(&mut allowance.cells, cost, output);
        if status == SearchStatus::BudgetExhausted && allowance.may_keep {
            *pending = Some(Box::new(std::mem::take(&mut *finder)));
        }
        status
    })
}

/// A crowd repair uses this tick's occupancy only. It shares the stamped
/// buffers but never allocates an owned continuation for a transient snapshot.
pub fn search_local(
    map: &Map,
    start: Point,
    goal: Point,
    limit: usize,
    cost: impl Fn(Point) -> Option<u32>,
    output: &mut Vec<Point>,
) -> SearchStatus {
    if !map.navigation().may_reach(start, goal) || (start != goal && cost(goal).is_none()) {
        return SearchStatus::Unreachable;
    }
    SCRATCH.with(|slot| {
        let mut finder = slot.borrow_mut();
        finder.begin(
            window(map, start, &[goal]),
            start,
            Destinations::one(goal),
            map.navigation().minimum_cost(goal),
        );
        finder.advance(&mut { limit }, cost, output)
    })
}

/// Keep the largest useful scratch allocation when cancelling or completing.
pub fn recycle(pending: &mut Option<Box<PathFinder>>) {
    if let Some(mut finder) = pending.take() {
        SCRATCH.with(|slot| {
            let mut scratch = slot.borrow_mut();
            if finder.seen.capacity() > scratch.seen.capacity() {
                std::mem::swap(&mut *finder, &mut *scratch);
            }
        });
    }
}

#[cfg(test)]
pub fn find_path(
    window: impl Into<Bounds>,
    start: Point,
    goal: Point,
    limit: usize,
    cost: impl Fn(Point) -> Option<u32>,
) -> Option<Vec<Point>> {
    SCRATCH.with(|finder| finder.borrow_mut().find(window, start, goal, limit, cost))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{FLOOR, Map, Size, WALL};

    #[test]
    fn a_map_edit_discards_the_unfinished_search_instead_of_using_stale_parents() {
        let mut map = open(12, 1);
        let mut pending = None;
        let mut output = Vec::new();
        let goal = Point::new(11, 0);
        assert_eq!(
            search(
                &mut pending,
                &map,
                Point::ORIGIN,
                Destinations::one(goal),
                &mut Allowance::new(2),
                walkable(&map),
                &mut output
            ),
            SearchStatus::BudgetExhausted
        );
        map.set_terrain(Point::new(5, 0), WALL);
        assert_eq!(
            search(
                &mut pending,
                &map,
                Point::ORIGIN,
                Destinations::one(goal),
                &mut Allowance::new(100),
                walkable(&map),
                &mut output
            ),
            SearchStatus::Unreachable
        );
        assert!(pending.is_none());
        assert!(output.is_empty());
    }

    #[test]
    fn budget_exhaustion_resumes_the_same_search_and_matches_one_shot() {
        let road = crate::map::TerrainId::from_name("road").unwrap();
        let map = Map::new(Size::new(35, 35), road);
        let start = Point::ORIGIN;
        let goals = Destinations::one(Point::new(34, 34));
        let mut sliced = PathFinder::new();
        sliced.begin(map.site(), start, goals, 1);
        let mut output = Vec::new();
        assert_eq!(
            sliced.advance(&mut 7, walkable(&map), &mut output),
            SearchStatus::BudgetExhausted
        );
        assert_eq!(sliced.expanded, 7);
        assert!(output.is_empty());
        let mut slices = 1;
        loop {
            slices += 1;
            match sliced.advance(&mut 7, walkable(&map), &mut output) {
                SearchStatus::BudgetExhausted => assert!(slices < 1000),
                SearchStatus::Found(cell) => {
                    assert_eq!(cell, goals.first().unwrap());
                    break;
                }
                SearchStatus::Unreachable => panic!("open ground is reachable"),
            }
        }
        assert!(slices > 2);
        let mut whole = PathFinder::new();
        let expected = whole
            .find(
                map.size(),
                start,
                goals.first().unwrap(),
                100_000,
                walkable(&map),
            )
            .unwrap();
        assert_eq!(output, expected);
        assert_eq!(
            sliced.expanded, whole.expanded,
            "no expansions replayed between slices"
        );
    }

    #[test]
    fn connectivity_rejects_a_sealed_room_without_expanding_or_asking_costs() {
        let mut map = open(300, 300);
        for y in 0..300 {
            map.set_terrain(Point::new(150, y), WALL);
        }
        let calls = std::cell::Cell::new(0);
        let mut pending = None;
        let mut output = Vec::new();
        assert_eq!(
            search(
                &mut pending,
                &map,
                Point::new(148, 1),
                Destinations::one(Point::new(152, 1)),
                &mut Allowance::new(1),
                |c| {
                    calls.set(calls.get() + 1);
                    map.is_passable(c).then_some(1)
                },
                &mut output
            ),
            SearchStatus::Unreachable
        );
        assert_eq!(calls.get(), 0);
        assert!(pending.is_none());
    }

    #[test]
    fn uniform_expensive_ground_fits_the_normal_budget() {
        for name in ["grass", "road"] {
            let map = Map::new(
                Size::new(300, 300),
                crate::map::TerrainId::from_name(name).unwrap(),
            );
            let mut pending = None;
            let mut output = Vec::new();
            let goal = Point::new(299, 299);
            assert_eq!(
                search(
                    &mut pending,
                    &map,
                    Point::ORIGIN,
                    Destinations::one(goal),
                    &mut Allowance::new(crate::sim::walker::FAR_LIMIT),
                    walkable(&map),
                    &mut output
                ),
                SearchStatus::Found(goal)
            );
            assert_eq!(output.len(), 598);
            assert!(pending.is_none());
        }
    }

    #[test]
    fn generations_wrap_and_buffers_survive_repeated_searches() {
        let map = open(8, 8);
        let mut finder = PathFinder::new();
        finder
            .find(
                map.size(),
                Point::ORIGIN,
                Point::new(7, 7),
                100,
                walkable(&map),
            )
            .unwrap();
        let slots = finder.seen.as_ptr();
        finder.generation = u32::MAX;
        let route = finder
            .find(
                map.size(),
                Point::new(7, 7),
                Point::ORIGIN,
                100,
                walkable(&map),
            )
            .unwrap();
        assert_eq!(finder.generation, 1);
        assert_eq!(finder.seen.as_ptr(), slots);
        assert!(is_a_walk(Point::new(7, 7), Point::ORIGIN, &route));
    }

    #[test]
    fn an_unfinished_search_can_move_to_another_worker_without_changing_its_route() {
        let map = open(40, 40);
        let mut pending = None;
        let mut output = Vec::new();
        let goal = Point::new(39, 39);
        assert_eq!(
            search(
                &mut pending,
                &map,
                Point::ORIGIN,
                Destinations::one(goal),
                &mut Allowance::new(1),
                walkable(&map),
                &mut output
            ),
            SearchStatus::BudgetExhausted
        );
        let expected = route(&map, Point::ORIGIN, goal).unwrap();
        let actual = std::thread::spawn(move || {
            let status = search(
                &mut pending,
                &map,
                Point::ORIGIN,
                Destinations::one(goal),
                &mut Allowance::new(1000),
                walkable(&map),
                &mut output,
            );
            assert_eq!(status, SearchStatus::Found(goal));
            assert!(pending.is_none());
            output
        })
        .join()
        .unwrap();
        assert_eq!(actual, expected);
    }

    /// The route budget's promise, at the bottom: however the cells are
    /// handed out, the route is the one an unlimited search finds, and the
    /// allowance is charged exactly what was expanded.
    #[test]
    fn a_search_paid_a_few_cells_at_a_time_finds_the_route_an_unlimited_one_does() {
        let mut map = open(30, 30);
        for y in 0..28 {
            map.set_terrain(Point::new(15, y), WALL);
        }
        let (start, goal) = (Point::new(2, 2), Point::new(28, 2));
        let expected = route(&map, start, goal).unwrap();

        let (mut pending, mut output) = (None, Vec::new());
        let mut spent = 0;
        let mut slices = 0;
        loop {
            slices += 1;
            let mut allowance = Allowance::new(9);
            let status = search(&mut pending, &map, start, Destinations::one(goal), &mut allowance, walkable(&map), &mut output);
            assert!(allowance.cells <= 9);
            spent += 9 - allowance.cells;
            match status {
                SearchStatus::BudgetExhausted => assert_eq!(allowance.cells, 0, "ran out, so spent it all"),
                SearchStatus::Found(_) => break,
                SearchStatus::Unreachable => panic!("round the end of the wall"),
            }
        }
        assert!(slices > 20, "the wall makes it a long search: {slices} slices");
        assert_eq!(output, expected);

        let mut whole = PathFinder::new();
        whole.find(map.size(), start, goal, usize::MAX, walkable(&map)).unwrap();
        assert_eq!(spent, whole.expanded, "charged for exactly what was expanded");
    }

    #[test]
    fn a_search_that_may_not_keep_its_work_lets_it_go_and_starts_again_the_same() {
        let mut map = open(30, 30);
        for y in 0..28 {
            map.set_terrain(Point::new(15, y), WALL);
        }
        let (start, goal) = (Point::new(2, 2), Point::new(28, 2));
        let (mut pending, mut output) = (None, Vec::new());
        let mut trial = Allowance { cells: 50, may_keep: false };
        let status = search(&mut pending, &map, start, Destinations::one(goal), &mut trial, walkable(&map), &mut output);
        assert_eq!(status, SearchStatus::BudgetExhausted);
        assert!(pending.is_none(), "nothing kept");
        let status = search(&mut pending, &map, start, Destinations::one(goal), &mut Allowance::new(usize::MAX), walkable(&map), &mut output);
        assert_eq!(status, SearchStatus::Found(goal));
        assert_eq!(output, route(&map, start, goal).unwrap());
    }

    #[test]
    fn with_nothing_to_spend_a_miss_is_still_a_miss_and_a_route_waits() {
        let mut map = open(12, 5);
        for y in 0..5 {
            map.set_terrain(Point::new(6, y), WALL);
        }
        let (mut pending, mut output) = (None, Vec::new());
        let mut nothing = Allowance::new(0);
        let sealed = search(&mut pending, &map, Point::new(1, 1), Destinations::one(Point::new(9, 1)), &mut nothing, walkable(&map), &mut output);
        assert_eq!(sealed, SearchStatus::Unreachable, "the components say so for free");
        let open_side = search(&mut pending, &map, Point::new(1, 1), Destinations::one(Point::new(4, 3)), &mut nothing, walkable(&map), &mut output);
        assert_eq!(open_side, SearchStatus::BudgetExhausted);
        assert!(pending.is_none(), "nothing was started, so nothing is held");
        let here = search(&mut pending, &map, Point::new(1, 1), Destinations::one(Point::new(1, 1)), &mut nothing, walkable(&map), &mut output);
        assert_eq!(here, SearchStatus::Found(Point::new(1, 1)), "already there costs nothing");
    }

    #[test]
    fn repairing_a_long_route_keeps_its_far_buffer_and_handles_nested_repairs() {
        let mut path = Path::new((1..=1000).map(|x| Point::new(x, 0)).collect());
        let far = path.steps.as_ptr();
        let mut repair = vec![
            Point::new(0, 1),
            Point::new(1, 1),
            Point::new(2, 1),
            Point::new(3, 1),
            Point::new(4, 1),
            Point::new(5, 1),
            Point::new(5, 0),
        ];
        path.splice(5, &mut repair);
        assert_eq!(path.steps.as_ptr(), far);
        path.advance();
        repair.extend([Point::new(0, 2), Point::new(1, 2), Point::new(1, 1)]);
        path.splice(1, &mut repair);
        assert_eq!(path.steps.as_ptr(), far);
        assert_eq!(
            path.remaining().take(4).collect::<Vec<_>>(),
            [
                Point::new(0, 2),
                Point::new(1, 2),
                Point::new(1, 1),
                Point::new(2, 1)
            ]
        );
        assert_eq!(path.destination(), Some(Point::new(1000, 0)));
        while !path.is_done() {
            path.advance();
        }
        assert!(path.current().is_none());
    }

    const PLENTY: usize = 100_000;

    fn open(width: i32, height: i32) -> Map {
        Map::new(Size::new(width, height), FLOOR)
    }

    /// What the walker's far stage asks: the map's passability and its
    /// terrain's costs, and nothing else.
    fn walkable(map: &Map) -> impl Fn(Point) -> Option<u32> + '_ {
        |cell| map.is_passable(cell).then(|| map.walk_cost(cell) as u32)
    }

    fn route(map: &Map, from: Point, to: Point) -> Option<Vec<Point>> {
        find_path(map.size(), from, to, PLENTY, walkable(map))
    }

    fn tile(name: &str) -> crate::map::TerrainId {
        crate::map::TerrainId::from_name(name).unwrap()
    }

    /// What walking `steps` costs.
    fn cost_of(map: &Map, steps: &[Point]) -> u32 {
        steps.iter().map(|&cell| map.walk_cost(cell) as u32).sum()
    }

    /// The cheapest cost from `from` to every cell, the slow and obvious way:
    /// relax every cell until nothing changes. What A* has to agree with.
    fn cheapest_by_brute_force(map: &Map, from: Point, to: Point) -> Option<u32> {
        let size = map.size();
        let mut best = vec![u32::MAX; size.area()];
        best[size.index_of(from).unwrap()] = 0;
        loop {
            let mut changed = false;
            for cell in size.points() {
                let here = best[size.index_of(cell).unwrap()];
                if here == u32::MAX {
                    continue;
                }
                for next in cell.cardinal_neighbours() {
                    let Some(index) = size.index_of(next) else {
                        continue;
                    };
                    if !map.is_passable(next) {
                        continue;
                    }
                    let through = here + map.walk_cost(next) as u32;
                    if through < best[index] {
                        best[index] = through;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let cost = best[size.index_of(to).unwrap()];
        (cost != u32::MAX).then_some(cost)
    }

    /// Every cell is a cardinal step from the one before, the first is a step
    /// from `start`, and the last is `goal`.
    fn is_a_walk(start: Point, goal: Point, steps: &[Point]) -> bool {
        let mut here = start;
        for &step in steps {
            if !Point::CARDINALS.contains(&(step - here)) {
                return false;
            }
            here = step;
        }
        here == goal
    }

    #[test]
    fn a_path_across_an_empty_room_is_as_long_as_the_manhattan_distance() {
        let map = open(10, 10);
        let (from, to) = (Point::new(1, 1), Point::new(8, 6));
        let steps = route(&map, from, to).expect("an open room has a route");

        assert_eq!(steps.len() as i32, from.manhattan_distance(to));
        assert!(is_a_walk(from, to, &steps));
    }

    #[test]
    fn the_start_is_not_in_the_path_and_the_goal_is() {
        let map = open(5, 5);
        let (from, to) = (Point::new(0, 0), Point::new(3, 0));
        let steps = route(&map, from, to).unwrap();

        assert!(!steps.contains(&from));
        assert_eq!(steps.last(), Some(&to));
    }

    #[test]
    fn going_nowhere_is_an_empty_path_and_not_a_failure() {
        let map = open(4, 4);
        let here = Point::new(2, 2);
        assert_eq!(route(&map, here, here), Some(Vec::new()));
    }

    #[test]
    fn a_path_goes_round_a_wall_rather_than_through_it() {
        // A wall down the middle of a 9x9 room, with a gap at the top.
        let mut map = open(9, 9);
        for y in 0..8 {
            map.set_terrain(Point::new(4, y), WALL);
        }
        let (from, to) = (Point::new(1, 1), Point::new(7, 1));
        let steps = route(&map, from, to).expect("the gap is a route");

        assert!(is_a_walk(from, to, &steps));
        assert!(steps.iter().all(|&cell| map.is_passable(cell)));
        // Through the gap at the top, so far longer than the straight line.
        assert!(steps.len() as i32 > from.manhattan_distance(to));
    }

    #[test]
    fn a_walled_off_goal_has_no_path() {
        let mut map = open(9, 9);
        for y in 0..9 {
            map.set_terrain(Point::new(4, y), WALL);
        }
        assert_eq!(route(&map, Point::new(1, 1), Point::new(7, 1)), None);
    }

    #[test]
    fn a_goal_in_a_wall_has_no_path() {
        let mut map = open(5, 5);
        let wall = Point::new(2, 2);
        map.set_terrain(wall, WALL);
        assert_eq!(route(&map, Point::new(0, 0), wall), None);
    }

    #[test]
    fn a_goal_off_the_map_has_no_path() {
        let map = open(5, 5);
        assert_eq!(route(&map, Point::new(0, 0), Point::new(9, 9)), None);
    }

    #[test]
    fn something_standing_in_a_wall_can_still_walk_out_of_it() {
        // `start` is never asked about: an entity spawned inside a wall has to
        // be able to leave, and refusing to plan is how it never would.
        let mut map = open(5, 5);
        let wall = Point::new(2, 2);
        map.set_terrain(wall, WALL);

        let to = Point::new(4, 4);
        let steps = route(&map, wall, to).expect("out of the wall and away");
        assert!(is_a_walk(wall, to, &steps));
    }

    #[test]
    fn the_budget_is_a_wall_and_not_a_suggestion() {
        // A goal sealed off in a big room: the search has to flood everything
        // it can reach before it can say no, and the limit is what stops it.
        let mut map = open(60, 60);
        for y in 0..60 {
            map.set_terrain(Point::new(30, y), WALL);
        }
        let (from, to) = (Point::new(1, 1), Point::new(50, 50));

        assert_eq!(find_path(map.size(), from, to, 16, walkable(&map)), None);
        assert_eq!(
            find_path(map.size(), from, to, PLENTY, walkable(&map)),
            None
        );

        // ...and a route that fits in the budget still comes back.
        let near = Point::new(4, 1);
        assert!(find_path(map.size(), from, near, 16, walkable(&map)).is_some());
    }

    #[test]
    fn the_same_search_gives_the_same_path_every_time() {
        // An open floor is the case with the most ties to break: every route
        // of the right length is optimal, and the choice between them has to
        // be the same on every run or the crowd stops being reproducible.
        let map = open(20, 20);
        let (from, to) = (Point::new(2, 3), Point::new(17, 16));
        let first = route(&map, from, to).unwrap();
        for _ in 0..8 {
            assert_eq!(route(&map, from, to), Some(first.clone()));
        }
    }

    #[test]
    fn a_finder_reused_does_not_remember_the_last_search() {
        let map = open(12, 12);
        let mut finder = PathFinder::new();
        let long = finder
            .find(
                map.size(),
                Point::new(0, 0),
                Point::new(11, 11),
                PLENTY,
                walkable(&map),
            )
            .unwrap();
        let short = finder
            .find(
                map.size(),
                Point::new(0, 0),
                Point::new(0, 1),
                PLENTY,
                walkable(&map),
            )
            .unwrap();

        assert_eq!(long.len(), 22);
        assert_eq!(short, vec![Point::new(0, 1)]);
    }

    #[test]
    fn the_predicate_is_what_blocks_and_not_the_map() {
        // The near stage's whole trick: the same terrain, one extra cell
        // treated as blocked, a different route.
        let map = open(3, 3);
        let (from, to) = (Point::new(0, 1), Point::new(2, 1));
        let occupied = Point::new(1, 1);

        assert_eq!(route(&map, from, to), Some(vec![occupied, to]));

        let terrain = walkable(&map);
        let round = find_path(map.size(), from, to, PLENTY, |c| {
            if c == occupied { None } else { terrain(c) }
        })
        .expect("round the outside");
        assert!(!round.contains(&occupied));
        assert!(is_a_walk(from, to, &round));
    }

    // --- costs ---

    /// A 9x5 floor with a strip of `name` across the middle row's left part,
    /// the start and the goal at either end of that row.
    fn strip_across(name: &str, length: i32) -> (Map, Point, Point) {
        let mut map = open(length + 2, 5);
        for x in 1..=length {
            map.set_terrain(Point::new(x, 2), tile(name));
        }
        (map, Point::new(0, 2), Point::new(length + 1, 2))
    }

    #[test]
    fn a_route_steps_off_dear_ground_when_going_round_is_cheaper() {
        // Seven cells of road straight ahead cost 35; one row up the same
        // seven cells of floor and two steps to get there and back cost 11.
        let (map, from, to) = strip_across("road", 7);
        let steps = route(&map, from, to).unwrap();

        assert!(is_a_walk(from, to, &steps));
        assert!(
            steps.iter().all(|&cell| map.walk_cost(cell) == 1),
            "walked the road: {steps:?}"
        );
        assert_eq!(cost_of(&map, &steps), 7 + 2 + 1);
    }

    #[test]
    fn a_route_crosses_dear_ground_when_there_is_no_cheaper_way() {
        // The road fills a whole column between two walls of floor: the only
        // way over is across it, and a crossing goes straight over.
        let mut map = open(7, 7);
        for y in 0..7 {
            map.set_terrain(Point::new(3, y), tile("road"));
        }
        let (from, to) = (Point::new(0, 3), Point::new(6, 3));
        let steps = route(&map, from, to).unwrap();

        let on_road = steps
            .iter()
            .filter(|&&cell| map.walk_cost(cell) > 1)
            .count();
        assert_eq!(on_road, 1, "one cell of road and no more: {steps:?}");
        assert_eq!(steps.len() as i32, from.manhattan_distance(to));
    }

    #[test]
    fn a_short_stretch_of_dear_ground_is_walked_rather_than_gone_round() {
        // One cell of grass costs 3, and with a post either side of it going
        // round is six steps of floor. Cheapest, not "never on grass".
        let (mut map, from, to) = strip_across("grass", 1);
        map.set_terrain(Point::new(1, 1), WALL);
        map.set_terrain(Point::new(1, 3), WALL);
        let steps = route(&map, from, to).unwrap();
        assert_eq!(steps, vec![Point::new(1, 2), to]);
    }

    #[test]
    fn a_weighted_route_costs_what_the_brute_force_cheapest_costs() {
        // A* with a heuristic that ever overestimated would come back with a
        // route that is fine to walk and quietly not the cheapest. Pseudo-random
        // mazes of every ground there is, checked against relaxing the grid.
        let names = ["floor", "pavement", "grass", "road", "wall brown"];
        for seed in 0..24u64 {
            let mut map = open(14, 11);
            let mut state = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            for cell in map.size().points() {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                map.set_terrain(cell, tile(names[(state >> 33) as usize % names.len()]));
            }
            let (from, to) = (Point::new(0, 0), Point::new(13, 10));
            map.set_terrain(from, FLOOR);
            map.set_terrain(to, FLOOR);

            let expected = cheapest_by_brute_force(&map, from, to);
            let found = route(&map, from, to);
            assert_eq!(
                found.as_ref().map(|steps| cost_of(&map, steps)),
                expected,
                "seed {seed}"
            );
            if let Some(steps) = found {
                assert!(is_a_walk(from, to, &steps), "seed {seed}");
            }
        }
    }

    #[test]
    fn the_same_weighted_search_gives_the_same_path_every_time() {
        let mut map = open(20, 20);
        for x in 0..20 {
            for y in 8..12 {
                map.set_terrain(Point::new(x, y), tile("road"));
            }
        }
        let (from, to) = (Point::new(2, 3), Point::new(17, 16));
        let first = route(&map, from, to).unwrap();
        for _ in 0..8 {
            assert_eq!(route(&map, from, to), Some(first.clone()));
        }
    }

    // --- the path itself ---

    #[test]
    fn walking_a_path_reaches_the_end_and_stops() {
        let mut path = Path::new(vec![Point::new(1, 0), Point::new(2, 0)]);
        assert_eq!(path.current(), Some(Point::new(1, 0)));
        assert_eq!(path.destination(), Some(Point::new(2, 0)));

        path.advance();
        assert_eq!(path.current(), Some(Point::new(2, 0)));
        assert!(!path.is_done());

        path.advance();
        assert_eq!(path.current(), None);
        assert!(path.is_done());
        assert!(path.is_done());
    }

    #[test]
    fn an_empty_path_is_already_done() {
        assert!(Path::none().is_done());
        assert_eq!(Path::none().current(), None);
        assert_eq!(Path::none().destination(), None);
    }

    #[test]
    fn a_splice_replaces_the_next_few_cells_and_keeps_the_rest() {
        let cells: Vec<Point> = (1..=6).map(|x| Point::new(x, 0)).collect();
        let mut path = Path::new(cells);
        path.advance(); // walked (1, 0); (2, 0) is next

        let detour = vec![Point::new(2, 1), Point::new(3, 1), Point::new(4, 0)];
        path.splice(3, &mut detour.clone());

        // The three replaced cells are gone, the detour is in their place, and
        // (5, 0) and (6, 0) still follow.
        assert_eq!(
            path.remaining().collect::<Vec<_>>(),
            [detour, vec![Point::new(5, 0), Point::new(6, 0)]].concat()
        );
        assert_eq!(path.destination(), Some(Point::new(6, 0)));
    }

    #[test]
    fn splicing_more_cells_than_are_left_replaces_the_whole_tail() {
        let mut path = Path::new(vec![Point::new(1, 0), Point::new(2, 0)]);
        let detour = vec![Point::new(1, 1), Point::new(2, 1), Point::new(2, 0)];
        path.splice(50, &mut detour.clone());
        assert_eq!(path.remaining().collect::<Vec<_>>(), detour);
    }

    /// A wall across the whole site, on an endless map: the way round is out
    /// through the wasteland.
    fn walled_across() -> Map {
        let mut map = open(9, 6);
        for y in 0..6 {
            map.set_terrain(Point::new(4, y), WALL);
        }
        map.with_wasteland(1)
    }

    fn search_once(map: &Map, start: Point, goal: Point) -> (SearchStatus, Vec<Point>) {
        let mut pending = None;
        let mut output = Vec::new();
        let status = search(
            &mut pending,
            map,
            start,
            Destinations::one(goal),
            &mut Allowance::new(crate::sim::walker::FAR_LIMIT),
            walkable(map),
            &mut output,
        );
        assert!(pending.is_none(), "a windowed search never runs out of budget");
        (status, output)
    }

    #[test]
    fn on_an_endless_map_a_route_goes_out_round_the_site() {
        let map = walled_across();
        let (from, to) = (Point::new(1, 2), Point::new(7, 2));
        let (status, route) = search_once(&map, from, to);
        assert_eq!(status, SearchStatus::Found(to));
        assert!(is_a_walk(from, to, &route));
        assert!(route.iter().any(|&cell| !map.contains(cell)), "it left the site");
        assert!(route.iter().all(|&cell| map.is_passable(cell)));
        // ...and on the same map with nothing round it there is no way.
        let mut bounded = walled_across();
        bounded.set_wasteland(None);
        assert_eq!(search_once(&bounded, from, to).0, SearchStatus::Unreachable);
    }

    #[test]
    fn a_route_far_out_in_the_wasteland_is_searched_in_a_window_of_its_own() {
        let map = walled_across();
        let from = Point::new(5000, -3000);
        let to = from.offset(25, 10);
        let window = window(&map, from, &[to]);
        assert!(!window.overlaps(map.navigation_area()));
        assert!(window.area() < 100 * 100, "{window:?}");
        // The far wasteland may not be dry ground just there: walk from the
        // nearest dry cells.
        let dry = |near: Point| {
            Bounds::new(near.offset(-8, -8), crate::map::Size::new(17, 17))
                .points()
                .find(|&cell| map.is_passable(cell))
                .unwrap()
        };
        let (from, to) = (dry(from), dry(to));
        let (status, route) = search_once(&map, from, to);
        if status == SearchStatus::Found(to) {
            assert!(is_a_walk(from, to, &route));
        } else {
            assert_eq!(status, SearchStatus::Unreachable, "a lake between them, at worst");
        }
    }

    #[test]
    fn water_is_never_a_destination_and_never_floods_the_world() {
        let map = walled_across();
        let water = Bounds::new(Point::new(-2000, -2000), crate::map::Size::new(400, 400))
            .points()
            .find(|&cell| map.terrain(cell) == Some(crate::map::WATER))
            .expect("a lake somewhere in a big square");
        assert_eq!(search_once(&map, Point::new(1, 1), water).0, SearchStatus::Unreachable);
    }
}
