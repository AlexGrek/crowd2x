//! A person's stats as progress bars, down the right edge of the screen.
//!
//! ```text
//!                        +--------------------+
//!                        | health        100  |   <- a bar behind each row:
//!                        | hunger         84  |      full is a need well met
//!                        | thirst         71  |
//!                        | bladder        90  |
//!                        | fun            58  |
//!                        | stamina        12  |   <- red below a quarter
//!                        | satisfaction   77  |
//!                        | mental health  41  |
//!                        | attention      30  |
//!                        +--------------------+
//! ```
//!
//! It appears while a **human** is selected and is gone with them, the way
//! [`unitpanel`](super::unitpanel)'s bar is. A dog has stats too but no
//! needs to read, so it gets neither the panel nor the room the panel takes.
//!
//! # Every bar reads the same way up
//!
//! A bar is how well the stat is **met**, from empty to full, whichever way
//! the body keeps it: hunger, thirst and the bladder *rise* towards the thing
//! to be done about them and are turned over here (`100 - hunger`), fun,
//! stamina and the rest *drain* and are shown as they are. So a full green bar
//! is never a problem and a short red one always is, with no row to
//! remember the direction of. The number beside it is the same percentage.
//! The colour steps at 50 and at 25 — a need past 75 is the one a unit hurries
//! for, which is below 25 met.
//!
//! # Built once, written every frame
//!
//! Structure — the nine rows — is rebuilt only when the selected person
//! changes. The fill's width, its colour and the number follow the simulation,
//! so [`update_bars`] rewrites them in place, and only when they differ: the
//! fill is a whole number of canvas pixels wide, so a stat creeping along
//! changes the panel a few times a minute, not sixty times a second.
//!
//! The panel is a view. It reads [`Stats`] through the entity and decides
//! nothing about the world.

use bevy::prelude::*;

use crate::sim::{EntityType, GameEntity, Stats, Uid};
use crate::state::AppState;
use crate::ui::nav::NavSystems;
use crate::ui::{FONT_BODY, PANEL, TEXT, label};

use super::actors::Sim;
use super::hud;
use super::selection::Selected;

/// How wide a bar is, in canvas pixels.
const TRACK: f32 = 68.0;

/// The panel's padding on every side.
const PAD: f32 = 3.0;

/// How far the panel is from the right and bottom edges of the screen.
const MARGIN: f32 = 4.0;

/// How much of the screen's width the panel and its margins take — what
/// [`unitpanel`](super::unitpanel)'s bar leaves clear on its right so the two
/// do not sit on each other.
pub const GUTTER: f32 = TRACK + 2.0 * PAD + MARGIN + 4.0;

/// One row of the panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Need {
    Health,
    Hunger,
    Thirst,
    Bladder,
    Fun,
    Stamina,
    Satisfaction,
    MentalHealth,
    Attention,
}

impl Need {
    /// Top to bottom: how it is, what it needs, then how it feels.
    pub(crate) const ALL: [Need; 9] = [
        Need::Health,
        Need::Hunger,
        Need::Thirst,
        Need::Bladder,
        Need::Fun,
        Need::Stamina,
        Need::Satisfaction,
        Need::MentalHealth,
        Need::Attention,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Need::Health => "health",
            Need::Hunger => "hunger",
            Need::Thirst => "thirst",
            Need::Bladder => "bladder",
            Need::Fun => "fun",
            Need::Stamina => "stamina",
            Need::Satisfaction => "satisfaction",
            Need::MentalHealth => "mental health",
            Need::Attention => "attention",
        }
    }

    /// How well met this is, 0 to 100, whichever way up the stat is kept.
    pub(crate) fn met(self, stats: &Stats) -> f32 {
        let met = match self {
            Need::Health => stats.health(),
            Need::Hunger => 100.0 - stats.hunger(),
            Need::Thirst => 100.0 - stats.thirst(),
            Need::Bladder => 100.0 - stats.bladder(),
            Need::Fun => stats.fun(),
            Need::Stamina => stats.stamina(),
            Need::Satisfaction => stats.satisfaction(),
            Need::MentalHealth => stats.mental_health(),
            // The one stat on a scale of 0 to 1.
            Need::Attention => stats.attention() * 100.0,
        };
        met.clamp(0.0, 100.0)
    }
}

/// Green while it is well met, amber when it is getting low, red once it is
/// the kind of low a unit hurries for.
fn tone(met: f32) -> Color {
    if met >= 50.0 {
        Color::srgb(0.27, 0.62, 0.36)
    } else if met >= 25.0 {
        Color::srgb(0.78, 0.60, 0.20)
    } else {
        Color::srgb(0.78, 0.29, 0.27)
    }
}

/// The fill's width: whole canvas pixels, so it is crisp and so it changes
/// only when it has to.
fn fill_width(met: f32) -> f32 {
    (met / 100.0 * TRACK).round()
}

/// The percentage beside the bar.
fn percent_text(met: f32) -> String {
    format!("{:.0}", met)
}

/// The root of the panel. One at a time.
#[derive(Component)]
struct NeedsPanel;

/// The fill of one bar.
#[derive(Component, Clone, Copy)]
struct Fill(Need);

/// The number at the end of one bar.
#[derive(Component, Clone, Copy)]
struct Figure(Need);

pub struct NeedsPlugin;

impl Plugin for NeedsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (rebuild, update_bars)
                .chain()
                .after(NavSystems)
                .run_if(in_state(AppState::Game)),
        );
    }
}

/// Whether this entity gets the panel — and so whether the bottom bar makes
/// room for it.
pub(crate) fn shows(entity: &dyn GameEntity) -> bool {
    entity.kind() == Some(EntityType::Human) && entity.biology().is_some()
}

