//! A*, and the route it produces.
//!
//! Plain Rust like the rest of `src/sim/`, and deliberately ignorant of what
//! is walking the path: it is handed a start, a goal, a budget and a predicate
//! that says whether a cell can be stood in. Which passability that predicate
//! consults is the caller's business, and it is the whole of the difference
//! between this module's two customers — see [`crate::sim::kinds::Walker`],
//! which asks twice with two different predicates.
//!
//! # Orthogonal, and only orthogonal
//!
//! A search step is one of [`Point::CARDINALS`]. Nothing here expands a
//! diagonal, so every cost is a whole number of steps and
//! [`Point::manhattan_distance`] is an exact, admissible, consistent
//! heuristic — which is what lets the closed set be a plain "already seen this
//! cheaply" test with no reopening.
//!
//! **Movement is still diagonal.** A walker aims at the centre of the next
//! cell in its path and moves on the moment it is inside that cell, so it
//! leaves for the next one before reaching the middle and the line it actually
//! walks cuts the corner. That is intended: the grid is what the *search* is
//! 4-connected over, not what a body may travel along. A straight segment from
//! anywhere inside one cell to the centre of an edge-adjacent one stays inside
//! those two cells, so cutting a corner can never skip a cell the search
//! vetted.
//!
//! # The budget is a count of expansions
//!
//! [`PathFinder::find`] gives up after `limit` cells have been popped and
//! returns `None`. Unreachable is the case that costs: a goal behind a sealed
//! wall makes A* flood every cell it *can* reach before it knows, and that
//! bill is paid per entity per tick by whoever asks. The limit is what turns
//! "this map has a walled-off room in it" from a frame-rate cliff into a
//! bounded miss, and callers treat a miss the same way they treat no route.
//!
//! # Ties are broken on coordinates, not on insertion order
//!
//! Two cells with the same `f` come off the heap in the order `Candidate`'s
//! `Ord` gives, which is a total order over `(f, h, point)` and has nothing to
//! do with when they went in. A binary heap promises nothing about equal
//! elements, and "the sequence of pushes happens to be the same" is not what
//! determinism should rest on when determinism is a test this crate runs for a
//! thousand ticks.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::map::Point;

/// A route: the cells to walk through, and how far along it we are.
///
/// The start cell is **not** in the steps — a path is what is left to do, and
/// the cell you are standing in is not. The destination is the last entry.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Path {
    steps: Vec<Point>,
    /// Index of the cell currently being walked to.
    next: usize,
}

impl Path {
    /// An empty path: nothing left to walk.
    pub fn none() -> Path {
        Path::default()
    }

    pub fn new(steps: Vec<Point>) -> Path {
        Path { steps, next: 0 }
    }

    /// The cell being walked to right now, or `None` when the path is done.
    pub fn current(&self) -> Option<Point> {
        self.steps.get(self.next).copied()
    }

    /// Where this path ends up, whether or not it has been started.
    pub fn destination(&self) -> Option<Point> {
        self.steps.last().copied()
    }

    /// Everything still to be walked, current step first.
    pub fn remaining(&self) -> &[Point] {
        &self.steps[self.next.min(self.steps.len())..]
    }

    pub fn is_done(&self) -> bool {
        self.next >= self.steps.len()
    }

    /// One cell reached; move on to the next.
    pub fn advance(&mut self) {
        self.next += 1;
    }

    pub fn clear(&mut self) {
        self.steps.clear();
        self.next = 0;
    }

    /// Replace the next `count` remaining cells with `detour`.
    ///
    /// The already-walked prefix is dropped rather than kept: it is history,
    /// and keeping it would mean preserving `next`, which is exactly the index
    /// the splice invalidates. What comes back starts at `detour` and runs to
    /// the same destination.
    pub fn splice(&mut self, count: usize, detour: Vec<Point>) {
        let tail = self.next.saturating_add(count).min(self.steps.len());
        let mut steps = detour;
        steps.extend_from_slice(&self.steps[tail..]);
        *self = Path::new(steps);
    }
}

/// A cell waiting to be expanded.
///
/// Ordered so a `BinaryHeap` — a *max* heap — pops the smallest `f` first:
/// every comparison below is written backwards on purpose. `h` breaks a tie
/// towards the goal, which is what stops A* fanning out across an open floor,
/// and the point itself breaks what is left so the order is total. See the
/// module docs for why that last one matters.
#[derive(PartialEq, Eq)]
struct Candidate {
    f: i32,
    h: i32,
    point: Point,
}

impl Ord for Candidate {
    fn cmp(&self, other: &Candidate) -> Ordering {
        other
            .f
            .cmp(&self.f)
            .then_with(|| other.h.cmp(&self.h))
            .then_with(|| other.point.cmp(&self.point))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Candidate) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// How a cell was reached, and how cheaply.
#[derive(Clone, Copy)]
struct Visit {
    /// Steps from the start.
    cost: i32,
    from: Point,
}

/// A* with its working memory kept between searches.
///
/// The scratch is why this is a struct and not a free function: a search over
/// a big map touches thousands of cells, and allocating and freeing that table
/// and heap on every call is most of the cost of a short path. [`find_path`]
/// is the free-function door, over one of these per thread.
#[derive(Default)]
pub struct PathFinder {
    open: BinaryHeap<Candidate>,
    seen: HashMap<Point, Visit>,
    trace: Vec<Point>,
}

impl PathFinder {
    pub fn new() -> PathFinder {
        PathFinder::default()
    }

