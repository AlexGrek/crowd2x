//! A crop of wheat, looked at: the panel along the bottom while a crop is
//! selected ([`SelectedCrop`]).
//!
//! It says how far the crop has grown and how long it has to go, how well it
//! has been kept — its quality, which is what its harvest yields — whether it
//! has been tended today, and whose farm it was sown for (`sim::fields`).
//! Nothing on it changes the world: a crop is the farmers' to tend.
//!
//! Built like the box panel ([`super::boxpanel`]) and for the same reasons:
//! rebuilt when the selection changes, its readouts rewritten in place every
//! frame, since a crop grows while it is looked at. A crop harvested under it
//! is no longer there to look at, and the selection goes with it.

use bevy::prelude::*;

use crate::sim::clock::{DAY, HOUR, MINUTE, TIME_SCALE};
use crate::sim::fields::{Crop, Stage, BEST};
use crate::sim::Clock;
use crate::state::AppState;
use crate::ui::nav::{Activated, NavSystems};
use crate::ui::{hover_highlight, label, plain_button, FONT_BODY, PANEL, TEXT, TEXT_ACCENT, TEXT_DIM};

use super::actors::Sim;
use super::hud;
use super::selection::SelectedCrop;

/// The panel's root.
#[derive(Component)]
struct CropPanel;

/// Its one button.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
struct CloseCrop;

/// Text that follows the world.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum CropReadout {
    /// `young, stage 2 of 4: ripe in 3 days, 4 hours`.
    Stage,
    /// `quality 40%, tended today`.
    Quality,
}

pub struct CropPanelPlugin;

impl Plugin for CropPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (press, forget_a_crop_that_is_not_there, rebuild, update_readouts, hover_highlight::<CloseCrop>)
                .chain()
                .after(NavSystems)
                .run_if(in_state(AppState::Game)),
        );
    }
}

/// `close`, clicked or pressed by a script.
fn press(
    mut activated: MessageReader<Activated>,
    clicked: Query<&Interaction, (With<CloseCrop>, Changed<Interaction>)>,
    buttons: Query<(), With<CloseCrop>>,
    mut selected: ResMut<SelectedCrop>,
) {
    let pressed = clicked.iter().any(|interaction| *interaction == Interaction::Pressed)
        | activated.read().any(|message| buttons.contains(message.0));
    if pressed {
        selected.clear();
    }
}

/// A crop harvested, or a world with no crop there, is nothing to look at.
fn forget_a_crop_that_is_not_there(sim: Option<Res<Sim>>, mut selected: ResMut<SelectedCrop>) {
    let Some(at) = selected.get() else {
        return;
    };
    if !sim.is_some_and(|sim| sim.0.fields().at(at).is_some()) {
        selected.clear();
    }
}

