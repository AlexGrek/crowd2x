//! The statistics menu's graphs: the last week of the world, an hour a point.
//!
//! Four small charts, one measure each and each on its own scale (never two
//! measures on one axis): the crowd's mean **mental health** and mean
//! **health** (0 to 100), the district's **budget**, and the **cash** every
//! living person holds between them (from the week's lowest to its highest,
//! with a line at zero when a debt takes it under). The samples are the
//! simulation's ([`GameState::history`], `sim::history`); this only draws them.
//!
//! A chart is a picture made here at one texel to the canvas pixel, so a
//! line is one canvas pixel thick like everything else on the screen, and it
//! is drawn again only when a new hour's sample has come in — not every
//! frame. Its title says what it is and where it stands now.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::sim::history::{Sample, HOURS};
use crate::sim::economy::Money;
use crate::sim::GameState;
use crate::ui::{label, FONT_BODY, TEXT};

/// A chart's picture, in canvas pixels.
pub const WIDTH: usize = 140;
pub const HEIGHT: usize = 36;

/// The plot's backing — dark, as the menu's own is see-through and the floor
/// behind it busy — its grid at the quarters, and the line at zero.
const BACKING: [u8; 4] = [10, 10, 15, 200];
const GRID: [u8; 4] = [70, 72, 82, 200];
const ZERO: [u8; 4] = [255, 255, 255, 90];
/// The line, the accent gold, and the area under it, a wash of the same on
/// the backing — as solid as the backing, so the floor stays out of it.
const LINE: [u8; 4] = [250, 209, 79, 255];
const WASH: [u8; 4] = [72, 62, 34, 215];

/// What a chart shows.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Series {
    Mind,
    Health,
    Budget,
    Cash,
}

impl Series {
    pub const ALL: [Series; 4] = [Series::Mind, Series::Health, Series::Budget, Series::Cash];

    pub fn name(self) -> &'static str {
        match self {
            Series::Mind => "mental health",
            Series::Health => "health",
            Series::Budget => "budget",
            Series::Cash => "personal money",
        }
    }

    /// The sample's value for this chart: a percentage, or cents.
    fn value(self, sample: &Sample) -> f64 {
        match self {
            Series::Mind => sample.mind as f64,
            Series::Health => sample.health as f64,
            Series::Budget => sample.budget.as_cents() as f64,
            Series::Cash => sample.cash.as_cents() as f64,
        }
    }

    fn is_money(self) -> bool {
        matches!(self, Series::Budget | Series::Cash)
    }

    /// The samples this chart has a value for: an hour with nobody alive has
    /// no mean mental health or health, though it has a budget.
    fn shown(self, samples: &[Sample]) -> Vec<Sample> {
        samples.iter().filter(|sample| self.is_money() || sample.humans > 0).copied().collect()
    }

    fn format(self, value: f64) -> String {
        if self.is_money() { Money(value.round() as i64).to_string() } else { format!("{value:.0}%") }
    }

    /// The bottom and top of the chart: 0 to 100 for a percentage; for
    /// money, the week's lowest to highest, a little apart at the least so
    /// a flat week is a line across the middle rather than nothing.
    fn range<'a>(self, samples: impl Iterator<Item = &'a Sample>) -> (f64, f64) {
        if !self.is_money() {
            return (0.0, 100.0);
        }
        let (low, high) = samples
            .map(|sample| self.value(sample))
            .fold((f64::MAX, f64::MIN), |(low, high), value| (low.min(value), high.max(value)));
        if low > high {
            return (0.0, 100.0);
        }
        // A dollar either way at the least, a tenth of the span otherwise.
        let pad = ((high - low) * 0.1).max(100.0);
        (low - pad, high + pad)
    }

    /// `mental health  62%   7 days` or `budget  $12,345.00   $11,000.00 to $13,000.00`.
    pub fn title(self, samples: &[Sample]) -> String {
        if samples.is_empty() {
            return format!("{}   no hours yet", self.name());
        }
        let days = days_of(samples);
        let samples = self.shown(samples);
        let Some(latest) = samples.last() else {
            return format!("{}   nobody alive, {days}", self.name());
        };
        let now = self.format(self.value(latest));
        if self.is_money() {
            let (low, high) = samples
                .iter()
                .map(|sample| self.value(sample))
                .fold((f64::MAX, f64::MIN), |(low, high), value| (low.min(value), high.max(value)));
            format!("{}  {now}   {} to {}, {days}", self.name(), self.format(low), self.format(high))
        } else {
            format!("{}  {now}   {days}", self.name())
        }
    }
}

