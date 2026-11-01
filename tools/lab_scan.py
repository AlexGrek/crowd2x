"""Scan a lab run (metrics.csv + log.txt) for anomalies of every kind.

    uv run tools/lab_scan.py lab-runs/<run> [more runs...]

Prints one section per check; a line starting with '!!' is a finding.
"""
import collections
import csv
import re
import sys

TICKS_PER_HOUR = 1920
UID = re.compile(r"(?:human|dog|deathdog):[0-9a-f]+")
NUM = re.compile(r"-?\d+(?:\.\d+)?")


def hour_of(clock):
    # "day 3 Wed  10:33:13"
    h, m, _ = clock.split()[-1].split(":")
    return int(h) + int(m) / 60


def day_of(clock):
    return int(clock.split()[1])


def fnum(x):
    try:
        return float(x)
    except (TypeError, ValueError):
        return 0.0


def load_metrics(run):
    with open(f"{run}/metrics.csv", encoding="utf-8") as f:
        return list(csv.DictReader(f))


def load_log(run):
    with open(f"{run}/log.txt", encoding="utf-8", errors="replace") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t", 2)
            if len(parts) == 3:
                yield int(parts[0]), parts[1], parts[2]


def finding(text):
    print("  !! " + text)


def note(text):
    print("     " + text)


