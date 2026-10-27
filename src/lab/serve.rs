//! The lab held behind a socket: a world that runs, stops at an anomaly, and
//! waits there to be looked into and let go again.
//!
//! One command a connection, on `127.0.0.1`: a line in, text back, closed —
//! which is all `tools/lab.py` (or `nc`) needs, and what lets each command be
//! a separate shell call. One thread does everything: between commands it
//! runs the world a slice at a time while it is running, and sleeps while it
//! is paused, so a command never sees a world half way through a tick.
//!
//! `run` returns at once; `wait` holds its connection until the run stops —
//! at its end, at an anomaly or at a `pause` — and then answers with the
//! status. Commands that move the world themselves (`tick`, `trace`, `load`)
//! pause a running one first.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use super::inspect;
use super::watch::{Watch, WatchSpec};
use super::world::{duration, parse_duration, Lab, TICKS_PER_DAY, TICKS_PER_HOUR};

/// How long the world runs between looks at the socket: short enough that a
/// `pause` or a `status` is answered at once, long enough to cost nothing.
const SLICE: Duration = Duration::from_millis(40);
/// How long a `wait` holds on by default: under the ten minutes a shell
/// command is given.
const WAIT: f32 = 540.0;

const HELP: &str = "\
commands (one a connection; DURATION is 90m, 6h, 3d, 2w or 5000t):
  status                      where the world stands; anomalies
  run [DURATION]              let it run (until the next anomaly, or for DURATION); returns at once
  resume                      run, with no end
  pause                       stop it where it is
  wait [SECONDS]              hold on until the run stops, then the status (540 s)
  tick DURATION               live through DURATION now, then answer
  metrics [PATTERN...] [by=DURATION] [last=N]   the sampled numbers as a table (by=1d)
  values [PATTERN]            every metric's latest value
  stats                       the statistics menu
  stores [FILTER]             every store, where, and what is in it
  units [FILTER]              all, humans, dogs, dead, asleep, outside, jobless, goal:NAME or a job
  unit WHO [lines=N]          everything about one unit: id, start of one, or part of a name
  log [N] [WORD...]           the last N lines of the log (40) containing every word
  shifts [JOB] | shifts reset where the workers' shifts have gone
  trace WHO DURATION          live through DURATION watching one unit change
  anomalies                   every anomaly so far, with its snapshot
  watches                     the rules, and what each last saw
  watch add JSON | watch rm NAME
  save [NAME] [store]         snapshot to the run's directory (store: and .saves/, for the Load screen)
  load PATH                   carry on from a .savegame instead
  export                      write metrics.csv
  quit                        stop serving";

/// A run in progress.
struct Run {
    /// The tick it ends at; `None` runs until something stops it.
    until: Option<u64>,
    from: u64,
    started: Instant,
}

struct Server {
    run: Option<Run>,
    /// Why the world last stopped.
    stopped: String,
    /// `wait`s held open, and when each gives up.
    waiters: Vec<(TcpStream, Instant)>,
    quit: bool,
}

impl Server {
    fn running(&self, lab: &Lab) -> String {
        match &self.run {
            Some(run) => {
                let rate = (lab.state.tick() - run.from) as f32 / run.started.elapsed().as_secs_f32().max(1e-3);
                let until = run.until.map_or("the next anomaly".to_string(), |end| {
                    format!("{} more", duration(end.saturating_sub(lab.state.tick())))
                });
                format!("RUNNING for {until} at {rate:.0} ticks/s")
            }
            None => format!("PAUSED ({})", self.stopped),
        }
    }

    fn stop(&mut self, why: String) {
        self.run = None;
        self.stopped = why;
    }

    fn status(&self, lab: &Lab) -> String {
        inspect::status(lab, &self.running(lab))
    }
}

