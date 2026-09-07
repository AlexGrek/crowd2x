//! The map browser: the screen that owns saved maps.
//!
//! Everything the editor cannot do to a map from inside it happens here —
//! naming a new one, playing, editing, duplicating and deleting an existing
//! one — so neither the editor nor the game has to grow a file dialog of its
//! own.
//!
//! A map's **name** is the button that plays it, and `edit` beside it is the
//! button that opens it in the editor. Playing is what a map is for, and it is
//! the biggest target in the row; editing it is a deliberate second choice.
//!
//! It is built entirely out of [`ui::nav`] focusables, which is what makes the
//! whole screen work on a gamepad without a second code path: the mouse and
//! the d-pad move one highlight, and every button is reached by the same
//! [`Activated`] message however it was pressed. The one thing a gamepad
//! genuinely cannot do — type a name — is what [`ui::keyboard`] is for.
//!
//! The list is a real scroll view rather than a page of fixed entries, because
//! the number of saved maps is not something this screen gets to decide.
//! Scrolling follows the focus ([`ui::scroll_to_show`]) so a controller can
//! reach the bottom of a long list, and the wheel moves it directly for a
//! mouse.
//!
//! Deleting asks first. It is the only button here that destroys work, and on
//! a gamepad it sits one press away from the button that opens a map.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::ScrollPosition;

use crate::editor::CurrentMap;
use crate::map::{sanitize_name, Map, MapStore, Size, VOID};
use crate::state::AppState;
use crate::ui::keyboard::{self, TextEntry, TextEntryClosed, MODAL_Z};
use crate::ui::nav::{Activated, Cancelled, Focus, FocusStyle, Focusable, NavSystems, Scope};
use crate::ui::{
    button, danger_button, label, scroll_to_show, FIELD, FONT_BODY, FONT_TITLE, HIGHLIGHT_SOLID,
    MODAL, ROW, TEXT, TEXT_ACCENT, TEXT_DIM,
};

/// Nearly opaque, unlike the menu's translucent [`PANEL`]: this screen is a
/// page of 6px text, and the demo crowd showing through it lands faces behind
/// the map names.
const BACKDROP: Color = Color::srgba(0.04, 0.04, 0.06, 0.97);

/// Where saved maps live. Relative, like `assets/`, so it resolves against the
/// manifest under `cargo run` — see the note in CLAUDE.md about running the
/// binary directly.
pub const MAPS_DIR: &str = "maps";
/// Points the store somewhere else. A scripted QA run creates and deletes
/// maps, so it needs a directory that is not somebody's work.
const ENV_MAPS_DIR: &str = "CROWD2X_MAPS";

pub fn maps_dir() -> String {
    std::env::var(ENV_MAPS_DIR).unwrap_or_else(|_| MAPS_DIR.to_string())
}

/// Size of a map made by the `create` button, in cells.
///
/// There is no size picker: a dimension is the one thing about a map that
/// cannot be changed by painting, and asking for two numbers before anything
/// can be drawn is a worse first minute than starting big enough. About four
/// screens across and four down at the current zoom.
const NEW_MAP: (i32, i32) = (24, 16);

/// Focus layers. Only one is live at a time, so an open dialog cannot be
/// navigated around.
const BROWSER: u8 = 0;
const TYPING: u8 = 1;
const CONFIRMING: u8 = 2;

/// The name field sits above the first file, so `Focusable::row` can be the
/// file's index and the scroll offset falls straight out of it.
const NAME_ROW: i32 = -1;

const ROW_HEIGHT: f32 = 12.0;
const ROW_GAP: f32 = 1.0;
const ROW_STRIDE: f32 = ROW_HEIGHT + ROW_GAP;
const LIST_HEIGHT: f32 = 100.0;
const ACTION_WIDTH: f32 = 30.0;
/// Canvas pixels scrolled per line of wheel movement.
const WHEEL_LINE: f32 = ROW_STRIDE;

const HINTS: &str = "press a name to play it   arrows/dpad move   esc/B back";

/// The maps directory, as a resource so the store is opened once.
#[derive(Resource)]
pub struct Maps(pub MapStore);

/// Saved map names, in the order the list shows them.
///
/// Read from disk on entering the screen and after anything that changes it,
/// rather than tracked incrementally: the directory is the truth, and it can
/// change under us while the game is running.
#[derive(Resource, Default)]
struct Files(Vec<String>);