def metrics_checks(rows):
    days = len(rows) // 24
    print(f"\n== metrics: {len(rows)} hourly samples, {days} days")

    def series(name):
        return [fnum(r.get(name)) for r in rows]

    def at_hour(name, hours):
        # Hour of day from the clock column: the world opens at 08:00, not midnight.
        return [(day_of(r["clock"]), fnum(r.get(name))) for r in rows if int(hour_of(r["clock"])) in hours]

    # People.
    alive = series("alive.human")
    note(f"humans alive {alive[0]:.0f} -> {alive[-1]:.0f}; died {series('died')[-1]:.0f}; jobless {series('jobless')[0]:.0f} -> {series('jobless')[-1]:.0f}")
    if alive[-1] < 0.9 * alive[0]:
        finding(f"lost {100 * (1 - alive[-1] / alive[0]):.0f}% of the humans in the run")

    # Jobs that fell below strength and stayed there.
    for name in rows[0]:
        if not name.startswith("jobs."):
            continue
        s = series(name)
        start = max(s[:24]) if s[:24] else 0
        low = [i for i, v in enumerate(s) if v < start]
        if low and s[-1] < start:
            finding(f"{name[5:]}: {start:.0f} posts filled at the start, {s[-1]:.0f} at the end (short since day {low[0] // 24 + 1})")

    # Workers on shift spending it on something else.
    print("   on-shift workers at their job's goal (share of on-shift worker-hours):")
    for name in rows[0]:
        if not name.endswith(".on shift"):
            continue
        job = name[: -len(".on shift")]
        on = sum(series(name))
        if on < 24:
            continue
        goals = {k[len(job) + 1:]: sum(series(k)) for k in rows[0] if k.startswith(job + ".") and k != name}
        if not goals:
            continue
        top = sorted(goals.items(), key=lambda kv: -kv[1])
        work_goal = top[0][0]
        share = top[0][1] / on
        rest = ", ".join(f"{g} {v / on:.0%}" for g, v in top[1:4] if v > 0)
        line = f"{job}: {work_goal} {share:.0%} of {on:.0f} worker-hours; then {rest}"
        (finding if share < 0.5 else note)(line)

    # Stores: flows that stopped, stocks that sat empty.
    print("   stores:")
    for name in rows[0]:
        if name.startswith("in."):
            s = series(name)
            if s[-1] == 0:
                continue
            first = next(i for i, v in enumerate(s) if v > 0)
            longest, run_start, best_start = 0, first, first
            for i in range(first + 1, len(s)):
                if s[i] > s[i - 1]:
                    run_start = i
                elif i - run_start > longest:
                    longest, best_start = i - run_start, run_start
            per_week = [s[min(i + 167, len(s) - 1)] - s[i] for i in range(0, len(s) - 1, 168)]
            line = f"{name[3:]}: {s[-1]:.0f} in, by week {[round(w) for w in per_week]}; longest stall {longest} h from day {best_start // 24 + 1}"
            (finding if longest >= 72 else note)(line)
        if name.startswith("store."):
            s = series(name)
            empty = sum(1 for v in s if v <= 0)
            if 0 < empty < len(s) and empty >= 24:
                finding(f"{name[6:]} empty for {empty} of {len(s)} hours")

    # Money.
    budget = series("budget")
    weekly = [budget[min(i + 168, len(budget) - 1)] - budget[i] for i in range(0, len(budget) - 1, 168)]
    line = f"budget {budget[0]:,.0f} -> {budget[-1]:,.0f}; by week {[round(w) for w in weekly]}"
    (finding if budget[-1] < budget[0] else note)(line)
    debt = series("in debt")
    line = f"in debt {debt[0]:.0f} -> {debt[-1]:.0f} (most {max(debt):.0f})"
    (finding if debt[-1] > 5 else note)(line)

    # Minds and bodies.
    low = series("mental_health.low")
    line = f"mental health under 20: {low[0]:.0f} -> {low[-1]:.0f} (most {max(low):.0f}); avg employed {series('mental_health.employed.avg')[-1]:.0f}, jobless {series('mental_health.jobless.avg')[-1]:.0f}"
    (finding if max(low) >= 10 else note)(line)
    hmin = series("health.min")
    sick = sum(1 for v in hmin if v < 30)
    if sick:
        finding(f"somebody under 30 health in {sick} of {len(hmin)} hours")

    # The night: deathdogs by day, people outside in the dark.
    dogs_by_day = [(d, v) for d, v in at_hour("alive.deathdog", range(10, 17)) if v > 0]
    if dogs_by_day:
        finding(f"deathdogs out by daylight (10-16h) in {len(dogs_by_day)} samples on {len({d for d, _ in dogs_by_day})} days")
    out_night = at_hour("outside", range(22, 24))
    worst = max(out_night, key=lambda dv: dv[1], default=(0, 0))
    avg = sum(v for _, v in out_night) / max(1, len(out_night))
    line = f"outside at 22-23h: average {avg:.1f}, most {worst[1]:.0f} (day {worst[0]})"
    (finding if avg >= 3 else note)(line)
    asleep_day = at_hour("asleep", [12, 13, 14])
    avg = sum(v for _, v in asleep_day) / max(1, len(asleep_day))
    note(f"asleep at noon-14h: average {avg:.1f} (night watches sleep by day)")

    # Scrap and fields.
    note(f"scrap left {series('scrap.left')[0]:.0f} -> {series('scrap.left')[-1]:.0f}; nearest heap {series('scrap.nearest')[0]:.0f} -> {series('scrap.nearest')[-1]:.0f} cells")
    if series("scrap.nearest")[-1] > 200:
        finding(f"the nearest scrap is {series('scrap.nearest')[-1]:.0f} cells from the bins")
    note(f"outposts built {series('outposts.built')[-1]:.0f}")


