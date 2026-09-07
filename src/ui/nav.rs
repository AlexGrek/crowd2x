//! One highlight, three input devices.
//!
//! Every screen in the game is a grid of things that can be chosen, so rather
//! than each screen growing its own selection index, keyboard handler and
//! hover handler, they all spawn [`Focusable`] widgets and read [`Activated`].
//! The mouse, the keyboard and a gamepad then move the *same* highlight, which
//! is the only way the three can never disagree about what is selected — the
//! bug that appears the moment a screen keeps a `Selection(usize)` next to
//! `Interaction`.
//!
//! A widget declares where it sits: `Focusable::new(row, col)`. Directional
//! movement is worked out from those coordinates ([`step`]), not from spawn
//! order, so a list of rows with buttons along each one navigates the way it
//! looks.
//!
//! [`KeyboardCapture`] is the other half of that: while a text field is being
//! typed into, the physical keyboard stops being a navigation device. Without
//! it, typing `sad` walks the highlight around and `enter` both finishes the
//! name and presses whatever the highlight had landed on. A gamepad and the
//! mouse keep working throughout, because neither is what is doing the typing.
//!
//! [`Scope`] is what makes a modal modal. Focus never leaves the current
//! scope, so an open dialog or on-screen keyboard cannot be navigated past,
//! and the widgets behind it neither highlight nor respond to a click.

use bevy::ecs::message::Messages;
use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;

use super::HIGHLIGHT;
use crate::state::AppState;

/// Held direction: how long until it starts repeating, and how fast then.
const REPEAT_DELAY: f32 = 0.34;
const REPEAT_INTERVAL: f32 = 0.09;
/// How far a stick has to be pushed to count as a direction. Well above the
/// resting noise of a worn thumbstick, below a deliberate push.
const STICK_THRESHOLD: f32 = 0.55;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// A widget that can hold focus, and where it sits in its screen's grid.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Focusable {
    pub row: i32,
    pub col: i32,
    /// Which layer of the screen this belongs to; see [`Scope`].
    pub scope: u8,
}

impl Focusable {
    pub const fn new(row: i32, col: i32) -> Self {
        Self { row, col, scope: 0 }
    }

    /// Put this widget in a modal layer.
    pub const fn in_scope(mut self, scope: u8) -> Self {
        self.scope = scope;
        self
    }
}

/// Colours for a widget that wants something other than the default
/// transparent-until-focused.
#[derive(Component, Clone, Copy)]
pub struct FocusStyle {
    pub idle: Color,
    pub focused: Color,
}

/// The layer of the screen that currently owns input.
///
/// `0` is the screen itself; a dialog or an on-screen keyboard raises it while
/// it is open and puts it back when it closes.
#[derive(Resource, Default, PartialEq, Eq, Debug)]
pub struct Scope(pub u8);

/// The focused widget, if there is one.
#[derive(Resource, Default)]
pub struct Focus(pub Option<Entity>);

/// Set while a text field owns the physical keyboard.
///
/// Kept as a frame-start snapshot rather than read live from the field: the
/// keypress that *opens* a field arrives in the same frame that opens it, and
/// a live flag would let that one press be typed as well as acted on.
#[derive(Resource, Default, PartialEq, Eq)]
pub struct KeyboardCapture(pub bool);

/// Everything navigation does, in order. Systems that have to see input before
/// or after the highlight moves hang off this.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NavSystems;

/// A widget was chosen — clicked, or focused and confirmed.
#[derive(Message, Clone, Copy)]
pub struct Activated(pub Entity);

/// The player asked to go back: escape, or B on a gamepad.
#[derive(Message, Clone, Copy)]
pub struct Cancelled;

pub struct NavPlugin;

impl Plugin for NavPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Focus>()
            .init_resource::<Scope>()
            .init_resource::<KeyboardCapture>()
            .init_resource::<Repeat>()
            .add_message::<Activated>()
            .add_message::<Cancelled>()
            // Chained: a click has to be seen by `point` before `activate`
            // fires, or the first click on an unfocused widget would activate
            // whatever was focused before it.
            .add_systems(
                Update,
                (
                    drop_input_from_the_last_screen,
                    ensure_focus,
                    point,
                    navigate,
                    activate,
                    highlight,
                )
                    .chain()
                    .in_set(NavSystems),
            );
    }
}