/// The line under the list: what just happened, or what went wrong.
#[derive(Resource, Default)]
struct Status(String);

/// The overlay that currently owns input, if any.
#[derive(Resource, Default)]
struct Modal(Option<Entity>);

#[derive(Component)]
struct FileList;

#[derive(Component)]
struct NameLabel;

#[derive(Component)]
struct StatusLabel;

/// What a button in the browser does. The map's name travels in the component
/// so an action cannot be applied to whichever row happens to be selected
/// afterwards.
#[derive(Component, Clone)]
enum Action {
    /// Start typing in the name field.
    TypeName,
    Create,
    /// Open a map in the game.
    Play(String),
    /// Open a map in the editor.
    Edit(String),
    Duplicate(String),
    AskDelete(String),
    ConfirmDelete(String),
    CloseModal,
    Back,
}

pub struct BrowserPlugin;

impl Plugin for BrowserPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Maps(MapStore::new(maps_dir())))
            .init_resource::<Files>()
            .init_resource::<Status>()
            .init_resource::<Modal>()
            .add_systems(OnEnter(AppState::Maps), open_browser)
            .add_systems(OnExit(AppState::Maps), close_browser)
            .add_systems(
                Update,
                (
                    run_actions,
                    finish_typing,
                    go_back,
                    rebuild_list,
                    follow_focus,
                    wheel_scroll,
                    show_name,
                    show_status,
                )
                    .chain()
                    .after(NavSystems)
                    .run_if(in_state(AppState::Maps)),
            );
    }
}

fn open_browser(
    mut commands: Commands,
    maps: Res<Maps>,
    mut files: ResMut<Files>,
    mut status: ResMut<Status>,
    mut scope: ResMut<Scope>,
    mut modal: ResMut<Modal>,
    mut entry: ResMut<TextEntry>,
) {
    *scope = Scope(BROWSER);
    modal.0 = None;
    entry.close();
    status.0.clear();
    reload(&maps, &mut files, &mut status);


    commands.spawn((
        Name::new("map browser"),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(6)),
            row_gap: px(3),
            ..default()
        },
        BackgroundColor(BACKDROP),
        DespawnOnExit(AppState::Maps),
        children![
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Baseline,
                    ..default()
                },
                children![
                    label("maps", FONT_TITLE, TEXT_ACCENT),
                    label(HINTS, FONT_BODY, TEXT_DIM),
                    // A visible way out, so leaving is not a keyboard secret.
                    (
                        Action::Back,
                        button("back", Focusable::new(NAME_ROW, 2), px(30)),
                    ),
                ],
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(3),
                    ..default()
                },
                children![
                    label("name", FONT_BODY, TEXT_DIM),
                    (
                        Action::TypeName,
                        Focusable::new(NAME_ROW, 0),
                        Button,
                        Node {
                            width: px(150),
                            height: px(11),
                            justify_content: JustifyContent::FlexStart,
                            align_items: AlignItems::Center,
                            padding: UiRect::axes(px(3), px(1)),
                            ..default()
                        },
                        BackgroundColor(FIELD),
                        FocusStyle {
                            idle: FIELD,
                            focused: HIGHLIGHT_SOLID,
                        },
                        children![(
                            NameLabel,
                            Text::new("new map..."),
                            TextFont::from_font_size(FONT_BODY),
                            TextColor(TEXT),
                        )],
                    ),
                    (
                        Action::Create,
                        button("create", Focusable::new(NAME_ROW, 1), px(40)),
                    ),
                ],
            ),
            (
                FileList,
                Node {
                    flex_direction: FlexDirection::Column,
                    height: px(LIST_HEIGHT),
                    // Clip across, scroll down: a name too long for its column
                    // must not push the buttons off the row.
                    overflow: Overflow {
                        x: OverflowAxis::Clip,
                        y: OverflowAxis::Scroll,
                    },
                    row_gap: px(ROW_GAP),
                    ..default()
                },
                ScrollPosition::default(),
            ),
            (
                StatusLabel,
                Text::new(String::new()),
                TextFont::from_font_size(FONT_BODY),
                TextColor(TEXT_DIM),
            ),
        ],
    ));
}

fn close_browser(mut scope: ResMut<Scope>, mut modal: ResMut<Modal>, mut entry: ResMut<TextEntry>) {
    *scope = Scope(BROWSER);
    modal.0 = None;
    entry.close();
}