/// The stats of whoever is selected, if they are somebody the panel is for.
fn stats_of(selected: &Selected, sim: &Sim) -> Option<(Uid, Stats)> {
    let uid = selected.get()?;
    let entity = sim.0.entities().get(uid)?;
    if !shows(entity) {
        return None;
    }
    Some((uid, *entity.biology()?.stats()))
}

/// Build the panel when somebody new is selected, and take it away when
/// nobody it is for is.
fn rebuild(
    mut commands: Commands,
    selected: Res<Selected>,
    sim: Option<Res<Sim>>,
    panels: Query<Entity, With<NeedsPanel>>,
    mut shown: Local<Option<Uid>>,
) {
    let wanted = sim.as_deref().and_then(|sim| stats_of(&selected, sim));
    if wanted.map(|(uid, _)| uid) == *shown {
        return;
    }
    *shown = wanted.map(|(uid, _)| uid);
    for panel in &panels {
        commands.entity(panel).despawn();
    }
    let Some((_, stats)) = wanted else {
        return;
    };

    commands
        .spawn((
            Name::new("needs panel"),
            NeedsPanel,
            Node {
                position_type: PositionType::Absolute,
                right: px(MARGIN),
                bottom: px(MARGIN),
                flex_direction: FlexDirection::Column,
                row_gap: px(1),
                padding: UiRect::all(px(PAD)),
                ..default()
            },
            BackgroundColor(PANEL),
            hud::absorbs_clicks(),
            DespawnOnExit(AppState::Game),
        ))
        .with_children(|panel| {
            for need in Need::ALL {
                bar(panel, need, need.met(&stats));
            }
        });
}

/// One row: the track, the fill in it, then the name and the number over both.
///
/// The fill is out of the flow, so the two texts lay out as a row of their own
/// on the track and the fill, drawn first, is behind them.
fn bar(panel: &mut ChildSpawnerCommands, need: Need, met: f32) {
    panel
        .spawn((
            Name::new(need.label()),
            Node {
                width: px(TRACK),
                height: px(7),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::horizontal(px(2)),
                ..default()
            },
            BackgroundColor(crate::ui::FIELD),
        ))
        .with_children(|track| {
            track.spawn((
                Fill(need),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(0),
                    height: percent(100),
                    width: px(fill_width(met)),
                    ..default()
                },
                BackgroundColor(tone(met)),
            ));
            track.spawn(label(need.label(), FONT_BODY, TEXT));
            track.spawn((Figure(need), label(percent_text(met), FONT_BODY, TEXT)));
        });
}

/// Keep the bars saying what is true, every frame, writing only what moved.
fn update_bars(
    selected: Res<Selected>,
    sim: Option<Res<Sim>>,
    mut fills: Query<(&Fill, &mut Node, &mut BackgroundColor)>,
    mut figures: Query<(&Figure, &mut Text)>,
) {
    let Some((_, stats)) = sim.as_deref().and_then(|sim| stats_of(&selected, sim)) else {
        return;
    };

    for (Fill(need), mut node, mut colour) in &mut fills {
        let met = need.met(&stats);
        let width = px(fill_width(met));
        if node.width != width {
            node.width = width;
        }
        let tone = tone(met);
        if colour.0 != tone {
            colour.0 = tone;
        }
    }
    for (Figure(need), mut text) in &mut figures {
        let wanted = percent_text(need.met(&stats));
        if **text != wanted {
            **text = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_bar_is_a_need_well_met_whichever_way_the_stat_is_kept() {
        // Nothing wrong with this body: not hungry, not thirsty, an empty
        // bladder, rested and entertained — and every bar is full.
        let stats = Stats::calm()
            .with_hunger(0.0)
            .with_thirst(0.0)
            .with_bladder(0.0)
            .with_fun(100.0)
            .with_stamina(100.0)
            .with_satisfaction(100.0);
        for need in [Need::Health, Need::Hunger, Need::Thirst, Need::Bladder, Need::Fun, Need::Stamina, Need::Satisfaction] {
            assert_eq!(need.met(&stats), 100.0, "{}", need.label());
        }
    }

    #[test]
    fn starving_is_an_empty_hunger_bar_and_a_spent_body_an_empty_stamina_bar() {
        let stats = Stats::calm().with_hunger(100.0).with_stamina(0.0);
        assert_eq!(Need::Hunger.met(&stats), 0.0);
        assert_eq!(Need::Stamina.met(&stats), 0.0);
    }

    #[test]
    fn attention_is_read_on_the_same_scale_as_the_rest() {
        // `calm` is attentive to 0.5, on a scale of 0 to 1.
        assert_eq!(Need::Attention.met(&Stats::calm()), 50.0);
    }

    #[test]
    fn there_is_a_row_for_every_stat_and_no_two_rows_share_a_name() {
        assert_eq!(Need::ALL.len(), Stats::named().len());
        let mut labels: Vec<&str> = Need::ALL.iter().map(|need| need.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), Need::ALL.len());
    }

    #[test]
    fn the_colour_steps_at_half_and_at_a_quarter() {
        assert_ne!(tone(50.0), tone(49.0));
        assert_ne!(tone(25.0), tone(24.0));
        assert_eq!(tone(100.0), tone(50.0));
        assert_eq!(tone(0.0), tone(24.0));
    }

    #[test]
    fn the_fill_is_whole_canvas_pixels_and_never_wider_than_its_track() {
        assert_eq!(fill_width(0.0), 0.0);
        assert_eq!(fill_width(100.0), TRACK);
        for met in 0..=100 {
            let width = fill_width(met as f32);
            assert_eq!(width, width.round());
            assert!((0.0..=TRACK).contains(&width));
        }
    }
}
