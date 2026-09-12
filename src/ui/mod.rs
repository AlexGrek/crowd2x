//! Shared look of the interface.
//!
//! The menu and the editor HUD are plain `bevy_ui`: `Node` layout, `Button`,
//! `Interaction`. They are drawn by the upscale camera, which is the only one
//! targeting the window, so UI is composited over the finished canvas rather
//! than being baked into it.
//!
//! That is also the only way `Interaction` can work: `bevy_ui`'s focus system
//! only computes a cursor position for cameras whose render target is a
//! window, and the world camera renders to an off-screen image.
//!
//! Screens are built out of [`nav`]-focusable widgets rather than each keeping
//! its own selection index, so the mouse, the keyboard and a gamepad all move
//! one highlight. [`keyboard`] is what lets a gamepad type.

pub mod keyboard;
pub mod nav;

use bevy::prelude::*;

use crate::render::PIXEL_SCALE;
use nav::{FocusStyle, Focusable};

/// Body text, in canvas pixels (see [`UiPlugin`]) — the canvas is only 180
/// tall, so these are small numbers by UI standards. At `PIXEL_SCALE` 4 this
/// lands on 24 real pixels.
pub const FONT_BODY: f32 = 6.0;
/// The menu title.
pub const FONT_TITLE: f32 = 14.0;

pub const TEXT: Color = Color::srgb(0.94, 0.95, 0.98);
pub const TEXT_DIM: Color = Color::srgb(0.66, 0.68, 0.76);
pub const TEXT_ACCENT: Color = Color::srgb(0.98, 0.82, 0.31);

/// Warning colour for the one button that destroys something.
pub const TEXT_DANGER: Color = Color::srgb(0.95, 0.45, 0.42);

/// Backing for panels and the menu, dark enough to read over a busy crowd.
pub const PANEL: Color = Color::srgba(0.04, 0.04, 0.06, 0.82);
/// Fill behind the highlighted menu entry.
pub const HIGHLIGHT: Color = Color::srgba(0.98, 0.82, 0.31, 0.16);
/// Focus fill for a widget that already has a background of its own, where the
/// translucent [`HIGHLIGHT`] would barely register.
pub const HIGHLIGHT_SOLID: Color = Color::srgba(0.98, 0.82, 0.31, 0.34);
/// Backing for a modal. Fully opaque, unlike [`PANEL`]: a dialog is asking a
/// question, and the screen it interrupted showing through it is just noise
/// behind the answer.
pub const MODAL: Color = Color::srgb(0.04, 0.04, 0.06);
/// A widget you can put something into: a text field, a key on the on-screen
/// keyboard.
pub const FIELD: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);
/// A row of the file list, so rows read as separate things to choose.
pub const ROW: Color = Color::srgba(1.0, 1.0, 1.0, 0.05);

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        // Measure the UI in canvas pixels rather than window pixels. `UiScale`
        // multiplies every `Val::Px` and font size, so a 12px font here is 12
        // canvas pixels — the same units the world is laid out in, and the
        // same apparent size whatever the window is doing.
        app.insert_resource(UiScale(PIXEL_SCALE as f32))
            .add_plugins((nav::NavPlugin, keyboard::KeyboardPlugin));
    }
}

/// A labelled button that can be focused.
///
/// Compose the screen's own action onto it at the call site:
/// `(Action::Create, ui::button("create", Focusable::new(0, 1), px(40)))`.
pub fn button(label: &str, focus: Focusable, width: Val) -> impl Bundle {
    (
        focus,
        Button,
        Node {
            width,
            height: px(11),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(3), px(1)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        children![(
            Text::new(label),
            TextFont::from_font_size(FONT_BODY),
            TextColor(TEXT),
        )],
    )
}

/// A labelled button the focus system knows nothing about.
///
/// For a screen that drives its own input rather than `nav`'s one highlight —
/// the game, where the arrows pan the camera and `A` zooms, so a highlight
/// would be walked by the camera controls and pressed by the zoom. It is
/// filled rather than transparent, because nothing is ever going to highlight
/// it to say that it is there.
pub fn plain_button(text: &str, width: Val) -> impl Bundle {
    labelled_button(label(text, FONT_BODY, TEXT), width)
}

/// A [`plain_button`] whose label carries something of its own — a marker for
/// whichever system rewrites it.
pub fn labelled_button(label: impl Bundle, width: Val) -> impl Bundle {
    (
        Button,
        Node {
            width,
            height: px(11),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(3), px(1)),
            ..default()
        },
        BackgroundColor(FIELD),
        children![label],
    )
}

/// A button that reads as dangerous whether or not it is focused.
pub fn danger_button(label: &str, focus: Focusable, width: Val) -> impl Bundle {
    (
        focus,
        Button,
        Node {
            width,
            height: px(11),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(3), px(1)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        FocusStyle {
            idle: Color::NONE,
            focused: Color::srgba(0.95, 0.45, 0.42, 0.28),
        },
        children![(
            Text::new(label),
            TextFont::from_font_size(FONT_BODY),
            TextColor(TEXT_DANGER),
        )],
    )
}

/// Light a button under the pointer, for buttons the focus system does not
/// know about.
///
/// [`nav::highlight`] does this for focusable widgets. The game screen's are
/// deliberately not focusable — the arrows pan the camera there and `A` zooms,
/// so one highlight would be walked around by the camera controls — and
/// without this the only clickable things on that screen would give no sign
/// that they can be clicked.
///
/// Generic over the marker so each panel registers it for its own buttons:
/// `app.add_systems(Update, ui::hover_highlight::<Control>)`. One
/// implementation, so two panels of plain buttons cannot come to disagree
/// about what hovering looks like.
pub fn hover_highlight<M: Component>(
    mut buttons: Query<(&Interaction, &mut BackgroundColor), (With<M>, Changed<Interaction>)>,
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

pub fn label(text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont::from_font_size(size),
        TextColor(color),
    )
}

/// Scroll offset that brings an item into view, moving as little as possible.
///
/// Only moves when it has to: a list that recentres on every step makes the
/// whole page slide under a d-pad press, and there is then nothing stable to
/// count rows against.
pub fn scroll_to_show(scroll: f32, viewport: f32, item_top: f32, item_height: f32) -> f32 {
    let bottom = item_top + item_height;
    if item_top < scroll {
        item_top
    } else if bottom > scroll + viewport {
        bottom - viewport
    } else {
        scroll
    }
    .max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_already_on_screen_does_not_move_the_list() {
        assert_eq!(scroll_to_show(0.0, 100.0, 40.0, 14.0), 0.0);
        assert_eq!(scroll_to_show(20.0, 100.0, 40.0, 14.0), 20.0);
    }

    #[test]
    fn an_item_below_the_fold_scrolls_just_far_enough_to_show_it() {
        // Just far enough: the row that was focused stays at the bottom edge,
        // so the rows above it keep their positions.
        assert_eq!(scroll_to_show(0.0, 100.0, 98.0, 14.0), 12.0);
    }

    #[test]
    fn an_item_above_the_fold_scrolls_back_up_to_it() {
        assert_eq!(scroll_to_show(50.0, 100.0, 28.0, 14.0), 28.0);
    }

    #[test]
    fn the_list_never_scrolls_above_its_first_row() {
        // Wrapping from the last row to the first must land on 0, not on a
        // negative offset that would leave a gap above the list.
        assert_eq!(scroll_to_show(50.0, 100.0, 0.0, 14.0), 0.0);
        assert_eq!(scroll_to_show(0.0, 100.0, -5.0, 14.0), 0.0);
    }
}