fn reload(maps: &Maps, files: &mut Files, status: &mut Status) {
    match maps.0.list() {
        Ok(names) => {
            files.0 = names;
            if status.0.is_empty() {
                status.0 = summary(files.0.len(), maps);
            }
        }
        Err(error) => {
            error!("maps: {error}");
            status.0 = format!("cannot read {}: {error}", maps.0.root().display());
        }
    }
}

fn summary(count: usize, maps: &Maps) -> String {
    let plural = if count == 1 { "map" } else { "maps" };
    format!("{count} {plural} in {}/", maps.0.root().display())
}

/// Rebuild the rows whenever the list of files changes.
///
/// Wholesale rather than by diffing: a row is four buttons and there are never
/// more than a directory's worth, and a diff would have to keep every row's
/// focus coordinates in step with its new index anyway.
fn rebuild_list(
    mut commands: Commands,
    files: Res<Files>,
    lists: Query<Entity, With<FileList>>,
    mut removed: RemovedComponents<FileList>,
) {
    // `removed` is drained so a stale despawn does not force a rebuild later.
    removed.clear();
    if !files.is_changed() {
        return;
    }

    for list in &lists {
        let mut list = commands.entity(list);
        list.despawn_related::<Children>();

        if files.0.is_empty() {
            list.with_children(|list| {
                list.spawn(label(
                    "no maps yet - type a name and press create",
                    FONT_BODY,
                    TEXT_DIM,
                ));
            });
            continue;
        }

        list.with_children(|list| {
            for (index, name) in files.0.iter().enumerate() {
                let row = index as i32;
                list.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        height: px(ROW_HEIGHT),
                        // Rows must not shrink to fit the viewport, or the
                        // scroll offset would stop matching the row stride.
                        flex_shrink: 0.0,
                        column_gap: px(2),
                        ..default()
                    },
                    BackgroundColor(ROW),
                    children![
                        (
                            Action::Play(name.clone()),
                            Focusable::new(row, 0),
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: px(ROW_HEIGHT),
                                align_items: AlignItems::Center,
                                overflow: Overflow::clip_x(),
                                padding: UiRect::horizontal(px(3)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                Text::new(name.clone()),
                                TextFont::from_font_size(FONT_BODY),
                                TextColor(TEXT),
                            )],
                        ),
                        (
                            Action::Edit(name.clone()),
                            button("edit", Focusable::new(row, 1), px(ACTION_WIDTH)),
                        ),
                        (
                            Action::Duplicate(name.clone()),
                            button("copy", Focusable::new(row, 2), px(ACTION_WIDTH)),
                        ),
                        (
                            Action::AskDelete(name.clone()),
                            danger_button("delete", Focusable::new(row, 3), px(ACTION_WIDTH)),
                        ),
                    ],
                ));
            }
        });
    }
}