/// The widget a direction leads to, or `None` if there is nothing that way.
///
/// Vertical movement wraps — a menu that stops dead at the bottom entry makes
/// the last item on a long list expensive to reach — and prefers the column
/// you were already in, so running down a list of rows keeps you on the same
/// button. Horizontal movement stays inside its row and does not wrap: the
/// buttons along a row are a short strip with two ends, and wrapping from
/// `delete` back round to the map name is how a map gets opened by accident.
pub fn step(slots: &[Focusable], current: usize, dir: Dir) -> Option<usize> {
    let here = *slots.get(current)?;

    match dir {
        Dir::Left | Dir::Right => {
            let forward = dir == Dir::Right;
            slots
                .iter()
                .enumerate()
                .filter(|(_, slot)| {
                    slot.row == here.row
                        && if forward {
                            slot.col > here.col
                        } else {
                            slot.col < here.col
                        }
                })
                .min_by_key(|(_, slot)| (slot.col - here.col).abs())
                .map(|(index, _)| index)
        }
        Dir::Up | Dir::Down => {
            let down = dir == Dir::Down;
            let pick = |rows: &mut dyn Iterator<Item = i32>| {
                if down {
                    rows.min()
                } else {
                    rows.max()
                }
            };

            let row = pick(&mut slots.iter().map(|slot| slot.row).filter(|&row| {
                if down {
                    row > here.row
                } else {
                    row < here.row
                }
            }))
            .or_else(|| pick(&mut slots.iter().map(|slot| slot.row)))?;

            slots
                .iter()
                .enumerate()
                .filter(|(_, slot)| slot.row == row)
                .min_by_key(|(_, slot)| ((slot.col - here.col).abs(), slot.col))
                .map(|(index, _)| index)
        }
    }
}

/// Direction currently held, and when it next repeats.
#[derive(Resource, Default)]
struct Repeat {
    dir: Option<Dir>,
    countdown: f32,
}

impl Repeat {
    /// Whether a held direction should act this frame: immediately when it is
    /// new, then after a pause, then steadily.
    fn tick(&mut self, dir: Option<Dir>, delta: f32) -> Option<Dir> {
        if dir != self.dir {
            self.dir = dir;
            self.countdown = REPEAT_DELAY;
            return dir;
        }
        let dir = dir?;
        self.countdown -= delta;
        if self.countdown <= 0.0 {
            self.countdown = REPEAT_INTERVAL;
            return Some(dir);
        }
        None
    }
}

/// The focusables of the active scope, in a stable order.
///
/// Sorted by position and then by entity, so navigation is reproducible
/// instead of depending on the order the ECS happens to return archetypes in.
fn slots_in_scope(
    focusables: &Query<(Entity, &Focusable)>,
    scope: &Scope,
) -> Vec<(Entity, Focusable)> {
    let mut slots: Vec<(Entity, Focusable)> = focusables
        .iter()
        .filter(|(_, slot)| slot.scope == scope.0)
        .map(|(entity, slot)| (entity, *slot))
        .collect();
    slots.sort_by_key(|(entity, slot)| (slot.row, slot.col, *entity));
    slots
}

/// A press belongs to the screen it was made on.
///
/// Messages outlive the frame they are written in, and a press that changes
/// screen is read again by whatever is listening on the *next* one: one escape
/// in the editor would otherwise take you back to the browser, be read there
/// as "leave the browser", and land in the main menu — and one more frame of
/// that quits the game.
fn drop_input_from_the_last_screen(
    mut transitions: MessageReader<StateTransitionEvent<AppState>>,
    mut activated: ResMut<Messages<Activated>>,
    mut cancelled: ResMut<Messages<Cancelled>>,
) {
    if transitions.read().count() > 0 {
        activated.clear();
        cancelled.clear();
    }
}

/// Something in the active scope is always focused, so a gamepad never has to
/// find the first item by pressing a direction into an empty screen.
fn ensure_focus(
    mut focus: ResMut<Focus>,
    scope: Res<Scope>,
    focusables: Query<(Entity, &Focusable)>,
) {
    let valid = focus.0.is_some_and(|entity| {
        focusables
            .get(entity)
            .is_ok_and(|(_, slot)| slot.scope == scope.0)
    });
    if valid {
        return;
    }
    focus.0 = slots_in_scope(&focusables, &scope)
        .first()
        .map(|(entity, _)| *entity);
}

/// Pointing at a widget focuses it, so the mouse and the stick share a cursor.
fn point(
    mut focus: ResMut<Focus>,
    scope: Res<Scope>,
    pointed: Query<(Entity, &Focusable, &Interaction), Changed<Interaction>>,
) {
    for (entity, slot, interaction) in &pointed {
        if slot.scope == scope.0 && matches!(interaction, Interaction::Hovered | Interaction::Pressed)
        {
            focus.0 = Some(entity);
        }
    }
}