def log_checks(run):
    print("\n== log")
    job_of = {}
    deaths = []
    gave_up_cells = collections.Counter()
    gave_up_units = collections.Counter()
    per_unit_hour = collections.Counter()
    neg_cash = collections.Counter()
    bites = collections.Counter()
    bites_daylight = 0
    panics_daylight = 0
    panics = 0
    patterns = collections.Counter()
    by_day = collections.defaultdict(collections.Counter)
    no_uid_lines = collections.Counter()
    for tick, clock, msg in load_log(run):
        hour = hour_of(clock)
        day = day_of(clock)
        uids = UID.findall(msg)
        norm = NUM.sub("N", UID.sub("U", msg))
        patterns[norm] += 1
        if not uids and not msg.startswith(("the ", "a pack", "no deathdogs", "spawned", "despawned")):
            no_uid_lines[norm] += 1
        m = re.match(r"(\S+) was hired as (?:an? |the )?(.+?) at ", msg)
        if m:
            job_of[m.group(1)] = m.group(2)
        m = re.match(r"(\S+) died of (.+)", msg)
        if m:
            deaths.append((day, hour, m.group(1), m.group(2)))
        m = re.match(r"(\S+) gave up on reaching (-?\d+, -?\d+)", msg)
        if m:
            gave_up_cells[m.group(2)] += 1
            gave_up_units[m.group(1)] += 1
        if re.match(r"\S+ paid \$[\d.,]+ for .+: -\$", msg):
            neg_cash[uids[0]] += 1
        m = re.match(r"(\S+) bit (\S+) for", msg)
        if m:
            bites[m.group(2)] += 1
            bites_daylight += 7 <= hour < 19
        if msg.endswith(" panicked"):
            panics += 1
            panics_daylight += 7 <= hour < 19
        if uids:
            per_unit_hour[(uids[0], tick // TICKS_PER_HOUR, norm)] += 1
        for key, needle in (("bladder accident", "bladder critical"), ("peed outside", "peed outside"), ("nowhere to go", "has nowhere to go"),
                            ("gave up", "gave up on reaching"), ("found no", " found no "), ("negative cash", ": -$")):
            if needle in msg:
                by_day[key][day] += 1

    # Deaths.
    causes = collections.Counter(c for *_, c in deaths)
    jobs = collections.Counter(job_of.get(u, "jobless") for *_, u, _ in deaths)
    print(f"   deaths: {len(deaths)}; causes {dict(causes)}")
    note(f"by job: {dict(jobs.most_common())}")
    for cause, n in causes.items():
        if cause != "deathdog bites" and n:
            finding(f"{n} died of {cause}")

    # Bites.
    if bites:
        victims = collections.Counter(job_of.get(u, "jobless/dog") for u in bites)
        note(f"bites: {sum(bites.values())} on {len(bites)} victims; by job {dict(victims.most_common(6))}")
        if bites_daylight:
            finding(f"{bites_daylight} bites in daylight (07-19h)")
    if panics:
        (finding if panics_daylight > 0.25 * panics else note)(f"panics: {panics}, {panics_daylight} in daylight")

    # Unreachable places.
    total = sum(gave_up_cells.values())
    finding(f"'gave up on reaching' {total} times, {len(gave_up_units)} units; top cells {gave_up_cells.most_common(8)}")
    worst = gave_up_units.most_common(5)
    note("most: " + ", ".join(f"{u[-6:]} ({job_of.get(u, 'jobless')}) {n}" for u, n in worst))

    # Loops: the same unit saying the same thing over and over within an hour.
    loops = collections.Counter()
    loopers = collections.defaultdict(set)
    for (uid, _, norm), n in per_unit_hour.items():
        if n >= 8 and not any(word in norm for word in ("slept", "chatted", "small talk", " paid U", "cleaned up")):
            loops[norm] += 1
            loopers[norm].add(uid)
    for norm, n in loops.most_common(8):
        finding(f"repeated 8+ times in one hour by one unit, {n} unit-hours ({len(loopers[norm])} units): {norm[:100]}")

    # Money trouble.
    if neg_cash:
        finding(f"{sum(neg_cash.values())} purchases into negative cash by {len(neg_cash)} units")

    # Daily counts of trouble.
    for key, counter in by_day.items():
        days = sorted(counter)
        series = [counter[d] for d in range(1, max(days) + 1)]
        note(f"{key} a day: {series}")

    # Lines with no unit named: unattributable.
    for norm, n in no_uid_lines.most_common(5):
        if n >= 10:
            finding(f"logged {n} times without naming the unit: {norm[:90]}")


def main():
    for run in sys.argv[1:]:
        print(f"######## {run}")
        metrics_checks(load_metrics(run))
        log_checks(run)
        print()


if __name__ == "__main__":
    main()