// A system takes its dependencies as parameters; the lint counts a Bevy
// signature as if it were a call site.
#[allow(clippy::too_many_arguments)]
fn run_actions(
    mut commands: Commands,
    mut activated: MessageReader<Activated>,
    actions: Query<&Action>,
    maps: Res<Maps>,
    mut files: ResMut<Files>,
    mut status: ResMut<Status>,
    mut entry: ResMut<TextEntry>,
    mut scope: ResMut<Scope>,
    mut modal: ResMut<Modal>,
    mut focus: ResMut<Focus>,
    mut next: ResMut<NextState<AppState>>,
) {
    for Activated(entity) in activated.read() {
        let Ok(action) = actions.get(*entity) else {
            continue;
        };

        match action {
            Action::TypeName => {
                entry.open();
                modal.0 = Some(open_keyboard(&mut commands));
                *scope = Scope(TYPING);
            }

            Action::Create => {
                let Some(name) = sanitize_name(entry.text()) else {
                    status.0 = "type a name first".into();
                    continue;
                };
                // Never silently over-write: a name already in use becomes
                // "<name> 2" rather than replacing somebody's map.
                let name = maps.0.unique_name(&name);
                let map = Map::new(Size::new(NEW_MAP.0, NEW_MAP.1), VOID);
                match maps.0.save(&name, &map) {
                    Ok(()) => {
                        entry.clear();
                        commands.insert_resource(CurrentMap::new(name, map));
                        next.set(AppState::Editor);
                    }
                    Err(error) => {
                        error!("maps: {error}");
                        status.0 = format!("could not create: {error}");
                    }
                }
            }

            // Playing and editing differ only in where they go: both load the
            // map and hand it over, so a map cannot be opened in one screen
            // from a file and in the other from something else.
            Action::Play(name) => open(name, &maps, &mut commands, &mut status, &mut next, AppState::Game),

            Action::Edit(name) => open(name, &maps, &mut commands, &mut status, &mut next, AppState::Editor),

            Action::Duplicate(name) => match maps.0.duplicate(name) {
                Ok(copy) => {
                    status.0 = format!("copied to {copy}");
                    reload(&maps, &mut files, &mut status);
                }
                Err(error) => {
                    error!("maps: {error}");
                    status.0 = format!("could not copy {name}: {error}");
                }
            },

            Action::AskDelete(name) => {
                modal.0 = Some(open_confirm(&mut commands, &mut focus, name));
                *scope = Scope(CONFIRMING);
            }

            Action::ConfirmDelete(name) => {
                match maps.0.delete(name) {
                    Ok(()) => status.0 = format!("deleted {name}"),
                    Err(error) => {
                        error!("maps: {error}");
                        status.0 = format!("could not delete {name}: {error}");
                    }
                }
                reload(&maps, &mut files, &mut status);
                close_modal(&mut commands, &mut modal, &mut scope, &mut entry);
            }

            Action::CloseModal => close_modal(&mut commands, &mut modal, &mut scope, &mut entry),

            Action::Back => next.set(AppState::MainMenu),
        }
    }
}

/// Load a saved map and go to the screen that shows it.
fn open(
    name: &str,
    maps: &Maps,
    commands: &mut Commands,
    status: &mut Status,
    next: &mut NextState<AppState>,
    screen: AppState,
) {
    match maps.0.load(name) {
        Ok(map) => {
            commands.insert_resource(CurrentMap::new(name.to_string(), map));
            next.set(screen);
        }
        Err(error) => {
            error!("maps: {error}");
            status.0 = format!("could not open {name}: {error}");
        }
    }
}

fn open_keyboard(commands: &mut Commands) -> Entity {
    let overlay = keyboard::spawn(commands, TYPING);
    commands
        .entity(overlay)
        .insert(DespawnOnExit(AppState::Maps));
    overlay
}

/// The delete confirmation. Focus starts on `cancel`, so the press that opened
/// the dialog cannot be repeated into a deletion.
fn open_confirm(commands: &mut Commands, focus: &mut Focus, name: &str) -> Entity {
    let cancel = commands
        .spawn((
            Action::CloseModal,
            button("cancel", Focusable::new(0, 0).in_scope(CONFIRMING), px(48)),
        ))
        .id();
    let confirm = commands
        .spawn((
            Action::ConfirmDelete(name.to_string()),
            danger_button("delete", Focusable::new(0, 1).in_scope(CONFIRMING), px(48)),
        ))
        .id();

    let overlay = commands
        .spawn((
            Name::new("confirm delete"),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(4),
                ..default()
            },
            BackgroundColor(MODAL),
            GlobalZIndex(MODAL_Z),
            DespawnOnExit(AppState::Maps),
            children![
                label(format!("delete {name}?"), FONT_TITLE, TEXT),
                label("this cannot be undone", FONT_BODY, TEXT_DIM),
            ],
        ))
        .id();

    // The buttons sit in an ordinary row under the message. They are spawned
    // first so their entities can be handed to `focus`, then parented here.
    commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(6),
                margin: UiRect::top(px(4)),
                ..default()
            },
            ChildOf(overlay),
        ))
        .add_children(&[cancel, confirm]);

    focus.0 = Some(cancel);
    overlay
}

fn close_modal(
    commands: &mut Commands,
    modal: &mut Modal,
    scope: &mut Scope,
    entry: &mut TextEntry,
) {
    if let Some(overlay) = modal.0.take() {
        commands.entity(overlay).despawn();
    }
    entry.close();
    *scope = Scope(BROWSER);
}

