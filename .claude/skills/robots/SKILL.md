---
name: robots
description: Robots in crowd2x - the fourth kind of entity (EntityType::Robot 4, sim::kinds::Robot, sim::robots), their Battery (charge drains over 20 world hours, condition wears over 8 days, breakdowns below 60% - 70% stop, 30% explode), the robot courier job (robotic, unpaid, round the clock, joins the courier dispatch), charging stations (3 a district, 4 charging docks each, map::district::charging_station, ChargeGoal, Task::charge), the robot factory (map::district::robot_factory: workbenches, assembly bay, parts rack), engineers (3, EngineerGoal: overhaul, build, tinker), the electrician's field repair (Effect::FixRobot), Effect::Explode and its blast (game::explosions), the robots' book (Robots: stopped, fixes, waiting, works), saving, the find menu and the tests. Use when changing robots, charging, breakdowns, explosions, engineers, the robot factory or the charging stations.
---

# Robots

A robot looks like a person to give work to but has a mind of its own: its only need is
**charge**, and it wears out (**condition**). It never sleeps, walks at one steady pace
(`ROBOT_SPEED`, 0.8 of a person's, no pace roll, never hurries or panics), carries 4x
(`Capacity::ROBOT`), and deathdogs ignore it (`is_deathdog_prey` is Human|Dog). Constants all
live at the top of `src/sim/robots.rs`.

| Concern | Where |
| --- | --- |
| Kind, body, react | `sim/kinds.rs` `Robot` (`EntityType::Robot = 4` in `uid.rs`) |
| Battery, `Robots` book, blast falloff, constants | `sim/robots.rs` |
| Brain | `Brain::robot()`; `RobotRoutine` in `brain/routines.rs` raises Charge (≤40 idle, ≤15 takes over, holds to 95) and Service (condition ≤10) |
| Charging | `goals/charge.rs` `ChargeGoal` (nearest powered `Charger` with a free side), `tasks/charge.rs` |
| Overhaul at the bay | `goals/service.rs`, robot says `Effect::AwaitService`; engineer's `tasks/overhaul.rs` sets condition 100 |
| Engineers | `JobKind::Engineer`, `goals/engineer.rs`: overhaul a waiting robot, else build (steel from the warehouse's rack to the parts rack, then `tasks/assemble.rs`, 8 hours), else tinker at the workbench |
| Field repair | electricians' `RepairGoal` also targets `robots.stopped`; `tasks/fix_robot.rs`, `Effect::FixRobot`: flat +30 charge, broken → condition 60 |
| Breakdown | 70% `Stopped` (waits for an electrician), 30% `Effect::Explode`: 100 damage at ≤1 cell falling to 0 at 4, people and dogs only; the robot and its pack vanish; death cause "a robot explosion" |
| Job | `JobKind::RobotCourier` (`robotic: true` in `JobDef`): nobody else may hold it, no workplace books (`GameState::employ_robot`), $0 wage, `HaulGoal`; `dispatch.rs` and `DeliverGoal` accept it |
| Buildings | `map/district/charging_station.rs` (3 at random), `robot_factory.rs` (1, nearest the warehouse); props "charging dock", "robot workbench", "assembly bay", "parts rack"; `FeatureKind::{Charger,RobotWorkbench,RepairBay,PartsRack}` |
| Spawning | two robots at the bay at district start (`ROBOTS_AT_START`); `comings_and_goings` spawns finished builds; one robot per 2 world days while robots < humans/4 |
| Drawing | robots use the dog pool (`Breed::Robot`, `characters/dog.rs`, `game/actors.rs`), lying while stopped; `game/explosions.rs` plays each new `Robots::explosions()` serial (real time) |
| Saving | `pb::Kind::Robot`, `pb::Battery`, `pb::World` robot_works / robot_fixes; `stopped` and the population are rebuilt on load |
| Art | `tools/robot_art.py` (robot, dock, bench, bay, rack, explosion, spark, smoke); engineer uniform from `tools/human_art.py` (remember to restore the PNGs it rewrites) |
| Find | `Search::Robots` ("robots") and the `robot courier` / `engineer` job searches, plus the factory and station landmarks |

Tests: `sim::robots` (rates, break chance, blast, save, and `behaviour`: a `testing::World`
of one robot), `save::tests::robots_saved_mid_trip_with_a_fix_pending_carry_on_exactly`, the
district tests that count residents/jobs (robots and robotic jobs are excluded where noted).
In district tests freeze robots if they must not tread dirt (a footstep trades dirt).
