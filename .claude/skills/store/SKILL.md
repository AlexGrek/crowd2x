---
name: store
description: The district shop, central food reserve, shopkeepers, store couriers and shared courier dispatch. Use when changing shop staffing, fridge food availability, replenishment, dispatch assignments, store.yaml, shop art or store saves.
---

# The shop and its supply chain

The generated district has one public `store`, on the available lot nearest
the warehouse after the earlier lot buildings have been placed. It contains
one `superfridge` and one `shop counter`. The three older decorative shops
remain separate. `map/district/store.rs` defines the building; the catalogues
in `map/props.rs`, `sim/feature.rs` and `editor/props.rs` bind its saved names
to passability, use and art. Both props block their own cell and are used
from beside it. They need no power.

## Food and money

`Facilities.superfridge` owns `StoreRules`, the sorted reserve cells and a
fractional debit. `Bins` owns the whole meal units in each reserve. Human food
and dog food taken from any ordinary fridge reduce this central supply;
water never does. Nothing is physically carried from the reserve to household
fridges. At zero central stock, all ordinary fridges lose food and dog food
but retain water. Maps without a superfridge retain unlimited fridge food.

`store.yaml` is embedded as the default and read by `game/store.rs` on new
games. `CROWD2X_STORE` selects another file. Missing files use defaults;
unreadable or invalid files also post a notice. Loaded games keep their own
rules. Every key is required; unknown keys, non-finite percentages,
percentages outside 0..100 and nonpositive capacity, or a delivery size that does not divide capacity into 1..1,000,000 whole batches are refused.

| Key | Default | Meaning |
| --- | --- | --- |
| `capacity` | 1 | Normalized full reserve per superfridge |
| `delivery size` | 0.01 | Reserve supplied by one warehouse meal batch |
| `falls per meal` | 0.014 | Percent of total reserve capacity consumed per fridge meal |
| `call at` | 40 | A keeper calls at or below this percentage in its own reserve; equal to `starts at`, so the reserve holds at its starting level |
| `starts at` | 40 | Initial percentage when the map supplies no explicit stock |

The default initial level is 0.4: forty physical delivery batches at 0.01 each.
Initial stock rounds to the nearest delivery batch. Explicit map `initial_stock`
counts warehouse batches and overrides `starts at`, capped at capacity.
Fractional consumption accumulates within a delivery batch; each whole batch
comes from the fullest reserve, lower cell on ties. The
remaining fraction belongs to that reserve's fullness readout. A shopkeeper
checks its own reserve, even when another shop is full. Empty/nonempty notices
refer to availability across all reserves.

The economy is separate: a successful ordinary fridge take charges the
human account at `economy.yaml`'s named price through `Effect::Buy`, once.
Food defaults to $12.50 (priced to keep the budget level, see the `economy`
skill), dog food to $0.50, water to $0. Debt is allowed. Dogs have no
account; their direct meals still consume reserve stock. A pet owner's dog
food purchase charges the owner. Simultaneous takes commit in slot order;
a take after the last stock has gone returns no item and incurs no charge.

Warehouse withdrawals, courier deliveries and returned excess are internal
transport: no sale, wage payment or budget change. Both shop jobs default to
$40 gross per worked shift, $32 after the default 20% tax, through normal
timesheets and office payroll. `Deliver` counts as work for every courier job.
Prices and wages remain editable in the economy menu and saved in `economy.pb`.

## Staff and dispatch

`JobKind::Shopkeeper` (stable number 18) employs four people per counter,
every day: 06–12, 12–18, 18–24, 00–06. `ShopGoal` (`GoalId::Shopkeep`, 30)
walks to the nearest superfridge to the counter, checks it for one world
minute (`CheckStock`), then minds its own counter for fifteen world minutes
(`Work`). Travel adds to the interval. With no reserve it minds the counter.
Needs interrupt work; resuming rebuilds tasks from the current position and
chosen destination. Missing or inaccessible destinations use normal goal
cooldowns and bounded crowd retries. Every check and counter stretch tires
the worker, including the elapsed portion of interrupted work.

`JobKind::StoreCourier` (19) employs one person per superfridge, 08–16 every
day, with the normal two-hour commute. Between dispatches it uses `HaulGoal`
for wheat from grain bins to food-factory hoppers and meals from factory
pallets to warehouse meal racks. Its workplace reserves the job; ordinary
hauling does not unload into the superfridge. Uniforms are `uniform shopkeeper`
and `uniform store courier`.

`Facilities.dispatch` owns FIFO orders for one warehouse batch (0.01 reserve by default) per call. `CheckStock`
requests `Effect::CallDispatch`; the world rechecks the reserve's level and
keeps at most one waiting request per reserve. A further call is allowed
once the earlier request has a courier. Recalled requests merge with any
waiting request; full reserves discard outstanding requests.

`Dispatch::send`, after the world effects settle, selects a stocked meal rack
nearest the requested reserve, then the free courier nearest that rack,
using Manhattan distance and lower UID on ties. All `JobDef.courier` jobs
participate: steel courier, food courier and store courier. Free means alive,
unfrozen, on duty, not on sick leave, carrying nothing, and currently doing
its normal work, wandering or idle. No courier gets two assignments. The
candidate buffer is reused; no crowd scan runs when there is no food source.