fn rebuild(mut commands: Commands, selected: Res<SelectedCrop>, sim: Option<Res<Sim>>, panels: Query<Entity, With<CropPanel>>) {
    if !selected.is_changed() {
        return;
    }
    for panel in &panels {
        commands.entity(panel).despawn();
    }
    let (Some(at), Some(sim)) = (selected.get(), sim) else {
        return;
    };
    let Some(crop) = sim.0.fields().at(at) else {
        return;
    };

    commands.spawn((
        Name::new("crop panel"),
        CropPanel,
        Node {
            position_type: PositionType::Absolute,
            bottom: px(4),
            left: px(0),
            right: px(0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        DespawnOnExit(AppState::Game),
        children![(
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(4),
                padding: UiRect::axes(px(4), px(3)),
                ..default()
            },
            BackgroundColor(PANEL),
            hud::absorbs_clicks(),
            children![
                (
                    Node { flex_direction: FlexDirection::Column, row_gap: px(2), ..default() },
                    children![
                        label(format!("wheat  {}, {}", at.x, at.y), FONT_BODY, TEXT_ACCENT),
                        (CropReadout::Stage, label(String::new(), FONT_BODY, TEXT)),
                        (CropReadout::Quality, label(String::new(), FONT_BODY, TEXT)),
                        label(format!("sown for the grain bin at {}, {}", crop.farm.x, crop.farm.y), FONT_BODY, TEXT_DIM),
                    ],
                ),
                (CloseCrop, plain_button("close", px(24))),
            ],
        )],
    ));
}

fn update_readouts(selected: Res<SelectedCrop>, sim: Option<Res<Sim>>, mut texts: Query<(&CropReadout, &mut Text)>) {
    let (Some(at), Some(sim)) = (selected.get(), sim) else {
        return;
    };
    let Some(crop) = sim.0.fields().at(at) else {
        return;
    };
    let now = sim.0.clock();
    for (readout, mut text) in &mut texts {
        let wanted = match readout {
            CropReadout::Stage => growth(crop, now),
            CropReadout::Quality => care(crop, now),
        };
        if **text != wanted {
            **text = wanted;
        }
    }
}

/// `young, stage 2 of 4: ripe in 3 days, 4 hours` — or `ripe, stage 4 of 4:
/// ready to harvest`.
pub fn growth(crop: &Crop, now: Clock) -> String {
    let stage = crop.stage(now);
    let of = format!("{}, stage {} of {}", stage.name(), stage.number(), Stage::ALL.len());
    if stage == Stage::Ripe {
        return format!("{of}: ready to harvest");
    }
    format!("{of}: ripe in {}", duration(crop.ripe_in(now)))
}

/// `quality 40%, tended today` — or what it is waiting for.
pub fn care(crop: &Crop, now: Clock) -> String {
    let sown = Clock::after_watching(crop.sown / TIME_SCALE as f64);
    let tended = if crop.is_ripe(now) {
        "grown"
    } else if sown.day() == now.day() {
        // A crop counts as tended the day it is sown, so that it is not
        // tended twice; it has not been.
        "sown today"
    } else if crop.tended >= now.day() {
        "tended today"
    } else {
        "not tended today"
    };
    format!("quality {}% of {BEST}%, {tended}", crop.quality)
}

/// World seconds as `3 days, 4 hours`, `5 hours` or `40 minutes`.
fn duration(seconds: f64) -> String {
    let (day, hour, minute) = (DAY as f64, HOUR as f64, MINUTE as f64);
    let days = (seconds / day) as u32;
    let hours = ((seconds % day) / hour) as u32;
    let plural = |n: u32, unit: &str| if n == 1 { format!("1 {unit}") } else { format!("{n} {unit}s") };
    match (days, hours) {
        (0, 0) => plural(((seconds / minute).ceil() as u32).max(1), "minute"),
        (0, hours) => plural(hours, "hour"),
        (days, 0) => plural(days, "day"),
        (days, hours) => format!("{}, {}", plural(days, "day"), plural(hours, "hour")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Point;

    fn at_hours(hours: f64) -> Clock {
        Clock::after_watching(hours * HOUR as f64 / TIME_SCALE as f64)
    }

    #[test]
    fn the_panel_reads_how_far_a_crop_has_grown_and_how_well_it_is_kept() {
        let crop = Crop { cell: Point::new(4, -6), farm: Point::new(1, 7), sown: 0.0, quality: 40, tended: 1 };
        assert_eq!(growth(&crop, at_hours(0.0)), "seedling, stage 1 of 4: ripe in 6 days");
        assert_eq!(growth(&crop, at_hours(48.0 + 20.0)), "young, stage 2 of 4: ripe in 3 days, 4 hours");
        assert_eq!(growth(&crop, at_hours(143.5)), "in ear, stage 3 of 4: ripe in 30 minutes");
        assert_eq!(growth(&crop, at_hours(144.0)), "ripe, stage 4 of 4: ready to harvest");
        assert_eq!(care(&crop, at_hours(0.0)), "quality 40% of 100%, sown today");
        assert_eq!(care(&Crop { tended: 2, ..crop }, at_hours(25.0)), "quality 40% of 100%, tended today");
        assert_eq!(care(&crop, at_hours(24.0)), "quality 40% of 100%, not tended today");
        assert_eq!(care(&crop, at_hours(150.0)), "quality 40% of 100%, grown");
    }
}