    /// The shortest orthogonal route from `start` to `goal`, or `None`.
    ///
    /// The returned cells **exclude `start` and include `goal`**, so the
    /// result is a list of moves and not a list of places. `start == goal` is
    /// an empty route rather than a failure: there is nothing to walk, which
    /// is a different answer from "you cannot get there".
    ///
    /// `passable` is asked about every cell except `start`. A walker standing
    /// somewhere it could not have walked into — spawned inside a wall — can
    /// therefore still find its way out, which is the only way it ever will.
    ///
    /// `None` means no route *within the budget*: unreachable, or reachable
    /// only after more than `limit` expansions. The two are deliberately the
    /// same answer, because a caller can do nothing different about them.
    pub fn find(
        &mut self,
        start: Point,
        goal: Point,
        limit: usize,
        passable: impl Fn(Point) -> bool,
    ) -> Option<Vec<Point>> {
        if start == goal {
            return Some(Vec::new());
        }
        // Cheaper than discovering it by flooding the whole reachable region.
        if !passable(goal) {
            return None;
        }

        self.open.clear();
        self.seen.clear();

        let heuristic = start.manhattan_distance(goal);
        self.seen.insert(
            start,
            Visit {
                cost: 0,
                from: start,
            },
        );
        self.open.push(Candidate {
            f: heuristic,
            h: heuristic,
            point: start,
        });

        let mut expanded = 0usize;
        while let Some(candidate) = self.open.pop() {
            let point = candidate.point;
            let cost = self.seen[&point].cost;

            // A stale heap entry: this cell was reached again more cheaply
            // after it was pushed. Dropping it here is what stands in for the
            // decrease-key a `BinaryHeap` does not have.
            if candidate.f - candidate.h != cost {
                continue;
            }
            if point == goal {
                return Some(self.unwind(start, goal));
            }

            expanded += 1;
            if expanded > limit {
                return None;
            }

            for step in Point::CARDINALS {
                let next = point + step;
                let cost = cost + 1;
                // The heuristic is consistent, so a cell already reached this
                // cheaply can never be improved and needs no reopening.
                if self.seen.get(&next).is_some_and(|seen| seen.cost <= cost) {
                    continue;
                }
                if !passable(next) {
                    continue;
                }
                self.seen.insert(next, Visit { cost, from: point });
                let h = next.manhattan_distance(goal);
                self.open.push(Candidate {
                    f: cost + h,
                    h,
                    point: next,
                });
            }
        }
        None
    }

    /// Walk the parent links back from `goal` and hand the route out forwards.
    fn unwind(&mut self, start: Point, goal: Point) -> Vec<Point> {
        self.trace.clear();
        let mut point = goal;
        while point != start {
            self.trace.push(point);
            point = self.seen[&point].from;
        }
        self.trace.reverse();
        // A fresh `Vec`, sized exactly: the route outlives this call and the
        // scratch does not.
        self.trace.clone()
    }
}

thread_local! {
    /// One [`PathFinder`]'s scratch per thread, reused by [`find_path`].
    ///
    /// The reaction round runs across threads and every entity in it may want
    /// a search, so the alternatives were a `&mut PathFinder` threaded through
    /// [`crate::sim::GameEntity::react`] — a parameter every kind would carry
    /// and most would ignore — or an allocation per search. This is neither,
    /// and it changes no result: the scratch is cleared at the top of every
    /// `find`, so which thread ran a search is not observable in what it
    /// returns.
    static SCRATCH: std::cell::RefCell<PathFinder> = std::cell::RefCell::new(PathFinder::new());
}

/// [`PathFinder::find`], over a scratch buffer reused per thread.
pub fn find_path(
    start: Point,
    goal: Point,
    limit: usize,
    passable: impl Fn(Point) -> bool,
) -> Option<Vec<Point>> {
    SCRATCH.with(|finder| finder.borrow_mut().find(start, goal, limit, passable))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR, WALL};

    const PLENTY: usize = 100_000;

    fn open(width: i32, height: i32) -> Map {
        Map::new(Size::new(width, height), FLOOR)
    }

    fn route(map: &Map, from: Point, to: Point) -> Option<Vec<Point>> {
        find_path(from, to, PLENTY, |cell| map.is_passable(cell))
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

        assert_eq!(find_path(from, to, 16, |c| map.is_passable(c)), None);
        assert_eq!(find_path(from, to, PLENTY, |c| map.is_passable(c)), None);

        // ...and a route that fits in the budget still comes back.
        let near = Point::new(4, 1);
        assert!(find_path(from, near, 16, |c| map.is_passable(c)).is_some());
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
            .find(Point::new(0, 0), Point::new(11, 11), PLENTY, |c| {
                map.is_passable(c)
            })
            .unwrap();
        let short = finder
            .find(Point::new(0, 0), Point::new(0, 1), PLENTY, |c| {
                map.is_passable(c)
            })
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

        let round = find_path(from, to, PLENTY, |c| c != occupied && map.is_passable(c))
            .expect("round the outside");
        assert!(!round.contains(&occupied));
        assert!(is_a_walk(from, to, &round));
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
        assert!(path.remaining().is_empty());
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
        path.splice(3, detour.clone());

        // The three replaced cells are gone, the detour is in their place, and
        // (5, 0) and (6, 0) still follow.
        assert_eq!(
            path.remaining(),
            [detour, vec![Point::new(5, 0), Point::new(6, 0)]].concat()
        );
        assert_eq!(path.destination(), Some(Point::new(6, 0)));
    }

    #[test]
    fn splicing_more_cells_than_are_left_replaces_the_whole_tail() {
        let mut path = Path::new(vec![Point::new(1, 0), Point::new(2, 0)]);
        let detour = vec![Point::new(1, 1), Point::new(2, 1), Point::new(2, 0)];
        path.splice(50, detour.clone());
        assert_eq!(path.remaining(), detour);
    }
}
