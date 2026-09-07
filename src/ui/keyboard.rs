//! Typing a name, with or without a keyboard.
//!
//! A gamepad cannot type, so a name field that only listened to
//! `KeyboardInput` would be a dead end on a controller — and the map browser
//! is mostly a name field. The answer is the console one: an on-screen
//! keyboard whose keys are ordinary [`Focusable`] widgets, so the d-pad walks
//! them, the mouse clicks them, and the physical keyboard keeps working the
//! whole time because both paths write to the same [`TextEntry`] buffer.
//!
//! The buffer is deliberately not a `bevy_ui` text input widget. What is being
//! typed is a *file name*, which has rules ([`map::sanitize_name`]) and a
//! length limit, and the on-screen keys have to be able to feed it too — a
//! plain `String` both devices push characters into is smaller than either
//! device's own path into an editable text field.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;

use super::nav::{Activated, Focusable, KeyboardCapture, NavSystems};
use super::{FIELD, FONT_BODY, FONT_TITLE, MODAL, TEXT, TEXT_ACCENT, TEXT_DIM};
use crate::map::MAX_NAME;

/// Rows of the on-screen keyboard, laid out like a phone rather than like a
/// QWERTY keyboard: alphabetical order is faster to *scan* with a d-pad, where
/// finding a letter costs one press per step and muscle memory does not help.
const ROWS: [&str; 4] = ["abcdefghij", "klmnopqrst", "uvwxyz0123", "456789 -_"];

/// Modal overlays sit above every screen. One shared constant so a dialog and
/// the keyboard cannot end up fighting each other.
pub const MODAL_Z: i32 = 10;

/// Key size in canvas pixels — 10 across a 320px canvas with room to spare.
const KEY: f32 = 22.0;
const KEY_HEIGHT: f32 = 13.0;
const KEY_GAP: f32 = 2.0;

/// What a key does. Characters are the common case; the other two are the keys
/// no physical keyboard equivalent can be assumed for.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyCap {
    Char(char),
    Backspace,
    Done,
}

/// The name being typed, shared by every device that can type it.
#[derive(Resource, Default)]
pub struct TextEntry {
    text: String,
    /// Whether typing is being accepted — the on-screen keyboard is open.
    open: bool,
}

impl TextEntry {
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replace the whole name. Used by the QA harness, which has no reason to
    /// spell a name out one key at a time.
    pub fn set(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.truncate();
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn clear(&mut self) {
        self.text.clear();
    }

    /// Accepts only what a map name may contain, so the field cannot show a
    /// name that saving would then refuse or silently change.
    pub fn push(&mut self, c: char) {
        if c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_') {
            self.text.push(c);
            self.truncate();
        }
    }

    pub fn backspace(&mut self) {
        self.text.pop();
    }

    fn truncate(&mut self) {
        while self.text.chars().count() > MAX_NAME {
            self.text.pop();
        }
    }

    /// What to draw in the field: the name, a caret while typing, and a prompt
    /// when it is empty and nobody is typing.
    pub fn display(&self) -> String {
        match (self.open, self.text.is_empty()) {
            (true, _) => format!("{}_", self.text),
            (false, true) => "new map...".to_string(),
            (false, false) => self.text.clone(),
        }
    }
}

/// Typing finished — `Done`, `Enter`, or the field losing interest.
#[derive(Message, Clone, Copy)]
pub struct TextEntryClosed;

pub struct KeyboardPlugin;

impl Plugin for KeyboardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TextEntry>()
            .add_message::<TextEntryClosed>()
            .add_systems(Update, capture_keyboard.before(NavSystems))
            .add_systems(
                Update,
                (type_physical_keys, press_on_screen_keys, show_entry).after(NavSystems),
            );
    }
}

/// Build the on-screen keyboard under `parent`.
///
/// The caller owns the returned entity: it decides where the overlay sits in
/// its own screen and despawns it when typing ends.
pub fn spawn(commands: &mut Commands, scope: u8) -> Entity {
    let mut keyboard = commands.spawn((
        Name::new("on-screen keyboard"),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: px(KEY_GAP),
            ..default()
        },
        BackgroundColor(MODAL),
        // Above the screen that opened it. Without an explicit z-index a
        // later-spawned root is not reliably on top, and a modal that is not
        // on top is not a modal.
        GlobalZIndex(MODAL_Z),
    ));

    keyboard.with_children(|parent| {
        // The field being typed into is behind the overlay, so the name is
        // echoed here — otherwise a gamepad user picks letters blind.
        parent.spawn(super::label("name", FONT_BODY, TEXT_DIM));
        parent.spawn((
            Echo,
            Text::new(String::new()),
            TextFont::from_font_size(FONT_TITLE),
            TextColor(TEXT_ACCENT),
            Node {
                margin: UiRect::bottom(px(4)),
                ..default()
            },
        ));

        for (row_index, row) in ROWS.iter().enumerate() {
            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(KEY_GAP),
                    ..default()
                })
                .with_children(|row_node| {
                    for (col, c) in row.chars().enumerate() {
                        let label = if c == ' ' { "spc" } else { &c.to_string() };
                        row_node.spawn(key(
                            label,
                            KeyCap::Char(c),
                            Focusable::new(row_index as i32, col as i32).in_scope(scope),
                            KEY,
                        ));
                    }
                });
        }

        // The two keys that are not characters, on a row of their own so a
        // mistyped letter is never one press away from ending the entry.
        parent
            .spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(KEY_GAP),
                margin: UiRect::top(px(KEY_GAP)),
                ..default()
            })
            .with_children(|row_node| {
                row_node.spawn(key(
                    "del",
                    KeyCap::Backspace,
                    Focusable::new(ROWS.len() as i32, 0).in_scope(scope),
                    KEY * 2.0,
                ));
                row_node.spawn(key(
                    "done",
                    KeyCap::Done,
                    Focusable::new(ROWS.len() as i32, 1).in_scope(scope),
                    KEY * 2.0,
                ));
            });
    });

    keyboard.id()
}