pub fn serve(mut lab: Lab, port: u16, length: Option<u64>) -> i32 {
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("lab: cannot listen on 127.0.0.1:{port}: {error}");
            return 2;
        }
    };
    if let Err(error) = listener.set_nonblocking(true) {
        eprintln!("lab: {error}");
        return 2;
    }
    let mut server = Server { run: None, stopped: "just opened".to_string(), waiters: Vec::new(), quit: false };
    if let Some(length) = length {
        server.run = Some(Run { until: Some(lab.state.tick() + length), from: lab.state.tick(), started: Instant::now() });
    }
    eprintln!("lab: serving on 127.0.0.1:{port} — {}", server.running(&lab));

    while !server.quit {
        loop {
            match listener.accept() {
                Ok((stream, _)) => answer(&mut server, &mut lab, stream),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    eprintln!("lab: {error}");
                    break;
                }
            }
        }
        if let Some(run) = &server.run {
            let until = run.until;
            let slice = Instant::now();
            while slice.elapsed() < SLICE {
                if until.is_some_and(|end| lab.state.tick() >= end) {
                    server.stop("reached the end of the run".to_string());
                    eprintln!("lab: {} — the run ended", lab.state.clock());
                    lab.finish();
                    break;
                }
                let fired = lab.step();
                if fired > 0 {
                    let anomaly = lab.anomalies.last().map(|a| a.to_string()).unwrap_or_default();
                    eprintln!("lab: ANOMALY {anomaly}");
                    server.stop(format!("at an anomaly: {}", lab.anomalies.last().map_or("", |a| a.watch.as_str())));
                    lab.finish();
                    break;
                }
                if lab.state.tick() % TICKS_PER_DAY == 0 {
                    eprintln!("lab: {}", lab.day_line(run.started.elapsed().as_secs_f32()));
                }
            }
        } else {
            std::thread::sleep(Duration::from_millis(10));
        }
        let now = Instant::now();
        let paused = server.run.is_none();
        let waiters = std::mem::take(&mut server.waiters);
        for (mut stream, deadline) in waiters {
            if paused || now >= deadline {
                let text = if paused { server.status(&lab) } else { format!("still {}\n{}", server.running(&lab), server.status(&lab)) };
                let _ = stream.write_all(text.as_bytes());
                let _ = stream.write_all(b"\n");
            } else {
                server.waiters.push((stream, deadline));
            }
        }
    }
    lab.finish();
    eprintln!("lab: stopped serving");
    0
}

/// Read one command off `stream` and answer it — or, for `wait`, keep the
/// stream until there is something to say.
fn answer(server: &mut Server, lab: &mut Lab, stream: TcpStream) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let line = line.trim();
    let mut stream = stream;
    let (command, rest) = line.split_once(' ').unwrap_or((line, ""));
    let rest = rest.trim();
    if command == "wait" {
        let seconds: f32 = rest.parse().unwrap_or(WAIT);
        server.waiters.push((stream, Instant::now() + Duration::from_secs_f32(seconds.max(0.0))));
        return;
    }
    let reply = execute(server, lab, command, rest);
    let _ = stream.write_all(reply.as_bytes());
    let _ = stream.write_all(b"\n");
}

