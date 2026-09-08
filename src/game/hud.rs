//! What the game screen puts over the map: two corners of controls, and the
//! panels under them.
//!
//! ```text
//! +---------------------------------------------------------------+
//! | [-] x4 [+]              [slower] x1 [faster] [pause] [menu]    |
//! | map, crowd, tick                        what the world is doing |
//! |                                                               |
//! |                       the map                                 |
//! +---------------------------------------------------------------+
//! ```
//!
//! **Top left is the view** — the zoom, which changes what you are looking at
//! and nothing about the world — with the map's own numbers beneath it.
//! **Top right is the world** — how fast it runs, whether it runs, and the way
//! out — with the log of what it has been doing beneath *that*, because the
//! log is the readout for exactly those controls: pausing and speeding up are
//! the things you then want to read the consequences of.
//!
//! # Why none of this is [`Focusable`]
//!
//! [`Focusable`]: crate::ui::nav::Focusable
//!
//! Every other screen is built out of `nav`'s one highlight, and this one
//! cannot be: the arrows and `WASD` pan the camera here and `A` on a gamepad
//! zooms, so a highlight would be walked around by the camera controls and
//! pressed by the zoom. Instead every control has a direct binding on both
//! devices — `q`/`e` and `A`/`B`, `+`/`-` and the bumpers, `p` and `Y`,
//! `esc` and `start` — and the panel is for the mouse, which is the one device
//! this screen otherwise has no use for.
//!
//! A [`Control`] still answers to [`Activated`] as well as to a click, so a QA
//! script can `press` one by name exactly as it presses anything else. That is
//! also why the speed pair says `slower` and `faster` where the zoom pair says
//! `-` and `+`: two buttons with one label are two buttons a test cannot tell
//! apart, and on a speed the words are the clearer of the two anyway — `+` on
//! a zoom means closer, and on a ladder of speeds it could mean either end.

use bevy::prelude::*;

use crate::editor::CurrentMap;
use crate::render::PixelZoom;
use crate::state::AppState;
use crate::ui::nav::{Activated, NavSystems};
use crate::ui::{
    label, labelled_button, plain_button, FIELD, FONT_BODY, HIGHLIGHT_SOLID, PANEL, TEXT,
    TEXT_ACCENT, TEXT_DIM,
};

use super::actors::Sim;
use super::logview;
use super::speed::{self, Change, GameSpeed};

/// Every control has a key and a gamepad button of its own, since the
/// highlight the rest of the game navigates with cannot live on this screen.
///
/// Kept narrow: this is the widest thing in the left corner, and the two
/// corners have to fit either side of a window that is only `width / 4` canvas
/// pixels across.
const KEY_HINTS: &str = "wasd/arrows/dpad/stick  move
            q/e or A/B  zoom
          -/+ or L1/R1  speed
                p or Y  pause
             esc/start  menu";

/// A button in one of the corners, and what pressing it asks for.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Control {
    ZoomOut,
    ZoomIn,
    Slower,
    Faster,
    Pause,
    Menu,
}

/// A piece of text that has to be kept true.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Readout {
    /// The zoom, as `x4`.
    Zoom,
    /// The speed, as `x1` — or `paused`.
    Speed,
    /// The pause button's own label, which flips to `resume`.
    Pause,
    /// The map, the size of the crowd and the tick count.
    Stats,
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Game), spawn_hud)
            .add_systems(
                Update,
                (press_controls, hover, update_readouts)
                    .chain()
                    .after(NavSystems)
                    .run_if(in_state(AppState::Game)),
            );
    }
}

/// Both corners, in one system.
///
/// One system rather than one per panel because the two columns are a layout
/// decision about the screen, and a screen laid out from three `OnEnter`
/// systems has no order between them to lay it out *in*.
fn spawn_hud(mut commands: Commands, zoom: Res<PixelZoom>, speed: Res<GameSpeed>) {
    // The view: what you are looking at, and the numbers about it.
    commands.spawn((
        Name::new("view controls"),
        corner(px(4), Val::Auto),
        DespawnOnExit(AppState::Game),
        children![
            row(children![
                (Control::ZoomOut, plain_button("-", px(11))),
                readout(Readout::Zoom, format!("x{}", zoom.get()), px(14)),
                (Control::ZoomIn, plain_button("+", px(11))),
            ]),
            (
                Name::new("map stats"),
                panel(),
                children![
                    (
                        Readout::Stats,
                        Text::new(String::new()),
                        TextFont::from_font_size(FONT_BODY),
                        TextColor(TEXT),
                    ),
                    (
                        Text::new(KEY_HINTS),
                        TextFont::from_font_size(FONT_BODY),
                        TextColor(TEXT_DIM),
                    ),
                ],
            ),
        ],
    ));

    // The world: how fast it runs, and what it has been doing.
    commands.spawn((
        Name::new("world controls"),
        corner(Val::Auto, px(4)),
        DespawnOnExit(AppState::Game),
        children![
            row(children![
                (Control::Slower, plain_button("slower", px(26))),
                readout(Readout::Speed, speed.label(), px(26)),
                (Control::Faster, plain_button("faster", px(26))),
                (
                    Control::Pause,
                    labelled_button(
                        (Readout::Pause, label(speed.toggle_label(), FONT_BODY, TEXT)),
                        px(30),
                    ),
                ),
                (Control::Menu, plain_button("menu", px(26))),
            ]),
            logview::panel(),
        ],
    ));
}

