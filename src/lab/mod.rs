//! The lab: a world lived through headless, as fast as the machine goes,
//! watched for anomalies, stopped at one so the paused world can be looked
//! into, and let go again.
//!
//! ```sh
//! cargo run --release -- lab run --seed 7 --for 14d      # through, then a report
//! cargo run --release -- lab serve --seed 7              # held behind a socket
//! uv run tools/lab.py start --seed 7                     # ...the same, detached
//! uv run tools/lab.py run 7d && uv run tools/lab.py wait # drive it
//! ```
//!
//! A QA script answers "does this do the right thing when somebody presses
//! these buttons"; the lab answers "what does a district come to over a few
//! weeks, and where did it go wrong". It is plain Rust over [`GameState`]: no
//! Bevy, no window, no assets, so a district goes by at the speed of its ticks
//! rather than of frames — a world day in a few seconds of release build,
//! where the game's fastest speed is a minute and a half.
//!
//! What it keeps as it goes ([`world::Lab`]): a sample of the world's numbers
//! every world hour ([`metrics`]), the log, where every worker's shift goes,
//! and how many pieces went into and out of every kind of store. What it
//! watches for ([`watch`]): rules over those numbers, in `lab/watches.json`.
//! When one fires the world is saved where it stands — a `.savegame` that
//! opens in the game (`CROWD2X_LOAD`) — and, served, the run pauses there for
//! [`serve`]'s commands ([`inspect`]) until it is let go again.
//!
//! A tick here is the game's tick ([`world::DT`], one spawn pass and one
//! processing pass, the game at x1), so a snapshot carries on in the game as it
//! would have in the lab. The `lab` skill is the manual.

mod inspect;
mod metrics;
mod serve;
mod watch;
mod world;

use std::path::PathBuf;

use world::{Lab, Source};

/// Where every run's files go unless `--out` says otherwise: gitignored.
const RUNS: &str = "lab-runs";
/// The port `serve` listens on unless `--port` says otherwise.
pub const DEFAULT_PORT: u16 = 7071;

/// What the command line asked for.
struct Options {
    source: Source,
    watches: Option<PathBuf>,
    no_watches: bool,
    every: u64,
    out: Option<PathBuf>,
    port: u16,
    length: Option<u64>,
    keep_going: bool,
}

const USAGE: &str = "\
usage: crowd2x lab <run|serve> [options]

  run     live through --for, stopping at the first anomaly (--keep-going: at none)
  serve   hold the world behind a socket for tools/lab.py; --for starts it running

world (one of; a district from seed 7 by default):
  --seed N          a generated district
  --map FILE        a map file
  --load FILE       a .savegame, carried on from where it was saved

  --for DURATION    how long: 90m, 6h, 3d, 2w, or 5000t for ticks
  --every DURATION  how often the world's numbers are sampled (1h)
  --watches FILE    the anomaly rules (lab/watches.json); --no-watches for none
  --out DIR         where snapshots, metrics and the report go (lab-runs/<world>)
  --port N          serve's port (7071)
";

/// `crowd2x lab ...`: the exit code, 0 for a run that saw nothing wrong, 1 for
/// one that stopped at an anomaly, 2 for one that could not start.
pub fn main(args: &[String]) -> i32 {
    let Some((mode, rest)) = args.split_first() else {
        eprint!("{USAGE}");
        return 2;
    };
    let options = match parse(rest) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("lab: {error}\n\n{USAGE}");
            return 2;
        }
    };
    let mut lab = match build(&options) {
        Ok(lab) => lab,
        Err(error) => {
            eprintln!("lab: {error}");
            return 2;
        }
    };
    match mode.as_str() {
        "run" => run(&mut lab, &options),
        "serve" => serve::serve(lab, options.port, options.length),
        _ => {
            eprint!("{USAGE}");
            2
        }
    }
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        source: Source::District(7),
        watches: None,
        no_watches: false,
        every: world::TICKS_PER_HOUR,
        out: None,
        port: DEFAULT_PORT,
        length: None,
        keep_going: false,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--seed" => options.source = Source::District(value()?.parse().map_err(|_| "--seed needs a number")?),
            "--map" => options.source = Source::MapFile(value()?.into()),
            "--load" => options.source = Source::Save(value()?.into()),
            "--for" => options.length = Some(world::parse_duration(value()?)?),
            "--every" => options.every = world::parse_duration(value()?)?.max(1),
            "--watches" => options.watches = Some(value()?.into()),
            "--no-watches" => options.no_watches = true,
            "--out" => options.out = Some(value()?.into()),
            "--port" => options.port = value()?.parse().map_err(|_| "--port needs a number")?,
            "--keep-going" => options.keep_going = true,
            other => return Err(format!("no option {other:?}")),
        }
    }
    Ok(options)
}

fn build(options: &Options) -> Result<Lab, String> {
    let watches = if options.no_watches {
        Vec::new()
    } else {
        match &options.watches {
            Some(path) => watch::read(path)?,
            None => watch::defaults(),
        }
    };
    let out = options.out.clone().unwrap_or_else(|| PathBuf::from(RUNS).join(options.source.slug()));
    let started = std::time::Instant::now();
    let lab = Lab::new(&options.source, out, options.every, watches)?;
    eprintln!(
        "lab: {} — {} units, {} — built in {:.1}s; files in {}",
        lab.title,
        lab.state.len(),
        lab.state.clock(),
        started.elapsed().as_secs_f32(),
        lab.out.display()
    );
    Ok(lab)
}

/// `lab run`: through to the end or the first anomaly, a line a world day,
/// then the report.
fn run(lab: &mut Lab, options: &Options) -> i32 {
    let Some(length) = options.length else {
        eprintln!("lab: `run` needs --for");
        return 2;
    };
    let end = lab.state.tick() + length;
    let started = std::time::Instant::now();
    let mut stopped = false;
    println!("{}", lab.day_line_header());
    while lab.state.tick() < end {
        let fired = lab.step();
        if lab.state.tick() % world::TICKS_PER_DAY == 0 {
            println!("{}", lab.day_line(started.elapsed().as_secs_f32()));
        }
        if fired > 0 {
            for anomaly in lab.anomalies.iter().rev().take(fired).collect::<Vec<_>>().into_iter().rev() {
                println!("ANOMALY {anomaly}");
            }
            if !options.keep_going {
                stopped = true;
                break;
            }
        }
    }
    let seconds = started.elapsed().as_secs_f32();
    lab.finish();
    println!();
    println!("{}", inspect::status(lab, if stopped { "stopped at an anomaly" } else { "finished" }));
    println!("{}", lab.series.table(&[], world::TICKS_PER_DAY, 0, 60));
    println!("{}", inspect::shifts(lab, &[]));
    println!(
        "{} ticks in {seconds:.1}s ({:.0} ticks/s); metrics in {}",
        lab.ticks_run,
        lab.ticks_run as f32 / seconds.max(1e-3),
        lab.out.join("metrics.csv").display()
    );
    if lab.anomalies.is_empty() { 0 } else { 1 }
}