fn execute(server: &mut Server, lab: &mut Lab, command: &str, rest: &str) -> String {
    let words: Vec<&str> = rest.split_whitespace().collect();
    match command {
        "" | "help" => HELP.to_string(),
        "status" => server.status(lab),
        "run" | "resume" => {
            let until = if rest.is_empty() {
                None
            } else {
                match parse_duration(rest) {
                    Ok(ticks) => Some(lab.state.tick() + ticks),
                    Err(error) => return error,
                }
            };
            server.run = Some(Run { until, from: lab.state.tick(), started: Instant::now() });
            server.status(lab)
        }
        "pause" => {
            if server.run.is_some() {
                server.stop("paused by request".to_string());
                lab.finish();
            }
            server.status(lab)
        }
        "tick" => {
            let ticks = match parse_duration(if rest.is_empty() { "1t" } else { rest }) {
                Ok(ticks) => ticks,
                Err(error) => return error,
            };
            let note = pause_for(server, lab, "tick");
            let before = lab.anomalies.len();
            let started = Instant::now();
            for _ in 0..ticks {
                lab.step();
            }
            let mut out = format!("{note}lived through {} in {:.1}s\n", duration(ticks), started.elapsed().as_secs_f32());
            for anomaly in &lab.anomalies[before..] {
                out.push_str(&format!("ANOMALY {anomaly}\n"));
            }
            out.push_str(&server.status(lab));
            out
        }
        "metrics" => {
            let mut patterns = Vec::new();
            let (mut by, mut last) = (TICKS_PER_DAY, 0);
            for word in &words {
                if let Some(value) = word.strip_prefix("by=") {
                    match parse_duration(value) {
                        Ok(ticks) => by = ticks.max(1),
                        Err(error) => return error,
                    }
                } else if let Some(value) = word.strip_prefix("last=") {
                    last = value.parse().unwrap_or(0);
                } else {
                    patterns.push(*word);
                }
            }
            if lab.series.samples.len() < 30 && by == TICKS_PER_DAY {
                by = TICKS_PER_HOUR;
            }
            lab.series.table(&patterns, by, last, 24)
        }
        "values" => inspect::values(lab, rest),
        "stats" => inspect::statistics(&lab.state),
        "stores" => inspect::stores(&lab.state, rest),
        "units" => inspect::units(&lab.state, rest),
        "unit" => {
            let mut lines = 25;
            let who: Vec<&str> = words
                .iter()
                .filter(|word| match word.strip_prefix("lines=") {
                    Some(n) => {
                        lines = n.parse().unwrap_or(lines);
                        false
                    }
                    None => true,
                })
                .copied()
                .collect();
            match inspect::resolve(&lab.state, &who.join(" ")) {
                Ok(uid) => inspect::unit(lab, uid, lines),
                Err(error) => error,
            }
        }
        "log" => {
            let (count, filter) = match words.first().and_then(|word| word.parse::<usize>().ok()) {
                Some(count) => (count, &words[1..]),
                None => (40, &words[..]),
            };
            inspect::log(lab, None, filter, count)
        }
        "shifts" => {
            if rest == "reset" {
                lab.shifts = super::world::Shifts { since: lab.state.tick(), ..Default::default() };
                return "shifts counted afresh from now".to_string();
            }
            inspect::shifts(lab, &words)
        }
        "trace" => {
            let Some((length, who)) = words.split_last() else {
                return "trace WHO DURATION".to_string();
            };
            let ticks = match parse_duration(length) {
                Ok(ticks) => ticks,
                Err(error) => return error,
            };
            let uid = match inspect::resolve(&lab.state, &who.join(" ")) {
                Ok(uid) => uid,
                Err(error) => return error,
            };
            let note = pause_for(server, lab, "trace");
            format!("{note}{}", inspect::trace(lab, uid, ticks))
        }
        "anomalies" => inspect::anomalies(lab),
        "watches" => {
            if lab.watches.is_empty() {
                "no watches".to_string()
            } else {
                lab.watches.iter().map(Watch::describe).collect::<Vec<_>>().join("\n")
            }
        }
        "watch" => {
            let (verb, rest) = rest.split_once(' ').unwrap_or((rest, ""));
            match verb {
                "add" => match serde_json::from_str::<WatchSpec>(rest).map_err(|e| e.to_string()).and_then(Watch::new) {
                    Ok(watch) => {
                        let line = watch.describe();
                        lab.watches.retain(|w| w.spec.name != watch.spec.name);
                        lab.watches.push(watch);
                        format!("watching: {line}")
                    }
                    Err(error) => error,
                },
                "rm" => {
                    let before = lab.watches.len();
                    lab.watches.retain(|w| w.spec.name != rest.trim());
                    if lab.watches.len() < before { format!("no longer watching {rest:?}") } else { format!("no watch {rest:?}") }
                }
                _ => "watch add JSON | watch rm NAME".to_string(),
            }
        }
        "save" => {
            let store = words.contains(&"store");
            let name: Vec<&str> = words.iter().filter(|word| **word != "store").copied().collect();
            let name = if name.is_empty() { format!("tick-{}", lab.state.tick()) } else { super::world::slug(&name.join(" ")) };
            let mut out = match lab.snapshot(&name) {
                Ok(path) => format!("saved {}  (open it: CROWD2X_LOAD={} cargo run)", path.display(), path.display()),
                Err(error) => return error,
            };
            if store {
                let moment = crate::save::store::Moment::now();
                match lab.save_bytes(&name).and_then(|zip| {
                    crate::save::SaveStore::default_store().write(&zip, &moment).map_err(|e| e.to_string())
                }) {
                    Ok(path) => out.push_str(&format!("\nand {} (the Load screen lists it)", path.display())),
                    Err(error) => out.push_str(&format!("\nnot in the store: {error}")),
                }
            }
            out
        }
        "load" => {
            let note = pause_for(server, lab, "load");
            match lab.replace(std::path::Path::new(rest)) {
                Ok(()) => format!("{note}{}", server.status(lab)),
                Err(error) => error,
            }
        }
        "export" => {
            lab.finish();
            format!("wrote {}", lab.out.join("metrics.csv").display())
        }
        "quit" => {
            server.quit = true;
            "bye".to_string()
        }
        other => format!("no command {other:?}\n{HELP}"),
    }
}

/// Pause a running world before a command that moves it itself, and say so.
fn pause_for(server: &mut Server, lab: &mut Lab, command: &str) -> String {
    if server.run.is_none() {
        return String::new();
    }
    server.stop(format!("paused for {command}"));
    lab.finish();
    format!("(paused the run for {command})\n")
}