/// One of the two corner columns: a strip of controls with a panel under it.
///
/// `align_items` follows the corner, so both the row and the panel below it
/// hang off the same edge of the screen however wide either of them turns out
/// to be.
fn corner(left: Val, right: Val) -> impl Bundle {
    let hug = if matches!(left, Val::Auto) {
        AlignItems::FlexEnd
    } else {
        AlignItems::FlexStart
    };
    Node {
        position_type: PositionType::Absolute,
        top: px(4),
        left,
        right,
        flex_direction: FlexDirection::Column,
        align_items: hug,
        row_gap: px(3),
        ..default()
    }
}

/// A strip of controls: the panel behind a corner's buttons.
fn row(buttons: impl Bundle) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(3),
            padding: UiRect::axes(px(4), px(3)),
            ..default()
        },
        BackgroundColor(PANEL),
        buttons,
    )
}

/// The backing every panel under a control strip shares.
pub fn panel() -> impl Bundle {
    (panel_node(), BackgroundColor(PANEL))
}

/// ...and its layout on its own, for a panel with something to add to it.
pub fn panel_node() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        padding: UiRect::axes(px(4), px(3)),
        row_gap: px(3),
        ..default()
    }
}

/// A number between two buttons that change it.
///
/// Fixed width and centred, so `x0.25` and `x8` do not shove the buttons
/// either side of them around as the speed changes.
fn readout(kind: Readout, text: impl Into<String>, width: Val) -> impl Bundle {
    (
        kind,
        Text::new(text.into()),
        TextFont::from_font_size(FONT_BODY),
        TextColor(TEXT_ACCENT),
        TextLayout::justify(Justify::Center),
        Node { width, ..default() },
    )
}

/// A click on a control, or a script pressing one by name.
#[allow(clippy::too_many_arguments)]
fn press_controls(
    mut activated: MessageReader<Activated>,
    clicked: Query<(&Control, &Interaction), Changed<Interaction>>,
    controls: Query<&Control>,
    mut zoom: ResMut<PixelZoom>,
    mut speed: ResMut<GameSpeed>,
    mut next: ResMut<NextState<AppState>>,
    sim: Option<Res<Sim>>,
) {
    for (control, interaction) in &clicked {
        if *interaction == Interaction::Pressed {
            press(*control, &mut zoom, &mut speed, &mut next, sim.as_deref());
        }
    }

    // Nothing on this screen is focusable, so `nav` writes this message for
    // these buttons and for nothing else.
    for message in activated.read() {
        if let Ok(control) = controls.get(message.0) {
            press(*control, &mut zoom, &mut speed, &mut next, sim.as_deref());
        }
    }
}

/// What a control does, once.
///
/// The resources arrive as their `ResMut` wrappers rather than as `&mut`, so
/// that pressing `menu` does not mark the zoom changed — a changed zoom is
/// what makes `render` rebuild the canvas.
fn press(
    control: Control,
    zoom: &mut ResMut<PixelZoom>,
    speed: &mut ResMut<GameSpeed>,
    next: &mut ResMut<NextState<AppState>>,
    sim: Option<&Sim>,
) {
    match control {
        Control::ZoomOut => {
            zoom.step(-1);
        }
        Control::ZoomIn => {
            zoom.step(1);
        }
        Control::Slower => speed::apply(Change::Slower, speed, sim),
        Control::Faster => speed::apply(Change::Faster, speed, sim),
        Control::Pause => speed::apply(Change::TogglePause, speed, sim),
        Control::Menu => next.set(AppState::Maps),
    }
}

/// The pointer's own highlight.
///
/// `nav::highlight` does this for focusable widgets and these are deliberately
/// not focusable, so the corners light their own buttons — otherwise the only
/// clickable things in the game give no sign that they can be clicked.
fn hover(
    mut buttons: Query<(&Interaction, &mut BackgroundColor), (With<Control>, Changed<Interaction>)>,
) {
    for (interaction, mut background) in &mut buttons {
        let wanted = match interaction {
            Interaction::None => FIELD,
            _ => HIGHLIGHT_SOLID,
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

/// Keep every readout saying what is true.
///
/// Each asks its own source whether it changed, except the stats — the tick
/// count moves every fixed step, so there is nothing to ask.
fn update_readouts(
    zoom: Res<PixelZoom>,
    speed: Res<GameSpeed>,
    current: Res<CurrentMap>,
    sim: Option<Res<Sim>>,
    mut texts: Query<(Ref<Readout>, &mut Text)>,
) {
    let running = sim.as_ref().map(|sim| (sim.0.len(), sim.0.tick()));

    for (kind, mut text) in &mut texts {
        // `is_added` matters on a second visit: the HUD is respawned empty and
        // none of the resources have necessarily changed since the first one.
        let fresh = kind.is_added();
        let wanted = match *kind {
            Readout::Zoom if fresh || zoom.is_changed() => format!("x{}", zoom.get()),
            Readout::Speed if fresh || speed.is_changed() => speed.label(),
            Readout::Pause if fresh || speed.is_changed() => speed.toggle_label().to_string(),
            Readout::Stats => stats(&current, running),
            // Nothing this readout is about has moved.
            _ => continue,
        };
        // Compared before writing: text that is rewritten with the same string
        // still costs a re-layout and a re-render of the glyphs.
        if **text != wanted {
            **text = wanted;
        }
    }
}

fn stats(current: &CurrentMap, running: Option<(usize, u64)>) -> String {
    let size = current.map.size();
    let (actors, tick) = running.unwrap_or((0, 0));
    format!(
        "map    {}  {}x{}\nactors {}  tick {}",
        current.title(),
        size.width,
        size.height,
        actors,
        tick,
    )
}