/// How many hours the samples cover: `5 hours`, `3 days`.
fn days_of(samples: &[Sample]) -> String {
    let (Some(first), Some(last)) = (samples.first(), samples.last()) else { return String::new() };
    let hours = last.hour - first.hour + 1;
    match hours {
        1 => "1 hour".to_string(),
        0..24 => format!("{hours} hours"),
        24..48 => "1 day".to_string(),
        _ => format!("{} days", hours / 24),
    }
}

/// How many hours the charts span, left edge to right: the hours there are
/// samples for, but at least a day — a few hours across the whole width is
/// a line with no shape — and at most the week kept.
fn window(samples: &[Sample]) -> u64 {
    match (samples.first(), samples.last()) {
        (Some(first), Some(last)) => (last.hour - first.hour + 1).clamp(24, HOURS as u64),
        _ => 24,
    }
}

/// The chart's picture, RGBA rows top first: the backing and grid, the wash
/// under the line down to zero (or the bottom), and the line. The newest
/// sample is at the right edge and the oldest, a week back at the most, at
/// the left ([`window`]).
pub fn plot(series: Series, samples: &[Sample]) -> Vec<u8> {
    let mut pixels = vec![0u8; WIDTH * HEIGHT * 4];
    let mut put = |x: usize, y: usize, colour: [u8; 4]| {
        let at = (y * WIDTH + x) * 4;
        pixels[at..at + 4].copy_from_slice(&colour);
    };
    for y in 0..HEIGHT {
        let grid = (1..4).any(|quarter| y == quarter * (HEIGHT - 1) / 4);
        for x in 0..WIDTH {
            put(x, y, if grid { GRID } else { BACKING });
        }
    }
    let Some(&latest) = samples.last() else { return pixels };
    let span = window(samples) as i32 - 1;
    let samples = series.shown(samples);
    if samples.is_empty() {
        return pixels;
    }
    let (low, high) = series.range(samples.iter());
    let y_of = |value: f64| -> i32 {
        let share = ((high - value) / (high - low)).clamp(0.0, 1.0);
        (share * (HEIGHT - 1) as f64).round() as i32
    };
    let x_of = |hour: u64| -> i32 {
        let back = (latest.hour - hour).min(span as u64) as i32;
        (WIDTH - 1) as i32 - (back * (WIDTH - 1) as i32 + span / 2) / span
    };
    let base = y_of(0.0f64.clamp(low, high));
    let points: Vec<(i32, i32)> = samples.iter().map(|sample| (x_of(sample.hour), y_of(series.value(sample)))).collect();
    // The wash: every column between two samples, from the line to the base.
    for pair in points.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        for x in x0..=x1 {
            let y = if x1 == x0 { y1 } else { y0 + ((y1 - y0) * (x - x0) + (x1 - x0) / 2) / (x1 - x0) };
            let (top, bottom) = if y < base { (y + 1, base) } else { (base, y - 1) };
            for fill in top..=bottom {
                put(x as usize, fill as usize, WASH);
            }
        }
    }
    if series.is_money() && low < 0.0 && high > 0.0 {
        for x in 0..WIDTH {
            put(x, base as usize, ZERO);
        }
    }
    // The line over it, one pixel thick, joined from sample to sample.
    let mut previous = points[0];
    put(previous.0 as usize, previous.1 as usize, LINE);
    for &point in &points[1..] {
        line(previous, point, |x, y| put(x as usize, y as usize, LINE));
        previous = point;
    }
    pixels
}

