---
name: economy
description: Money, wages, taxes, care packages, fridge prices, office payroll, the district accountant, daily books, economy.yaml, the live economy menu and economy.pb. Use when changing the ledger, shift recording, payroll, financial reports, tariffs or account saves.
---

# The economy

`src/sim/economy.rs` owns the district budget, accounts sorted by uid, a shared
round-robin payroll cursor, and 28 daily books. Money is signed integer cents.
Both cash and the budget may go below zero. Tax rounds half a cent away from zero.
Accounts exist for humans only, including bodies until despawn; dogs have none.
An arrival adds a grant; an estate, including debt, returns to the budget.
Death forfeits unpaid shifts and care days. Grants are excluded from daily balance.

`economy.yaml` is the single source of built-in defaults, parsed once through
`LazyLock`. Wages and prices use catalogue names; every job and item must be
present, and unknown names fail. Starting wages: $60 office/accountant/scrap/farmer,
$100 king, $40 other jobs. Tax 20%, care $20/day, arrival grant $1000, starting
cash $10?90, food $1 (a meal an outside worker keeps in its pocket is paid for at the fridge too), dog food $1, water free, payroll five world minutes/account.

`Facilities.economy` is read through `Think.economy`. The world applies `Record`,
`Payroll`, `Buy`, `CloseBooks` in slot order, after parallel react. Never mutate the
ledger from a brain or a Bevy system. `Command::SetTariff` runs in the spawn pass,
including while paused, clamps bounds and posts an important notice.

## Timesheets and work

`brain/timesheet.rs` is inline on Brain. Step 4b records being on duty when the
job goal takes charge while due (commute included). On-call electricians clock
in without a repair. A shift is paid in full when it ends or the job changes.
Unemployment accrues one care day at midnight; a fresh timesheet accrues nothing
retroactively. Step 6c emits a pending record only when the effect slot is free;
tasks and playzone operation take precedence. Pending shifts use their last job's
wage, including if a person changes jobs before payroll.

`JobDef.paperwork` selects Payroll for office workers and Books for accountants.
WorkGoal queues `Task::payroll` when anyone is owed, otherwise its ordinary hour
of Work. `Task::close_books` takes thirty world minutes and closes one of the last
seven finished days. It computes day/week balances and budget runway in the brain,
posts an important report and a unit breakdown, then requests CloseBooks.
Both tasks reuse Work's distance/power/body guards and fatigue handling; interrupted
work tires the body but pays nobody and closes no books.

The accountant is catalogue job 13, hired last, Mon?Fri 09?17 with a two-hour
commute. Its powered accountant computer stands four cells from the palace throne.
Its charcoal waistcoat, white sleeves and green tie come from tools/human_art.py.
A completed TakeItem from a fridge emits Buy; a non-fridge take never charges.

## Menus and files

`game/economy.rs` reads the external YAML on new games and defaults, respecting
CROWD2X_ECONOMY. A missing file uses built-ins; other read/parse errors also log a
warning. Loaded games retain their own tariffs. The full-screen modal has its own
Scope, live values, row selection, lower/raise/defaults/close; escape closes it.
It changes the running world's tariffs without rewriting YAML.
Statistics counts money on demand, in two columns beside jobs; the job panel
shows live cash, gross wage and unpaid shifts/care days.

`economy.pb` is an optional archive member. New saves always include it. It holds
tariffs by name, accounts, budget, cursor, daily books and closed_through. Owing is
derived. Restore checks account membership/death against the human arena. An old
save without it gets default tariffs, $1000 per human, cash 10 + uid.raw()%81 dollars,
and no retroactive wages. Brain tag 15 saves the Timesheet; absence is fresh.
New timed tasks save by stable names through TimedTask. Keep exhaustive save
struct destructuring and include the economy in save fingerprints.

Tests: sim/economy.rs, brain/timesheet.rs, tasks/payroll.rs, tasks/close_books.rs,
goals/work.rs, sim/economy_tests.rs, save/tests.rs, and qa/economy.json. Run cargo
test, targeted UI QA and release perf_simulation; inspect captured menu frames.

The fixture wiring helper tools/wire_map.py also recognizes accountant computers;
a QA map must actually connect its powered desk, as a district does.
