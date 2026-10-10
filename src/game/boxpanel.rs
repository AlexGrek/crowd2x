//! The switch on a distribution box: the panel along the bottom while a box
//! is selected ([`SelectedBox`]).
//!
//! It says where the box is, whether it is on, and what it feeds — the
//! lamps, fridges and computers on its wiring — and its one button switches
//! it. Switching goes back through [`SimInput`] as a
//! [`Command::SwitchBox`](crate::sim::Command::SwitchBox), like freezing a
//! unit does: the screen still has exactly one writer of the world. Off, the
//! box feeds nothing, so its lamps go dark, its fridges stop being somewhere
//! to eat and start warming, and a go on its computers ends
//! (`map::utilities`, `sim::GameState::switch_box`).
//!
//! Built like the unit panel and for the same reasons: not focusable, since
//! the arrows pan the camera on this screen, and every button still answers
//! [`Activated`] so a script can press it by name. The panel is rebuilt when
//! the selection changes; the state and the button's own label are rewritten
//! in place every frame, since they follow the world.

use bevy::prelude::*;

use crate::map::utilities::fed_by;
use crate::map::Point;
use crate::state::AppState;
use crate::ui::nav::{Activated, NavSystems};
use crate::ui::{hover_highlight, label, labelled_button, plain_button, FONT_BODY, PANEL, TEXT, TEXT_ACCENT, TEXT_DIM};

use super::actors::{Sim, SimInput};
use super::hud;
use super::selection::SelectedBox;

/// The panel's root.
#[derive(Component)]
struct BoxPanel;

/// What a button on it does.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum BoxAction {
    Toggle,
    Close,
}

/// Text that follows the world.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum BoxReadout {
    /// `on, the line reaches it` — or why it is dead.
    State,
    /// The toggle's own label: `switch off`, or `switch on`.
    Toggle,
}

pub struct BoxPanelPlugin;

impl Plugin for BoxPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (press, forget_a_box_that_is_not_there, rebuild, update_readouts, hover_highlight::<BoxAction>)
                .chain()
                .after(NavSystems)
                .run_if(in_state(AppState::Game)),
        );
    }
}

/// A click on a button, or a script pressing one by name.
fn press(
    mut activated: MessageReader<Activated>,
    clicked: Query<(&BoxAction, &Interaction), Changed<Interaction>>,
    actions: Query<&BoxAction>,
    mut selected: ResMut<SelectedBox>,
    sim: Option<Res<Sim>>,
    mut input: ResMut<SimInput>,
) {
    let mut pressed: Vec<BoxAction> = clicked
        .iter()
        .filter(|(_, interaction)| **interaction == Interaction::Pressed)
        .map(|(action, _)| *action)
        .collect();
    pressed.extend(activated.read().filter_map(|message| actions.get(message.0).ok().copied()));

    let (Some(at), Some(sim)) = (selected.get(), sim) else {
        return;
    };
    for action in pressed {
        match action {
            BoxAction::Toggle => {
                input.0.switch_box(at, !sim.0.box_is_on(at));
            }
            BoxAction::Close => {
                selected.clear();
                return;
            }
        }
    }
}

/// A selection the world has no box for is none — the screen opened on
/// another map, say.
fn forget_a_box_that_is_not_there(sim: Option<Res<Sim>>, mut selected: ResMut<SelectedBox>) {
    let Some(at) = selected.get() else {
        return;
    };
    if !sim.is_some_and(|sim| sim.0.has_box(at)) {
        selected.clear();
    }
}

fn rebuild(
    mut commands: Commands,
    selected: Res<SelectedBox>,
    sim: Option<Res<Sim>>,
    panels: Query<Entity, With<BoxPanel>>,
) {
    if !selected.is_changed() {
        return;
    }
    for panel in &panels {
        commands.entity(panel).despawn();
    }
    let (Some(at), Some(sim)) = (selected.get(), sim) else {
        return;
    };

    commands.spawn((
        Name::new("box panel"),
        BoxPanel,
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
                        label(format!("distribution box  {}, {}", at.x, at.y), FONT_BODY, TEXT_ACCENT),
                        (BoxReadout::State, label(String::new(), FONT_BODY, TEXT)),
                        label(feeds(&sim, at), FONT_BODY, TEXT_DIM),
                    ],
                ),
                (BoxAction::Toggle, labelled_button((BoxReadout::Toggle, label("switch off", FONT_BODY, TEXT)), px(40))),
                (BoxAction::Close, plain_button("close", px(24))),
            ],
        )],
    ));
}

/// `feeds 3 ceiling lamps, 1 fridge` — what switching it would switch.
fn feeds(sim: &Sim, at: Point) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for object in fed_by(&sim.0.map, at) {
        let name = object.kind.as_str();
        match counts.iter_mut().find(|(known, _)| *known == name) {
            Some((_, count)) => *count += 1,
            None => counts.push((name, 1)),
        }
    }
    if counts.is_empty() {
        return "feeds nothing".to_string();
    }
    let list: Vec<String> = counts.iter().map(|(name, count)| format!("{count} {name}")).collect();
    format!("feeds {}", list.join(", "))
}

fn update_readouts(
    selected: Res<SelectedBox>,
    sim: Option<Res<Sim>>,
    mut texts: Query<(&BoxReadout, &mut Text)>,
) {
    let (Some(at), Some(sim)) = (selected.get(), sim) else {
        return;
    };
    let world = &sim.0;
    let on = world.box_is_on(at);
    let fed = world.supply().is_live(crate::map::utilities::Network::Line, at);
    for (readout, mut text) in &mut texts {
        let wanted = match readout {
            BoxReadout::State => match (on, fed) {
                (true, true) => "on".to_string(),
                (true, false) => "on, but no power line reaches it".to_string(),
                (false, _) => "off".to_string(),
            },
            BoxReadout::Toggle => if on { "switch off" } else { "switch on" }.to_string(),
        };
        if **text != wanted {
            **text = wanted;
        }
    }
}