/// Bresenham from `from` to `to`, both ends included.
fn line(from: (i32, i32), to: (i32, i32), mut put: impl FnMut(i32, i32)) {
    let (mut x, mut y) = from;
    let (dx, dy) = ((to.0 - x).abs(), -(to.1 - y).abs());
    let (sx, sy) = ((to.0 - x).signum(), (to.1 - y).signum());
    let mut error = dx + dy;
    loop {
        put(x, y);
        if (x, y) == to {
            return;
        }
        let twice = 2 * error;
        if twice >= dy {
            error += dy;
            x += sx;
        }
        if twice <= dx {
            error += dx;
            y += sy;
        }
    }
}

/// A chart's picture as a texture, blank to begin with.
fn texture() -> Image {
    Image::new_fill(
        Extent3d { width: WIDTH as u32, height: HEIGHT as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// A chart's title, rewritten with its picture.
#[derive(Component)]
pub struct ChartTitle(Series);

/// The picture of a chart, and the newest hour and the count of samples it
/// last showed (`Some(None)`: drawn with none; `None`: never drawn).
#[derive(Component)]
pub struct Chart {
    series: Series,
    image: Handle<Image>,
    drawn: Option<Option<(u64, usize)>>,
}

/// The four charts, two by two, under `parent`. They are drawn by
/// [`draw`] while `parent` is shown.
pub fn spawn(commands: &mut Commands, images: &mut Assets<Image>, parent: Entity) {
    let column = commands
        .spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(4), ..default() }, ChildOf(parent)))
        .id();
    let rows = [0, 1].map(|_| commands.spawn((Node { column_gap: px(8), ..default() }, ChildOf(column))).id());
    for (index, series) in Series::ALL.into_iter().enumerate() {
        let image = images.add(texture());
        commands.spawn((
            Node { flex_direction: FlexDirection::Column, row_gap: px(1), ..default() },
            ChildOf(rows[index / 2]),
            children![
                (ChartTitle(series), label(String::new(), FONT_BODY * 2.0 / 3.0, TEXT)),
                (
                    Chart { series, image: image.clone(), drawn: None },
                    ImageNode::new(image),
                    Node { width: px(WIDTH as f32), height: px(HEIGHT as f32), ..default() },
                ),
            ],
        ));
    }
}