// A system takes its dependencies as parameters; the lint counts a Bevy
// signature as if it were a call site.
#[allow(clippy::too_many_arguments)]
fn navigate(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    typing: Res<KeyboardCapture>,
    gamepads: Query<&Gamepad>,
    mut repeat: ResMut<Repeat>,
    mut focus: ResMut<Focus>,
    scope: Res<Scope>,
    focusables: Query<(Entity, &Focusable)>,
) {
    let held = held_direction(&keys, &gamepads, typing.0);
    let Some(dir) = repeat.tick(held, time.delta_secs()) else {
        return;
    };

    let slots = slots_in_scope(&focusables, &scope);
    let current = focus
        .0
        .and_then(|entity| slots.iter().position(|(candidate, _)| *candidate == entity));
    let Some(current) = current else {
        return;
    };

    let positions: Vec<Focusable> = slots.iter().map(|(_, slot)| *slot).collect();
    if let Some(next) = step(&positions, current, dir) {
        focus.0 = Some(slots[next].0);
    }
}

/// The direction being asked for, from whichever device is asking.
///
/// A stick is read as a direction rather than as an axis: menus move by whole
/// steps, so the only question is which way it is pushed past a threshold.
///
/// The keyboard drops out entirely while a field is being typed into — `w`,
/// `a`, `s` and `d` are letters in a map name before they are directions.
fn held_direction(
    keys: &ButtonInput<KeyCode>,
    gamepads: &Query<&Gamepad>,
    typing: bool,
) -> Option<Dir> {
    let mut axis = Vec2::ZERO;
    if !typing {
        if keys.any_pressed([KeyCode::ArrowUp, KeyCode::KeyW]) {
            axis.y += 1.0;
        }
        if keys.any_pressed([KeyCode::ArrowDown, KeyCode::KeyS]) {
            axis.y -= 1.0;
        }
        if keys.any_pressed([KeyCode::ArrowLeft, KeyCode::KeyA]) {
            axis.x -= 1.0;
        }
        if keys.any_pressed([KeyCode::ArrowRight, KeyCode::KeyD]) {
            axis.x += 1.0;
        }
    }

    for gamepad in gamepads {
        axis += gamepad.dpad();
        let stick = gamepad.left_stick();
        if stick.length() >= STICK_THRESHOLD {
            axis += stick;
        }
    }

    // Whichever way it leans furthest, so a diagonal is one step and not two.
    if axis.x.abs() > axis.y.abs() && axis.x.abs() >= STICK_THRESHOLD {
        Some(if axis.x > 0.0 { Dir::Right } else { Dir::Left })
    } else if axis.y.abs() >= STICK_THRESHOLD {
        Some(if axis.y > 0.0 { Dir::Up } else { Dir::Down })
    } else {
        None
    }
}

// A system takes its dependencies as parameters; the lint counts a Bevy
// signature as if it were a call site.
#[allow(clippy::too_many_arguments)]
fn activate(
    keys: Res<ButtonInput<KeyCode>>,
    typing: Res<KeyboardCapture>,
    gamepads: Query<&Gamepad>,
    mut clicks: MessageReader<MouseButtonInput>,
    focus: Res<Focus>,
    scope: Res<Scope>,
    pressed: Query<(Entity, &Focusable, &Interaction)>,
    mut activated: MessageWriter<Activated>,
    mut cancelled: MessageWriter<Cancelled>,
) {
    if keys.just_pressed(KeyCode::Escape)
        || gamepads
            .iter()
            .any(|pad| pad.just_pressed(GamepadButton::East))
    {
        cancelled.write(Cancelled);
    }

    // A click activates whatever it is over, which after `point` is also what
    // is focused. Reading the button message rather than `Changed<Interaction>`
    // means a second click on an already-pressed widget still counts.
    let clicked = clicks.read().any(|click| {
        click.button == MouseButton::Left && click.state == bevy::input::ButtonState::Pressed
    });
    if clicked {
        for (entity, slot, interaction) in &pressed {
            if slot.scope == scope.0 && *interaction != Interaction::None {
                activated.write(Activated(entity));
                return;
            }
        }
        return;
    }

    // `enter` and `space` belong to the field while one is being typed into:
    // enter ends the name, and space is a space.
    let confirmed = (!typing.0
        && keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]))
        || gamepads
            .iter()
            .any(|pad| pad.just_pressed(GamepadButton::South));
    if let (true, Some(entity)) = (confirmed, focus.0) {
        activated.write(Activated(entity));
    }
}