An assignment raises `DeliverGoal` (`GoalId::Deliver`, 31) instead of the
courier's own hauling goal. It walks to a stocked rack, `Fetch`es exactly one
meal, then walks to the requested reserve and `Deposit`s it. Fetch and deposit
each take five world minutes and tire the worker. Needs can interrupt; the
order and carried meal survive, and the plan resumes from the pack. An
assigned courier finishes past shift end. Death, freezing, dismissal, removal,
sick leave, no source while empty-handed, or a three-world-hour timeout returns
the assignment to waiting. Dismissal/removal release it in the spawn pass, so
a paused save never names an absent courier. Full-on-arrival excess stays in
the pack; any courier returns orphaned meals to a meal rack before its usual
hauling route.

## Ownership, saves and rendering

Keep decisions in routines/goals, timed work in tasks, shared changes in
`effect::apply`/`Dispatch::send`. Never mutate shop state from Bevy or scan
the crowd inside an individual brain. The dispatch reads one settled crowd
snapshot, and stock transfers commit sequentially. Preserve goods through
contention, cancellation and interruption.

World Protobuf tags 23/24 store reserve rules/fraction and orders; stock is in
the existing bins. Saves without store fields have default rules, zero fraction and no orders.
Older whole-capacity rules convert from 100 to 1 with delivery size 0.01,
preserving stock, fullness, consumption and money. StoreRules tags 5/6 save
normalized capacity/delivery size; tag 1 retains batch capacity for old readers.
Goals use `Seek` executor state. `CheckStock` and `Fetch` use stable timed-task
names `check stock` and `fetch`; jobs use their stable numbers. Restore refuses
invalid rules/fractions, excess reserve stock, nonexistent destinations,
duplicate courier assignments or waiting calls, nonhuman/missing couriers,
and assignment times in the future.

`tools/store_art.py` generates native 16px props/uniforms. The reserve has four
frames: empty, up to 30%, up to 65%, and over 65%, scaled against its saved
capacity. `game/props.rs` updates visible reserve frames; ordinary warehouse
bins retain their piece-count frames. Both jobs appear in the normal job,
statistics and economy menus. Statistics also shows normalized reserve level
and capacity, fullness, and waiting/delivering dispatch counts.

Coverage: `sim/superfridge.rs`, `sim/dispatch.rs`, `sim/store_tests.rs`,
`brain/goals/shop.rs`, `brain/tasks/check_stock.rs`, `brain/tasks/fetch.rs`,
`save/tests.rs`, district store/hiring tests, `qa/store.json` and
`qa/store_empty.json`, `qa/store_balance.json` (three live district days), and
`qa/store_week.json` (a full week, including the weekend). QA exposes
`expect_superfridge` (percentage range and food availability) and
`expect_dispatch` (waiting/assigned counts), so long scripted ticks assert
state directly. Run `cargo test`, shop/economy/food-factory/save UI QA, inspect
the screenshots, then run release `qa/perf_simulation.json`.

For example, `{"expect_superfridge":{"min":31,"max":40,"food_in_fridges":true}}`
checks total reserve fullness and fridge availability;
`{"expect_dispatch":{"waiting":0,"assigned":0}}` checks all orders completed.


## Balance at capacity 1

One retail meal consumes 0.00014 reserve; a 0.01 delivery batch supports about
71.4 meals. Full stock covers about 7,143 retail servings, and starting stock
0.4 covers about 2,857. The warehouse's initial thirty batches add roughly
2,143 servings of startup supply. The farm's first harvest is after three
world days. Two cookers can produce 160 batches across five eight-hour shifts
per week before travel and need breaks, about 11,429 retail servings.

**Holding the level** (retuned 2026-10-31 against seed 7 months in the lab).
A district eats about 380 meals and 44 dog dinners a day, about six batches at
0.014. Since a call is one batch and `call at` equals `starts at`, the reserve
holds at 40%: 36–45% for the whole month, a few points under overnight while
no courier is on duty. The first weekend is the tight spot. The food factory
does not cook on Saturday or Sunday, and in week one it has made only about a
dozen batches, so the warehouse's thirty plus that dozen must carry the shop
until Monday. At 0.018 (7.6 batches a day) with `call at` 40, the warehouse ran
dry from Saturday morning to Monday noon and the reserve fell from 41% to 26%.
With the old `call at` 30 the reserve fell from 40% to 27% in two days and then
stayed just under the call level all month. From week two production (70 to 85
batches a week) is well over consumption (about 42), so the meal rack grows.

At the hunger process's conservative no-sleep rate a human needs 2.5 meals a
day: $31.25 at $12.50, within the $32 net of a $40 low-tier shift; the $20
unemployment care covers most of it and the rest is debt (allowed; an estate
pays it back). In the retuned month about 80 of 170 humans, mostly the jobless,
were in debt at any time.
`normalized_shop_balance_covers_startup_production_and_affordable_food` checks
three generated districts against startup coverage and weekly production,
using three servings per resident per day (dogs included) as a safety margin,
and that a day's food fits a low wage and costs at most twice the care package.
The production calculation is a capacity bound; actual deliveries still
require staffed, supplied factories and available couriers. The mechanics tests'
`testing::store_world` pins the old rules (0.018, call at 30), so a retune of
`store.yaml` changes only the tests that check the defaults.


The capacity-one verification ran 1,472 Rust tests, the shop/empty-stock/
three-day-district/economy/food-factory/steel-factory/save UI scenarios, and a
release full-week district scenario. All passed. In the week-long seed-7 run,
food remained available at every daily checkpoint; the final reserve (at the
old 0.018 and call at 30) was 0.2479 / 1 with five warehouse batches remaining and food-factory output in
stock. The release simulation scaling test also passed (about 0.40 ms median
per tick for 4,000 humans on this machine). Performance numbers are local
observations, not fixed budgets or guarantees.