/// Where the name being typed is echoed, above the keys.
#[derive(Component)]
struct Echo;

fn show_entry(entry: Res<TextEntry>, mut echoes: Query<&mut Text, With<Echo>>) {
    for mut text in &mut echoes {
        if entry.is_changed() || text.is_empty() {
            **text = entry.display();
        }
    }
}

fn key(label: &str, cap: KeyCap, focus: Focusable, width: f32) -> impl Bundle {
    (
        cap,
        focus,
        Button,
        Node {
            width: px(width),
            height: px(KEY_HEIGHT),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(FIELD),
        super::nav::FocusStyle {
            idle: FIELD,
            focused: super::HIGHLIGHT_SOLID,
        },
        children![(
            Text::new(label),
            TextFont::from_font_size(FONT_BODY),
            TextColor(TEXT),
        )],
    )
}

/// Publish, once per frame and before anything reads input, whether a field is
/// taking the keyboard.
///
/// A snapshot rather than a live read: the keypress that opens a field arrives
/// in the same frame the field opens, and reading `entry.open` live would let
/// that press be typed into the field it just opened — an `enter` that opens
/// the keyboard and closes it again in one go.
fn capture_keyboard(entry: Res<TextEntry>, mut capture: ResMut<KeyboardCapture>) {
    capture.set_if_neq(KeyboardCapture(entry.open));
}

/// A real keyboard types into the same buffer the on-screen keys do.
fn type_physical_keys(
    mut input: MessageReader<KeyboardInput>,
    capture: Res<KeyboardCapture>,
    mut entry: ResMut<TextEntry>,
    mut closed: MessageWriter<TextEntryClosed>,
) {
    for event in input.read() {
        // `capture`, not `entry.open`: see `capture_keyboard`.
        if !capture.0 || event.state != ButtonState::Pressed {
            continue;
        }
        match &event.logical_key {
            Key::Backspace => entry.backspace(),
            Key::Enter => {
                entry.close();
                closed.write(TextEntryClosed);
            }
            // `text` rather than the key code: it is what the layout actually
            // produced, so a non-US keyboard types what is printed on it.
            _ => {
                if let Some(text) = &event.text {
                    for c in text.chars() {
                        entry.push(c);
                    }
                }
            }
        }
    }
}

fn press_on_screen_keys(
    mut activated: MessageReader<Activated>,
    caps: Query<&KeyCap>,
    mut entry: ResMut<TextEntry>,
    mut closed: MessageWriter<TextEntryClosed>,
) {
    for Activated(entity) in activated.read() {
        let Ok(cap) = caps.get(*entity) else {
            continue;
        };
        match cap {
            KeyCap::Char(c) => entry.push(*c),
            KeyCap::Backspace => entry.backspace(),
            KeyCap::Done => {
                entry.close();
                closed.write(TextEntryClosed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_characters_a_map_name_may_contain_are_accepted() {
        let mut entry = TextEntry::default();
        for c in "of/fi..ce 2".chars() {
            entry.push(c);
        }
        assert_eq!(entry.text(), "office 2");
    }

    #[test]
    fn the_field_cannot_be_typed_past_the_name_limit() {
        // Otherwise the field would show a name that saving would truncate to
        // something else.
        let mut entry = TextEntry::default();
        for _ in 0..MAX_NAME * 2 {
            entry.push('x');
        }
        assert_eq!(entry.text().chars().count(), MAX_NAME);
    }

    #[test]
    fn the_field_says_what_it_is_for_when_it_is_empty() {
        let mut entry = TextEntry::default();
        assert_eq!(entry.display(), "new map...");

        entry.open();
        assert_eq!(entry.display(), "_", "an open field shows its caret");
        entry.push('a');
        assert_eq!(entry.display(), "a_");

        entry.close();
        assert_eq!(entry.display(), "a");
    }

    #[test]
    fn backspacing_an_empty_field_is_harmless() {
        let mut entry = TextEntry::default();
        entry.backspace();
        assert_eq!(entry.text(), "");
    }

    /// Every key on screen has to be reachable from every other one, or a
    /// gamepad can strand itself on a letter with no way to reach `done`.
    #[test]
    fn the_key_rows_are_a_full_rectangle_of_reachable_keys() {
        use super::super::nav::{step, Dir};

        let mut slots: Vec<Focusable> = ROWS
            .iter()
            .enumerate()
            .flat_map(|(row, keys)| {
                (0..keys.chars().count())
                    .map(move |col| Focusable::new(row as i32, col as i32))
            })
            .collect();
        slots.push(Focusable::new(ROWS.len() as i32, 0));
        slots.push(Focusable::new(ROWS.len() as i32, 1));

        // From the last letter, pressing down reaches the action row...
        let last_letter = slots.len() - 3;
        let down = step(&slots, last_letter, Dir::Down).expect("down leaves the letters");
        assert_eq!(slots[down].row, ROWS.len() as i32);
        // ...and from there, down again wraps back to the first row.
        let wrapped = step(&slots, down, Dir::Down).expect("down wraps");
        assert_eq!(slots[wrapped].row, 0);
    }
}