fn highlight(
    focus: Res<Focus>,
    mut widgets: Query<(Entity, &mut BackgroundColor, Option<&FocusStyle>), With<Focusable>>,
) {
    for (entity, mut background, style) in &mut widgets {
        let (idle, focused) = match style {
            Some(style) => (style.idle, style.focused),
            None => (Color::NONE, HIGHLIGHT),
        };
        let wanted = if focus.0 == Some(entity) { focused } else { idle };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file row: the map name, then three buttons along it.
    fn browser_rows() -> Vec<Focusable> {
        (0..3)
            .flat_map(|row| (0..4).map(move |col| Focusable::new(row, col)))
            .collect()
    }

    fn at(slots: &[Focusable], row: i32, col: i32) -> usize {
        slots
            .iter()
            .position(|slot| slot.row == row && slot.col == col)
            .expect("no such slot")
    }

    #[test]
    fn moving_down_a_list_stays_in_the_same_column() {
        // Running down a column of `delete` buttons must not drift onto the
        // map names, or a confirmed press deletes nothing and opens a map.
        let slots = browser_rows();
        let from = at(&slots, 0, 3);
        assert_eq!(step(&slots, from, Dir::Down), Some(at(&slots, 1, 3)));
    }

    #[test]
    fn vertical_movement_wraps_around_the_ends() {
        let slots = browser_rows();
        let bottom = at(&slots, 2, 1);
        assert_eq!(step(&slots, bottom, Dir::Down), Some(at(&slots, 0, 1)));
        let top = at(&slots, 0, 1);
        assert_eq!(step(&slots, top, Dir::Up), Some(at(&slots, 2, 1)));
    }

    #[test]
    fn horizontal_movement_stops_at_the_ends_of_its_row() {
        let slots = browser_rows();
        let last = at(&slots, 1, 3);
        assert_eq!(step(&slots, last, Dir::Right), None);
        let first = at(&slots, 1, 0);
        assert_eq!(step(&slots, first, Dir::Left), None);
        // ...and never leaves the row it started on.
        assert_eq!(step(&slots, first, Dir::Right), Some(at(&slots, 1, 1)));
    }

    #[test]
    fn a_short_row_is_entered_at_its_nearest_column() {
        // The top of the browser is a name field and a create button; dropping
        // into it from the `delete` column should land on create, not on the
        // field.
        let mut slots = vec![Focusable::new(0, 0), Focusable::new(0, 1)];
        slots.extend((0..4).map(|col| Focusable::new(1, col)));

        let from = at(&slots, 1, 3);
        assert_eq!(step(&slots, from, Dir::Up), Some(at(&slots, 0, 1)));
    }

    #[test]
    fn a_single_row_of_widgets_has_nowhere_vertical_to_go() {
        let slots = vec![Focusable::new(0, 0), Focusable::new(0, 1)];
        // Wrapping finds the row it is already on, so focus stays put rather
        // than jumping sideways.
        assert_eq!(step(&slots, 1, Dir::Down), Some(1));
        assert_eq!(step(&slots, 1, Dir::Up), Some(1));
    }

    #[test]
    fn navigating_from_nothing_is_not_a_panic() {
        assert_eq!(step(&[], 0, Dir::Down), None);
        assert_eq!(step(&browser_rows(), 99, Dir::Down), None);
    }

    #[test]
    fn a_held_direction_fires_once_then_pauses_then_repeats() {
        let mut repeat = Repeat::default();
        // Pressed: acts at once, so a single tap is one step.
        assert_eq!(repeat.tick(Some(Dir::Down), 0.016), Some(Dir::Down));
        // Held: nothing until the delay is up.
        assert_eq!(repeat.tick(Some(Dir::Down), 0.1), None);
        assert_eq!(repeat.tick(Some(Dir::Down), 0.2), None);
        assert_eq!(repeat.tick(Some(Dir::Down), 0.1), Some(Dir::Down));
        // Then steadily.
        assert_eq!(repeat.tick(Some(Dir::Down), 0.05), None);
        assert_eq!(repeat.tick(Some(Dir::Down), 0.05), Some(Dir::Down));
        // Changing direction is immediate again.
        assert_eq!(repeat.tick(Some(Dir::Up), 0.016), Some(Dir::Up));
        assert_eq!(repeat.tick(None, 0.016), None);
    }
}