/// When typing ends, put focus on `create`: the name is entered, and creating
/// is what happens next.
fn finish_typing(
    mut commands: Commands,
    mut closed: MessageReader<TextEntryClosed>,
    mut modal: ResMut<Modal>,
    mut scope: ResMut<Scope>,
    mut entry: ResMut<TextEntry>,
    mut focus: ResMut<Focus>,
    creates: Query<(Entity, &Action)>,
) {
    if closed.read().count() == 0 {
        return;
    }
    close_modal(&mut commands, &mut modal, &mut scope, &mut entry);
    focus.0 = creates
        .iter()
        .find(|(_, action)| matches!(action, Action::Create))
        .map(|(entity, _)| entity);
}

/// Escape and B: out of a dialog first, out of the screen only when there is
/// nothing left to back out of.
fn go_back(
    mut commands: Commands,
    mut cancelled: MessageReader<Cancelled>,
    mut modal: ResMut<Modal>,
    mut scope: ResMut<Scope>,
    mut entry: ResMut<TextEntry>,
    mut next: ResMut<NextState<AppState>>,
) {
    if cancelled.read().count() == 0 {
        return;
    }
    if modal.0.is_some() {
        close_modal(&mut commands, &mut modal, &mut scope, &mut entry);
    } else {
        next.set(AppState::MainMenu);
    }
}

/// Keep the focused row on screen, so a gamepad can reach the end of a list
/// longer than the viewport.
fn follow_focus(
    focus: Res<Focus>,
    files: Res<Files>,
    focusables: Query<&Focusable>,
    mut lists: Query<&mut ScrollPosition, With<FileList>>,
) {
    if !focus.is_changed() {
        return;
    }
    let Some(slot) = focus.0.and_then(|entity| focusables.get(entity).ok()) else {
        return;
    };
    if slot.scope != BROWSER {
        return;
    }

    for mut scroll in &mut lists {
        // The name field is above the list; being on it means the top of it.
        let wanted = if slot.row < 0 {
            0.0
        } else {
            scroll_to_show(
                scroll.0.y,
                LIST_HEIGHT,
                slot.row as f32 * ROW_STRIDE,
                ROW_HEIGHT,
            )
        };
        scroll.0.y = wanted.min(max_scroll(files.0.len()));
    }
}

fn wheel_scroll(
    mut wheel: MessageReader<MouseWheel>,
    files: Res<Files>,
    mut lists: Query<&mut ScrollPosition, With<FileList>>,
) {
    let mut delta = 0.0;
    for event in wheel.read() {
        delta += match event.unit {
            MouseScrollUnit::Line => event.y * WHEEL_LINE,
            MouseScrollUnit::Pixel => event.y,
        };
    }
    if delta == 0.0 {
        return;
    }

    for mut scroll in &mut lists {
        // Wheel up moves the content down, which is a smaller offset.
        scroll.0.y = (scroll.0.y - delta).clamp(0.0, max_scroll(files.0.len()));
    }
}

fn max_scroll(rows: usize) -> f32 {
    (rows as f32 * ROW_STRIDE - ROW_GAP - LIST_HEIGHT).max(0.0)
}

fn show_name(entry: Res<TextEntry>, mut labels: Query<&mut Text, With<NameLabel>>) {
    if !entry.is_changed() {
        return;
    }
    for mut text in &mut labels {
        **text = entry.display();
    }
}

fn show_status(status: Res<Status>, mut labels: Query<&mut Text, With<StatusLabel>>) {
    if !status.is_changed() {
        return;
    }
    for mut text in &mut labels {
        **text = status.0.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_that_fits_does_not_scroll() {
        assert_eq!(max_scroll(0), 0.0);
        assert_eq!(max_scroll((LIST_HEIGHT / ROW_STRIDE) as usize), 0.0);
    }

    #[test]
    fn the_last_row_can_be_scrolled_to_exactly() {
        // The bottom of the last row lands on the bottom of the viewport: any
        // more and the list scrolls past its own end into empty space.
        let rows = 20;
        let content = rows as f32 * ROW_STRIDE - ROW_GAP;
        assert_eq!(max_scroll(rows), content - LIST_HEIGHT);
    }

    #[test]
    fn focusing_the_last_row_scrolls_no_further_than_the_end() {
        let rows = 20;
        let last_top = (rows - 1) as f32 * ROW_STRIDE;
        let wanted = scroll_to_show(0.0, LIST_HEIGHT, last_top, ROW_HEIGHT);
        assert!(wanted.min(max_scroll(rows)) <= max_scroll(rows));
        // ...and it does have to move to get there.
        assert!(wanted > 0.0);
    }
}