/// Draw every chart whose samples have moved on since it was drawn.
pub fn draw(
    sim: &GameState,
    images: &mut Assets<Image>,
    charts: &mut Query<&mut Chart>,
    titles: &mut Query<(&ChartTitle, &mut Text)>,
) {
    let history = sim.history();
    let now = history.latest().map(|sample| (sample.hour, history.len()));
    if charts.iter().all(|chart| chart.drawn == Some(now)) {
        return;
    }
    let samples: Vec<Sample> = history.samples().copied().collect();
    for mut chart in charts.iter_mut() {
        if chart.drawn == Some(now) {
            continue;
        }
        if let Some(mut image) = images.get_mut(&chart.image) {
            image.data = Some(plot(chart.series, &samples));
        }
        chart.drawn = Some(now);
    }
    for (title, mut text) in titles.iter_mut() {
        let wanted = title.0.title(&samples);
        if **text != wanted {
            **text = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn week(values: impl Fn(u64) -> (f32, i64)) -> Vec<Sample> {
        (0..HOURS as u64)
            .map(|hour| {
                let (mind, cash) = values(hour);
                Sample { hour: 1000 + hour, humans: 3, mind, health: 90.0, budget: Money(500_00), cash: Money(cash) }
            })
            .collect()
    }

    fn at(pixels: &[u8], x: usize, y: usize) -> [u8; 4] {
        let at = (y * WIDTH + x) * 4;
        pixels[at..at + 4].try_into().unwrap()
    }

    fn line_rows(pixels: &[u8], x: usize) -> Vec<usize> {
        (0..HEIGHT).filter(|&y| at(pixels, x, y) == LINE).collect()
    }

    #[test]
    fn a_week_spans_the_chart_oldest_left_newest_right() {
        // Mental health climbing from 0 to 100 over the week.
        let samples = week(|hour| (hour as f32 * 100.0 / (HOURS - 1) as f32, 0));
        let pixels = plot(Series::Mind, &samples);
        assert_eq!(line_rows(&pixels, 0), vec![HEIGHT - 1], "a week back, at 0, is the bottom left");
        assert_eq!(line_rows(&pixels, WIDTH - 1), vec![0], "now, at 100, is the top right");
        assert_eq!(at(&pixels, WIDTH - 1, HEIGHT - 1), WASH, "under the line is washed");
        assert_eq!(at(&pixels, 0, 0), BACKING, "over it is not");
    }

    #[test]
    fn a_debt_draws_a_line_at_zero_and_washes_up_to_it() {
        let samples = week(|hour| (50.0, if hour < 84 { -100_00 } else { 100_00 }));
        let pixels = plot(Series::Cash, &samples);
        let zero = (0..HEIGHT).find(|&y| at(&pixels, WIDTH / 4, y) == ZERO || at(&pixels, 2, y) == ZERO);
        assert!(zero.is_some_and(|zero| zero > 0 && zero < HEIGHT - 1), "{zero:?}");
        let zero = zero.unwrap();
        // In debt, the line is under zero and the wash between them.
        let rows = line_rows(&pixels, 10);
        assert!(rows.iter().all(|&row| row > zero), "{rows:?} under {zero}");
        assert_eq!(at(&pixels, 10, zero + 1), WASH);
        assert_ne!(at(&pixels, 10, zero - 1), WASH);
    }

    #[test]
    fn a_short_history_starts_at_the_right_and_none_draws_only_the_grid() {
        let samples = &week(|_| (50.0, 0))[..3];
        let pixels = plot(Series::Health, samples);
        assert!(line_rows(&pixels, WIDTH - 1).len() == 1);
        assert!(line_rows(&pixels, 0).is_empty(), "a week back there is nothing yet");
        let empty = plot(Series::Budget, &[]);
        assert!((0..WIDTH).all(|x| line_rows(&empty, x).is_empty()));
    }

    #[test]
    fn an_hour_with_nobody_alive_has_no_mental_health_but_a_budget() {
        let mut samples = week(|_| (50.0, 0));
        samples[0].humans = 0;
        samples[0].mind = 0.0;
        let pixels = plot(Series::Mind, &samples);
        assert!(line_rows(&pixels, 0).is_empty(), "the empty hour is not a 0 on the chart");
        assert_eq!(line_rows(&pixels, 1).len(), 1, "the next is");
        assert_eq!(line_rows(&plot(Series::Budget, &samples), 0).len(), 1);
        for sample in &mut samples {
            sample.humans = 0;
        }
        assert_eq!(Series::Health.title(&samples), "health   nobody alive, 7 days");
    }

    #[test]
    fn titles_say_where_each_stands_now() {
        let samples = week(|hour| (62.0, if hour == 5 { -3_00 } else { 12_345_00 }));
        assert_eq!(Series::Mind.title(&samples), "mental health  62%   7 days");
        assert_eq!(Series::Cash.title(&samples), "personal money  $12,345.00   -$3.00 to $12,345.00, 7 days");
        assert_eq!(Series::Budget.title(&samples[..4]), "budget  $500.00   $500.00 to $500.00, 4 hours");
        assert_eq!(Series::Health.title(&[]), "health   no hours yet");
    }
}
